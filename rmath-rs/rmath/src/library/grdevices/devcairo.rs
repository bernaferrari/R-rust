//! Cairo graphics device module (devCairo.c, 94 lines)
//!
//! Provides Cairo device initialization by dynamically loading the
//! cairo shared library (libcairo on Unix, winCairo.dll on Windows).
//!
//! On Unix/macOS, this loads the cairo package via R_cairoCdynload.
//! The actual Cairo device is in the separate cairo package.
//!
//! Exported functions:
//!   devCairo(SEXP args) -> SEXP
//!   cairoVersion() -> SEXP (character string)
//!   pangoVersion() -> SEXP (character string)
//!   cairoFT() -> SEXP (character string)
//!
//! Note: On Windows, devCairo/cairoVersion/pangoVersion/cairoFT are
//! provided by devwindows.rs instead (which loads winCairo.dll).

use std::os::raw::c_char;

use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// devCairo — create a Cairo graphics device
// ---------------------------------------------------------------------------

/// Create a Cairo graphics device by loading the cairo shared library.
/// Stub: returns R_NilValue (cairo library not available).
#[cfg(not(target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devCairo(args: SEXP) -> SEXP {
    let _ = args;
    R_NilValue()
}

// ---------------------------------------------------------------------------
// cairoVersion — return the Cairo library version string
// ---------------------------------------------------------------------------

/// Return the Cairo library version string, or "" if not available.
/// Stub: returns empty string.
#[cfg(not(target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cairoVersion() -> SEXP {
    use crate::sexp::constructors::Rf_mkString;
    Rf_mkString(b"\0".as_ptr() as *const c_char)
}

// ---------------------------------------------------------------------------
// pangoVersion — return the Pango library version string
// ---------------------------------------------------------------------------

/// Return the Pango library version string, or "" if not available.
/// Stub: returns empty string.
#[cfg(not(target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pangoVersion() -> SEXP {
    use crate::sexp::constructors::Rf_mkString;
    Rf_mkString(b"\0".as_ptr() as *const c_char)
}

// ---------------------------------------------------------------------------
// cairoFT — return Cairo FreeType information
// ---------------------------------------------------------------------------

/// Return Cairo FreeType information, or "" if not available.
/// Stub: returns empty string.
#[cfg(not(target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cairoFT() -> SEXP {
    use crate::sexp::constructors::Rf_mkString;
    Rf_mkString(b"\0".as_ptr() as *const c_char)
}
