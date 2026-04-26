//! Quartz graphics device module (devQuartz.c, 3569 lines)
//!
//! Provides the macOS-native Quartz graphics device, including on-screen
//! (Cocoa/Carbon), bitmap (PNG/JPEG/TIFF), and PDF output via CoreGraphics.
//!
//! This file currently exposes unsupported entry points with explicit errors.
//! Quartz default probing still returns false, and the API accessor returns NULL.

use std::os::raw::{c_int, c_void};
use std::ptr;

use crate::main::errors::Rf_error_unimplemented;
use crate::sexp::constructors::Rf_ScalarLogical;
use crate::sexp::ffi::SEXP;

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

fn unsupported(name: &str) -> ! {
    Rf_error_unimplemented(name);
    unreachable!("Rf_error_unimplemented returned");
}

/// Quartz — create a Quartz graphics device (quartz() function).
pub unsafe fn Quartz(args: SEXP) -> SEXP {
    let _ = args;
    unsupported("grDevices::quartz")
}

/// makeQuartzDefault — check whether Quartz should be the default device.
pub unsafe fn makeQuartzDefault() -> SEXP {
    unsafe { Rf_ScalarLogical(0) }
}

/// Quartz_C — create a Quartz device from C code (public API).
pub unsafe fn Quartz_C(
    par: QuartzParameters_t,
    q_create: *const c_void,
    error_code: *mut c_int,
) -> QuartzDesc_t {
    let _ = (par, q_create, error_code);
    unsupported("grDevices::Quartz_C")
}

/// getQuartzAPI — return the Quartz device API function table.
pub unsafe fn getQuartzAPI() -> *mut c_void {
    ptr::null_mut()
}
