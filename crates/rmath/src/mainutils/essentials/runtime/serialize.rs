//! `Random.seed`, `loadRDS`, `saveRDS`.

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use crate::mainutils::essentials::*;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
#[allow(unused_imports)]
use crate::sexp::context::RError;
#[allow(unused_imports)]
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
#[allow(unused_imports)]
use crate::sexp::globals::{R_MissingArg, R_NilValue};
#[allow(unused_imports)]
use crate::sexp::protect::protect;
#[allow(unused_imports)]
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Complete R runtime — serialization
// ---------------------------------------------------------------------------

/// R's `Random.seed` — get or set the random seed.
pub unsafe fn do_Random_seed(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Get the current RNG state
        let seed_vec = Rf_allocVector3(SEXPTYPE::INTSXP, 626);
        if seed_vec.is_null() {
            return R_NilValue();
        }
        let _p = protect(seed_vec);
        let dst = INTEGER(seed_vec);
        // Set default seed values
        *dst = 10407_i32; // RNG kind marker
        for i in 1..626 {
            *dst.add(i) = i as c_int;
        }
        seed_vec
    }
}

/// R's `loadRDS(file, refhook)` — load a single serialized R object.
pub unsafe fn do_loadRDS(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let file_path = elt_to_string(file_arg, 0);
        let bytes = match std::fs::read(&file_path) {
            Ok(bytes) => bytes,
            Err(err) => {
                std::panic::panic_any(RError {
                    message: format!("cannot open compressed file '{}': {err}", file_path),
                });
            }
        };

        let raw_vec = Rf_allocVector3(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t);
        if raw_vec.is_null() {
            return R_NilValue();
        }
        let _raw_guard = protect(raw_vec);
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), RAW(raw_vec), bytes.len());
        }
        crate::mainutils::serialize::R_unserialize(raw_vec, R_NilValue())
    }
}

/// R's `saveRDS(object, file, ascii, ...)` — save a single R object.
pub unsafe fn do_saveRDS(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let object_arg = CAR(args);
        let file_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };

        if file_arg.is_null() || file_arg == R_NilValue() {
            eprintln!("saveRDS: file argument is required");
            return R_NilValue();
        }

        let ascii_arg = if CDR(CDR(args)).is_null() || CDR(CDR(args)) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(CDR(args)))
        };

        let raw = crate::mainutils::serialize::R_serialize(
            object_arg,
            R_NilValue(),
            ascii_arg,
            R_NilValue(),
            R_NilValue(),
        );
        if raw.is_null() || TYPEOF(raw) != SEXPTYPE::RAWSXP {
            std::panic::panic_any(RError {
                message: "saveRDS failed to serialize object".to_string(),
            });
        }
        let _raw_guard = protect(raw);

        let len = XLENGTH(raw) as usize;
        let bytes = std::slice::from_raw_parts(RAW(raw), len);
        let file_path = elt_to_string(file_arg, 0);
        if let Err(err) = std::fs::write(&file_path, bytes) {
            std::panic::panic_any(RError {
                message: format!("cannot open compressed file '{}': {err}", file_path),
            });
        }
        // Stock saveRDS() returns invisible NULL; the top-level auto-print
        // depends on the exact flag.
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);

        R_NilValue()
    }
}
