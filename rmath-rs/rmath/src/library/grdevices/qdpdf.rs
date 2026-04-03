#![allow(unsafe_op_in_unsafe_fn)]
//! Quartz PDF device module (qdPDF.c)
//!
//! Provides QuartzPDF_DeviceCreate for macOS PDF output using CoreGraphics.
//! On non-macOS platforms we export a stub that returns NULL.

use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;

// ---------------------------------------------------------------------------
// Type aliases matching QuartzDevice.h opaque types
// ---------------------------------------------------------------------------
/// Opaque device descriptor (void* in C).
type QuartzDesc_t = *mut c_void;

/// Opaque function table pointer (passed in by the Quartz device framework).
type QuartzFunctions_t = *const c_void;

/// Opaque parameters struct (defined in QuartzDevice.h).
type QuartzParameters_t = *const c_void;

/// QuartzPDF_GetCGContext - returns the PDF drawing context.
/// Stub: returns null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QuartzPDF_GetCGContext(
    _dev: QuartzDesc_t,
    _user_info: *mut c_void,
) -> *mut c_void {
    ptr::null_mut()
}

/// QuartzPDF_NewPage - handles page breaks.
/// Stub: no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QuartzPDF_NewPage(
    _dev: QuartzDesc_t,
    _user_info: *mut c_void,
    _flags: c_int,
) {
}

/// QuartzPDF_Close - cleanup and release resources.
/// Stub: no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QuartzPDF_Close(_dev: QuartzDesc_t, _user_info: *mut c_void) {}

/// QuartzPDF_DeviceCreate - creates the PDF device.
/// Stub: returns null (device creation failed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QuartzPDF_DeviceCreate(
    _dd: *mut c_void,
    _fn: QuartzFunctions_t,
    _par: QuartzParameters_t,
) -> QuartzDesc_t {
    ptr::null_mut()
}
