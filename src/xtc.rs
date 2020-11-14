use crate::c_abi;
use crate::c_abi::xdrfile::{Matrix, Rvec, XDRFILE};
use crate::errors::*;
use std::convert::TryInto;

/// Convert an error code from a C call to an Error
///
/// `code` should be an integer return code returned from the C API.
/// If `code` indicates the function returned successfully, None is returned;
/// otherwise, the code is converted into the appropriate `Error`.
pub fn check_code(code: impl Into<ErrorCode>, task: ErrorTask) -> Result<()> {
    let code: ErrorCode = code.into();
    if let ErrorCode::ExdrOk = code {
        Ok(())
    } else {
        Err(Error::from((code, task)))
    }
}

pub unsafe fn read_xtc_header(
    xd: *mut XDRFILE,
    natoms: *mut ::std::os::raw::c_int,
    step: *mut ::std::os::raw::c_int,
    time: *mut ::std::os::raw::c_float,
) -> Result<()> {
    let code = c_abi::xdrfile_xtc::xtc_header(xd, natoms, step, time, 1);
    check_code(code, ErrorTask::Read)
}

pub unsafe fn read_xtc_coord(
    xd: *mut XDRFILE,
    natoms: *mut ::std::os::raw::c_int,
    step: *mut ::std::os::raw::c_int,
    box_mat: *mut Matrix,
    x: *mut Rvec,
    precision: *mut ::std::os::raw::c_float,
) -> Result<()> {
    let code = c_abi::xdrfile_xtc::xtc_coord(xd, natoms, step, box_mat, x, precision, 1);
    check_code(code, ErrorTask::Read)
}

pub unsafe fn write_xtc_header(
    xd: *mut XDRFILE,
    natoms: *mut ::std::os::raw::c_int,
    step: *mut ::std::os::raw::c_int,
    time: *mut ::std::os::raw::c_float,
) -> Result<()> {
    let code = c_abi::xdrfile_xtc::xtc_header(xd, natoms, step, time, 0);
    check_code(code, ErrorTask::Read)
}

pub unsafe fn write_xtc_coord(
    xd: *mut XDRFILE,
    natoms: *mut ::std::os::raw::c_int,
    step: *mut ::std::os::raw::c_int,
    box_mat: *mut Matrix,
    x: *mut Rvec,
    precision: *mut ::std::os::raw::c_float,
) -> Result<()> {
    let code = c_abi::xdrfile_xtc::xtc_coord(xd, natoms, step, box_mat, x, precision, 0);
    check_code(code, ErrorTask::Read)
}

/// Parts of an XTC frame that do not require allocation
pub struct FrameHeader {
    pub n_atoms: usize,
    pub step: usize,
    pub time: f32,
    pub box_mat: [[f32; 3]; 3],
    pub prec: f32,
}

pub unsafe fn read_xtc(xd: &mut XDRFILE, x: &mut [[f32; 3]]) -> Result<FrameHeader> {
    let mut n_atoms = 0;
    let mut step = 0;
    let mut time = 0.0;
    let mut box_mat = [[0.0; 3]; 3];
    let mut prec = 0.0;

    read_xtc_header(xd, &mut n_atoms, &mut step, &mut time)?;
    read_xtc_coord(
        xd,
        &mut n_atoms,
        &mut step,
        &mut box_mat,
        x.as_mut_ptr(),
        &mut prec,
    )?;

    Ok(FrameHeader {
        n_atoms: n_atoms.try_into().unwrap(),
        step: step.try_into().unwrap(),
        time,
        box_mat,
        prec,
    })
}

pub unsafe fn write_xtc(xd: &mut XDRFILE, x: &mut [[f32; 3]], header: FrameHeader) -> Result<()> {
    let FrameHeader {
        n_atoms,
        step,
        mut time,
        mut box_mat,
        mut prec,
    } = header;
    let mut n_atoms = n_atoms.try_into().unwrap();
    let mut step = step.try_into().unwrap();

    write_xtc_header(xd, &mut n_atoms, &mut step, &mut time)?;
    write_xtc_coord(
        xd,
        &mut n_atoms,
        &mut step,
        &mut box_mat,
        x.as_mut_ptr(),
        &mut prec,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_approx_eq::assert_approx_eq;

    #[test]
    fn test_read_xtc() -> Result<(), Box<dyn std::error::Error>> {
        let path = b"tests/1l2y.xtc".as_ptr() as *const i8;
        let mode = b"r".as_ptr() as *const i8;
        const N_ATOMS: usize = 304;

        let xdr1 = unsafe { &mut *c_abi::xdrfile::xdrfile_open(path, mode) };
        let xdr2 = unsafe { &mut *c_abi::xdrfile::xdrfile_open(path, mode) };

        let mut x1 = [[0.0; 3]; N_ATOMS];
        let header1 = unsafe { read_xtc(xdr1, &mut x1)? };

        let natoms2 = N_ATOMS as i32;
        let mut time2: f32 = 0.0;
        let mut step2: i32 = 0;
        let box_mat2: Matrix = [[0.0; 3]; 3];
        let x2: Vec<Rvec> = vec![[0.0, 0.0, 0.0]; N_ATOMS];
        let mut prec2: f32 = 0.0;

        unsafe {
            let read_code = c_abi::xdrfile_xtc::read_xtc(
                xdr2,
                natoms2,
                &mut step2,
                &mut time2,
                box_mat2.as_ptr() as *mut Matrix,
                x2.as_ptr() as *mut Rvec,
                &mut prec2,
            );
            assert!(read_code as u32 == c_abi::xdrfile::exdrOK);
            c_abi::xdrfile::xdrfile_close(xdr2);
        }

        // make sure everything is still the same
        assert_eq!(step2 as usize, header1.step);
        assert_eq!(natoms2 as usize, header1.n_atoms);
        assert_approx_eq!(time2, header1.time);
        assert_eq!(box_mat2, header1.box_mat);
        assert_approx_eq!(prec2, header1.prec);
        assert_eq!(x2, x1);
        Ok(())
    }
}
