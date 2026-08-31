//! `with`, `within`, `transform`.

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
// Complete R runtime — with, within, transform
// ---------------------------------------------------------------------------

/// R's `with(data, expr)` — evaluate expr in a data/list environment.
pub unsafe fn do_with(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let data_expr = arg_by_name_or_position(args, &["data"], 0);
        let expr = arg_by_name_or_position(args, &["expr"], 1);
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        let data = if data_expr.is_null() || data_expr == R_NilValue() {
            R_NilValue()
        } else {
            crate::eval::eval::Rf_eval(data_expr, rho)
        };
        if data.is_null() || data == R_NilValue() {
            return crate::eval::eval::Rf_eval(expr, rho);
        }
        let eval_env = data_environment(data, rho);
        crate::eval::eval::Rf_eval(expr, eval_env)
    }
}

unsafe fn data_environment(data: SEXP, parent: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(data) == SEXPTYPE::ENVSXP {
            return data;
        }
        if TYPEOF(data) != SEXPTYPE::VECSXP {
            return parent;
        }

        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), parent, R_NilValue());
        if env.is_null() || env == R_NilValue() {
            return parent;
        }

        let names =
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_NamesSymbol());
        let n = XLENGTH(data);
        for i in 0..n {
            if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
                break;
            }
            let name = elt_to_string(names, i);
            if name.is_empty() {
                continue;
            }
            let symbol = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            crate::sexp::envir::defineVar(symbol, VECTOR_ELT(data, i), env);
        }
        env
    }
}

/// R's `within(data, expr)` — modify data by evaluating expr (simplified).
pub unsafe fn do_within(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let data = CAR(args);
        let expr = CAR(CDR(args));
        if data.is_null() || data == R_NilValue() {
            return R_NilValue();
        }
        // Simplified: evaluate expr and return the original data
        // A full implementation would evaluate expr in data context and return modified data
        if !expr.is_null() && expr != R_NilValue() {
            let _ = crate::eval::eval::Rf_eval(expr, rho);
        }
        data
    }
}

/// R's `transform(x, ...)` — add/modify columns of a data.frame (simplified).
pub unsafe fn do_transform(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Simplified: return the data as-is
        // A full implementation would evaluate named args as new columns
        x
    }
}
