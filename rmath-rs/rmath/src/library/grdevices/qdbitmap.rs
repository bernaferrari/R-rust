#![allow(unsafe_op_in_unsafe_fn)]
//! Quartz bitmap device module (qdBitmap.c)
//!
//! Provides QuartzBitmap_DeviceCreate for macOS bitmap output using CoreGraphics.
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

/// QuartzBitmap_GetCGContext - returns the bitmap drawing context.
/// Stub: returns null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QuartzBitmap_GetCGContext(
    _dev: QuartzDesc_t,
    _user_info: *mut c_void,
) -> *mut c_void {
    ptr::null_mut()
}

/// QuartzBitmap_Output - saves bitmap to file or clipboard.
/// Stub: no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QuartzBitmap_Output(_dev: QuartzDesc_t, _qbd: *mut c_void) {}

/// QuartzBitmap_NewPage - handles new page.
/// Stub: no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QuartzBitmap_NewPage(
    _dev: QuartzDesc_t,
    _user_info: *mut c_void,
    _flags: c_int,
) {
}

/// QuartzBitmap_Close - cleanup and free device resources.
/// Stub: no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QuartzBitmap_Close(_dev: QuartzDesc_t, _user_info: *mut c_void) {}

/// QuartzBitmap_DeviceCreate - creates the bitmap device.
/// Stub: returns null (device creation failed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QuartzBitmap_DeviceCreate(
    _dd: *mut c_void,
    _fn: QuartzFunctions_t,
    _par: QuartzParameters_t,
) -> QuartzDesc_t {
    ptr::null_mut()
}
