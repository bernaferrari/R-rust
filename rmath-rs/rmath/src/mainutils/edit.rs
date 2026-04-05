#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/edit.c — edit() function.
//!
//! Provides do_edit for interactive editing of R objects.
//! Currently stubbed since it depends on the parser, file I/O, and system editor.

use std::os::raw::c_int;
use std::ptr;

use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

/// Default file name for editing.
static mut DefaultFileName: *mut std::os::raw::c_char = ptr::null_mut();

/// Whether the edit file has been used.
static mut EdFileUsed: c_int = 0;

/// Initialize the edit subsystem.
pub unsafe fn InitEd() {
    // Stub: no temp file management needed
}

/// Clean up the edit subsystem.
pub unsafe fn CleanEd() {
    // Stub: no temp file to clean
}

/// Edit an R object (stub).
///
/// This is the equivalent of R's `do_edit()` from edit.c.
/// In the full implementation, this:
/// - Deparses the object to a temp file
/// - Invokes the system editor
/// - Re-parses the edited file
/// - Returns the result
pub unsafe fn do_edit(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Stub: return R_NilValue
        R_NilValue()
    }
}

/// R_EditFiles stub (may conflict with system.rs, so keep private).
pub(crate) unsafe fn R_EditFiles(
    _nfiles: c_int,
    _files: *mut *mut std::os::raw::c_char,
    _editor: *mut std::os::raw::c_char,
) -> c_int {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_do_edit_null() {
        unsafe {
            let result = do_edit(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_init_ed() {
        unsafe {
            InitEd();
        }
    }
}
