//! Windows bitmap device module (winbitmap.c)
//!
//! Provides R_SaveAsPng, R_SaveAsJpeg, R_SaveAsTIFF, R_SaveAsBmp,
//! R_pngVersion, R_jpegVersion, R_tiffVersion.
//!
//! The original C implementation uses libpng, libjpeg, and libtiff
//! for real image output on platforms where those libraries are available.
//! It also includes a platform-independent BMP writer (R_SaveAsBmp).
//!
//! PNG/JPEG/TIFF return failure when their external codec libraries are not
//! linked. BMP is implemented in pure Rust and works cross-platform.
//!
//! Ported from r-source/src/library/grDevices/src/winbitmap.c

use std::os::raw::{c_char, c_int, c_uint};
use std::ptr;

// ---------------------------------------------------------------------------
// Cross-platform stubs (always compiled, always exported)
// ---------------------------------------------------------------------------
// The original C code conditionally compiles real implementations under
// HAVE_PNG, HAVE_JPEG, HAVE_TIFF macros. Since we don't link against
// those libraries, all implementations are stubs that return failure.
//
// The BMP writer is platform-independent in the C source. This module delegates
// to the shared pure-Rust implementation used by the X11 bitmap path.

/// Save device contents as PNG.
/// Returns 1 on success, 0 on failure.
/// Stub: always returns 0 (failure) since libpng is not linked.
pub unsafe fn R_SaveAsPng(
    _d: *mut std::ffi::c_void,
    _width: c_int,
    _height: c_int,
    _gp: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, c_int) -> c_uint>,
    _bgr: c_int,
    _fp: *mut libc::FILE,
    _transparent: c_uint,
    _res: c_int,
) -> c_int {
    0
}

/// Save device contents as JPEG.
/// Returns 1 on success, 0 on failure.
/// Stub: always returns 0 (failure) since libjpeg is not linked.
pub unsafe fn R_SaveAsJpeg(
    _d: *mut std::ffi::c_void,
    _width: c_int,
    _height: c_int,
    _gp: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, c_int) -> c_uint>,
    _bgr: c_int,
    _quality: c_int,
    _outfile: *mut libc::FILE,
    _res: c_int,
) -> c_int {
    0
}

/// Save device contents as TIFF.
/// Returns 1 on success, 0 on failure.
/// Stub: always returns 0 (failure) since libtiff is not linked.
pub unsafe fn R_SaveAsTIFF(
    _d: *mut std::ffi::c_void,
    _width: c_int,
    _height: c_int,
    _gp: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, c_int) -> c_uint>,
    _bgr: c_int,
    _outfile: *const c_char,
    _res: c_int,
    _compression: c_int,
) -> c_int {
    0
}

/// Save device contents as Windows BMP.
/// Returns 1 on success, 0 on failure.
///
/// The real C implementation is platform-independent and writes a BMP
/// file with optional palette (256 colors) or 24-bit truecolor mode.
/// It handles palette construction via binary search, writes the BMP
/// header (54 bytes), palette entries, and pixel data with proper padding.
pub unsafe fn R_SaveAsBmp(
    d: *mut std::ffi::c_void,
    width: c_int,
    height: c_int,
    gp: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, c_int) -> c_uint>,
    bgr: c_int,
    fp: *mut libc::FILE,
    res: c_int,
) -> c_int {
    unsafe { crate::modules::x11::rbitmap::save_as_bmp(d, width, height, gp, bgr, fp.cast(), res) }
}

/// Return the libpng version string, or "" if not available.
pub fn R_pngVersion() -> *const c_char {
    static VERSION: [c_char; 1] = [0];
    VERSION.as_ptr()
}

/// Return the libjpeg version string, or "" if not available.
pub fn R_jpegVersion() -> *const c_char {
    static VERSION: [c_char; 1] = [0];
    VERSION.as_ptr()
}

/// Return the libtiff version string, or "" if not available.
pub fn R_tiffVersion() -> *const c_char {
    static VERSION: [c_char; 1] = [0];
    VERSION.as_ptr()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;

    unsafe extern "C" fn test_pixel(_device: *mut c_void, row: c_int, col: c_int) -> c_uint {
        match (row, col) {
            (0, 0) => 0x00ff0000,
            (0, 1) => 0x0000ff00,
            (1, 0) => 0x000000ff,
            _ => 0x00ffffff,
        }
    }

    unsafe fn read_tmpfile(fp: *mut libc::FILE) -> Vec<u8> {
        unsafe {
            libc::fflush(fp);
            libc::fseek(fp, 0, libc::SEEK_SET);
            let mut output = Vec::new();
            let mut buffer = [0u8; 256];
            loop {
                let read = libc::fread(buffer.as_mut_ptr().cast(), 1, buffer.len(), fp);
                if read == 0 {
                    break;
                }
                output.extend_from_slice(&buffer[..read]);
            }
            output
        }
    }

    #[test]
    fn save_as_bmp_writes_cross_platform_bitmap() {
        unsafe {
            let fp = libc::tmpfile();
            assert!(!fp.is_null());

            let status = R_SaveAsBmp(std::ptr::null_mut(), 2, 2, Some(test_pixel), 0, fp, 72);
            assert_eq!(status, 1);

            let bytes = read_tmpfile(fp);
            libc::fclose(fp);

            assert_eq!(&bytes[0..2], b"BM");
            let file_size = u32::from_le_bytes(bytes[2..6].try_into().unwrap()) as usize;
            assert_eq!(file_size, bytes.len());
            assert_eq!(u32::from_le_bytes(bytes[18..22].try_into().unwrap()), 2);
            assert_eq!(u32::from_le_bytes(bytes[22..26].try_into().unwrap()), 2);
            assert_eq!(u16::from_le_bytes(bytes[28..30].try_into().unwrap()), 8);
        }
    }

    #[test]
    fn save_as_bmp_rejects_invalid_inputs() {
        unsafe {
            assert_eq!(
                R_SaveAsBmp(
                    std::ptr::null_mut(),
                    0,
                    2,
                    Some(test_pixel),
                    0,
                    std::ptr::null_mut(),
                    72,
                ),
                0
            );
        }
    }
}
