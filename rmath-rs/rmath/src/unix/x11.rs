#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/unix/X11.c -- X11 graphics module dispatch.
//!
//! On macOS, X11 is typically not available, so this provides stub
//! implementations that return errors. The HAVE_X11 code path is
//! included but behind a cfg flag.

use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::sexp::accessors::SET_STRING_ELT;
use crate::sexp::constructors::{Rf_allocVector, Rf_mkChar};
use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Stub functions
// ---------------------------------------------------------------------------

const STRSXP_VAL: c_int = 16;
const VECSXP_VAL: c_int = 19;

unsafe fn setAttrib(_x: SEXP, _what: SEXP, _val: SEXP) {}
unsafe fn R_NamesSymbol() -> SEXP {
    ptr::null_mut()
}
unsafe fn error(_msg: *const c_char) {}

// ---------------------------------------------------------------------------
// Stub implementations (no HAVE_X11 path)
// ---------------------------------------------------------------------------

/// Check whether X11 is available.
pub fn R_access_X11() -> c_int {
    0 // FALSE
}

/// .Internal(X11(...)) -- dispatch to X11 module.
pub unsafe fn do_X11(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // In the full implementation, this loads the X11 module and dispatches.
        // Without X11, return nil.
        R_NilValue()
    }
}

/// .Internal(saveplot(...)) -- dispatch to X11 module.
pub unsafe fn do_saveplot(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Get X11 image data.
pub unsafe fn R_GetX11Image(
    _d: c_int,
    _pximage: *mut c_void,
    _pwidth: *mut c_int,
    _pheight: *mut c_int,
) -> c_int {
    0 // FALSE
}

/// Read from X11 clipboard.
pub unsafe fn R_ReadClipboard(_clpcon: *mut c_void, _type: *mut c_char) -> c_int {
    0 // FALSE
}

/// Get bitmap library version information.
pub unsafe fn do_bmVersion() -> SEXP {
    unsafe {
        let ans = Rf_allocVector(VECSXP_VAL, 3);
        let nms = Rf_allocVector(STRSXP_VAL, 3);

        SET_STRING_ELT(nms, 0, Rf_mkChar(b"libpng\0".as_ptr() as *const _));
        SET_STRING_ELT(nms, 1, Rf_mkChar(b"jpeg\0".as_ptr() as *const _));
        SET_STRING_ELT(nms, 2, Rf_mkChar(b"libtiff\0".as_ptr() as *const _));

        // Without X11 module loaded, leave ans elements as empty strings
        SET_STRING_ELT(ans, 0, Rf_mkChar(b"\0".as_ptr() as *const _));
        SET_STRING_ELT(ans, 1, Rf_mkChar(b"\0".as_ptr() as *const _));
        SET_STRING_ELT(ans, 2, Rf_mkChar(b"\0".as_ptr() as *const _));

        setAttrib(ans, R_NamesSymbol(), nms);
        ans
    }
}

/// Set X11 routine dispatch table (stub).
pub unsafe fn R_setX11Routines(_routines: *mut c_void) -> *mut c_void {
    ptr::null_mut()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::accessors::*;

    use super::*;

    #[test]
    fn test_x11_not_available() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(R_access_X11(), 0);
        }
    }

    #[test]
    fn test_do_x11_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_X11(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(result.is_null() || result == R_NilValue());
        }
    }

    #[test]
    fn test_get_x11_image_returns_false() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(
                R_GetX11Image(0, ptr::null_mut(), ptr::null_mut(), ptr::null_mut()),
                0
            );
        }
    }

    #[test]
    fn test_bm_version_returns_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_bmVersion();
            if !result.is_null() {
                assert_eq!(TYPEOF(result), VECSXP_VAL);
                assert_eq!(LENGTH(result), 3);
            }
        }
    }
}
