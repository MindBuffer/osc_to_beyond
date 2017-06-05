// RUST TIME!! 

extern crate find_folder;
extern crate libloading;
pub use libloading::{Library, Symbol};
use std::os::raw::c_void;

pub struct Pangolin<'lib> {
    ldb_destroy: Symbol<'lib, unsafe extern fn() -> i32>,
    ldb_beyond_exe_started: Symbol<'lib, unsafe extern fn() -> i32>,
    ldb_beyond_exe_ready: Symbol<'lib, unsafe extern fn() -> i32>,
    ldb_enable_laser_output: Symbol<'lib, unsafe extern fn() -> i32>,
    ldb_disable_laser_output: Symbol<'lib, unsafe extern fn() -> i32>,
    ldb_blackout: Symbol<'lib, unsafe extern fn() -> i32>,
    ldb_get_dll_version: Symbol<'lib, unsafe extern fn() -> i32>,
    ldb_get_beyond_version: Symbol<'lib, unsafe extern fn() -> i32>,
    ldb_get_projector_count: Symbol<'lib, unsafe extern fn() -> i32>,
    ldb_get_zone_count: Symbol<'lib, unsafe extern fn() -> i32>,
    ldb_create_zone_image: Symbol<'lib, unsafe extern fn(i32, *const u8) -> i32>,
    ldb_create_projector_image: Symbol<'lib, unsafe extern fn(i32, *const u8) -> i32>,
    ldb_delete_zone_image: Symbol<'lib, unsafe extern fn(*const u8) -> i32>,
    ldb_delete_projector_image: Symbol<'lib, unsafe extern fn(*const u8) -> i32>,
    ldb_send_frame_to_image: Symbol<'lib, unsafe extern fn(*const u8, i32, *const c_void, *const c_void, i32 ) -> i32>,
}

/*
--------- LASER POINT ------------
X,Y,Z - 32 bit float point value. Standard "single". The coordinate system is -32K...+32K. Please fit your data in the range.
Color - 32 bit integer number. Color is 24bit RGB, standard encoding in windows format. Red bytes comes low (00..FF), Green after that, Blue the most signification. It exactly as in GDI.
RepCount -  usigned byte. Repeat counter of the point. 0 - no repeats. 1 - one repeat and so on. - usigned byte.
Focus - usigned byte. Now it unused
Status - flags, now leave it zero.
Zero - usigned byte. leave it zero.

You need have array with points and supply pointed on this array into ldSendFrameToImage.
*/

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct BeyondLaserPoint {
    /// 32bit float point, Coordinate system -32k to +32k
    x: f32, 
    y: f32,
    z: f32,
    /// RGB in Windows style
    point_colour: i32,
    /// Repeat Count
    rep_count: u8,
    /// Beam brush reserved, leave it zero
    focus: u8,
    /// bitmask -- attributes
    status: u8,
    /// Leave it zero
    zero: u8,
}

// typedef struct {
// 	float x, y, z; // Normalised Coords
// 	int r, g, b; 	
// } LaserPoint;

pub fn load_library() -> libloading::Result<Library> {
    let exe_path = std::env::current_exe()?;
    let pangolin_path = find_folder::Search::ParentsThenKids(7, 7).of(exe_path).for_folder("pangolin").unwrap();
    let path = pangolin_path.join("/BEYONDIO.dll");
    //let path = concat!(env!("CARGO_MANIFEST_DIR"), "/BEYONDIO.dll");
    let lib = Library::new(path)?;
    Ok(lib)
}

impl<'lib> Pangolin<'lib> {
    pub fn new(lib: &'lib Library) -> libloading::Result<Self> {

        unsafe {
            let create: Symbol<unsafe extern fn() -> i32> = lib.get(b"ldbCreate\0")?;
            //Return type of 1 equals success
            assert!(create()==1);

            let pangolin = Pangolin {
                ldb_destroy: lib.get(b"ldbDestroy\0")?,
                ldb_beyond_exe_started: lib.get(b"ldbBeyondExeStarted\0")?,
                ldb_beyond_exe_ready: lib.get(b"ldbBeyondExeReady\0")?,
                ldb_enable_laser_output: lib.get(b"ldbEnableLaserOutput\0")?,
                ldb_disable_laser_output: lib.get(b"ldbDisableLaserOutput\0")?,
                ldb_blackout: lib.get(b"ldbBlackout\0")?,
                ldb_get_dll_version: lib.get(b"ldbGetDllVersion\0")?,
                ldb_get_beyond_version: lib.get(b"ldbGetBeyondVersion\0")?,
                ldb_get_projector_count: lib.get(b"ldbGetProjectorCount\0")?,
                ldb_get_zone_count: lib.get(b"ldbGetZoneCount\0")?,
                ldb_create_zone_image: lib.get(b"ldbCreateZoneImage\0")?,
                ldb_create_projector_image: lib.get(b"ldbCreateProjectorImage\0")?,
                ldb_delete_zone_image: lib.get(b"ldbDeleteZoneImage\0")?,
                ldb_delete_projector_image: lib.get(b"ldbDeleteProjectorImage\0")?,
                ldb_send_frame_to_image: lib.get(b"ldbSendFrameToImage\0")?,
            };
            Ok(pangolin)
        }
    }

    /// Positive value indicates a percentage of the projector scan rate.
    /// Negative value indicates *actual* scan rate (i.e. -30000 is 30000hz).
    ///Image name is a zero terminated ANSII string 
    pub fn send_frame_to_image(&self, image_name: &[u8], laser_points: &[BeyondLaserPoint], zone_indices: &[u8], scan_rate: i32) -> i32 {
        // Make sure we dont exceed the max num of points for Pangolin
        assert!(laser_points.len() <= 8192);

        let mut zone_array = [0u8; 256];
        let mut i = 0;
        assert!(zone_indices.len() < 255);
        while i < zone_indices.len() {
            assert!(i < 256);
            zone_array[i] = 1 + zone_indices[i];
            i+=1;
        }
        // Last elem is indicated by a trailing 0
        zone_array[i] = 0;

        unsafe {
            (self.ldb_send_frame_to_image)(image_name.as_ptr(), 
                                           laser_points.len() as i32, 
                                           laser_points.as_ptr() as *const c_void, 
                                           zone_array.as_ptr() as *const c_void, 
                                           scan_rate) 
        }
    }

    ///Image name is a zero terminated ANSII string 
    pub fn create_zone_image(&self, zone_index: i32, image_name: &[u8]) -> i32 {
        unsafe {
            // No idea what i32 return is yet
            (self.ldb_create_zone_image)(zone_index, image_name.as_ptr())
        }
    }

    ///Image name is a zero terminated ANSII string 
    pub fn create_projector_image(&self, projector_index: i32, image_name: &[u8]) -> i32 {
        unsafe {
            // No idea what i32 return is yet
            (self.ldb_create_projector_image)(projector_index, image_name.as_ptr())
        }
    }

    ///Image name is a zero terminated ANSII string 
    pub fn delete_zone_image(&self, image_name: &[u8]) -> i32 {
        unsafe {
            // No idea what i32 return is yet
            (self.ldb_delete_zone_image)(image_name.as_ptr())
        }
    }

    ///Image name is a zero terminated ANSII string 
    pub fn delete_projector_image(&self, image_name: &[u8]) -> i32 {
        unsafe {
            // No idea what i32 return is yet
            (self.ldb_delete_projector_image)(image_name.as_ptr())
        }
    }

    pub fn get_dll_version(&self) -> i32 {
        unsafe {
            (self.ldb_get_dll_version)()
        }
    }

    pub fn get_beyond_version(&self) -> i32 {
        unsafe {
            (self.ldb_get_beyond_version)()
        }
    }

    pub fn get_projector_count(&self) -> i32 {
        unsafe {
            (self.ldb_get_projector_count)()
        }
    }

    pub fn get_zone_count(&self) -> i32 {
        unsafe {
            (self.ldb_get_zone_count)()
        }
    }

    pub fn destroy(&self) -> i32 {
        unsafe {
            (self.ldb_destroy)()
        }
    }

    pub fn beyond_exe_started(&self) -> bool {
        unsafe {
            match (self.ldb_beyond_exe_started)() {
                0 => false,
                1 => true,
                _ => unreachable!(),
            }
        }
    }

    pub fn beyond_exe_ready(&self) -> bool {
        unsafe {
            match (self.ldb_beyond_exe_ready)() {
                0 => false,
                1 => true,
                _ => unreachable!(),
            }
        }
    }

    pub fn enable_laser_output(&self) -> bool {
        unsafe {
            match (self.ldb_enable_laser_output)() {
                0 | -1 => false,
                1 => true,
                n => panic!("{:?}",n),
            }
        }
    }

    pub fn disable_laser_output(&self) -> bool {
        unsafe {
            match (self.ldb_disable_laser_output)() {
                0 => false,
                1 => true,
                _ => unreachable!(),
            }
        }
    }

    pub fn blackout(&self) -> bool {
        unsafe {
            match (self.ldb_blackout)() {
                0 => false,
                1 => true,
                _ => unreachable!(),
            }
        }
    }
}

impl<'lib> Drop for Pangolin<'lib> {
    fn drop(&mut self){
        self.disable_laser_output();
        self.destroy();
    }
}

impl BeyondLaserPoint {
    /// Coords are normalised
    pub fn new(x: f32, y: f32, z: f32, r: u8, g: u8, b: u8) -> Self {
        let a = 0;
        let colour = (a << 24i32) | ((b as i32) << 16i32) | ((g as i32) << 8i32) | (r as i32); 
        BeyondLaserPoint {
            x: x * 64_000.0 - 32_000.0,
            y: y * -64_000.0 + 32_000.0,
            z: z * 64_000.0 - 32_000.0,
            point_colour: colour,
            rep_count: 0,
            focus: 0,
            status: 0,
            zero: 0,
        }
    } 
}

#[test]
fn test() {
    let lib = load_library().unwrap();
    let pangolin = Pangolin::new(&lib).unwrap();
    assert!(pangolin.beyond_exe_started());
    assert!(pangolin.enable_laser_output());
    println!("Dll version = {}", pangolin.get_dll_version());
}