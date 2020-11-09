//! # xdrfile
//! Read and write xdr trajectory files in .xtc and .trr file format
//!
//! This crate is mainly intended to be a wrapper around the GROMACS libxdrfile
//! XTC library and provides basic functionality to read and write xtc and trr
//! files with a safe api.
//!
//! # Basic usage example
//! ```rust
//! use xdrfile::*;
//!
//! fn main() -> Result<()> {
//!     // get a handle to the file
//!     let mut trj = XTCTrajectory::open_read("tests/1l2y.xtc")?;
//!
//!     // find number of atoms in the file
//!     let num_atoms = trj.get_num_atoms()?;
//!
//!     // a frame object is used to get to read or write from a trajectory
//!     // without instantiating data arrays for every step
//!     let mut frame = Frame::with_capacity(num_atoms);
//!
//!     // read the first frame of the trajectory
//!     trj.read(&mut frame)?;
//!
//!     assert_eq!(frame.step, 1);
//!     assert_eq!(frame.num_atoms, num_atoms);
//!
//!     let first_atom_coords = frame.coords[0];
//!     assert_eq!(first_atom_coords, [-0.8901, 0.4127, -0.055499997]);
//!
//!     Ok(())
//! }
//! ```
//!
//! # Frame iteration
//! For convenience, the trajectory implementations provide "into_iter" to
//! be turned into an iterator that yields Rc<Frame>. If a frame is not kept
//! during iteration, the Iterator reuses it for better performance (and hence,
//! Rc is required)
//!
//! ```rust
//! use xdrfile::*;
//!
//! fn main() -> Result<()> {
//!     // get a handle to the file
//!     let trj = XTCTrajectory::open_read("tests/1l2y.xtc")?;
//!
//!     // iterate over all frames
//!     for (idx, result) in trj.into_iter().enumerate() {
//!         let frame = result?;
//!         println!("{}", frame.time);
//!         assert_eq!(idx+1, frame.step as usize);
//!     }
//!     Ok(())
//! }
//! ```

#[cfg(test)]
#[macro_use]
extern crate assert_approx_eq;
extern crate lazy_init;

pub mod c_abi;
mod errors;
mod frame;
mod iterator;
pub use errors::*;
pub use frame::Frame;
pub use iterator::*;

use c_abi::xdr_seek;
use c_abi::xdrfile;
use c_abi::xdrfile::XDRFILE;
use c_abi::xdrfile_trr;
use c_abi::xdrfile_xtc;

use lazy_init::Lazy;
use std::cell::Cell;
use std::ffi::CString;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum FileMode {
    Write,
    Append,
    Read,
}

impl FileMode {
    /// Get a CStr slice corresponding to the file mode
    fn to_cstr(&self) -> &'static std::ffi::CStr {
        let bytes: &[u8; 2] = match *self {
            FileMode::Write => b"w\0",
            FileMode::Append => b"a\0",
            FileMode::Read => b"r\0",
        };

        std::ffi::CStr::from_bytes_with_nul(bytes).expect("CStr::from_bytes_with_nul failed")
    }
}

fn path_to_cstring(path: impl AsRef<Path>) -> Result<CString> {
    let s = path.as_ref().to_str().ok_or_else(Error::from_convert)?;
    Ok(CString::new(s)?)
}

/// A safe wrapper around the c implementation of an XDRFile
struct XDRFile {
    xdrfile: *mut XDRFILE,
    #[allow(dead_code)]
    filemode: FileMode,
    path: PathBuf,
}

impl XDRFile {
    pub fn open(path: impl AsRef<Path>, filemode: FileMode) -> Result<XDRFile> {
        let path = path.as_ref();
        unsafe {
            let path_p = path_to_cstring(path)?.into_raw();
            // SAFETY: mode_p must not be mutated by the C code
            let mode_p = filemode.to_cstr().as_ptr();

            let xdrfile = xdrfile::xdrfile_open(path_p, mode_p);

            // Reconstitute the CString so it is deallocated correctly
            let _ = CString::from_raw(path_p);

            if !xdrfile.is_null() {
                let path = path.to_owned();
                Ok(XDRFile {
                    xdrfile,
                    filemode,
                    path,
                })
            } else {
                // Something went wrong. But the C api does not tell us what
                Err(Error::from_open(path, filemode))
            }
        }
    }

    /// Get the current position in the file
    pub fn tell(&self) -> u64 {
        use std::convert::TryInto as _;
        unsafe {
            xdr_seek::xdr_tell(self.xdrfile)
                .try_into()
                .expect("i64 could not be converted to u64")
        }
    }
}

impl io::Seek for XDRFile {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        use std::io::SeekFrom::*;
        let (whence, pos) = match pos {
            Start(u) => (0, u as i64),
            Current(i) => (1, i),
            End(i) => (2, i),
        };
        unsafe {
            let code = xdr_seek::xdr_seek(self.xdrfile, pos, whence);
            if code as u32 != xdrfile::exdrOK {
                return Err(io::Error::new(io::ErrorKind::Other, "Seek failed"));
            }
        };

        Ok(self.tell())
    }
}

impl Drop for XDRFile {
    /// Close the underlying xdr file on drop
    fn drop(&mut self) {
        unsafe {
            xdrfile::xdrfile_close(self.xdrfile);
        }
    }
}

/// The trajectory trait defines shared methods for xtc and trr trajectories
pub trait Trajectory {
    /// Read the next step of the trajectory into the frame object
    fn read(&mut self, frame: &mut Frame) -> Result<()>;

    /// Write the frame to the trajectory file
    fn write(&mut self, frame: &Frame) -> Result<()>;

    /// Flush the trajectory file
    fn flush(&mut self) -> Result<()>;

    /// Get the number of atoms from the give trajectory
    fn get_num_atoms(&mut self) -> Result<u32>;
}

/// Read/Write XTC Trajectories
pub struct XTCTrajectory {
    handle: XDRFile,
    precision: Cell<f32>, // internal mutability required for read method
    num_atoms: Lazy<Result<u32>>,
}

impl XTCTrajectory {
    pub fn open(path: impl AsRef<Path>, filemode: FileMode) -> Result<XTCTrajectory> {
        let xdr = XDRFile::open(path, filemode)?;
        Ok(XTCTrajectory {
            handle: xdr,
            precision: Cell::new(1000.0),
            num_atoms: Lazy::new(),
        })
    }

    /// Open a file in read mode
    pub fn open_read(path: impl AsRef<Path>) -> Result<Self> {
        Self::open(path, FileMode::Read)
    }

    /// Open a file in append mode
    pub fn open_append(path: impl AsRef<Path>) -> Result<Self> {
        Self::open(path, FileMode::Append)
    }

    /// Open a file in write mode
    pub fn open_write(path: impl AsRef<Path>) -> Result<Self> {
        Self::open(path, FileMode::Write)
    }
}

impl Trajectory for XTCTrajectory {
    fn read(&mut self, frame: &mut Frame) -> Result<()> {
        let mut step: i32 = 0;
        unsafe {
            // C lib requires an i32 to be passed, but step is exposed it as u32
            // (A step cannot be negative, can it?). So we need to create a step
            // variable to pass to read_xtc and cast it afterwards to u32
            let code = xdrfile_xtc::read_xtc(
                self.handle.xdrfile,
                frame.num_atoms as i32,
                &mut step,
                &mut frame.time,
                &mut frame.box_vector,
                frame.coords.as_ptr() as *mut [f32; 3],
                &mut self.precision.get(),
            ) as u32;
            frame.step = step as u32;
            ErrorCode::check(code, ()).map_err(Error::from_read)
        }
    }

    fn write(&mut self, frame: &Frame) -> Result<()> {
        unsafe {
            let code = xdrfile_xtc::write_xtc(
                self.handle.xdrfile,
                frame.num_atoms as i32,
                frame.step as i32,
                frame.time,
                frame.box_vector.as_ptr() as *mut [[f32; 3]; 3],
                frame.coords[..].as_ptr() as *mut [f32; 3],
                1000.0,
            ) as u32;
            ErrorCode::check(code, ()).map_err(Error::from_write)
        }
    }

    fn flush(&mut self) -> Result<()> {
        unsafe {
            let code = xdr_seek::xdr_flush(self.handle.xdrfile) as u32;
            ErrorCode::check(code, ()).map_err(Error::from_flush)
        }
    }

    fn get_num_atoms(&mut self) -> Result<u32> {
        self.num_atoms
            .get_or_create(|| {
                let mut num_atoms: i32 = 0;

                unsafe {
                    let path = path_to_cstring(&self.handle.path)?;
                    let path_p = path.into_raw();
                    let code =
                        xdrfile_xtc::read_xtc_natoms(path_p, &mut num_atoms as *const i32) as u32;
                    // Reconstitute the CString so it is deallocated correctly
                    let _ = CString::from_raw(path_p);

                    ErrorCode::check(code, num_atoms as u32).map_err(Error::from_read_num_atoms)
                }
            })
            .clone()
    }
}

/// Read/Write TRR Trajectories
pub struct TRRTrajectory {
    handle: XDRFile,
    num_atoms: Lazy<Result<u32>>,
}

impl TRRTrajectory {
    pub fn open(path: impl AsRef<Path>, filemode: FileMode) -> Result<TRRTrajectory> {
        let xdr = XDRFile::open(path, filemode)?;
        Ok(TRRTrajectory {
            handle: xdr,
            num_atoms: Lazy::new(),
        })
    }

    /// Open a file in read mode
    pub fn open_read(path: impl AsRef<Path>) -> Result<Self> {
        Self::open(path, FileMode::Read)
    }

    /// Open a file in append mode
    pub fn open_append(path: impl AsRef<Path>) -> Result<Self> {
        Self::open(path, FileMode::Append)
    }

    /// Open a file in write mode
    pub fn open_write(path: impl AsRef<Path>) -> Result<Self> {
        Self::open(path, FileMode::Write)
    }
}

impl Trajectory for TRRTrajectory {
    fn read(&mut self, frame: &mut Frame) -> Result<()> {
        let mut step: i32 = 0;
        let mut lambda: f32 = 0.0;
        unsafe {
            // C lib requires an i32 to be passed, but step is exposed it as u32
            // (A step cannot be negative, can it?). So we need to create a step
            // variable to pass to read_trr and cast it afterwards to u32.
            // Similar for lambda.
            let code = xdrfile_trr::read_trr(
                self.handle.xdrfile,
                frame.num_atoms as i32,
                &mut step,
                &mut frame.time,
                &mut lambda,
                &mut frame.box_vector,
                frame.coords.as_ptr() as *mut [f32; 3],
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) as u32;
            frame.step = step as u32;
            ErrorCode::check(code, ()).map_err(Error::from_read)
        }
    }

    fn write(&mut self, frame: &Frame) -> Result<()> {
        unsafe {
            let code = xdrfile_trr::write_trr(
                self.handle.xdrfile,
                frame.num_atoms as i32,
                frame.step as i32,
                frame.time,
                0.0,
                frame.box_vector.as_ptr() as *mut [[f32; 3]; 3],
                frame.coords[..].as_ptr() as *mut [f32; 3],
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) as u32;
            ErrorCode::check(code, ()).map_err(Error::from_write)
        }
    }

    fn flush(&mut self) -> Result<()> {
        unsafe {
            let code = xdr_seek::xdr_flush(self.handle.xdrfile) as u32;
            ErrorCode::check(code, ()).map_err(Error::from_flush)
        }
    }

    fn get_num_atoms(&mut self) -> Result<u32> {
        self.num_atoms
            .get_or_create(|| {
                let mut num_atoms: i32 = 0;
                unsafe {
                    let path = path_to_cstring(&self.handle.path)?;
                    let path_p = path.into_raw();
                    let code =
                        xdrfile_trr::read_trr_natoms(path_p, &mut num_atoms as *const i32) as u32;
                    // Reconstitute the CString so it is deallocated correctly
                    let _ = CString::from_raw(path_p);

                    ErrorCode::check(code, num_atoms as u32).map_err(Error::from_read_num_atoms)
                }
            })
            .clone()
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_read_write_xtc() -> Result<()> {
        let tempfile = NamedTempFile::new().expect("Could not create temporary file");
        let tmp_path = tempfile.path();

        let natoms: u32 = 2;
        let frame = Frame {
            num_atoms: natoms,
            step: 5,
            time: 2.0,
            box_vector: [[1.0, 2.0, 3.0], [2.0, 1.0, 3.0], [3.0, 2.0, 1.0]],
            coords: vec![[1.0, 1.0, 1.0], [1.0, 1.0, 1.0]],
        };
        let mut f = XTCTrajectory::open_write(&tmp_path)?;
        let write_status = f.write(&frame);
        match write_status {
            Err(_) => panic!("Failed"),
            Ok(()) => {}
        }
        f.flush()?;

        let mut new_frame = Frame::with_capacity(natoms);
        let mut f = XTCTrajectory::open_read(tmp_path)?;
        let num_atoms = f.get_num_atoms()?;
        assert_eq!(num_atoms, natoms);

        let read_status = f.read(&mut new_frame);
        match read_status {
            Err(e) => assert!(false, "{:?}", e),
            Ok(()) => {}
        }

        assert_eq!(new_frame.num_atoms, frame.num_atoms);
        assert_eq!(new_frame.step, frame.step);
        assert_approx_eq!(new_frame.time, frame.time);
        assert_eq!(new_frame.box_vector, frame.box_vector);
        assert_eq!(new_frame.coords, frame.coords);
        Ok(())
    }

    #[test]
    fn test_read_write_trr() -> Result<()> {
        let tempfile = NamedTempFile::new().expect("Could not create temporary file");
        let tmp_path = tempfile.path();

        let natoms: u32 = 2;
        let frame = Frame {
            num_atoms: natoms,
            step: 5,
            time: 2.0,
            box_vector: [[1.0, 2.0, 3.0], [2.0, 1.0, 3.0], [3.0, 2.0, 1.0]],
            coords: vec![[1.0, 1.0, 1.0], [1.0, 1.0, 1.0]],
        };
        let mut f = TRRTrajectory::open_write(tmp_path)?;
        let write_status = f.write(&frame);
        match write_status {
            Err(_) => panic!("Failed"),
            Ok(()) => {}
        }
        f.flush()?;

        let mut new_frame = Frame::with_capacity(natoms);
        let mut f = TRRTrajectory::open_read(tmp_path)?;
        // let num_atoms = f.get_num_atoms()?;
        // assert_eq!(num_atoms, natoms);

        let read_status = f.read(&mut new_frame);
        match read_status {
            Err(e) => assert!(false, "{:?}", e),
            Ok(()) => {}
        }

        assert_eq!(new_frame.num_atoms, frame.num_atoms);
        assert_eq!(new_frame.step, frame.step);
        assert_eq!(new_frame.time, frame.time);
        assert_eq!(new_frame.box_vector, frame.box_vector);
        assert_eq!(new_frame.coords, frame.coords);
        Ok(())
    }

    #[test]
    fn test_path_to_cstring() -> Result<(), Box<dyn std::error::Error>> {
        let result_invalid = path_to_cstring(PathBuf::from("invalid/\0path"));

        assert_eq!(
            result_invalid,
            CString::new("invalid/\0path").map_err(Error::from)
        );
        assert!(!result_invalid.is_ok());
        if let Err(e) = result_invalid {
            match e.task() {
                ErrorTask::ToCString(_) => (),
                _ => panic!("path_to_cstring's errortask should be ErrorTask::ToCString(_)"),
            }
        } else {
            panic!("path_to_cstring on a NULL-containing string should return an error");
        }

        let result_valid = path_to_cstring("valid/path");

        assert_eq!(result_valid, Ok(CString::new("valid/path")?));
        Ok(())
    }

    #[test]
    fn test_tell() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let tempfile = NamedTempFile::new()?;
        let tmp_path = tempfile.path();

        let natoms: u32 = 2;
        let frame = Frame {
            num_atoms: natoms,
            step: 5,
            time: 2.0,
            box_vector: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            coords: vec![[0.0, 0.0, 0.0], [0.5, 0.5, 0.5]],
        };
        let mut f = TRRTrajectory::open_write(tmp_path)?;
        assert_eq!(f.handle.tell(), 0);
        f.write(&frame)?;
        assert_eq!(f.handle.tell(), 144);
        f.flush()?;

        let mut new_frame = Frame::with_capacity(natoms);
        let mut f = TRRTrajectory::open_read(tmp_path)?;
        assert_eq!(f.handle.tell(), 0);

        f.read(&mut new_frame)?;
        assert_eq!(f.handle.tell(), 144);

        Ok(())
    }

    #[test]
    fn test_seek() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let tempfile = NamedTempFile::new()?;
        let tmp_path = tempfile.path();

        let natoms: u32 = 2;
        let mut frame = Frame {
            num_atoms: natoms,
            step: 0,
            time: 0.0,
            box_vector: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            coords: vec![[0.0, 0.0, 0.0], [0.5, 0.5, 0.5]],
        };
        let mut f = TRRTrajectory::open_write(tmp_path)?;
        f.write(&frame)?;
        let after_first_frame = f.handle.tell();
        frame.step += 1;
        frame.time += 10.0;
        f.write(&frame)?;
        let after_second_frame = f.handle.tell();
        f.flush()?;

        let mut new_frame = Frame::with_capacity(natoms);
        let mut f = TRRTrajectory::open_read(tmp_path)?;
        use std::io::Seek as _;
        let pos = f.handle.seek(std::io::SeekFrom::Current(144))?;
        assert_eq!(pos, after_first_frame);

        f.read(&mut new_frame)?;
        assert_eq!(f.handle.tell(), after_second_frame);

        assert_eq!(new_frame.num_atoms, frame.num_atoms);
        assert_eq!(new_frame.step, frame.step);
        assert_eq!(new_frame.time, frame.time);
        assert_eq!(new_frame.box_vector, frame.box_vector);
        assert_eq!(new_frame.coords, frame.coords);

        Ok(())
    }

    #[test]
    fn test_err_could_not_open() {
        let file_name = "non-existent.xtc";

        let path = Path::new(&file_name);
        if let Err(e) = XDRFile::open(file_name, FileMode::Read) {
            match e.task() {
                ErrorTask::OpenFile(err_path, err_mode) => {
                    assert_eq!(path, err_path);
                    assert_eq!(FileMode::Read, *err_mode)
                }
                _ => panic!("Wrong Error type"),
            }
        }
    }

    #[test]
    fn test_err_could_not_read_atom_nr() -> Result<()> {
        let file_name = "README.md"; // not a trajectory
        let mut trr = TRRTrajectory::open_read(file_name)?;
        if let Err(e) = trr.get_num_atoms() {
            match e.task() {
                ErrorTask::ReadNumAtoms => {
                    assert_eq!(Some(ErrorCode::ExdrMagic), *e.code());
                }
                _ => panic!("Wrong Error type"),
            }
        };
        Ok(())
    }

    #[test]
    fn test_err_could_not_read() -> Result<()> {
        let file_name = "README.md"; // not a trajectory
        let mut frame = Frame::with_capacity(1);
        let mut trr = TRRTrajectory::open_read(file_name)?;
        if let Err(e) = trr.read(&mut frame) {
            match e.task() {
                ErrorTask::Read => {
                    assert_eq!(Some(ErrorCode::ExdrMagic), *e.code());
                }
                _ => panic!("Wrong Error type"),
            }
        }
        Ok(())
    }

    #[test]
    fn test_err_file_eof() -> Result<(), Box<dyn std::error::Error>> {
        let tempfile = NamedTempFile::new()?;
        let tmp_path = tempfile.path();

        let natoms: u32 = 2;
        let frame = Frame {
            num_atoms: natoms,
            step: 5,
            time: 2.0,
            box_vector: [[1.0, 2.0, 3.0], [2.0, 1.0, 3.0], [3.0, 2.0, 1.0]],
            coords: vec![[1.0, 1.0, 1.0], [1.0, 1.0, 1.0]],
        };
        let mut f = XTCTrajectory::open_write(&tmp_path)?;
        f.write(&frame)?;
        f.flush()?;

        let mut new_frame = Frame::with_capacity(natoms);
        let mut f = XTCTrajectory::open_read(tmp_path)?;

        f.read(&mut new_frame)?;

        let result = f.read(&mut new_frame); // Should be eof as we only wrote one frame
        if let Err(e) = result {
            assert!(e.is_eof());
        } else {
            panic!("read two frames after writing one");
        }

        let mut file = std::fs::File::create(tmp_path)?;
        use std::io::Write as _;
        file.write_all(&[0; 999])?;
        file.flush()?;

        let mut f = XTCTrajectory::open_read(tmp_path)?;
        let result = f.read(&mut new_frame); // Should be an invalid XTC file
        if let Err(e) = result {
            assert!(!e.is_eof());
        } else {
            panic!("999 zero bytes was read as a valid XTC file");
        }

        Ok(())
    }
}
