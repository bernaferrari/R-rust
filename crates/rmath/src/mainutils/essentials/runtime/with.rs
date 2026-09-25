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

pub(crate) unsafe fn data_environment(data: SEXP, parent: SEXP) -> SEXP {

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

/// GNU `transform.data.frame`: eval extras in the data, then replace/add columns.
pub unsafe fn do_transform(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let data_expr = CAR(args);
        if data_expr.is_null() || data_expr == R_NilValue() {
            return R_NilValue();
        }
        let data = crate::eval::eval::Rf_eval(data_expr, rho);
        let data = if TYPEOF(data) != SEXPTYPE::VECSXP {
            let cell = Rf_cons(data, R_NilValue());
            let _g = protect(cell);
            crate::mainutils::essentials::s3::do_as_data_frame(R_NilValue(), R_NilValue(), cell, rho)
        } else {
            data
        };
        if data.is_null() || data == R_NilValue() || TYPEOF(data) != SEXPTYPE::VECSXP {
            return data;
        }
        let eval_env = data_environment(data, rho);
        let names =
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_NamesSymbol());
        let old_n = XLENGTH(data);
        let mut extras: Vec<(String, SEXP)> = Vec::new();
        let mut p = CDR(args);
        while !p.is_null() && p != R_NilValue() {
            let tag = TAG(p);
            let name = if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let val = crate::eval::eval::Rf_eval(CAR(p), eval_env);
            extras.push((name, val));
            p = CDR(p);
        }
        if extras.is_empty() {
            return data;
        }
        let nrow = if old_n > 0 {
            XLENGTH(VECTOR_ELT(data, 0))
        } else {
            0
        };
        let mut columns: Vec<(String, SEXP)> = Vec::with_capacity(old_n as usize + extras.len());
        for i in 0..old_n {
            let name = if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
            {
                elt_to_string(names, i)
            } else {
                String::new()
            };
            columns.push((name, VECTOR_ELT(data, i)));
        }
        for (name, val) in extras {
            let val = if nrow > 0 {
                let n = XLENGTH(val);
                if n > 0 && n != nrow && (nrow % n == 0 || n == 1) {
                    crate::mainutils::seq::rep3(val, n, nrow)
                } else {
                    val
                }
            } else {
                val
            };
            if !name.is_empty() {
                if let Some(pos) = columns.iter().position(|(existing, _)| existing == &name) {
                    columns[pos].1 = val;
                    continue;
                }
            }
            columns.push((name, val));
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, columns.len() as R_xlen_t);
        let _r = protect(result);
        let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, columns.len() as R_xlen_t);
        let _n = protect(out_names);
        for (i, (name, val)) in columns.into_iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, val);
            let c = CString::new(name).unwrap_or_default();
            SET_STRING_ELT(out_names, i as R_xlen_t, Rf_mkChar(c.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            out_names,
        );
        crate::mainutils::essentials::set_compact_row_names(result, nrow);
        crate::mainutils::essentials::set_data_frame_class(result);
        result
    }
}

