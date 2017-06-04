extern crate pangolin;
extern crate rosc;

use pangolin::{Pangolin, BeyondLaserPoint};
use rosc::{OscPacket, OscType};
use std::collections::HashMap;

#[derive(Default)]
struct LayerPacket {
    blob_chunks: Vec<Vec<u8>>,
}

#[derive(Default)]
struct Layer {
    // All blobs received for timestamp
    blob_map: HashMap<i64, LayerPacket>,
    outputs: Vec<i32>,
    frame: Vec<BeyondLaserPoint>,
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
    ", pangolin.beyond_exe_started(), pangolin.beyond_exe_ready(), pangolin.get_beyond_version(), pangolin.get_dll_version(), pangolin.get_projector_count(), pangolin.get_zone_count());

    pangolin.create_zone_image(0, b"/output1\0");
    pangolin.create_zone_image(1, b"/output2\0");
    pangolin.create_zone_image(2, b"/output3\0");
    pangolin.create_zone_image(3, b"/output4\0");
    pangolin.create_zone_image(4, b"/output5\0");

    pangolin.enable_laser_output();

    let sleep_interval = std::time::Duration::from_millis(8);
    let osc_socket = std::net::UdpSocket::bind("0.0.0.0:9001").unwrap();
    osc_socket.set_nonblocking(true).unwrap();
    let mut osc_buffer = [0u8; 20_000];
    let mut last_time_stamp = 0i64;
    let mut layer_map = HashMap::new();
    const NUM_OUTPUTS: usize = 5;
    let mut output_frames: Vec<Vec<BeyondLaserPoint>> = vec![vec![]; NUM_OUTPUTS];
    loop {

        // Check for waiting OSC messages.
        while let Ok((size, _addr)) = osc_socket.recv_from(&mut osc_buffer) {
            let packet = rosc::decoder::decode(&osc_buffer[..size]).unwrap();
            let mut message = match packet {
                OscPacket::Message(msg) => msg, 
                OscPacket::Bundle(mut bundle) => { 
                    //OpenFrameworks sends us a bundle where the first packet is always a message
                    match bundle.content.swap_remove(0) {
                        OscPacket::Message(msg) => msg,
                        _ => unreachable!(),
                    }
                }
            };
            let mut args = message.args.take().unwrap().into_iter();
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
            let is_last = match args.next() {
                Some(OscType::Bool(is_last)) => is_last,
                _ => unreachable!(),
            };

            let mut outputs = vec![];
            let blob;
            loop {
                match args.next() {
                    Some(OscType::Int(output)) => outputs.push(output),
                    arg => {
                        blob = match arg {
                            Some(OscType::Blob(b)) => b,
                            None => vec![],
                            _ => unreachable!(),
                        };
                        break;
                    }
                }
            }
            {
                let mut layer = layer_map.entry(message.addr).or_insert(Layer::default());
                {
                    let mut layer_packet = layer.blob_map.entry(time_stamp).or_insert(LayerPacket::default());
                    layer_packet.blob_chunks.push(blob);
                }
                let bytes_per_point = 8;
                if is_last {
                    layer.outputs = outputs;
                    layer.frame.clear();
                    {
                        let layer_packet = &layer.blob_map[&time_stamp];
                        for chunk in &layer_packet.blob_chunks {
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
                                layer.frame.push(point);
                            }
                        }
                    }
                    layer.blob_map.remove(&time_stamp);
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

        // If Pangolin isn't ready , nothing to do. 
        if !pangolin.beyond_exe_ready() {
            std::thread::sleep(sleep_interval);
            continue;
        }

        // Time to submit frames to Pangolin 
        // Clear the frame for each output
        for frame in &mut output_frames {
            frame.clear();
        }

        // Fill each output from the layers that target it.
        for layer in layer_map.values() {
            for &out in &layer.outputs {
                let output = &mut output_frames[out as usize];
                for &point in &layer.frame {
                    output.push(point);
                }
            }
        } 

        // Send each output to Pangolin 
        for (i,frame) in output_frames.iter().enumerate() {
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
