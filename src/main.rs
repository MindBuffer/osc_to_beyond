extern crate pangolin;
extern crate rosc;

use pangolin::{Pangolin, BeyondLaserPoint};
use rosc::{OscPacket, OscType};
use std::collections::HashMap;
use std::sync::mpsc;

type BlobChunks = Vec<Vec<u8>>;

#[derive(Default)]
struct Layer {
    // All blobs received for a timestamp, representing parts of a single frame.
    // Once the `is_last` packet is received, the blobs are rendered to a `LayerFrame`.
    blob_map: HashMap<i64, BlobChunks>,
}

/// A frame for a single `layer`, ready to be drawn to the given `outputs`.
struct LayerFrame {
    frame: Vec<BeyondLaserPoint>,
    layer: String,
    outputs: Vec<i32>,
}

// This is currently used for the scienceworks laser lattice installation, for which we know we
// have 5 outputs.
const NUM_OUTPUTS: usize = 5;


/// We run the OSC receiver on a separate thread.
///
/// The receiver receives packets as fast as possible, updates the `Layer` map and sends new frames
/// to the main thread when available.
fn run_osc(frame_sender: mpsc::Sender<LayerFrame>) {

    // Listen for packets on 9001.
    let osc_socket = std::net::UdpSocket::bind("0.0.0.0:9001").unwrap();

    // Re-use a buffer for receiving and decoding OSC via UDP packets.
    let mut osc_buffer = [0u8; 20_000];

    // Track the last time that a time stamp was received.
    // We do this in order to remove old blobs from the layer map to avoid leaking memory in the
    // case that we miss the last packet of a frame.
    let mut last_time_stamp;

    // Tracks the state of received layers.
    let mut layer_map = HashMap::new();

    // Check for waiting OSC messages.
    'osc: loop {
        let (size, _addr) = match osc_socket.recv_from(&mut osc_buffer) {
            Ok(packet) => packet,
            Err(err) => {
                // If we receive an error, print it and sleep for a bit before trying to receive again.
                println!("UdpSocket::recv_from Err: {}", err);
                std::thread::sleep(std::time::Duration::from_millis(16));
                continue 'osc;
            },
        };

        let packet = rosc::decoder::decode(&osc_buffer[..size]).unwrap();

        // We're expecting a single `OscMessage` per packet.
        let mut message = match packet {
            OscPacket::Message(msg) => msg,
            OscPacket::Bundle(mut bundle) => {
                // OpenFrameworks sends us a bundle where the first packet is always a message
                match bundle.content.swap_remove(0) {
                    OscPacket::Message(msg) => msg,
                    packet => panic!("unexpected OscPacket: {:?}", packet),
                }
            }
        };

        // Get the arguments as an iterator so that we can handle them one at a time.
        let mut args = message.args.take().unwrap().into_iter();

        // The time stamp should always be the very first message.
        let time_stamp = match args.next() {
            Some(OscType::Long(time_stamp)) => time_stamp,
            arg => panic!("unexpected arg {:?}",arg),
        };

        // Track the most recent time_stamp so that we can remove the old blobs.
        last_time_stamp = time_stamp;

        // Ignore the messages that just keep the udp stream alive
        if "/alive" == &message.addr {
            continue;
        }

        // Indicates if the packet is the last for the frame at the given time_stamp.
        let is_last = match args.next() {
            Some(OscType::Bool(is_last)) => is_last,
            arg => panic!("unexpected arg {:?}",arg),
        };

        // Collect the outputs to which this layer should be drawn.
        let mut outputs = vec![];
        let blob;
        loop {
            match args.next() {
                Some(OscType::Int(output)) => outputs.push(output),
                arg => {
                    blob = match arg {
                        Some(OscType::Blob(b)) => b,
                        None => vec![],
                        arg => panic!("unexpected arg {:?}",arg),
                    };
                    break;
                }
            }
        }

        {
            // The layer at the given address, e.g. `/layer1`, `/layer2` or `/layer3`.
            let mut layer = layer_map.entry(message.addr.clone()).or_insert(Layer::default());

            // Get the length of the blob in case we need to allocate a `Vec` for a new frame.
            let blob_len = blob.len();

            // Append the received blob to the layer at the given time stamp.
            {
                let mut blob_chunks = layer.blob_map.entry(time_stamp).or_insert(Vec::new());
                blob_chunks.push(blob);
            }

            // If this is the last packet, turn the blobs into a frame and send it.
            if is_last {
                let bytes_per_point = 8;
                let mut frame = Vec::with_capacity(blob_len / bytes_per_point);
                // Take the complete blob from the map and remove the blobs one point at a time.
                for chunk in layer.blob_map.remove(&time_stamp).unwrap() {
                    assert!(chunk.len() % bytes_per_point == 0);
                    for data in chunk.chunks(bytes_per_point) {
                        let xa = data[0] as i8;
                        let xb = data[1] as i8;
                        let ya = data[2] as i8;
                        let yb = data[3] as i8;
                        let r = data[4] as i8;
                        let g = data[5] as i8;
                        let b = data[6] as i8;
                        let _a = data[7] as i8;

                        let x = ((xb as i16) << 8) | (xa as i16) & 0xff;
                        let y = ((yb as i16) << 8) | (ya as i16) & 0xff;

                        let x = (x as f32 + 32768.0) / 65535.0;
                        let y = (y as f32 + 32768.0) / 65535.0;

                        let r = (r as i16 + 128) as u8;
                        let g = (g as i16 + 128) as u8;
                        let b = (b as i16 + 128) as u8;

                        let point = BeyondLaserPoint::new(x, y, 0.0, r, g, b);
                        frame.push(point);
                    }
                }

                // Send the frame to the main pangolin thread.
                let complete_frame = LayerFrame {
                    frame: frame,
                    layer: message.addr,
                    outputs: outputs,
                };

                // If the channel is closed, assume we are finished and exit the osc loop.
                if frame_sender.send(complete_frame).is_err() {
                    println!("OSC thread: channel has closed, finishing up");
                    break 'osc;
                }
            }
        }

        // Find old time_stamps and remove them from the layer map
        for layer in layer_map.values_mut() {

            // Collect the time stamps that we want to remove.
            let mut to_remove = vec![];
            for &time_stamp in layer.blob_map.keys() {
                let one_sec_micros = 1_000_000;
                let dt_micros = last_time_stamp - time_stamp;
                if dt_micros > one_sec_micros {
                    to_remove.push(time_stamp);
                }
            }

            // Remove the blobs at the collected time stamps
            for stamp in to_remove {
                layer.blob_map.remove(&stamp);
            }
        }
    }
}


fn main() {
    let lib = pangolin::load_library().unwrap();
    let pangolin = Pangolin::new(&lib).unwrap();

    println!("
        Beyond Exe Started = {}
        Beyond Exe Ready = {}
        Beyond App Version = {}
        Beyond DLL Version = {}
        Beyond Projection Count = {}
        Beyond Zone Count = {}
    ", pangolin.beyond_exe_started(),
       pangolin.beyond_exe_ready(),
       pangolin.get_beyond_version(),
       pangolin.get_dll_version(),
       pangolin.get_projector_count(),
       pangolin.get_zone_count());

    pangolin.create_zone_image(0, b"/output1\0");
    pangolin.create_zone_image(1, b"/output2\0");
    pangolin.create_zone_image(2, b"/output3\0");
    pangolin.create_zone_image(3, b"/output4\0");
    pangolin.create_zone_image(4, b"/output5\0");

    // Ask Beyonod to enable the laser output.
    pangolin.enable_laser_output();

    // Spawn the OSC receiving thread.
    let (frame_sender, frame_receiver) = mpsc::channel();
    std::thread::spawn(move || run_osc(frame_sender));

    // Send frames to beyond roughly 60 times per second.
    let sleep_interval = std::time::Duration::from_millis(5);

    // Track the most recently received frame per layer.
    let mut layer_frames = HashMap::new();

    // A frame for each output.
    let mut output_frames = vec![vec![]; NUM_OUTPUTS];

    loop {
        // Receive pending `LayerFrame`s, sent from the OSC receiver thread.
        for LayerFrame { frame, layer, outputs } in frame_receiver.try_iter() {
            layer_frames.insert(layer, (frame, outputs));
        }

        // If Pangolin isn't ready there's nothing more to do.
        if !pangolin.beyond_exe_ready() {
            std::thread::sleep(sleep_interval);
            continue;
        }

        // Time to submit frames to Pangolin! First, clear the frame for each output.
        for frame in &mut output_frames {
            frame.clear();
        }

        // Fill each output from the layers that target it.
        for &(ref frame, ref outputs) in layer_frames.values() {
            for &out in outputs {
                let output = &mut output_frames[out as usize];
                for &point in frame {
                    output.push(point);
                }
            }
        }

        // Send each output frame to Pangolin.
        for (i, frame) in output_frames.iter().enumerate() {
            let address = match i {
                0 => b"/output1\0",
                1 => b"/output2\0",
                2 => b"/output3\0",
                3 => b"/output4\0",
                4 => b"/output5\0",
                _ => unreachable!(),
            };
            let zone_indices = vec![(i+1) as u8];
            let scan_rate = 100;
            pangolin.send_frame_to_image(address, frame, &zone_indices, scan_rate);
        }

        std::thread::sleep(sleep_interval);
    }
}
