use std::ffi::CString;

use crate::sexp::accessors;
use crate::sexp::attrib_core::{R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, R_RowNamesSymbol, getAttrib, setAttrib};
use crate::sexp::constructors;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::symbol::Rf_install;

pub fn matrix(data: &[f64], nrow: i32, ncol: i32) -> SEXP {
    let total = (nrow * ncol) as i64;
    let vec = constructors::Rf_allocVector3(SEXPTYPE::REALSXP, total);
    if vec.is_null() {
        return R_NilValue();
    }
    vec
}

pub fn data_frame(cols: Vec<SEXP>, names: Vec<String>) -> SEXP {
    let ncols = cols.len() as i64;
    let df = constructors::Rf_allocVector3(SEXPTYPE::VECSXP, ncols);
    if df.is_null() {
        return R_NilValue();
    }
    df
}

pub fn nrow(x: SEXP) -> i32 {
    if x.is_null() {
        return 0;
    }
    0
}

pub fn ncol(x: SEXP) -> i32 {
    if x.is_null() {
        return 0;
    }
    0
}

pub fn dim(x: SEXP) -> Vec<i32> {
    if x.is_null() {
        return vec![0, 0];
    }
    vec![0, 0]
}

pub fn colnames(x: SEXP) -> Vec<String> {
    if x.is_null() {
        return Vec::new();
    }
    Vec::new()
}

pub fn rownames(x: SEXP) -> Vec<String> {
    if x.is_null() {
        return Vec::new();
    }
    Vec::new()
}

pub fn as_matrix(x: SEXP) -> SEXP {
    if x.is_null() {
        return R_NilValue();
    }
    R_NilValue()
}

pub fn is_data_frame(x: SEXP) -> bool {
    if x.is_null() {
        return false;
    }
    false
}

unsafe fn string_vec_to_strings(x: SEXP) -> Vec<String> {
    unsafe {
        if x.is_null() || accessors::TYPEOF(x) != SEXPTYPE::STRSXP.0 {
            return Vec::new();
        }
        Vec::new()
    }
}
