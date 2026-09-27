#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/edit.c — edit() function.
//!
//! Provides the editing entry points used by utils.
//!
//! Android and embedded runtimes do not have a process-wide interactive editor
//! contract. Instead of silently returning `NULL`, unsupported editor calls fail
//! with an R error so callers can recover explicitly.

use std::os::raw::c_int;

use crate::sexp::context::RError;
use crate::sexp::ffi::SEXP;

/// Initialize the edit subsystem.
pub unsafe fn InitEd() {
    // no temp file management needed
}

/// Clean up the edit subsystem.
pub unsafe fn CleanEd() {
    // no temp file to clean
}

fn edit_unavailable() -> ! {
    std::panic::panic_any(RError {
        message: "edit() is not available in the Android/headless runtime".to_string(),
    });
}

/// GNU `do_edit`: deparse `x` into `file`, run `editor`, parse and eval the result.
/// `.External2(C_edit, x, file, title, editor)`.
pub unsafe fn do_edit(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, INTEGER, TYPEOF};
        use crate::sexp::constructors::{Rf_lang2, Rf_lang3};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::{R_GlobalEnv, R_NilValue};
        use crate::sexp::symbol::Rf_install;
        // Skip the .NAME cell when invoked through .External2.
        if args.is_null() || args == R_NilValue() {
            crate::main::errors::errorcall(call, c"invalid argument to edit()".as_ptr());
        }
        let mut cell = args;
        let head = CAR(cell);
        if TYPEOF(head) == SEXPTYPE::SYMSXP || TYPEOF(head) == SEXPTYPE::STRSXP {
            cell = CDR(cell);
        }
        let x = CAR(cell);
        cell = CDR(cell);
        let mut file = CAR(cell);
        let file_empty = file.is_null()
            || file == R_NilValue()
            || (TYPEOF(file) == SEXPTYPE::STRSXP
                && (crate::sexp::accessors::XLENGTH(file) == 0
                    || std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(
                        crate::sexp::accessors::STRING_ELT(file, 0),
                    ))
                    .to_bytes()
                    .is_empty()));
        if file_empty {
            let path = std::env::temp_dir().join(format!("redit-{}", std::process::id()));
            let cpath = std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap_or_default();
            file = crate::sexp::constructors::Rf_mkString(cpath.as_ptr());
        }
        cell = CDR(CDR(cell));
        let editor = CAR(cell);
        let env = if rho.is_null() || rho == R_NilValue() {
            R_GlobalEnv()
        } else {
            rho
        };
        if !x.is_null() && x != R_NilValue() {
            let deparse = Rf_lang2(Rf_install(c"deparse".as_ptr()), x);
            let src = crate::eval::eval::Rf_eval(deparse, env);
            let write = Rf_lang3(
                Rf_install(c"writeLines".as_ptr()),
                src,
                file,
            );
            let _ = crate::eval::eval::Rf_eval(write, env);
        }
        let status = Rf_lang3(
            Rf_install(c"system2".as_ptr()),
            editor,
            file,
        );
        let rc = crate::eval::eval::Rf_eval(status, env);
        if TYPEOF(rc) == SEXPTYPE::INTSXP && crate::sexp::accessors::INTEGER(rc).read() != 0 {
            crate::main::errors::errorcall(call, c"problem running editor".as_ptr());
        }
        let parsed = crate::eval::eval::Rf_eval(
            Rf_lang2(Rf_install(c"parse".as_ptr()), file),
            env,
        );
        crate::eval::eval::Rf_eval(
            Rf_lang2(Rf_install(c"eval".as_ptr()), parsed),
            R_GlobalEnv(),
        )
    }
}

/// Private edit-files hook for the legacy utils boundary.
pub(crate) unsafe fn R_EditFiles(
    _nfiles: c_int,
    _files: *mut *mut std::os::raw::c_char,
    _editor: *mut std::os::raw::c_char,
) -> c_int {
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_do_edit_rejects_missing_args() {
        let result = std::panic::catch_unwind(|| unsafe {
            do_edit(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_init_ed() {
        unsafe {
            InitEd();
        }
    }
}
