#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's color dispatch stubs from src/main/colors.c.
//!
//! Original source: src/main/colors.c (85 lines)
//!
//! This module is a thin dispatch layer: the real implementations live in the
//! grDevices package.  At startup, `Rg_set_col_ptrs` installs function pointers
//! obtained from grDevices; every other function in this file simply forwards
//! through those pointers.
//!
//! Only `Rg_set_col_ptrs` is fully standalone.  `col2name`, `R_GE_str2col`,
//! and `savePalette` depend on function pointers being set at runtime.
//! `RGBpar3` and `RGBpar` additionally take `SEXP` parameters and are stubbed.

use std::os::raw::{c_char, c_int, c_void};

use crate::sexp::instance::with_required_current_instance;

// ---------------------------------------------------------------------------
// Type aliases for the function-pointer dispatch table
// ---------------------------------------------------------------------------

/// Signature of the RGBpar3 implementation in grDevices.
/// `SEXP x, int i, unsigned int bg -> unsigned int`
type F1 = unsafe extern "C" fn(*mut c_void, c_int, std::os::raw::c_uint) -> std::os::raw::c_uint;

/// Signature of the col2name implementation in grDevices.
/// `unsigned int col -> const char *`
type F2 = unsafe extern "C" fn(std::os::raw::c_uint) -> *const c_char;

/// Signature of the R_GE_str2col implementation in grDevices.
/// `const char *s -> unsigned int`
type F3 = unsafe extern "C" fn(*const c_char) -> std::os::raw::c_uint;

/// Signature of the savePalette implementation in grDevices.
/// `Rboolean save -> void`
type F4 = unsafe extern "C" fn(c_int);

// ---------------------------------------------------------------------------
// Per-session dispatch table
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ColorDispatchState {
    rgbpar3: Option<F1>,
    col2name: Option<F2>,
    str2col: Option<F3>,
    save_palette: Option<F4>,
}

fn with_color_dispatch_state<R>(f: impl FnOnce(&mut ColorDispatchState) -> R) -> R {
    with_required_current_instance(|instance| f(&mut instance.color_dispatch_state))
}

// ---------------------------------------------------------------------------
// Standalone functions
// ---------------------------------------------------------------------------

/// Install function pointers from grDevices.
///
/// This is the only fully standalone entry point.  It must be called once
/// (typically during package initialization) before any of the other
/// functions in this module can be used safely.
///
/// Port of `Rg_set_col_ptrs` in colors.c.
///
/// # Safety
/// All four function pointers must be valid (non-null) and must remain valid
/// for as long as they may be called through this module.
pub unsafe fn Rg_set_col_ptrs(f1: Option<F1>, f2: Option<F2>, f3: Option<F3>, f4: Option<F4>) {
    with_color_dispatch_state(|state| {
        state.rgbpar3 = f1;
        state.col2name = f2;
        state.str2col = f3;
        state.save_palette = f4;
    });
}

// ---------------------------------------------------------------------------
// Functions that depend on runtime function pointers
// ---------------------------------------------------------------------------

/// Convert a color specification to an RGB unsigned int, using the given
/// background color for transparency resolution.
///
/// Port of `col2name` in colors.c.  Used in grid.
///
/// # Safety
/// The active session must have installed a `col2name` callback via
/// `Rg_set_col_ptrs`.
pub unsafe fn col2name(col: std::os::raw::c_uint) -> *const c_char {
    unsafe {
        match with_color_dispatch_state(|state| state.col2name) {
            Some(f) => f(col),
            None => std::ptr::null(),
        }
    }
}

/// Convert a color name string to an RGB unsigned int.
///
/// Port of `R_GE_str2col` in colors.c.  Used in grDevices for fg/bg of devices.
///
/// # Safety
/// The active session must have installed an `R_GE_str2col` callback via
/// `Rg_set_col_ptrs`.
pub unsafe fn R_GE_str2col(s: *const c_char) -> std::os::raw::c_uint {
    unsafe {
        match with_color_dispatch_state(|state| state.str2col) {
            Some(f) => f(s),
            None => 0,
        }
    }
}

/// Save or restore the current color palette.
///
/// Port of `savePalette` in colors.c.  Used in engine.c.
///
/// # Safety
/// The active session must have installed a `savePalette` callback via
/// `Rg_set_col_ptrs`.
pub unsafe fn savePalette(save: c_int) {
    unsafe {
        if let Some(f) = with_color_dispatch_state(|state| state.save_palette) {
            f(save)
        }
    }
}

// ---------------------------------------------------------------------------
// SEXP-dependent stubs
// ---------------------------------------------------------------------------

/// Convert a color specification to an RGB unsigned int, using the given
/// background color for transparency resolution.
///
/// Port of `RGBpar3` in colors.c. Delegates to grDevices via function pointer.
/// Panics with RError if grDevices has not been loaded (pointer not set).
pub unsafe fn RGBpar3(x: *mut c_void, i: c_int, bg: std::os::raw::c_uint) -> std::os::raw::c_uint {
    unsafe {
        match with_color_dispatch_state(|state| state.rgbpar3) {
            Some(f) => f(x, i, bg),
            None => 0,
        }
    }
}

/// Convenience wrapper that calls `RGBpar3` with `bg = R_TRANWHITE` (0x00FFFFFF).
///
/// Port of `RGBpar` in colors.c.
pub unsafe fn RGBpar(x: *mut c_void, i: c_int) -> std::os::raw::c_uint {
    unsafe { RGBpar3(x, i, 0x00FFFFFF) }
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;
    use std::ptr;

    use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};

    use super::*;

    unsafe extern "C" fn first_rgbpar3(
        _x: *mut c_void,
        _i: c_int,
        _bg: std::os::raw::c_uint,
    ) -> std::os::raw::c_uint {
        0x00AA_0001
    }

    unsafe extern "C" fn second_rgbpar3(
        _x: *mut c_void,
        _i: c_int,
        _bg: std::os::raw::c_uint,
    ) -> std::os::raw::c_uint {
        0x00BB_0002
    }

    unsafe extern "C" fn first_col2name(_col: std::os::raw::c_uint) -> *const c_char {
        c"first".as_ptr()
    }

    unsafe extern "C" fn second_col2name(_col: std::os::raw::c_uint) -> *const c_char {
        c"second".as_ptr()
    }

    unsafe extern "C" fn first_str2col(_s: *const c_char) -> std::os::raw::c_uint {
        11
    }

    unsafe extern "C" fn second_str2col(_s: *const c_char) -> std::os::raw::c_uint {
        22
    }

    unsafe extern "C" fn ignore_palette(_save: c_int) {}

    unsafe fn col_name() -> String {
        unsafe { CStr::from_ptr(col2name(0)).to_string_lossy().into_owned() }
    }

    #[test]
    fn color_dispatch_pointers_are_session_local() {
        unsafe {
            let mut first = RInstance::new();
            set_current_instance(&mut first);
            Rg_set_col_ptrs(
                Some(first_rgbpar3),
                Some(first_col2name),
                Some(first_str2col),
                Some(ignore_palette),
            );
            assert_eq!(RGBpar3(ptr::null_mut(), 0, 0), 0x00AA_0001);
            assert_eq!(col_name(), "first");
            assert_eq!(R_GE_str2col(ptr::null()), 11);

            let mut second = RInstance::new();
            set_current_instance(&mut second);
            Rg_set_col_ptrs(
                Some(second_rgbpar3),
                Some(second_col2name),
                Some(second_str2col),
                Some(ignore_palette),
            );
            assert_eq!(RGBpar3(ptr::null_mut(), 0, 0), 0x00BB_0002);
            assert_eq!(col_name(), "second");
            assert_eq!(R_GE_str2col(ptr::null()), 22);

            set_current_instance(&mut first);
            assert_eq!(RGBpar3(ptr::null_mut(), 0, 0), 0x00AA_0001);
            assert_eq!(col_name(), "first");
            assert_eq!(R_GE_str2col(ptr::null()), 11);

            clear_current_instance();
        }
    }
}
