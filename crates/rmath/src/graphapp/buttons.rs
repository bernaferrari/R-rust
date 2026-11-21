#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

//! Button-related functions for GraphApp.

use super::controls;
use super::types::*;
use super::windows;

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn clickbutton(w: window, b: button) {
    if !w.is_null() {
        windows::set_current_window(w);
    }

    // SAFETY: forwarded raw pointers satisfy `activatecontrol`'s contract per this function's own `# Safety` requirements.
    unsafe { controls::activatecontrol(b) };
}
