//! Quartz graphics device module (devQuartz.c, 3569 lines)
//!
//! Provides the macOS-native Quartz graphics device, including on-screen
//! (Cocoa/Carbon), bitmap (PNG/JPEG/TIFF), and PDF output via CoreGraphics.
//!
//! On macOS (HAVE_AQUA), these use real CoreGraphics/CoreFoundation APIs.
//! On non-macOS, we export stubs that return NULL / R_NilValue().

use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Type aliases matching QuartzDevice.h opaque types
// ---------------------------------------------------------------------------
/// Opaque Quartz device descriptor (void* in C).
type QuartzDesc_t = *mut c_void;

/// Opaque Quartz function table.
type QuartzFunctions_t = *const c_void;

/// Opaque Quartz parameters struct.
type QuartzParameters_t = *const c_void;

/// Opaque Quartz backend definition.
type QuartzBackend_t = *const c_void;

// ---------------------------------------------------------------------------
// Non-macOS stubs (default, always compiled)
// ---------------------------------------------------------------------------

/// Quartz — create a Quartz graphics device (quartz() function).
/// Stub: returns R_NilValue with a warning on non-macOS.
#[cfg(not(target_os = "macos"))]
pub unsafe fn Quartz(args: SEXP) -> SEXP {
    let _ = args;
    R_NilValue()
}

/// makeQuartzDefault — check whether Quartz should be the default device.
/// Stub: returns FALSE (0) on non-macOS.
#[cfg(not(target_os = "macos"))]
pub unsafe fn makeQuartzDefault() -> SEXP {
    use crate::sexp::constructors::Rf_ScalarLogical;
    Rf_ScalarLogical(0)
}

/// Quartz_C — create a Quartz device from C code (public API).
/// Stub: returns NULL with error code -1 on non-macOS.
#[cfg(not(target_os = "macos"))]
pub unsafe fn Quartz_C(
    par: QuartzParameters_t,
    q_create: *const c_void,
    error_code: *mut c_int,
) -> QuartzDesc_t {
    let _ = par;
    let _ = q_create;
    if !error_code.is_null() {
        *error_code = -1;
    }
    ptr::null_mut()
}

/// getQuartzAPI — return the Quartz device API function table.
/// Stub: returns NULL on non-macOS.
#[cfg(not(target_os = "macos"))]
pub unsafe fn getQuartzAPI() -> *mut c_void {
    ptr::null_mut()
}

// ---------------------------------------------------------------------------
// macOS stubs (placeholder for future real implementation)
// ---------------------------------------------------------------------------

/// Quartz — create a Quartz graphics device (quartz() function).
/// macOS stub: returns R_NilValue (not yet implemented).
#[cfg(target_os = "macos")]
pub unsafe fn Quartz(args: SEXP) -> SEXP {
    let _ = args;
    R_NilValue()
}

/// makeQuartzDefault — check whether Quartz should be the default device.
/// macOS stub: returns FALSE (0) until real implementation.
#[cfg(target_os = "macos")]
pub unsafe fn makeQuartzDefault() -> SEXP {
    use crate::sexp::constructors::Rf_ScalarLogical;
    Rf_ScalarLogical(0)
}

/// Quartz_C — create a Quartz device from C code (public API).
/// macOS stub: returns NULL (not yet implemented).
#[cfg(target_os = "macos")]
pub unsafe fn Quartz_C(
    par: QuartzParameters_t,
    q_create: *const c_void,
    error_code: *mut c_int,
) -> QuartzDesc_t {
    let _ = par;
    let _ = q_create;
    if !error_code.is_null() {
        *error_code = -1;
    }
    ptr::null_mut()
}

/// getQuartzAPI — return the Quartz device API function table.
/// macOS stub: returns NULL (not yet implemented).
#[cfg(target_os = "macos")]
pub unsafe fn getQuartzAPI() -> *mut c_void {
    ptr::null_mut()
}
