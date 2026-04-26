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

use crate::main::errors::Rf_error_unimplemented;
use crate::sexp::ffi::SEXP;

fn unsupported(name: &str) -> ! {
    Rf_error_unimplemented(name);
    unreachable!("Rf_error_unimplemented returned");
}

// ---------------------------------------------------------------------------
// devCairo — create a Cairo graphics device
// ---------------------------------------------------------------------------

/// Create a Cairo graphics device by loading the cairo shared library.
/// Stub: reports that Cairo support is unavailable on this target.
#[cfg(not(target_os = "windows"))]
pub unsafe fn devCairo(args: SEXP) -> SEXP {
    let _ = args;
    unsupported("grDevices::devCairo")
}

// ---------------------------------------------------------------------------
// cairoVersion — return the Cairo library version string
// ---------------------------------------------------------------------------

/// Return the Cairo library version string, or report that it is unavailable.
#[cfg(not(target_os = "windows"))]
pub unsafe fn cairoVersion() -> SEXP {
    unsupported("grDevices::cairoVersion")
}

// ---------------------------------------------------------------------------
// pangoVersion — return the Pango library version string
// ---------------------------------------------------------------------------

/// Return the Pango library version string, or report that it is unavailable.
#[cfg(not(target_os = "windows"))]
pub unsafe fn pangoVersion() -> SEXP {
    unsupported("grDevices::pangoVersion")
}

// ---------------------------------------------------------------------------
// cairoFT — return Cairo FreeType information
// ---------------------------------------------------------------------------

/// Return Cairo FreeType information, or report that it is unavailable.
#[cfg(not(target_os = "windows"))]
pub unsafe fn cairoFT() -> SEXP {
    unsupported("grDevices::cairoFT")
}
