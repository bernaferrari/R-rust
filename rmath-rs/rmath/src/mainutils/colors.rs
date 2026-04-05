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
// Static dispatch table (module-level mutable state)
// ---------------------------------------------------------------------------

/// Function pointer for the RGBpar3 implementation in grDevices.
static mut ptr_RGBpar3: Option<F1> = None;

/// Function pointer for the col2name implementation in grDevices.
static mut ptr_col2name: Option<F2> = None;

/// Function pointer for the R_GE_str2col implementation in grDevices.
static mut ptr_R_GE_str2col: Option<F3> = None;

/// Function pointer for the savePalette implementation in grDevices.
static mut ptr_savePalette: Option<F4> = None;

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
    unsafe {
        ptr_RGBpar3 = f1;
        ptr_col2name = f2;
        ptr_R_GE_str2col = f3;
        ptr_savePalette = f4;
    }
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
/// `ptr_col2name` must have been set via `Rg_set_col_ptrs`.
pub unsafe fn col2name(col: std::os::raw::c_uint) -> *const c_char {
    unsafe {
        match ptr_col2name {
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
/// `ptr_R_GE_str2col` must have been set via `Rg_set_col_ptrs`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GE_str2col(s: *const c_char) -> std::os::raw::c_uint {
    unsafe {
        match ptr_R_GE_str2col {
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
/// `ptr_savePalette` must have been set via `Rg_set_col_ptrs`.
pub unsafe fn savePalette(save: c_int) {
    unsafe {
        match ptr_savePalette {
            Some(f) => f(save),
            None => {}
        }
    }
}

// ---------------------------------------------------------------------------
// SEXP-dependent stubs
// ---------------------------------------------------------------------------

/// Stub for `RGBpar3(SEXP x, int i, unsigned int bg) -> unsigned int`.
///
/// Used in grid/src/gpar.c with `bg = R_TRANWHITE`, and in packages Cairo,
/// canvas, and jpeg.  Depends on `SEXP` and the grDevices function pointer.
///
/// Port of `RGBpar3` in colors.c.
///
/// # Safety
/// This is a stub that always returns 0.  The real implementation requires
/// the full R runtime and `SEXP` support.
pub unsafe fn RGBpar3(
    _x: *mut c_void,
    _i: c_int,
    _bg: std::os::raw::c_uint,
) -> std::os::raw::c_uint {
    0
}

/// Stub for `RGBpar(SEXP x, int i) -> unsigned int`.
///
/// Convenience wrapper that calls `RGBpar3` with `bg = R_TRANWHITE`.
/// Depends on `SEXP`.
///
/// Port of `RGBpar` in colors.c.
///
/// # Safety
/// This is a stub that always returns 0.  The real implementation requires
/// the full R runtime and `SEXP` support.
pub unsafe fn RGBpar(_x: *mut c_void, _i: c_int) -> std::os::raw::c_uint {
    0
}
