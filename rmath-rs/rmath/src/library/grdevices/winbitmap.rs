#![allow(
    unsafe_op_in_unsafe_fn,
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
//! Windows bitmap device module (winbitmap.c)
//!
//! Provides R_SaveAsPng, R_SaveAsJpeg, R_SaveAsTIFF, R_SaveAsBmp,
//! R_pngVersion, R_jpegVersion, R_tiffVersion.
//!
//! The original C implementation uses libpng, libjpeg, and libtiff
//! for real image output on platforms where those libraries are available.
//! It also includes a platform-independent BMP writer (R_SaveAsBmp).
//!
//! On non-Windows platforms we export stubs that return failure / empty
//! strings so the linker can always find these symbols.
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
// The BMP writer (R_SaveAsBmp) is platform-independent in the C source
// (no #ifdef guard), but it writes BMP files which is a Windows-centric
// format. We still provide a stub.

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
/// Stub: always returns 0 (failure).
///
/// The real C implementation is platform-independent and writes a BMP
/// file with optional palette (256 colors) or 24-bit truecolor mode.
/// It handles palette construction via binary search, writes the BMP
/// header (54 bytes), palette entries, and pixel data with proper padding.
pub unsafe fn R_SaveAsBmp(
    _d: *mut std::ffi::c_void,
    _width: c_int,
    _height: c_int,
    _gp: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, c_int) -> c_uint>,
    _bgr: c_int,
    _fp: *mut libc::FILE,
    _res: c_int,
) -> c_int {
    0
}

/// Return the libpng version string, or "" if not available.
pub unsafe fn R_pngVersion() -> *const c_char {
    static VERSION: [c_char; 1] = [0];
    VERSION.as_ptr()
}

/// Return the libjpeg version string, or "" if not available.
pub unsafe fn R_jpegVersion() -> *const c_char {
    static VERSION: [c_char; 1] = [0];
    VERSION.as_ptr()
}

/// Return the libtiff version string, or "" if not available.
pub unsafe fn R_tiffVersion() -> *const c_char {
    static VERSION: [c_char; 1] = [0];
    VERSION.as_ptr()
}
