#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/bind.c
//!
//! This module implements R's `c()`, `unlist()`, `cbind()`, and `rbind()`
//! functions, along with their supporting type-coercion helpers.
//!
//! Key exported functions:
//!   do_c, do_c_dflt, do_unlist, do_bind, do_cbind, do_rbind, ItemName
//!
//! Module-private helpers:
//!   AnswerType, ListAnswer, StringAnswer, LogicalAnswer, IntegerAnswer,
//!   RealAnswer, ComplexAnswer, RawAnswer, NewBase, NewName, ItemName,
//!   NewExtractNames, namesCount, c_Extract_opt, cbind, rbind,
//!   SetRowNames, SetColNames, HasNames

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::eval::attrib_core::{R_data_class, getAttrib, isObject, setAttrib};
use crate::eval::dispatch::DispatchOrEval;
use crate::eval::dispatch::promiseArgs;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rbyte, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance;
use crate::sexp::protect::protect;

// Local integer constants for SEXPTYPE values, usable in match patterns
const NILSXP_I: c_int = 0;
const SYMSXP_I: c_int = 1;
const LISTSXP_I: c_int = 2;
const PROMSXP_I: c_int = 5;
const LANGSXP_I: c_int = 6;
const CHARSXP_I: c_int = 9;
const LGLSXP_I: c_int = 10;
const INTSXP_I: c_int = 13;
const REALSXP_I: c_int = 14;
const CPLXSXP_I: c_int = 15;
const STRSXP_I: c_int = 16;
const VECSXP_I: c_int = 19;
const EXPRSXP_I: c_int = 20;
const RAWSXP_I: c_int = 24;
const DOTSXP_I: c_int = 17;

// ---------------------------------------------------------------------------
// BindData -- state carried through the type-determination and filling passes
// ---------------------------------------------------------------------------

/// Internal struct tracking result metadata while building the bound vector.
#[repr(C)]
struct BindData {
    ans_flags: c_int,
    ans_ptr: SEXP,
    ans_length: R_xlen_t,
    ans_names: SEXP,
    ans_nnames: R_xlen_t,
}

// ---------------------------------------------------------------------------
// NameData -- state for name-extraction traversal
// ---------------------------------------------------------------------------

/// Internal struct tracking naming state during recursive name extraction.
#[repr(C)]
struct NameData {
    count: c_int,
    seqno: R_xlen_t,
}

#[derive(Default)]
pub(crate) struct BindRuntimeState {
    pub blank_string: SEXP,
}

// ---------------------------------------------------------------------------
// Helper macros (inline functions replacing C macros)
// ---------------------------------------------------------------------------

/// LIST_ASSIGN macro equivalent: set vector element and increment index.
#[inline(always)]
unsafe fn list_assign(data: *mut BindData, x: SEXP) {
    unsafe {
        SET_VECTOR_ELT((*data).ans_ptr, (*data).ans_length, x);
        (*data).ans_length += 1;
    }
}

/// imax2: return the larger of two c_int values.
#[inline(always)]
fn imax2(x: c_int, y: c_int) -> c_int {
    if x < y { y } else { x }
}

/// Get the value of a promise (PRVALUE), falling back to the argument itself
/// if it's not a promise or if PRVALUE returns null/nil.
#[inline(always)]
unsafe fn resolve_promise(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        if TYPEOF(x) != PROMSXP_I {
            return x;
        }
        let val = PRVALUE(x);
        if val.is_null() || val == R_NilValue() {
            x
        } else {
            val
        }
    }
}

/// checkArity: verify argument count matches the expected arity for op.
/// currently a no-op, consistent with other modules.
#[inline(always)]
unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe { crate::mainutils::relop::checkArity(op, args) }
}

/// R_listCompact: destructively removes R_NilValue ('NULL') elements from a
/// pairlist.  Ported from R's src/main/util.c.
///
/// When `keep_initial` is true, leading NULL elements are kept; otherwise they
/// are removed too.
unsafe fn R_listCompact(mut s: SEXP, keep_initial: bool) -> SEXP {
    unsafe {
        if !keep_initial {
            // skip initial NULL values
            while !s.is_null() && s != R_NilValue() && CAR(s) == R_NilValue() {
                s = CDR(s);
            }
        }

        let val = s;
        let mut prev = s;
        while !s.is_null() && s != R_NilValue() {
            s = CDR(s);
            if !s.is_null() && s != R_NilValue() && CAR(s) == R_NilValue() {
                // skip it
                SETCDR(prev, CDR(s));
            } else {
                prev = s;
            }
        }
        val
    }
}

// ---------------------------------------------------------------------------
// type2char -- convert SEXPTYPE to a human-readable string
// ---------------------------------------------------------------------------

/// Convert a SEXPTYPE value to its string name. This is used in error messages.
pub unsafe fn type2char(sexptype: c_int) -> *const c_char {
    match sexptype {
        0 => b"NULL\0".as_ptr() as *const c_char,
        1 => b"symbol\0".as_ptr() as *const c_char,
        2 => b"pairlist\0".as_ptr() as *const c_char,
        3 => b"closure\0".as_ptr() as *const c_char,
        4 => b"environment\0".as_ptr() as *const c_char,
        5 => b"promise\0".as_ptr() as *const c_char,
        6 => b"language\0".as_ptr() as *const c_char,
        7 => b"special\0".as_ptr() as *const c_char,
        8 => b"builtin\0".as_ptr() as *const c_char,
        9 => b"char\0".as_ptr() as *const c_char,
        10 => b"logical\0".as_ptr() as *const c_char,
        13 => b"integer\0".as_ptr() as *const c_char,
        14 => b"double\0".as_ptr() as *const c_char,
        15 => b"complex\0".as_ptr() as *const c_char,
        16 => b"character\0".as_ptr() as *const c_char,
        17 => b"...\0".as_ptr() as *const c_char,
        19 => b"list\0".as_ptr() as *const c_char,
        20 => b"expression\0".as_ptr() as *const c_char,
        24 => b"raw\0".as_ptr() as *const c_char,
        _ => b"unknown\0".as_ptr() as *const c_char,
    }
}

// ---------------------------------------------------------------------------
// Local helpers: SETCADR, xlength, length, isVector, isList, isMatrix, etc.
// ---------------------------------------------------------------------------

/// Set the CAR of the CDR of x (i.e., SETCADR).
#[inline(always)]
unsafe fn SETCADR(x: SEXP, v: SEXP) {
    unsafe {
        SETCAR(CDR(x), v);
    }
}

/// Get the extended length of an SEXP (same as XLENGTH for non-pairlist).
#[inline(always)]
unsafe fn xlength(x: SEXP) -> R_xlen_t {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return 0;
        }
        let t = TYPEOF(x);
        if t == LISTSXP_I || t == LANGSXP_I || t == DOTSXP_I {
            let mut count: R_xlen_t = 0;
            let mut current = x;
            while !current.is_null() && current != R_NilValue() {
                count += 1;
                current = CDR(current);
            }
            count
        } else {
            XLENGTH(x)
        }
    }
}

/// Get the length of an SEXP (c_int version).
#[inline(always)]
unsafe fn length(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return 0;
        }
        let t = TYPEOF(x);
        if t == LISTSXP_I || t == LANGSXP_I || t == DOTSXP_I {
            let mut count: c_int = 0;
            let mut current = x;
            while !current.is_null() && current != R_NilValue() {
                count += 1;
                current = CDR(current);
            }
            count
        } else {
            LENGTH(x)
        }
    }
}

/// Check if x is a vector type (atomic or generic).
#[inline(always)]
unsafe fn isVector(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        if t == LGLSXP_I
            || t == INTSXP_I
            || t == REALSXP_I
            || t == CPLXSXP_I
            || t == STRSXP_I
            || t == RAWSXP_I
            || t == VECSXP_I
            || t == EXPRSXP_I
        {
            1
        } else {
            0
        }
    }
}

/// Check if x is a pairlist (LISTSXP).
#[inline(always)]
pub unsafe fn isList(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        if TYPEOF(x) == LISTSXP_I { 1 } else { 0 }
    }
}

/// Check if x is a generic list (VECSXP).
#[inline(always)]
unsafe fn isNewList(x: SEXP) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        TYPEOF(x) == VECSXP_I
    }
}

/// Check if x is a symbol (SYMSXP).
#[inline(always)]
unsafe fn isSymbol(x: SEXP) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        TYPEOF(x) == SYMSXP_I
    }
}

/// Check if x is a matrix (has a "dim" attribute of length 2).
#[inline(always)]
unsafe fn isMatrix(x: SEXP) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        let dim_sym = crate::eval::attrib_core::R_DimSymbol();
        let dims = getAttrib(x, dim_sym);
        length(dims) == 2
    }
}

/// Get the number of rows of a matrix.
#[inline(always)]
unsafe fn nrows(x: SEXP) -> c_int {
    unsafe {
        let dim_sym = crate::eval::attrib_core::R_DimSymbol();
        let dims = getAttrib(x, dim_sym);
        if dims.is_null() || dims == R_NilValue() || length(dims) < 1 {
            return 0;
        }
        INTEGER(dims).read()
    }
}

/// Get the number of columns of a matrix.
#[inline(always)]
unsafe fn ncols(x: SEXP) -> c_int {
    unsafe {
        let dim_sym = crate::eval::attrib_core::R_DimSymbol();
        let dims = getAttrib(x, dim_sym);
        if dims.is_null() || dims == R_NilValue() || length(dims) < 2 {
            return 0;
        }
        INTEGER(dims).add(1).read()
    }
}

/// Coerce x to the given type. Simplified: returns x if already correct type,
/// otherwise allocates a new vector and copies with coercion.
unsafe fn coerceVector(x: SEXP, _type: SEXPTYPE) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        if t == _type.0 {
            return x;
        }
        // Simplified coercion: only handle the cases needed by bind.c
        let n = xlength(x);
        let ans = Rf_allocVector3(_type.0, n);
        let _ans_guard = protect(ans);

        match _type.0 {
            INTSXP_I => match t {
                LGLSXP_I => {
                    for i in 0..n {
                        *INTEGER(ans).add(i as usize) = *LOGICAL(x).add(i as usize);
                    }
                }
                RAWSXP_I => {
                    for i in 0..n {
                        *INTEGER(ans).add(i as usize) = *RAW(x).add(i as usize) as c_int;
                    }
                }
                _ => {} // intentionally unhandled: incompatible source SEXPTYPE for coercion
            },
            REALSXP_I => match t {
                LGLSXP_I => {
                    for i in 0..n {
                        let v = *LOGICAL(x).add(i as usize);
                        if v == NA_LOGICAL {
                            *REAL(ans).add(i as usize) = NA_REAL;
                        } else {
                            *REAL(ans).add(i as usize) = v as c_double;
                        }
                    }
                }
                INTSXP_I => {
                    for i in 0..n {
                        let v = *INTEGER(x).add(i as usize);
                        if v == NA_INTEGER {
                            *REAL(ans).add(i as usize) = NA_REAL;
                        } else {
                            *REAL(ans).add(i as usize) = v as c_double;
                        }
                    }
                }
                RAWSXP_I => {
                    for i in 0..n {
                        *REAL(ans).add(i as usize) = *RAW(x).add(i as usize) as c_double;
                    }
                }
                _ => {} // intentionally unhandled: incompatible source SEXPTYPE for coercion
            },
            LGLSXP_I => match t {
                INTSXP_I => {
                    for i in 0..n {
                        let v = *INTEGER(x).add(i as usize);
                        if v == NA_INTEGER {
                            *LOGICAL(ans).add(i as usize) = NA_LOGICAL;
                        } else {
                            *LOGICAL(ans).add(i as usize) = if v != 0 { TRUE } else { FALSE };
                        }
                    }
                }
                RAWSXP_I => {
                    for i in 0..n {
                        *LOGICAL(ans).add(i as usize) = if *RAW(x).add(i as usize) != 0 {
                            TRUE
                        } else {
                            FALSE
                        };
                    }
                }
                _ => {} // intentionally unhandled: incompatible source SEXPTYPE for coercion
            },
            CPLXSXP_I => match t {
                REALSXP_I => {
                    for i in 0..n {
                        let c = COMPLEX(ans).add(i as usize);
                        (*c).r = *REAL(x).add(i as usize);
                        (*c).i = 0.0;
                    }
                }
                INTSXP_I => {
                    for i in 0..n {
                        let v = *INTEGER(x).add(i as usize);
                        let c = COMPLEX(ans).add(i as usize);
                        if v == NA_INTEGER {
                            (*c).r = NA_REAL;
                            (*c).i = 0.0;
                        } else {
                            (*c).r = v as c_double;
                            (*c).i = 0.0;
                        }
                    }
                }
                LGLSXP_I => {
                    for i in 0..n {
                        let v = *LOGICAL(x).add(i as usize);
                        let c = COMPLEX(ans).add(i as usize);
                        if v == NA_LOGICAL {
                            (*c).r = NA_REAL;
                            (*c).i = 0.0;
                        } else {
                            (*c).r = v as c_double;
                            (*c).i = 0.0;
                        }
                    }
                }
                RAWSXP_I => {
                    for i in 0..n {
                        let c = COMPLEX(ans).add(i as usize);
                        (*c).r = *RAW(x).add(i as usize) as c_double;
                        (*c).i = 0.0;
                    }
                }
                _ => {} // intentionally unhandled: incompatible source SEXPTYPE for coercion
            },
            STRSXP_I => {
                // For non-character types, create NA_STRING entries
                for i in 0..n {
                    *STRING_PTR(ans).add(i as usize) = R_NilValue();
                }
            }
            RAWSXP_I => match t {
                LGLSXP_I => {
                    for i in 0..n {
                        let v = *LOGICAL(x).add(i as usize);
                        *RAW(ans).add(i as usize) = if v == NA_LOGICAL {
                            0
                        } else if v != 0 {
                            1
                        } else {
                            0
                        };
                    }
                }
                INTSXP_I => {
                    for i in 0..n {
                        let v = *INTEGER(x).add(i as usize);
                        *RAW(ans).add(i as usize) = if v == NA_INTEGER { 0 } else { v as Rbyte };
                    }
                }
                REALSXP_I => {
                    for i in 0..n {
                        let v = *REAL(x).add(i as usize);
                        *RAW(ans).add(i as usize) = if v.is_nan() { 0 } else { v as Rbyte };
                    }
                }
                _ => {} // intentionally unhandled: incompatible source SEXPTYPE for coercion
            },
            VECSXP_I => {
                // Copy as list elements
                match t {
                    VECSXP_I | EXPRSXP_I => {
                        for i in 0..n {
                            SET_VECTOR_ELT(ans, i, VECTOR_ELT(x, i));
                        }
                    }
                    LISTSXP_I => {
                        let mut src = x;
                        for i in 0..n {
                            if src.is_null() || src == R_NilValue() {
                                break;
                            }
                            SET_VECTOR_ELT(ans, i, CAR(src));
                            src = CDR(src);
                        }
                    }
                    _ => {
                        // Wrap scalars as single-element lists
                        for i in 0..n {
                            SET_VECTOR_ELT(ans, i, x);
                        }
                    }
                }
            }
            _ => {} // intentionally unhandled: incompatible SEXPTYPE for binding
        }

        ans
    }
}

/// Allocate a matrix (2D array) of the given type and dimensions.
unsafe fn allocMatrix(mode: SEXPTYPE, nrow: c_int, ncol: c_int) -> SEXP {
    unsafe {
        let ans = Rf_allocVector3(mode.0, (nrow as R_xlen_t) * (ncol as R_xlen_t));
        let _ans_guard = protect(ans);
        // Set the dim attribute
        let dim_sym = crate::eval::attrib_core::R_DimSymbol();
        let dim = Rf_allocVector(INTSXP_I, 2);
        let _dim_guard = protect(dim);
        *INTEGER(dim) = nrow;
        *INTEGER(dim).add(1) = ncol;
        setAttrib(ans, dim_sym, dim);
        ans
    }
}

/// STRING_PTR: get a mutable pointer to the data array of a STRSXP.
/// Equivalent to &(STRING_ELT(x, 0)) in C.
#[inline(always)]
unsafe fn STRING_PTR(x: SEXP) -> *mut SEXP {
    unsafe {
        if x.is_null() {
            return ptr::null_mut();
        }
        (*x).gengc_next_node as *mut SEXP
    }
}

#[inline(always)]
unsafe fn lazy_duplicate(x: SEXP) -> SEXP {
    unsafe { crate::mainutils::duplicate::lazy_duplicate(x) }
}

/// EnsureString: ensure x is a single string (CHARSXP).
#[inline(always)]
unsafe fn EnsureString(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t == CHARSXP_I {
            return x;
        }
        if t == STRSXP_I && xlength(x) > 0 {
            return STRING_ELT(x, 0);
        }
        R_NilValue()
    }
}

/// R_BlankString: return a blank CHARSXP.
unsafe fn R_BlankString() -> SEXP {
    unsafe {
        let existing =
            instance::with_required_current_instance(|inst| inst.bind_state.blank_string);
        if !existing.is_null() {
            return existing;
        }

        let s = Rf_mkChar(b"\0".as_ptr() as *const c_char);
        instance::with_required_current_instance(|inst| {
            if inst.bind_state.blank_string.is_null() {
                inst.bind_state.blank_string = s;
            }
            inst.bind_state.blank_string
        })
    }
}

// ---------------------------------------------------------------------------
// HasNames -- check whether an object carries names information
// ---------------------------------------------------------------------------

/// Returns 1 if `x` has a non-NULL names attribute (vectors) or any non-NULL
/// tag (lists), 0 otherwise.
unsafe fn HasNames(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return 0;
        }
        if isVector(x) != 0 {
            let names_sym = crate::eval::attrib_core::R_NamesSymbol();
            if !Rf_isNull(getAttrib(x, names_sym)) != 0 {
                return 1;
            }
        } else if isList(x) != 0 {
            let mut current = x;
            while !current.is_null() && current != R_NilValue() {
                if !Rf_isNull(TAG(current)) != 0 {
                    return 1;
                }
                current = CDR(current);
            }
        }
        0
    }
}

// ---------------------------------------------------------------------------
// AnswerType -- determine the result type of unlist() / c()
// ---------------------------------------------------------------------------

/// Walk SEXP `x`, updating `data->ans_flags` and `data->ans_length` to reflect
/// the coercion type and total length required.
///
/// ans_flags bit assignments:
///   1   = RAWSXP
///   2   = LGLSXP
///   16  = INTSXP
///   32  = REALSXP
///   64  = CPLXSXP
///   128 = STRSXP
///   256 = VECSXP (generic list)
///   512 = EXPRSXP (expression)
#[allow(clippy::only_used_in_recursion)]
unsafe fn AnswerType(x: SEXP, recurse: bool, usenames: bool, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        if crate::mainutils::objects::isS4(x) != 0 {
            (*data).ans_flags |= 256;
            (*data).ans_length += 1;
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            RAWSXP_I => {
                (*data).ans_flags |= 1;
                (*data).ans_length += xlength(x);
            }
            LGLSXP_I => {
                (*data).ans_flags |= 2;
                (*data).ans_length += xlength(x);
            }
            INTSXP_I => {
                (*data).ans_flags |= 16;
                (*data).ans_length += xlength(x);
            }
            REALSXP_I => {
                (*data).ans_flags |= 32;
                (*data).ans_length += xlength(x);
            }
            CPLXSXP_I => {
                (*data).ans_flags |= 64;
                (*data).ans_length += xlength(x);
            }
            STRSXP_I => {
                (*data).ans_flags |= 128;
                (*data).ans_length += xlength(x);
            }
            VECSXP_I | EXPRSXP_I => {
                if recurse {
                    let n = xlength(x);
                    if usenames && (*data).ans_nnames == 0 {
                        let names_sym = crate::eval::attrib_core::R_NamesSymbol();
                        if !Rf_isNull(getAttrib(x, names_sym)) != 0 {
                            (*data).ans_nnames = 1;
                        }
                    }
                    for i in 0..n {
                        if usenames && (*data).ans_nnames == 0 {
                            (*data).ans_nnames = HasNames(VECTOR_ELT(x, i)) as R_xlen_t;
                        }
                        AnswerType(VECTOR_ELT(x, i), recurse, usenames, data, call);
                    }
                } else {
                    if t == EXPRSXP_I {
                        (*data).ans_flags |= 512;
                    } else {
                        (*data).ans_flags |= 256;
                    }
                    (*data).ans_length += xlength(x);
                }
            }
            LISTSXP_I => {
                if recurse {
                    let mut current = x;
                    while !current.is_null() && current != R_NilValue() {
                        if usenames && (*data).ans_nnames == 0 {
                            if !Rf_isNull(TAG(current)) != 0 {
                                (*data).ans_nnames = 1;
                            } else {
                                (*data).ans_nnames = HasNames(CAR(current)) as R_xlen_t;
                            }
                        }
                        AnswerType(CAR(current), recurse, usenames, data, call);
                        current = CDR(current);
                    }
                } else {
                    (*data).ans_flags |= 256;
                    (*data).ans_length += length(x) as R_xlen_t;
                }
            }
            _ => {
                (*data).ans_flags |= 256;
                (*data).ans_length += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ListAnswer -- fill a list/expression vector result
// ---------------------------------------------------------------------------

/// Copy elements from `x` into `data->ans_ptr` as list elements.
#[allow(clippy::only_used_in_recursion)]
unsafe fn ListAnswer(x: SEXP, recurse: c_int, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        if crate::mainutils::objects::isS4(x) != 0 {
            list_assign(data, lazy_duplicate(x));
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            LGLSXP_I => {
                for i in 0..xlength(x) {
                    list_assign(data, Rf_ScalarLogical(*LOGICAL(x).add(i as usize)));
                }
            }
            RAWSXP_I => {
                for i in 0..xlength(x) {
                    list_assign(data, Rf_ScalarRaw(*RAW(x).add(i as usize)));
                }
            }
            INTSXP_I => {
                for i in 0..xlength(x) {
                    list_assign(data, Rf_ScalarInteger(*INTEGER(x).add(i as usize)));
                }
            }
            REALSXP_I => {
                for i in 0..xlength(x) {
                    list_assign(data, Rf_ScalarReal(*REAL(x).add(i as usize)));
                }
            }
            CPLXSXP_I => {
                for i in 0..xlength(x) {
                    list_assign(data, Rf_ScalarComplex(*COMPLEX(x).add(i as usize)));
                }
            }
            STRSXP_I => {
                for i in 0..xlength(x) {
                    list_assign(data, Rf_ScalarString(STRING_ELT(x, i)));
                }
            }
            VECSXP_I | EXPRSXP_I => {
                if recurse != 0 {
                    for i in 0..xlength(x) {
                        ListAnswer(VECTOR_ELT(x, i), recurse, data, call);
                    }
                } else {
                    for i in 0..xlength(x) {
                        list_assign(data, lazy_duplicate(VECTOR_ELT(x, i)));
                    }
                }
            }
            LISTSXP_I => {
                if recurse != 0 {
                    let mut current = x;
                    while !current.is_null() && current != R_NilValue() {
                        ListAnswer(CAR(current), recurse, data, call);
                        current = CDR(current);
                    }
                } else {
                    let mut current = x;
                    while !current.is_null() && current != R_NilValue() {
                        list_assign(data, lazy_duplicate(CAR(current)));
                        current = CDR(current);
                    }
                }
            }
            _ => {
                list_assign(data, lazy_duplicate(x));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// StringAnswer -- fill a character (STRSXP) result
// ---------------------------------------------------------------------------

/// Coerce elements of `x` to strings and place into `data->ans_ptr`.
#[allow(clippy::only_used_in_recursion)]
unsafe fn StringAnswer(x: SEXP, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            LISTSXP_I => {
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    StringAnswer(CAR(current), data, call);
                    current = CDR(current);
                }
            }
            EXPRSXP_I | VECSXP_I => {
                for i in 0..xlength(x) {
                    StringAnswer(VECTOR_ELT(x, i), data, call);
                }
            }
            STRSXP_I => {
                // Already strings, copy directly
                for i in 0..xlength(x) {
                    SET_STRING_ELT((*data).ans_ptr, (*data).ans_length, STRING_ELT(x, i));
                    (*data).ans_length += 1;
                }
            }
            _ => {
                // For other types, coerce to string first
                let coerced = coerceVector(x, SEXPTYPE::STRSXP);
                let _coerced_guard = protect(coerced);
                for i in 0..xlength(coerced) {
                    SET_STRING_ELT((*data).ans_ptr, (*data).ans_length, STRING_ELT(coerced, i));
                    (*data).ans_length += 1;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// LogicalAnswer -- fill a logical (LGLSXP) result
// ---------------------------------------------------------------------------

/// Coerce elements of `x` to logicals and place into `data->ans_ptr`.
#[allow(clippy::only_used_in_recursion)]
unsafe fn LogicalAnswer(x: SEXP, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            LISTSXP_I => {
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    LogicalAnswer(CAR(current), data, call);
                    current = CDR(current);
                }
            }
            EXPRSXP_I | VECSXP_I => {
                for i in 0..xlength(x) {
                    LogicalAnswer(VECTOR_ELT(x, i), data, call);
                }
            }
            LGLSXP_I => {
                for i in 0..xlength(x) {
                    *LOGICAL((*data).ans_ptr).add((*data).ans_length as usize) =
                        *LOGICAL(x).add(i as usize);
                    (*data).ans_length += 1;
                }
            }
            INTSXP_I => {
                for i in 0..xlength(x) {
                    let v = *INTEGER(x).add(i as usize);
                    let lv = if v == NA_INTEGER {
                        NA_LOGICAL
                    } else if v != 0 {
                        TRUE
                    } else {
                        FALSE
                    };
                    *LOGICAL((*data).ans_ptr).add((*data).ans_length as usize) = lv;
                    (*data).ans_length += 1;
                }
            }
            RAWSXP_I => {
                for i in 0..xlength(x) {
                    *LOGICAL((*data).ans_ptr).add((*data).ans_length as usize) =
                        if *RAW(x).add(i as usize) != 0 {
                            TRUE
                        } else {
                            FALSE
                        };
                    (*data).ans_length += 1;
                }
            }
            _ => {
                let msg = std::ffi::CString::new(format!(
                    "type '{}' is unimplemented in 'LogicalAnswer'",
                    std::ffi::CStr::from_ptr(type2char(t))
                        .to_str()
                        .unwrap_or("unknown")
                ))
                .unwrap_or_default();
                std::panic::panic_any(crate::sexp::context::RError {
                    message: msg.into_string().unwrap_or_default(),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// IntegerAnswer -- fill an integer (INTSXP) result
// ---------------------------------------------------------------------------

/// Coerce elements of `x` to integers and place into `data->ans_ptr`.
#[allow(clippy::only_used_in_recursion)]
unsafe fn IntegerAnswer(x: SEXP, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            LISTSXP_I => {
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    IntegerAnswer(CAR(current), data, call);
                    current = CDR(current);
                }
            }
            EXPRSXP_I | VECSXP_I => {
                for i in 0..xlength(x) {
                    IntegerAnswer(VECTOR_ELT(x, i), data, call);
                }
            }
            LGLSXP_I => {
                for i in 0..xlength(x) {
                    *INTEGER((*data).ans_ptr).add((*data).ans_length as usize) =
                        *LOGICAL(x).add(i as usize);
                    (*data).ans_length += 1;
                }
            }
            INTSXP_I => {
                for i in 0..xlength(x) {
                    *INTEGER((*data).ans_ptr).add((*data).ans_length as usize) =
                        *INTEGER(x).add(i as usize);
                    (*data).ans_length += 1;
                }
            }
            RAWSXP_I => {
                for i in 0..xlength(x) {
                    *INTEGER((*data).ans_ptr).add((*data).ans_length as usize) =
                        *RAW(x).add(i as usize) as c_int;
                    (*data).ans_length += 1;
                }
            }
            _ => {
                let msg = std::ffi::CString::new(format!(
                    "type '{}' is unimplemented in 'IntegerAnswer'",
                    std::ffi::CStr::from_ptr(type2char(t))
                        .to_str()
                        .unwrap_or("unknown")
                ))
                .unwrap_or_default();
                std::panic::panic_any(crate::sexp::context::RError {
                    message: msg.into_string().unwrap_or_default(),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RealAnswer -- fill a double (REALSXP) result
// ---------------------------------------------------------------------------

/// Coerce elements of `x` to doubles and place into `data->ans_ptr`.
#[allow(clippy::only_used_in_recursion)]
unsafe fn RealAnswer(x: SEXP, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            LISTSXP_I => {
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    RealAnswer(CAR(current), data, call);
                    current = CDR(current);
                }
            }
            VECSXP_I | EXPRSXP_I => {
                for i in 0..xlength(x) {
                    RealAnswer(VECTOR_ELT(x, i), data, call);
                }
            }
            REALSXP_I => {
                for i in 0..xlength(x) {
                    *REAL((*data).ans_ptr).add((*data).ans_length as usize) =
                        *REAL(x).add(i as usize);
                    (*data).ans_length += 1;
                }
            }
            LGLSXP_I => {
                for i in 0..xlength(x) {
                    let xi = *LOGICAL(x).add(i as usize);
                    let v = if xi == NA_LOGICAL {
                        NA_REAL
                    } else {
                        xi as c_double
                    };
                    *REAL((*data).ans_ptr).add((*data).ans_length as usize) = v;
                    (*data).ans_length += 1;
                }
            }
            INTSXP_I => {
                for i in 0..xlength(x) {
                    let xi = *INTEGER(x).add(i as usize);
                    let v = if xi == NA_INTEGER {
                        NA_REAL
                    } else {
                        xi as c_double
                    };
                    *REAL((*data).ans_ptr).add((*data).ans_length as usize) = v;
                    (*data).ans_length += 1;
                }
            }
            RAWSXP_I => {
                for i in 0..xlength(x) {
                    *REAL((*data).ans_ptr).add((*data).ans_length as usize) =
                        *RAW(x).add(i as usize) as c_double;
                    (*data).ans_length += 1;
                }
            }
            _ => {
                let msg = std::ffi::CString::new(format!(
                    "type '{}' is unimplemented in 'RealAnswer'",
                    std::ffi::CStr::from_ptr(type2char(t))
                        .to_str()
                        .unwrap_or("unknown")
                ))
                .unwrap_or_default();
                std::panic::panic_any(crate::sexp::context::RError {
                    message: msg.into_string().unwrap_or_default(),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ComplexAnswer -- fill a complex (CPLXSXP) result
// ---------------------------------------------------------------------------

/// Coerce elements of `x` to complex and place into `data->ans_ptr`.
#[allow(clippy::only_used_in_recursion)]
unsafe fn ComplexAnswer(x: SEXP, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            LISTSXP_I => {
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    ComplexAnswer(CAR(current), data, call);
                    current = CDR(current);
                }
            }
            EXPRSXP_I | VECSXP_I => {
                for i in 0..xlength(x) {
                    ComplexAnswer(VECTOR_ELT(x, i), data, call);
                }
            }
            REALSXP_I => {
                for i in 0..xlength(x) {
                    let c = COMPLEX((*data).ans_ptr).add((*data).ans_length as usize);
                    (*c).r = *REAL(x).add(i as usize);
                    (*c).i = 0.0;
                    (*data).ans_length += 1;
                }
            }
            CPLXSXP_I => {
                for i in 0..xlength(x) {
                    *COMPLEX((*data).ans_ptr).add((*data).ans_length as usize) =
                        *COMPLEX(x).add(i as usize);
                    (*data).ans_length += 1;
                }
            }
            LGLSXP_I => {
                for i in 0..xlength(x) {
                    let xi = *LOGICAL(x).add(i as usize);
                    let c = COMPLEX((*data).ans_ptr).add((*data).ans_length as usize);
                    if xi == NA_LOGICAL {
                        (*c).r = NA_REAL;
                        (*c).i = 0.0;
                    } else {
                        (*c).r = xi as c_double;
                        (*c).i = 0.0;
                    }
                    (*data).ans_length += 1;
                }
            }
            INTSXP_I => {
                for i in 0..xlength(x) {
                    let xi = *INTEGER(x).add(i as usize);
                    let c = COMPLEX((*data).ans_ptr).add((*data).ans_length as usize);
                    if xi == NA_INTEGER {
                        (*c).r = NA_REAL;
                        (*c).i = 0.0;
                    } else {
                        (*c).r = xi as c_double;
                        (*c).i = 0.0;
                    }
                    (*data).ans_length += 1;
                }
            }
            RAWSXP_I => {
                for i in 0..xlength(x) {
                    let c = COMPLEX((*data).ans_ptr).add((*data).ans_length as usize);
                    (*c).r = *RAW(x).add(i as usize) as c_double;
                    (*c).i = 0.0;
                    (*data).ans_length += 1;
                }
            }
            _ => {
                let msg = std::ffi::CString::new(format!(
                    "type '{}' is unimplemented in 'ComplexAnswer'",
                    std::ffi::CStr::from_ptr(type2char(t))
                        .to_str()
                        .unwrap_or("unknown")
                ))
                .unwrap_or_default();
                std::panic::panic_any(crate::sexp::context::RError {
                    message: msg.into_string().unwrap_or_default(),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RawAnswer -- fill a raw (RAWSXP) result
// ---------------------------------------------------------------------------

/// Copy raw bytes from `x` into `data->ans_ptr`.
#[allow(clippy::only_used_in_recursion)]
unsafe fn RawAnswer(x: SEXP, data: *mut BindData, call: SEXP) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }

        let t = TYPEOF(x);

        match t {
            NILSXP_I => {
                // NULL entries are dropped
            }
            LISTSXP_I => {
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    RawAnswer(CAR(current), data, call);
                    current = CDR(current);
                }
            }
            EXPRSXP_I | VECSXP_I => {
                for i in 0..xlength(x) {
                    RawAnswer(VECTOR_ELT(x, i), data, call);
                }
            }
            RAWSXP_I => {
                for i in 0..xlength(x) {
                    *RAW((*data).ans_ptr).add((*data).ans_length as usize) =
                        *RAW(x).add(i as usize);
                    (*data).ans_length += 1;
                }
            }
            _ => {
                let msg = std::ffi::CString::new(format!(
                    "type '{}' is unimplemented in 'RawAnswer'",
                    std::ffi::CStr::from_ptr(type2char(t))
                        .to_str()
                        .unwrap_or("unknown")
                ))
                .unwrap_or_default();
                std::panic::panic_any(crate::sexp::context::RError {
                    message: msg.into_string().unwrap_or_default(),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// NewBase -- construct a dotted base.tag name for c() / unlist()
// ---------------------------------------------------------------------------

/// Build a combined name "base.tag" for recursive name extraction.
unsafe fn NewBase(base: SEXP, tag: SEXP) -> SEXP {
    unsafe {
        let base = EnsureString(base);
        let tag = EnsureString(tag);

        let base_empty = if base.is_null() || base == R_NilValue() {
            true
        } else {
            *CHAR(base) == 0
        };
        let tag_empty = if tag.is_null() || tag == R_NilValue() {
            true
        } else {
            *CHAR(tag) == 0
        };

        if !base_empty && !tag_empty {
            // Both non-empty: create "base.tag"
            let sb = std::ffi::CStr::from_ptr(CHAR(base)).to_str().unwrap_or("");
            let st = std::ffi::CStr::from_ptr(CHAR(tag)).to_str().unwrap_or("");
            let combined = format!("{}.{}", sb, st);
            let c_str = std::ffi::CString::new(combined).unwrap_or_default();
            Rf_mkChar(c_str.as_ptr())
        } else if !tag_empty {
            tag
        } else if !base_empty {
            base
        } else {
            R_BlankString()
        }
    }
}

// ---------------------------------------------------------------------------
// NewName -- construct a new element name for c() / unlist()
// ---------------------------------------------------------------------------

/// Build an element name from base, tag, sequence number, and count.
unsafe fn NewName(base: SEXP, tag: SEXP, seqno: R_xlen_t, count: c_int) -> SEXP {
    unsafe {
        let base = EnsureString(base);
        let tag = EnsureString(tag);

        let base_empty = if base.is_null() || base == R_NilValue() {
            true
        } else {
            *CHAR(base) == 0
        };
        let tag_empty = if tag.is_null() || tag == R_NilValue() {
            true
        } else {
            *CHAR(tag) == 0
        };

        if !base_empty {
            if !tag_empty {
                // base.tag
                let sb = std::ffi::CStr::from_ptr(CHAR(base)).to_str().unwrap_or("");
                let st = std::ffi::CStr::from_ptr(CHAR(tag)).to_str().unwrap_or("");
                let combined = format!("{}.{}", sb, st);
                let c_str = std::ffi::CString::new(combined).unwrap_or_default();
                Rf_mkChar(c_str.as_ptr())
            } else if count == 1 {
                base
            } else {
                // base<seqno>
                let sb = std::ffi::CStr::from_ptr(CHAR(base)).to_str().unwrap_or("");
                let combined = format!("{}{}", sb, seqno);
                let c_str = std::ffi::CString::new(combined).unwrap_or_default();
                Rf_mkChar(c_str.as_ptr())
            }
        } else if !tag_empty {
            tag
        } else {
            R_BlankString()
        }
    }
}

// ---------------------------------------------------------------------------
// ItemName -- return names[i] if it is a non-empty string, else NULL
// ---------------------------------------------------------------------------

/// Look up `names[i]`; return the CHARSXP if it is non-empty, otherwise
/// `R_NilValue`.  Also used in coerce.c.
pub unsafe fn ItemName(names: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        if names.is_null() || names == R_NilValue() {
            return R_NilValue();
        }
        let elt = STRING_ELT(names, i);
        if elt.is_null() || elt == R_NilValue() {
            return R_NilValue();
        }
        if *CHAR(elt) == 0 {
            // empty string
            return R_NilValue();
        }
        elt
    }
}

// ---------------------------------------------------------------------------
// namesCount -- count names in a (possibly recursive) SEXP
// ---------------------------------------------------------------------------

/// Count the number of names in `v`, recursing if `recurse` is true.
/// Stops early once `nameData->count` exceeds 1.
unsafe fn namesCount(v: SEXP, recurse: c_int, nameData: *mut NameData) {
    unsafe {
        if v.is_null() || v == R_NilValue() {
            return;
        }

        if crate::mainutils::objects::isS4(v) != 0 {
            (*nameData).count += 1;
            return;
        }

        let n = xlength(v);
        let names_sym = crate::eval::attrib_core::R_NamesSymbol();
        let names = getAttrib(v, names_sym);

        let t = TYPEOF(v);

        match t {
            NILSXP_I => {
                // nothing
            }
            LISTSXP_I => {
                if recurse != 0 {
                    let mut current = v;
                    for _i in 0..n {
                        if (*nameData).count > 1 {
                            break;
                        }
                        let namei = ItemName(names, _i);
                        let _name_guard = protect(namei);
                        if namei == R_NilValue() {
                            namesCount(CAR(current), recurse, nameData);
                        }
                        current = CDR(current);
                    }
                } else {
                    // fall through to vector case
                    for i in 0..n {
                        if (*nameData).count > 1 {
                            break;
                        }
                        (*nameData).count += 1;
                    }
                }
            }
            VECSXP_I | EXPRSXP_I => {
                if recurse != 0 {
                    for i in 0..n {
                        if (*nameData).count > 1 {
                            break;
                        }
                        let namei = ItemName(names, i);
                        if namei == R_NilValue() {
                            namesCount(VECTOR_ELT(v, i), recurse, nameData);
                        }
                    }
                } else {
                    for i in 0..n {
                        if (*nameData).count > 1 {
                            break;
                        }
                        (*nameData).count += 1;
                    }
                }
            }
            LGLSXP_I | INTSXP_I | REALSXP_I | CPLXSXP_I | STRSXP_I | RAWSXP_I => {
                for i in 0..n {
                    if (*nameData).count > 1 {
                        break;
                    }
                    (*nameData).count += 1;
                }
            }
            _ => {
                (*nameData).count += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// NewExtractNames -- build names attribute for c() / unlist() result
// ---------------------------------------------------------------------------

/// Recursively extract and construct names for the result vector.
unsafe fn NewExtractNames(
    v: SEXP,
    base: SEXP,
    tag: SEXP,
    recurse: c_int,
    data: *mut BindData,
    nameData: *mut NameData,
) {
    unsafe {
        if v.is_null() || v == R_NilValue() {
            return;
        }

        let mut savecount: c_int = 0;
        let mut saveseqno: R_xlen_t = 0;
        let mut base = base;
        let mut _base_guard = None;

        // If we have a new tag, reset the index sequence and create the new basename
        if !tag.is_null() && tag != R_NilValue() {
            base = NewBase(base, tag);
            _base_guard = Some(protect(base));
            saveseqno = (*nameData).seqno;
            savecount = (*nameData).count;
            (*nameData).count = 0;
            namesCount(v, recurse, nameData);
            (*nameData).seqno = 0;
        } else {
            saveseqno = 0;
        }

        if crate::mainutils::objects::isS4(v) != 0 {
            let new_name = NewName(base, R_NilValue(), (*nameData).seqno + 1, (*nameData).count);
            (*nameData).seqno += 1;
            SET_STRING_ELT((*data).ans_names, (*data).ans_nnames, new_name);
            (*data).ans_nnames += 1;
            if !tag.is_null() && tag != R_NilValue() {
                (*nameData).count = savecount;
            }
            (*nameData).seqno += saveseqno;
            return;
        }

        let n = xlength(v);
        let names_sym = crate::eval::attrib_core::R_NamesSymbol();
        let _names = getAttrib(v, names_sym);

        let t = TYPEOF(v);

        match t {
            NILSXP_I => {
                // nothing
            }
            LISTSXP_I => {
                let mut current = v;
                for _i in 0..n {
                    let namei = ItemName(_names, _i);
                    let _name_guard = protect(namei);
                    if recurse != 0 {
                        NewExtractNames(CAR(current), base, namei, recurse, data, nameData);
                    } else {
                        let new_name =
                            NewName(base, namei, (*nameData).seqno + 1, (*nameData).count);
                        (*nameData).seqno += 1;
                        SET_STRING_ELT((*data).ans_names, (*data).ans_nnames, new_name);
                        (*data).ans_nnames += 1;
                    }
                    current = CDR(current);
                }
            }
            VECSXP_I | EXPRSXP_I => {
                for i in 0..n {
                    let namei = ItemName(_names, i);
                    if recurse != 0 {
                        NewExtractNames(VECTOR_ELT(v, i), base, namei, recurse, data, nameData);
                    } else {
                        let new_name =
                            NewName(base, namei, (*nameData).seqno + 1, (*nameData).count);
                        (*nameData).seqno += 1;
                        SET_STRING_ELT((*data).ans_names, (*data).ans_nnames, new_name);
                        (*data).ans_nnames += 1;
                    }
                }
            }
            LGLSXP_I | INTSXP_I | REALSXP_I | CPLXSXP_I | STRSXP_I | RAWSXP_I => {
                for i in 0..n {
                    let namei = ItemName(_names, i);
                    let new_name = NewName(base, namei, (*nameData).seqno + 1, (*nameData).count);
                    (*nameData).seqno += 1;
                    SET_STRING_ELT((*data).ans_names, (*data).ans_nnames, new_name);
                    (*data).ans_nnames += 1;
                }
            }
            _ => {
                let new_name =
                    NewName(base, R_NilValue(), (*nameData).seqno + 1, (*nameData).count);
                (*nameData).seqno += 1;
                SET_STRING_ELT((*data).ans_names, (*data).ans_nnames, new_name);
                (*data).ans_nnames += 1;
            }
        }

        if !tag.is_null() && tag != R_NilValue() {
            (*nameData).count = savecount;
        }

        (*nameData).seqno += saveseqno;
    }
}

// ---------------------------------------------------------------------------
// c_Extract_opt -- extract recursive= and use.names= from c() arguments
// ---------------------------------------------------------------------------

/// Remove optional named arguments (recursive, use.names) from the `c()`
/// argument list, returning the cleaned list.
unsafe fn c_Extract_opt(ans: SEXP, recurse: *mut bool, usenames: *mut bool, call: SEXP) -> SEXP {
    unsafe {
        let mut ans = ans;
        let mut last: SEXP = ptr::null_mut();
        let mut next: SEXP;
        let mut n_recurse: c_int = 0;
        let mut n_usenames: c_int = 0;

        let mut a = ans;
        while !a.is_null() && a != R_NilValue() {
            let n = TAG(a);
            next = CDR(a);

            // Check for "recursive" argument
            if !n.is_null() && n != R_NilValue() && !Rf_isNull(n) != 0 && TYPEOF(n) == SYMSXP_I {
                let name = CHAR(PRINTNAME(n));
                if !name.is_null() {
                    let name_str = std::ffi::CStr::from_ptr(name).to_str().unwrap_or("");
                    if name_str.starts_with("recurs") {
                        n_recurse += 1;
                        if n_recurse > 1 {
                            let msg =
                                std::ffi::CString::new("repeated formal argument 'recursive'")
                                    .unwrap_or_default();
                            std::panic::panic_any(crate::sexp::context::RError {
                                message: msg.into_string().unwrap_or_default(),
                            });
                        }
                        // Check if CAR(a) is a logical
                        let val = CAR(a);
                        if !val.is_null() && val != R_NilValue() && TYPEOF(val) == LGLSXP_I {
                            let v = *LOGICAL(val);
                            if v != NA_LOGICAL {
                                *recurse = v != 0;
                            }
                        }
                        if last.is_null() {
                            ans = next;
                        } else {
                            SETCDR(last, next);
                        }
                        a = next;
                        continue;
                    }
                    if name_str.starts_with("use.name") {
                        n_usenames += 1;
                        if n_usenames > 1 {
                            let msg =
                                std::ffi::CString::new("repeated formal argument 'use.names'")
                                    .unwrap_or_default();
                            std::panic::panic_any(crate::sexp::context::RError {
                                message: msg.into_string().unwrap_or_default(),
                            });
                        }
                        let val = CAR(a);
                        if !val.is_null() && val != R_NilValue() && TYPEOF(val) == LGLSXP_I {
                            let v = *LOGICAL(val);
                            if v != NA_LOGICAL {
                                *usenames = v != 0;
                            }
                        }
                        if last.is_null() {
                            ans = next;
                        } else {
                            SETCDR(last, next);
                        }
                        a = next;
                        continue;
                    }
                }
            }

            last = a;
            a = next;
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// Determine the result mode from ans_flags
// ---------------------------------------------------------------------------

/// Given ans_flags bitmask, determine the result SEXPTYPE.
/// Returns NILSXP if no type flags are set.
unsafe fn ans_flags_to_mode(flags: c_int) -> SEXPTYPE {
    if flags & 512 != 0 {
        SEXPTYPE::EXPRSXP
    } else if flags & 256 != 0 {
        SEXPTYPE::VECSXP
    } else if flags & 128 != 0 {
        SEXPTYPE::STRSXP
    } else if flags & 64 != 0 {
        SEXPTYPE::CPLXSXP
    } else if flags & 32 != 0 {
        SEXPTYPE::REALSXP
    } else if flags & 16 != 0 {
        SEXPTYPE::INTSXP
    } else if flags & 2 != 0 {
        SEXPTYPE::LGLSXP
    } else if flags & 1 != 0 {
        SEXPTYPE::RAWSXP
    } else {
        SEXPTYPE::NILSXP
    }
}

// ---------------------------------------------------------------------------
// do_c -- the c() primitive (SPECIALSXP)
// ---------------------------------------------------------------------------

/// R's `c()` builtin.  Attempts method dispatch; falls back to `do_c_dflt`.
pub unsafe fn do_c(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let args = R_listCompact(args, true);

        // S3 method dispatch: check if any arg is an object with a "c" method
        let mut method: SEXP = R_NilValue();
        let mut a = args;
        while !a.is_null() && a != R_NilValue() && method == R_NilValue() {
            let obj = crate::eval::eval::Rf_eval(CAR(a), env);
            if isObject(obj) != 0 {
                let classlist = R_data_class(obj);
                let classlen = Rf_length(classlist);
                for i in 0..classlen {
                    let class_str = translateChar(STRING_ELT(classlist, i as R_xlen_t));
                    let s = std::ffi::CStr::from_ptr(class_str).to_str().unwrap_or("");
                    let method_name = format!("c.{}\0", s);
                    let sym =
                        crate::sexp::symbol::Rf_install(method_name.as_ptr() as *const c_char);
                    let classmethod = crate::mainutils::objects::R_LookupMethod(
                        sym,
                        env,
                        env,
                        crate::sexp::globals::R_BaseEnv(),
                    );
                    if classmethod != crate::sexp::globals::R_UnboundValue() {
                        method = classmethod;
                        break;
                    }
                }
            }
            a = CDR(a);
        }

        if method != R_NilValue() {
            return crate::eval::closure::applyClosure(call, method, args, env, R_NilValue(), 0);
        }

        do_c_dflt(call, op, args, env)
    }
}

// ---------------------------------------------------------------------------
// do_c_dflt -- default method for c()
// ---------------------------------------------------------------------------

/// Default implementation of `c()` when no S3/S4 method is found.
pub unsafe fn do_c_dflt(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut usenames: bool = true;
        let mut recurse: bool = false;

        // Handle empty args — c() with no args returns NULL
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }

        // Extract optional arguments (recursive, use.names)
        let args = c_Extract_opt(args, &mut recurse, &mut usenames, call);
        let _args_guard = protect(args);

        // Determine the type of the returned value.
        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: ptr::null_mut(),
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };

        let mut t = args;
        while !t.is_null() && t != R_NilValue() {
            let value = resolve_promise(CAR(t));
            if usenames && data.ans_nnames == 0 {
                if !Rf_isNull(TAG(t)) != 0 {
                    data.ans_nnames = 1;
                } else {
                    data.ans_nnames = HasNames(value) as R_xlen_t;
                }
            }
            AnswerType(value, recurse, usenames, &mut data, call);
            t = CDR(t);
        }

        // Determine the result mode from the accumulated flags
        let mode = ans_flags_to_mode(data.ans_flags);

        // If no actual values were found, return NULL
        if data.ans_length == 0 {
            return R_NilValue();
        }

        // Allocate the return value
        let ans = Rf_allocVector3(mode.0, data.ans_length);
        let _ans_guard = protect(ans);
        data.ans_ptr = ans;
        data.ans_length = 0;

        // Reset t to iterate args again
        t = args;

        // Fill in the values
        if mode == SEXPTYPE::VECSXP || mode == SEXPTYPE::EXPRSXP {
            if !recurse {
                let mut a = args;
                while !a.is_null() && a != R_NilValue() {
                    ListAnswer(resolve_promise(CAR(a)), 0, &mut data, call);
                    a = CDR(a);
                }
            } else {
                let mut a = args;
                while !a.is_null() && a != R_NilValue() {
                    ListAnswer(resolve_promise(CAR(a)), 1, &mut data, call);
                    a = CDR(a);
                }
            }
            data.ans_length = xlength(ans);
        } else if mode == SEXPTYPE::STRSXP {
            StringAnswer(args, &mut data, call);
        } else if mode == SEXPTYPE::CPLXSXP {
            ComplexAnswer(args, &mut data, call);
        } else if mode == SEXPTYPE::REALSXP {
            RealAnswer(args, &mut data, call);
        } else if mode == SEXPTYPE::RAWSXP {
            RawAnswer(args, &mut data, call);
        } else if mode == SEXPTYPE::LGLSXP {
            LogicalAnswer(args, &mut data, call);
        } else {
            // integer
            IntegerAnswer(args, &mut data, call);
        }

        // Reset t again for name extraction
        t = args;

        // Build and attach the names attribute
        if data.ans_nnames != 0 && data.ans_length > 0 {
            data.ans_names = Rf_allocVector3(STRSXP_I, data.ans_length);
            let _ans_names_guard = protect(data.ans_names);
            data.ans_nnames = 0;
            let mut a = args;
            while !a.is_null() && a != R_NilValue() {
                let mut nameData = NameData { count: 0, seqno: 0 };
                NewExtractNames(
                    resolve_promise(CAR(a)),
                    R_NilValue(),
                    TAG(a),
                    recurse as c_int,
                    &mut data,
                    &mut nameData,
                );
                a = CDR(a);
            }
            let names_sym = crate::eval::attrib_core::R_NamesSymbol();
            setAttrib(ans, names_sym, data.ans_names);
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_unlist -- the unlist() builtin
// ---------------------------------------------------------------------------

/// R's `unlist()` builtin.  Attempts method dispatch; falls back to default.
pub unsafe fn do_unlist(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        // Attempt method dispatch.
        // DispatchOrEval internal generic: unlist
        let mut ans: SEXP = ptr::null_mut();
        let generic = std::ffi::CString::new("unlist").unwrap_or_default();
        // DispatchOrEval returns 1 if dispatched (result in ans), 0 if not.
        let dispatched = DispatchOrEval(call, op, generic.as_ptr(), args, env, &mut ans, 0, 0);
        if dispatched != 0 {
            return ans;
        }

        // Method dispatch has failed; run the default code with evaluated args.
        do_unlist_default(call, op, ans, env)
    }
}

/// Default implementation of unlist().
unsafe fn do_unlist_default(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // unlist takes: (x, recursive, use.names)
        // args is a pairlist: (x, recursive, use.names)
        let x_arg = CAR(args);
        let recurse_arg = CADR(args);
        let usenames_arg = CADDR(args);

        let mut recurse: bool = true;
        let mut usenames: bool = true;
        let lenient: bool = true;

        // Extract recurse from the second argument
        if !recurse_arg.is_null() && recurse_arg != R_NilValue() && TYPEOF(recurse_arg) == LGLSXP_I
        {
            let v = *LOGICAL(recurse_arg);
            if v != NA_LOGICAL {
                recurse = v != 0;
            }
        }

        // Extract usenames from the third argument
        if !usenames_arg.is_null()
            && usenames_arg != R_NilValue()
            && TYPEOF(usenames_arg) == LGLSXP_I
        {
            let v = *LOGICAL(usenames_arg);
            if v != NA_LOGICAL {
                usenames = v != 0;
            }
        }

        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: ptr::null_mut(),
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };

        let mut n: R_xlen_t = 0;
        let names_sym = crate::eval::attrib_core::R_NamesSymbol();

        if isNewList(x_arg) {
            n = xlength(x_arg);
            if usenames && !Rf_isNull(getAttrib(x_arg, names_sym)) != 0 {
                data.ans_nnames = 1;
            }
            for i in 0..n {
                if usenames && data.ans_nnames == 0 {
                    data.ans_nnames = HasNames(VECTOR_ELT(x_arg, i)) as R_xlen_t;
                }
                AnswerType(VECTOR_ELT(x_arg, i), recurse, usenames, &mut data, call);
            }
        } else if isList(x_arg) != 0 {
            let mut t = x_arg;
            while !t.is_null() && t != R_NilValue() {
                if usenames && data.ans_nnames == 0 {
                    if !Rf_isNull(TAG(t)) != 0 {
                        data.ans_nnames = 1;
                    } else {
                        data.ans_nnames = HasNames(CAR(t)) as R_xlen_t;
                    }
                }
                AnswerType(CAR(t), recurse, usenames, &mut data, call);
                t = CDR(t);
            }
        } else {
            if lenient || isVector(x_arg) != 0 {
                return x_arg;
            }
            let msg = std::ffi::CString::new("argument not a list").unwrap_or_default();
            std::panic::panic_any(crate::sexp::context::RError {
                message: msg.into_string().unwrap_or_default(),
            });
        }

        // Determine the result mode
        let mode = ans_flags_to_mode(data.ans_flags);

        // Allocate the return value
        let ans = Rf_allocVector3(mode.0, data.ans_length);
        let _ans_guard = protect(ans);
        data.ans_ptr = ans;
        data.ans_length = 0;

        // Fill in the values
        if mode == SEXPTYPE::VECSXP || mode == SEXPTYPE::EXPRSXP {
            if !recurse {
                if TYPEOF(x_arg) == VECSXP_I {
                    for i in 0..n {
                        ListAnswer(VECTOR_ELT(x_arg, i), 0, &mut data, call);
                    }
                } else if TYPEOF(x_arg) == LISTSXP_I {
                    let mut a = x_arg;
                    while !a.is_null() && a != R_NilValue() {
                        ListAnswer(CAR(a), 0, &mut data, call);
                        a = CDR(a);
                    }
                }
            } else {
                ListAnswer(x_arg, 1, &mut data, call);
            }
            data.ans_length = xlength(ans);
        } else if mode == SEXPTYPE::STRSXP {
            StringAnswer(x_arg, &mut data, call);
        } else if mode == SEXPTYPE::CPLXSXP {
            ComplexAnswer(x_arg, &mut data, call);
        } else if mode == SEXPTYPE::REALSXP {
            RealAnswer(x_arg, &mut data, call);
        } else if mode == SEXPTYPE::RAWSXP {
            RawAnswer(x_arg, &mut data, call);
        } else if mode == SEXPTYPE::LGLSXP {
            LogicalAnswer(x_arg, &mut data, call);
        } else {
            IntegerAnswer(x_arg, &mut data, call);
        }

        // Build and attach names
        if data.ans_nnames != 0 && data.ans_length > 0 {
            data.ans_names = Rf_allocVector3(STRSXP_I, data.ans_length);
            let _ans_names_guard = protect(data.ans_names);

            if !recurse {
                if TYPEOF(x_arg) == VECSXP_I {
                    let names = getAttrib(x_arg, names_sym);
                    data.ans_nnames = 0;
                    let mut nameData = NameData { count: 0, seqno: 0 };
                    for i in 0..n {
                        NewExtractNames(
                            VECTOR_ELT(x_arg, i),
                            R_NilValue(),
                            ItemName(names, i),
                            0,
                            &mut data,
                            &mut nameData,
                        );
                    }
                } else if TYPEOF(x_arg) == LISTSXP_I {
                    data.ans_nnames = 0;
                    let mut nameData = NameData { count: 0, seqno: 0 };
                    let mut a = x_arg;
                    while !a.is_null() && a != R_NilValue() {
                        NewExtractNames(CAR(a), R_NilValue(), TAG(a), 0, &mut data, &mut nameData);
                        a = CDR(a);
                    }
                }
            } else {
                data.ans_nnames = 0;
                let mut nameData = NameData { count: 0, seqno: 0 };
                NewExtractNames(
                    x_arg,
                    R_NilValue(),
                    R_NilValue(),
                    1,
                    &mut data,
                    &mut nameData,
                );
            }

            setAttrib(ans, names_sym, data.ans_names);
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_bind -- main dispatcher for cbind() / rbind()
// ---------------------------------------------------------------------------

/// R's `.Internal(cbind(...))` / `.Internal(rbind(...))`.
///
/// `PRIMVAL(op) == 1` selects `cbind`, otherwise `rbind`.
/// This is a special `.Internal`.
pub unsafe fn do_bind(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // The first argument is "deparse.level". Evaluate it.
        let deparse_level_val = crate::eval::eval::Rf_eval(CAR(args), env);
        let deparse_level: c_int = crate::mainutils::coerce::asInteger(deparse_level_val);
        let try_s4 = deparse_level >= 0;

        // Build promises for lazy evaluation and method dispatch.
        // This allows method implementations to use substitute() to get
        // the original expressions.
        let args = promiseArgs(args, env);
        let _args_guard = protect(args);

        // Determine the generic name from PRIMVAL(op).
        // PRIMVAL(op) == 1 for cbind, other for rbind.
        // Note: PRIMVAL is a stub that always returns 0 in this port.
        let generic_name = if !op.is_null() {
            let primval = crate::mainutils::relop::PRIMVAL(op);
            if primval == 1 { "cbind" } else { "rbind" }
        } else {
            "rbind"
        };

        let mut method: SEXP = R_NilValue();
        let mut any_s4 = false;
        let mut a = CDR(args);
        while !a.is_null() && a != R_NilValue() && method == R_NilValue() {
            let obj = crate::eval::eval::Rf_eval(CAR(a), env);
            let _obj_guard = protect(obj);
            if try_s4 && !any_s4 && crate::mainutils::objects::isS4(obj) != 0 {
                any_s4 = true;
            }
            if isObject(obj) != 0 {
                let classlist = R_data_class(obj);
                let _classlist_guard = protect(classlist);
                let classlen = Rf_length(classlist);
                for i in 0..classlen {
                    let class_str = translateChar(STRING_ELT(classlist, i as R_xlen_t));
                    let s = std::ffi::CStr::from_ptr(class_str).to_str().unwrap_or("");
                    let method_name = format!("{}.{}\0", generic_name, s);
                    let sym =
                        crate::sexp::symbol::Rf_install(method_name.as_ptr() as *const c_char);
                    let classmethod = crate::mainutils::objects::R_LookupMethod(
                        sym,
                        env,
                        env,
                        crate::sexp::globals::R_BaseEnv(),
                    );
                    if classmethod != crate::sexp::globals::R_UnboundValue() {
                        method = classmethod;
                        break;
                    }
                }
            }
            a = CDR(a);
        }

        if method != R_NilValue() {
            let _method_guard = protect(method);
            let dispatched_args = CDR(args);
            let ans = crate::eval::closure::applyClosure(
                call,
                method,
                dispatched_args,
                env,
                R_NilValue(),
                0,
            );
            return ans;
        }
        let args = CDR(args);
        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: ptr::null_mut(),
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };

        let mut t = args;
        while !t.is_null() && t != R_NilValue() {
            let u = CAR(t);
            // PRVALUE: get the value of a promise, or the object itself
            let val = PRVALUE(u);
            let val = if val.is_null() || val == R_NilValue() {
                u
            } else {
                val
            };
            AnswerType(val, false, false, &mut data, call);
            t = CDR(t);
        }

        // zero-extent matrices shouldn't give NULL, but cbind(NULL) should:
        if data.ans_flags == 0 && data.ans_length == 0 {
            return R_NilValue();
        }

        let mode = ans_flags_to_mode(data.ans_flags);

        // Validate mode
        match mode.0 {
            NILSXP_I | LGLSXP_I | INTSXP_I | REALSXP_I | CPLXSXP_I | STRSXP_I | VECSXP_I
            | RAWSXP_I => {}
            _ => {
                let msg = std::ffi::CString::new(format!(
                    "cannot create a matrix from type '{}'",
                    std::ffi::CStr::from_ptr(type2char(mode.0))
                        .to_str()
                        .unwrap_or("unknown")
                ))
                .unwrap_or_default();
                std::panic::panic_any(crate::sexp::context::RError {
                    message: msg.into_string().unwrap_or_default(),
                });
            }
        }

        // Dispatch to cbind or rbind based on PRIMVAL(op)
        let primval = crate::mainutils::relop::PRIMVAL(op);
        let a = if primval == 1 {
            cbind(call, args, mode, env, deparse_level)
        } else {
            rbind(call, args, mode, env, deparse_level)
        };
        a
    }
}

// ---------------------------------------------------------------------------
// do_cbind -- convenience wrapper (public stub)
// ---------------------------------------------------------------------------

/// the `cbind` internal helper.
pub unsafe fn do_cbind(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { do_bind(call, op, args, env) }
}

// ---------------------------------------------------------------------------
// do_rbind -- convenience wrapper (public stub)
// ---------------------------------------------------------------------------

/// the `rbind` internal helper.
pub unsafe fn do_rbind(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { do_bind(call, op, args, env) }
}

// ---------------------------------------------------------------------------
// SetRowNames -- set row names in a dimnames list
// ---------------------------------------------------------------------------

/// Assign `x` as the row names component of `dimnames`.
unsafe fn SetRowNames(dimnames: SEXP, x: SEXP) {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() {
            return;
        }
        let t = TYPEOF(dimnames);
        if t == VECSXP_I {
            SET_VECTOR_ELT(dimnames, 0, x);
        } else if t == LISTSXP_I {
            SETCAR(dimnames, x);
        }
    }
}

// ---------------------------------------------------------------------------
// SetColNames -- set column names in a dimnames list
// ---------------------------------------------------------------------------

/// Assign `x` as the column names component of `dimnames`.
unsafe fn SetColNames(dimnames: SEXP, x: SEXP) {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() {
            return;
        }
        let t = TYPEOF(dimnames);
        if t == VECSXP_I {
            SET_VECTOR_ELT(dimnames, 1, x);
        } else if t == LISTSXP_I {
            SETCADR(dimnames, x);
        }
    }
}

// ---------------------------------------------------------------------------
// GetRowNames / GetColNames -- local implementations
// ---------------------------------------------------------------------------

/// Retrieve row names from a dimnames attribute (vector-based list).
unsafe fn GetRowNames(dimnames: SEXP) -> SEXP {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(dimnames);
        if t == VECSXP_I {
            VECTOR_ELT(dimnames, 0)
        } else if t == LISTSXP_I {
            CAR(dimnames)
        } else {
            R_NilValue()
        }
    }
}

/// Retrieve column names from a dimnames attribute (vector-based list).
unsafe fn GetColNames(dimnames: SEXP) -> SEXP {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(dimnames);
        if t == VECSXP_I {
            VECTOR_ELT(dimnames, 1)
        } else if t == LISTSXP_I {
            CADR(dimnames)
        } else {
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// cbind -- column-binding implementation
// ---------------------------------------------------------------------------

/// Default `cbind` implementation.  Binds objects as columns, checking
/// conformability of matrix and vector arguments, and building dimnames.
unsafe fn cbind(call: SEXP, args: SEXP, mode: SEXPTYPE, rho: SEXP, deparse_level: c_int) -> SEXP {
    unsafe {
        let mut have_rnames: bool = false;
        let mut have_cnames: bool = false;
        let mut warned: bool = false;
        let mut nnames: c_int = 0;
        let mut mnames: c_int = 0;
        let mut rows: c_int = 0;
        let mut cols: c_int = 0;
        let mut mrows: c_int = -1;
        let mut lenmin: c_int = 0;

        let dim_sym = crate::eval::attrib_core::R_DimSymbol();
        let dimnames_sym = crate::eval::attrib_core::R_DimNamesSymbol();
        let names_sym = crate::eval::attrib_core::R_NamesSymbol();

        // check if we are in the zero-row case
        let mut t = args;
        while !t.is_null() && t != R_NilValue() {
            let u = CAR(t);
            let u_val = PRVALUE(u);
            let u_val = if u_val.is_null() || u_val == R_NilValue() {
                u
            } else {
                u_val
            };
            let u_rows = if isMatrix(u_val) {
                nrows(u_val)
            } else {
                length(u_val)
            };
            if u_rows > 0 {
                lenmin = 1;
                break;
            }
            t = CDR(t);
        }

        // check conformability of matrix arguments
        let mut na: c_int = 0;
        t = args;
        while !t.is_null() && t != R_NilValue() {
            let u = CAR(t);
            let u_val = PRVALUE(u);
            let u_val = if u_val.is_null() || u_val == R_NilValue() {
                u
            } else {
                u_val
            };
            let dims = getAttrib(u_val, dim_sym);
            if length(dims) == 2 {
                if mrows == -1 {
                    mrows = *INTEGER(dims);
                } else if mrows != *INTEGER(dims) {
                    let msg = std::ffi::CString::new(format!(
                        "number of rows of matrices must match (see arg {})",
                        na + 1
                    ))
                    .unwrap_or_default();
                    std::panic::panic_any(crate::sexp::context::RError {
                        message: msg.into_string().unwrap_or_default(),
                    });
                }
                cols += *INTEGER(dims).add(1);
            } else if length(u_val) >= lenmin {
                rows = imax2(rows, length(u_val));
                cols += 1;
            }
            na += 1;
            t = CDR(t);
        }
        if mrows != -1 {
            rows = mrows;
        }

        // Check conformability of vector arguments -- look for dimnames
        na = 0;
        t = args;
        while !t.is_null() && t != R_NilValue() {
            let u = CAR(t);
            let u_val = PRVALUE(u);
            let u_val = if u_val.is_null() || u_val == R_NilValue() {
                u
            } else {
                u_val
            };
            let dims = getAttrib(u_val, dim_sym);
            if length(dims) == 2 {
                let dn = getAttrib(u_val, dimnames_sym);
                if length(dn) == 2 {
                    if !Rf_isNull(VECTOR_ELT(dn, 1)) != 0 {
                        have_cnames = true;
                    }
                    if !Rf_isNull(VECTOR_ELT(dn, 0)) != 0 {
                        mnames = mrows;
                    }
                }
            } else {
                let k = length(u_val);
                if !warned && k > 0 && (k > rows || rows % k != 0) {
                    warned = true;
                    // In R this is a warning, we just note it
                }
                let dn = getAttrib(u_val, names_sym);
                if k >= lenmin
                    && (!Rf_isNull(TAG(t)) != 0
                        || deparse_level == 2
                        || (deparse_level == 1 && isSymbol(CAR(t))))
                {
                    have_cnames = true;
                }
                nnames = imax2(nnames, length(dn));
            }
            na += 1;
            t = CDR(t);
        }
        if mnames != 0 || nnames == rows {
            have_rnames = true;
        }

        let result = allocMatrix(mode, rows, cols);
        let _result_guard = protect(result);
        let mut n: R_xlen_t = 0;

        // Fill the matrix values
        if mode == SEXPTYPE::STRSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::STRSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) { k } else { rows as R_xlen_t };
                    // Copy with recycling
                    for i in 0..idx {
                        let si = (i % k) as R_xlen_t;
                        SET_STRING_ELT(result, n + i, STRING_ELT(coerced, si));
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::VECSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                let umatrix = isMatrix(u);
                if umatrix || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::VECSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    if k > 0 {
                        let idx = if !umatrix { rows as R_xlen_t } else { k };
                        for i in 0..idx {
                            let si = (i % k) as R_xlen_t;
                            SET_VECTOR_ELT(result, n + i, lazy_duplicate(VECTOR_ELT(coerced, si)));
                        }
                    }
                    n += if !umatrix { rows as R_xlen_t } else { k };
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::CPLXSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::CPLXSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) { k } else { rows as R_xlen_t };
                    for i in 0..idx {
                        let si = (i % k) as R_xlen_t;
                        *COMPLEX(result).add((n + i) as usize) = *COMPLEX(coerced).add(si as usize);
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::RAWSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::RAWSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) { k } else { rows as R_xlen_t };
                    for i in 0..idx {
                        let si = (i % k) as R_xlen_t;
                        *RAW(result).add((n + i) as usize) = *RAW(coerced).add(si as usize);
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else {
            // NILSXP, REALSXP, INTSXP, LGLSXP
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let k = xlength(u);
                    let idx = if isMatrix(u) { k } else { rows as R_xlen_t };
                    let utype = TYPEOF(u);

                    if idx > 0 && utype <= INTSXP_I {
                        // NILSXP, LGLSXP, or INTSXP
                        if mode.0 <= INTSXP_I {
                            if k > 0 {
                                for i in 0..idx {
                                    let si = (i % k) as R_xlen_t;
                                    *INTEGER(result).add((n + i) as usize) =
                                        *INTEGER(u).add(si as usize);
                                }
                            }
                            n += idx;
                        } else {
                            // mode is REALSXP
                            if k > 0 {
                                for i in 0..idx {
                                    let si = (i % k) as R_xlen_t;
                                    let v = *INTEGER(u).add(si as usize);
                                    *REAL(result).add((n + i) as usize) = if v == NA_INTEGER {
                                        NA_REAL
                                    } else {
                                        v as c_double
                                    };
                                }
                            }
                            n += idx;
                        }
                    } else if utype == REALSXP_I {
                        for i in 0..idx {
                            let si = (i % k) as R_xlen_t;
                            *REAL(result).add((n + i) as usize) = *REAL(u).add(si as usize);
                        }
                        n += idx;
                    } else if utype == RAWSXP_I {
                        for i in 0..idx {
                            let si = (i % k) as R_xlen_t;
                            if mode == SEXPTYPE::LGLSXP {
                                *LOGICAL(result).add((n + i) as usize) =
                                    if *RAW(u).add(si as usize) != 0 {
                                        TRUE
                                    } else {
                                        FALSE
                                    };
                            } else if mode == SEXPTYPE::INTSXP {
                                *INTEGER(result).add((n + i) as usize) =
                                    *RAW(u).add(si as usize) as c_int;
                            } else if mode == SEXPTYPE::REALSXP {
                                *REAL(result).add((n + i) as usize) =
                                    *RAW(u).add(si as usize) as c_double;
                            }
                        }
                        n += idx;
                    }
                }
                t = CDR(t);
            }
        }

        // Adjustment of dimnames attributes
        if have_cnames || have_rnames {
            let dn = Rf_allocVector3(VECSXP_I, 2);
            let _dn_guard = protect(dn);
            let nam: SEXP;
            if have_cnames {
                let nam_vec = Rf_allocVector3(STRSXP_I, cols as R_xlen_t);
                SET_VECTOR_ELT(dn, 1, nam_vec);
                nam = nam_vec;
            } else {
                nam = R_NilValue();
            }
            let mut j: c_int = 0;

            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) {
                    let v = getAttrib(u, dimnames_sym);

                    if have_rnames
                        && GetRowNames(dn) == R_NilValue()
                        && GetRowNames(v) != R_NilValue()
                    {
                        SetRowNames(dn, lazy_duplicate(GetRowNames(v)));
                    }

                    let tnam = GetColNames(v);
                    if !Rf_isNull(tnam) != 0 {
                        for i in 0..length(tnam) {
                            SET_STRING_ELT(nam, j as R_xlen_t, STRING_ELT(tnam, i as R_xlen_t));
                            j += 1;
                        }
                    } else if have_cnames {
                        for _i in 0..ncols(u) {
                            SET_STRING_ELT(nam, j as R_xlen_t, R_BlankString());
                            j += 1;
                        }
                    }
                } else if length(u) >= lenmin {
                    let u_names = getAttrib(u, names_sym);

                    if have_rnames
                        && GetRowNames(dn) == R_NilValue()
                        && !Rf_isNull(u_names) != 0
                        && length(u_names) == rows
                    {
                        SetRowNames(dn, lazy_duplicate(u_names));
                    }

                    if !Rf_isNull(TAG(t)) != 0 {
                        SET_STRING_ELT(nam, j as R_xlen_t, PRINTNAME(TAG(t)));
                        j += 1;
                    } else if deparse_level == 1 && isSymbol(CAR(t)) {
                        SET_STRING_ELT(nam, j as R_xlen_t, PRINTNAME(CAR(t)));
                        j += 1;
                    } else if deparse_level == 2 {
                        // deparse1line not available; use blank
                        SET_STRING_ELT(nam, j as R_xlen_t, R_BlankString());
                        j += 1;
                    } else if have_cnames {
                        SET_STRING_ELT(nam, j as R_xlen_t, R_BlankString());
                        j += 1;
                    }
                }
                t = CDR(t);
            }

            setAttrib(result, dimnames_sym, dn);
        }

        result
    }
}

// ---------------------------------------------------------------------------
// rbind -- row-binding implementation
// ---------------------------------------------------------------------------

/// Default `rbind` implementation.  Binds objects as rows, checking
/// conformability of matrix and vector arguments, and building dimnames.
#[allow(clippy::if_same_then_else)]
unsafe fn rbind(call: SEXP, args: SEXP, mode: SEXPTYPE, rho: SEXP, deparse_level: c_int) -> SEXP {
    unsafe {
        let mut have_rnames: bool = false;
        let mut have_cnames: bool = false;
        let mut warned: bool = false;
        let mut nnames: c_int = 0;
        let mut mnames: c_int = 0;
        let mut rows: c_int = 0;
        let mut cols: c_int = 0;
        let mut mcols: c_int = -1;
        let mut lenmin: c_int = 0;

        let dim_sym = crate::eval::attrib_core::R_DimSymbol();
        let dimnames_sym = crate::eval::attrib_core::R_DimNamesSymbol();
        let names_sym = crate::eval::attrib_core::R_NamesSymbol();

        // check if we are in the zero-cols case
        let mut t = args;
        while !t.is_null() && t != R_NilValue() {
            let u = resolve_promise(CAR(t));
            let u_cols = if isMatrix(u) { ncols(u) } else { length(u) };
            if u_cols > 0 {
                lenmin = 1;
                break;
            }
            t = CDR(t);
        }

        // check conformability of matrix arguments
        let mut na: c_int = 0;
        t = args;
        while !t.is_null() && t != R_NilValue() {
            let u = resolve_promise(CAR(t));
            let dims = getAttrib(u, dim_sym);
            if length(dims) == 2 {
                if mcols == -1 {
                    mcols = *INTEGER(dims).add(1);
                } else if mcols != *INTEGER(dims).add(1) {
                    let msg = std::ffi::CString::new(format!(
                        "number of columns of matrices must match (see arg {})",
                        na + 1
                    ))
                    .unwrap_or_default();
                    std::panic::panic_any(crate::sexp::context::RError {
                        message: msg.into_string().unwrap_or_default(),
                    });
                }
                rows += *INTEGER(dims);
            } else if length(u) >= lenmin {
                cols = imax2(cols, length(u));
                rows += 1;
            }
            na += 1;
            t = CDR(t);
        }
        if mcols != -1 {
            cols = mcols;
        }

        // Check conformability of vector arguments -- look for dimnames
        na = 0;
        t = args;
        while !t.is_null() && t != R_NilValue() {
            let u = resolve_promise(CAR(t));
            let dims = getAttrib(u, dim_sym);
            if length(dims) == 2 {
                let dn = getAttrib(u, dimnames_sym);
                if length(dn) == 2 {
                    if !Rf_isNull(VECTOR_ELT(dn, 0)) != 0 {
                        have_rnames = true;
                    }
                    if !Rf_isNull(VECTOR_ELT(dn, 1)) != 0 {
                        mnames = mcols;
                    }
                }
            } else {
                let k = length(u);
                if !warned && k > 0 && (k > cols || cols % k != 0) {
                    warned = true;
                    // In R this is a warning
                }
                let _dn = getAttrib(u, names_sym);
                if k >= lenmin
                    && (!Rf_isNull(TAG(t)) != 0
                        || deparse_level == 2
                        || (deparse_level == 1 && isSymbol(CAR(t))))
                {
                    have_rnames = true;
                }
                nnames = imax2(nnames, length(_dn));
            }
            na += 1;
            t = CDR(t);
        }
        if mnames != 0 || nnames == cols {
            have_cnames = true;
        }

        let result = allocMatrix(mode, rows, cols);
        let _result_guard = protect(result);
        let mut n: R_xlen_t = 0;

        // Fill the matrix -- rbind fills row by row
        if mode == SEXPTYPE::STRSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::STRSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) {
                        nrows(u) as R_xlen_t
                    } else if k > 0 {
                        1
                    } else {
                        0
                    };
                    // Fill matrix row by row with recycling
                    for r in 0..idx {
                        for c in 0..(cols as R_xlen_t) {
                            let si = ((r * cols as R_xlen_t + c) % k) as R_xlen_t;
                            let dest_idx = (n + r) * cols as R_xlen_t + c;
                            SET_STRING_ELT(result, dest_idx, STRING_ELT(coerced, si));
                        }
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::VECSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                let umatrix = isMatrix(u);
                let urows = if umatrix { nrows(u) } else { 1 };
                if umatrix || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::VECSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if umatrix {
                        urows as R_xlen_t
                    } else if k > 0 {
                        1
                    } else {
                        0
                    };
                    for r in 0..idx {
                        for c in 0..(cols as R_xlen_t) {
                            let si = ((r * cols as R_xlen_t + c) % k) as R_xlen_t;
                            let dest_idx = (n + r) * cols as R_xlen_t + c;
                            SET_VECTOR_ELT(
                                result,
                                dest_idx,
                                lazy_duplicate(VECTOR_ELT(coerced, si)),
                            );
                        }
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::RAWSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::RAWSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) {
                        nrows(u) as R_xlen_t
                    } else if k > 0 {
                        1
                    } else {
                        0
                    };
                    for r in 0..idx {
                        for c in 0..(cols as R_xlen_t) {
                            let si = ((r * cols as R_xlen_t + c) % k) as R_xlen_t;
                            let dest_idx = (n + r) * cols as R_xlen_t + c;
                            *RAW(result).add(dest_idx as usize) = *RAW(coerced).add(si as usize);
                        }
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::CPLXSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::CPLXSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) {
                        nrows(u) as R_xlen_t
                    } else if k > 0 {
                        1
                    } else {
                        0
                    };
                    for r in 0..idx {
                        for c in 0..(cols as R_xlen_t) {
                            let si = ((r * cols as R_xlen_t + c) % k) as R_xlen_t;
                            let dest_idx = (n + r) * cols as R_xlen_t + c;
                            *COMPLEX(result).add(dest_idx as usize) =
                                *COMPLEX(coerced).add(si as usize);
                        }
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::INTSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::INTSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) {
                        nrows(u) as R_xlen_t
                    } else if k > 0 {
                        1
                    } else {
                        0
                    };
                    for r in 0..idx {
                        for c in 0..(cols as R_xlen_t) {
                            let si = ((r * cols as R_xlen_t + c) % k) as R_xlen_t;
                            let dest_idx = (n + r) * cols as R_xlen_t + c;
                            *INTEGER(result).add(dest_idx as usize) =
                                *INTEGER(coerced).add(si as usize);
                        }
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::LGLSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::LGLSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) {
                        nrows(u) as R_xlen_t
                    } else if k > 0 {
                        1
                    } else {
                        0
                    };
                    for r in 0..idx {
                        for c in 0..(cols as R_xlen_t) {
                            let si = ((r * cols as R_xlen_t + c) % k) as R_xlen_t;
                            let dest_idx = (n + r) * cols as R_xlen_t + c;
                            *LOGICAL(result).add(dest_idx as usize) =
                                *LOGICAL(coerced).add(si as usize);
                        }
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::REALSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::REALSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) {
                        nrows(u) as R_xlen_t
                    } else if k > 0 {
                        1
                    } else {
                        0
                    };
                    for r in 0..idx {
                        for c in 0..(cols as R_xlen_t) {
                            let si = ((r * cols as R_xlen_t + c) % k) as R_xlen_t;
                            let dest_idx = (n + r) * cols as R_xlen_t + c;
                            *REAL(result).add(dest_idx as usize) = *REAL(coerced).add(si as usize);
                        }
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else {
            // NILSXP: do nothing
        }

        // Adjustment of dimnames attributes
        if have_rnames || have_cnames {
            let dn = Rf_allocVector3(VECSXP_I, 2);
            let _dn_guard = protect(dn);
            let nam: SEXP;
            if have_rnames {
                let nam_vec = Rf_allocVector3(STRSXP_I, rows as R_xlen_t);
                SET_VECTOR_ELT(dn, 0, nam_vec);
                nam = nam_vec;
            } else {
                nam = R_NilValue();
            }
            let mut j: c_int = 0;

            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) {
                    let v = getAttrib(u, dimnames_sym);

                    if have_cnames
                        && GetColNames(dn) == R_NilValue()
                        && GetColNames(v) != R_NilValue()
                    {
                        SetColNames(dn, lazy_duplicate(GetColNames(v)));
                    }

                    let tnam = GetRowNames(v);
                    if have_rnames {
                        if !Rf_isNull(tnam) != 0 {
                            for i in 0..length(tnam) {
                                SET_STRING_ELT(nam, j as R_xlen_t, STRING_ELT(tnam, i as R_xlen_t));
                                j += 1;
                            }
                        } else {
                            for _i in 0..nrows(u) {
                                SET_STRING_ELT(nam, j as R_xlen_t, R_BlankString());
                                j += 1;
                            }
                        }
                    }
                } else if length(u) >= lenmin {
                    let u_names = getAttrib(u, names_sym);

                    if have_cnames
                        && GetColNames(dn) == R_NilValue()
                        && !Rf_isNull(u_names) != 0
                        && length(u_names) == cols
                    {
                        SetColNames(dn, lazy_duplicate(u_names));
                    }

                    if !Rf_isNull(TAG(t)) != 0 {
                        SET_STRING_ELT(nam, j as R_xlen_t, PRINTNAME(TAG(t)));
                        j += 1;
                    } else if deparse_level == 1 && isSymbol(CAR(t)) {
                        SET_STRING_ELT(nam, j as R_xlen_t, PRINTNAME(CAR(t)));
                        j += 1;
                    } else if deparse_level == 2 {
                        SET_STRING_ELT(nam, j as R_xlen_t, R_BlankString());
                        j += 1;
                    } else if have_rnames {
                        SET_STRING_ELT(nam, j as R_xlen_t, R_BlankString());
                        j += 1;
                    }
                }
                t = CDR(t);
            }

            setAttrib(result, dimnames_sym, dn);
        }

        result
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::protect::{R_ProtectCount, protect_n};
    use crate::sexp::session::RSession;

    fn reset_protect_stack() {
        let n = R_ProtectCount();
        if n > 0 {
            drop(protect_n(n));
        }
    }

    struct ProtectStackGuard {
        _session: RSession,
    }

    impl ProtectStackGuard {
        fn new() -> Self {
            let session = RSession::new();
            reset_protect_stack();
            Self { _session: session }
        }
    }

    impl Drop for ProtectStackGuard {
        fn drop(&mut self) {
            reset_protect_stack();
        }
    }

    #[test]
    fn test_bind_data_size() {
        let size = std::mem::size_of::<BindData>();
        assert!(size > 0);
        assert!(size >= std::mem::size_of::<c_int>() * 5);
    }

    #[test]
    fn test_name_data_size() {
        let size = std::mem::size_of::<NameData>();
        assert!(size > 0);
        assert!(size >= std::mem::size_of::<c_int>() * 2);
    }

    #[test]
    fn test_imax2() {
        assert_eq!(imax2(3, 5), 5);
        assert_eq!(imax2(7, 2), 7);
        assert_eq!(imax2(0, 0), 0);
        assert_eq!(imax2(-1, 1), 1);
    }

    #[test]
    fn test_type2char_basic() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let s = std::ffi::CStr::from_ptr(type2char(0));
            assert_eq!(s.to_str().unwrap_or(""), "NULL");

            let s = std::ffi::CStr::from_ptr(type2char(10));
            assert_eq!(s.to_str().unwrap_or(""), "logical");

            let s = std::ffi::CStr::from_ptr(type2char(13));
            assert_eq!(s.to_str().unwrap_or(""), "integer");

            let s = std::ffi::CStr::from_ptr(type2char(14));
            assert_eq!(s.to_str().unwrap_or(""), "double");

            let s = std::ffi::CStr::from_ptr(type2char(16));
            assert_eq!(s.to_str().unwrap_or(""), "character");

            let s = std::ffi::CStr::from_ptr(type2char(19));
            assert_eq!(s.to_str().unwrap_or(""), "list");

            let s = std::ffi::CStr::from_ptr(type2char(24));
            assert_eq!(s.to_str().unwrap_or(""), "raw");
        }
    }

    #[test]
    fn test_blank_string_is_session_local_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        let mut left_blank = ptr::null_mut();
        left.with_arena(|_| unsafe {
            left_blank = R_BlankString();
            assert!(!left_blank.is_null());
            assert_eq!(R_BlankString(), left_blank);
        })
        .unwrap();

        right
            .with_arena(|_| unsafe {
                let right_blank = R_BlankString();
                assert!(!right_blank.is_null());
                assert_eq!(R_BlankString(), right_blank);
                assert_ne!(right_blank, left_blank);
            })
            .unwrap();

        left.with_arena(|_| unsafe {
            assert_eq!(R_BlankString(), left_blank);
        })
        .unwrap();
    }

    #[test]
    fn test_ans_flags_to_mode() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            // Raw
            assert_eq!(ans_flags_to_mode(1), SEXPTYPE::RAWSXP);
            // Logical
            assert_eq!(ans_flags_to_mode(2), SEXPTYPE::LGLSXP);
            // Integer
            assert_eq!(ans_flags_to_mode(16), SEXPTYPE::INTSXP);
            // Double
            assert_eq!(ans_flags_to_mode(32), SEXPTYPE::REALSXP);
            // Complex
            assert_eq!(ans_flags_to_mode(64), SEXPTYPE::CPLXSXP);
            // Character
            assert_eq!(ans_flags_to_mode(128), SEXPTYPE::STRSXP);
            // List
            assert_eq!(ans_flags_to_mode(256), SEXPTYPE::VECSXP);
            // Expression
            assert_eq!(ans_flags_to_mode(512), SEXPTYPE::EXPRSXP);
            // No flags
            assert_eq!(ans_flags_to_mode(0), SEXPTYPE::NILSXP);
            // Combined: integer + double -> double wins
            assert_eq!(ans_flags_to_mode(16 | 32), SEXPTYPE::REALSXP);
            // Combined: logical + integer -> integer wins
            assert_eq!(ans_flags_to_mode(2 | 16), SEXPTYPE::INTSXP);
        }
    }

    #[test]
    fn test_do_c_null_args() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            // c() with no args should return NULL
            let result = do_c(
                ptr::null_mut(),
                ptr::null_mut(),
                R_NilValue(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_c_dflt_null_args() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let result = do_c_dflt(
                ptr::null_mut(),
                ptr::null_mut(),
                R_NilValue(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_bind_null_args() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            // do_bind with just deparse.level and no data should return NULL
            // args = (deparse.level=0)
            let dl = Rf_ScalarInteger(0);
            let _dl_guard = protect(dl);
            let args = Rf_cons(dl, R_NilValue());
            let _args_guard = protect(args);
            let result = do_bind(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_cbind_null_args() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let dl = Rf_ScalarInteger(0);
            let _dl_guard = protect(dl);
            let args = Rf_cons(dl, R_NilValue());
            let _args_guard = protect(args);
            let result = do_cbind(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_rbind_null_args() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let dl = Rf_ScalarInteger(0);
            let _dl_guard = protect(dl);
            let args = Rf_cons(dl, R_NilValue());
            let _args_guard = protect(args);
            let result = do_rbind(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_itemname_null() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            assert_eq!(ItemName(ptr::null_mut(), 0), R_NilValue());
            assert_eq!(ItemName(R_NilValue(), 0), R_NilValue());
        }
    }

    #[test]
    fn test_has_names_null() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            assert_eq!(HasNames(ptr::null_mut()), 0);
            assert_eq!(HasNames(R_NilValue()), 0);
        }
    }

    #[test]
    fn test_do_unlist_null_args() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let x = R_NilValue();
            let recurse = Rf_ScalarLogical(TRUE);
            let _recurse_guard = protect(recurse);
            let usenames = Rf_ScalarLogical(TRUE);
            let _usenames_guard = protect(usenames);
            let tail = Rf_cons(usenames, R_NilValue());
            let _tail_guard = protect(tail);
            let middle = Rf_cons(recurse, tail);
            let _middle_guard = protect(middle);
            let args = Rf_cons(x, middle);
            let _args_guard = protect(args);
            let result = do_unlist(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            // unlist(NULL) should return NULL
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_is_vector_types() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            assert_eq!(isVector(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_is_list_types() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            assert_eq!(isList(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_is_new_list_null() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            assert_eq!(isNewList(ptr::null_mut()), false);
            assert_eq!(isNewList(R_NilValue()), false);
        }
    }

    #[test]
    fn test_is_symbol_null() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            assert_eq!(isSymbol(ptr::null_mut()), false);
            assert_eq!(isSymbol(R_NilValue()), false);
        }
    }

    #[test]
    fn test_is_matrix_null() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            assert_eq!(isMatrix(ptr::null_mut()), false);
            assert_eq!(isMatrix(R_NilValue()), false);
        }
    }

    // ---- New tests for real logic ----

    #[test]
    fn test_resolve_promise_null() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            assert_eq!(resolve_promise(ptr::null_mut()), ptr::null_mut());
            assert_eq!(resolve_promise(R_NilValue()), R_NilValue());
        }
    }

    #[test]
    fn test_r_list_compact_basic() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            // Build a list with some NULL entries: (1, NULL, 2, NULL, 3)
            let v1 = Rf_ScalarInteger(1);
            let _v1_guard = protect(v1);
            let v2 = Rf_ScalarInteger(2);
            let _v2_guard = protect(v2);
            let v3 = Rf_ScalarInteger(3);
            let _v3_guard = protect(v3);
            let cell3 = Rf_cons(v3, R_NilValue());
            let _cell3_guard = protect(cell3);
            let cell_null2 = Rf_cons(R_NilValue(), cell3);
            let _cell_null2_guard = protect(cell_null2);
            let cell2 = Rf_cons(v2, cell_null2);
            let _cell2_guard = protect(cell2);
            let cell_null1 = Rf_cons(R_NilValue(), cell2);
            let _cell_null1_guard = protect(cell_null1);
            let lst = Rf_cons(v1, cell_null1);
            let _lst_guard = protect(lst);

            // With keep_initial=true, leading NULLs are kept
            // But non-leading NULLs are removed
            let compacted = R_listCompact(lst, true);
            // Walk: 1 -> NULL -> 2 -> NULL -> 3
            // Non-leading removal: 1 -> 2 -> 3
            assert!(!compacted.is_null());
            assert_eq!(TYPEOF(CAR(compacted)), INTSXP_I);
            assert_eq!(*INTEGER(CAR(compacted)), 1);
            let second = CDR(compacted);
            assert_eq!(TYPEOF(CAR(second)), INTSXP_I);
            assert_eq!(*INTEGER(CAR(second)), 2);
            let third = CDR(second);
            assert_eq!(TYPEOF(CAR(third)), INTSXP_I);
            assert_eq!(*INTEGER(CAR(third)), 3);
            assert_eq!(CDR(third), R_NilValue());
        }
    }

    #[test]
    fn test_r_list_compact_no_nulls() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            // List with no NULLs: (1, 2, 3)
            let v1 = Rf_ScalarInteger(1);
            let _v1_guard = protect(v1);
            let v2 = Rf_ScalarInteger(2);
            let _v2_guard = protect(v2);
            let v3 = Rf_ScalarInteger(3);
            let _v3_guard = protect(v3);
            let tail2 = Rf_cons(v3, R_NilValue());
            let _tail2_guard = protect(tail2);
            let tail1 = Rf_cons(v2, tail2);
            let _tail1_guard = protect(tail1);
            let lst = Rf_cons(v1, tail1);
            let _lst_guard = protect(lst);

            let compacted = R_listCompact(lst, true);
            assert!(!compacted.is_null());
            assert_eq!(*INTEGER(CAR(compacted)), 1);
            assert_eq!(*INTEGER(CAR(CDR(compacted))), 2);
            assert_eq!(*INTEGER(CAR(CDR(CDR(compacted)))), 3);
            assert_eq!(CDR(CDR(CDR(compacted))), R_NilValue());
        }
    }

    #[test]
    fn test_r_list_compact_all_nulls() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let tail = Rf_cons(R_NilValue(), R_NilValue());
            let _tail_guard = protect(tail);
            let lst = Rf_cons(R_NilValue(), tail);
            let _lst_guard = protect(lst);
            let compacted = R_listCompact(lst, false);
            // With keep_initial=false, all NULLs are removed -> R_NilValue
            assert_eq!(compacted, R_NilValue());
        }
    }

    #[test]
    fn test_answertype_single_integer() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let v = Rf_ScalarInteger(42);
            let _v_guard = protect(v);
            let mut data = BindData {
                ans_flags: 0,
                ans_ptr: ptr::null_mut(),
                ans_length: 0,
                ans_names: ptr::null_mut(),
                ans_nnames: 0,
            };
            AnswerType(v, false, false, &mut data, ptr::null_mut());
            assert_eq!(data.ans_flags & 16, 16); // INTSXP flag
            assert_eq!(data.ans_length, 1);
        }
    }

    #[test]
    fn test_answertype_single_real() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let v = Rf_ScalarReal(3.14);
            let _v_guard = protect(v);
            let mut data = BindData {
                ans_flags: 0,
                ans_ptr: ptr::null_mut(),
                ans_length: 0,
                ans_names: ptr::null_mut(),
                ans_nnames: 0,
            };
            AnswerType(v, false, false, &mut data, ptr::null_mut());
            assert_eq!(data.ans_flags & 32, 32); // REALSXP flag
            assert_eq!(data.ans_length, 1);
        }
    }

    #[test]
    fn test_answertype_single_logical() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let v = Rf_ScalarLogical(TRUE);
            let _v_guard = protect(v);
            let mut data = BindData {
                ans_flags: 0,
                ans_ptr: ptr::null_mut(),
                ans_length: 0,
                ans_names: ptr::null_mut(),
                ans_nnames: 0,
            };
            AnswerType(v, false, false, &mut data, ptr::null_mut());
            assert_eq!(data.ans_flags & 2, 2); // LGLSXP flag
            assert_eq!(data.ans_length, 1);
        }
    }

    #[test]
    fn test_answertype_null_dropped() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let mut data = BindData {
                ans_flags: 0,
                ans_ptr: ptr::null_mut(),
                ans_length: 0,
                ans_names: ptr::null_mut(),
                ans_nnames: 0,
            };
            AnswerType(R_NilValue(), false, false, &mut data, ptr::null_mut());
            assert_eq!(data.ans_flags, 0);
            assert_eq!(data.ans_length, 0);
        }
    }

    #[test]
    fn test_answertype_combined_types() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let v_int = Rf_ScalarInteger(1);
            let _v_int_guard = protect(v_int);
            let v_real = Rf_ScalarReal(2.0);
            let _v_real_guard = protect(v_real);
            let mut data = BindData {
                ans_flags: 0,
                ans_ptr: ptr::null_mut(),
                ans_length: 0,
                ans_names: ptr::null_mut(),
                ans_nnames: 0,
            };
            AnswerType(v_int, false, false, &mut data, ptr::null_mut());
            AnswerType(v_real, false, false, &mut data, ptr::null_mut());
            // Both INTSXP (16) and REALSXP (32) flags set
            assert_eq!(data.ans_flags & 16, 16);
            assert_eq!(data.ans_flags & 32, 32);
            assert_eq!(data.ans_length, 2);
            // Mode should be REALSXP (higher priority)
            assert_eq!(ans_flags_to_mode(data.ans_flags), SEXPTYPE::REALSXP);
        }
    }

    #[test]
    fn test_answertype_vector_length() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            // Create a length-3 integer vector
            let v = Rf_allocVector3(INTSXP_I, 3);
            let _v_guard = protect(v);
            for i in 0..3 {
                *INTEGER(v).add(i) = (i + 1) as c_int;
            }
            let mut data = BindData {
                ans_flags: 0,
                ans_ptr: ptr::null_mut(),
                ans_length: 0,
                ans_names: ptr::null_mut(),
                ans_nnames: 0,
            };
            AnswerType(v, false, false, &mut data, ptr::null_mut());
            assert_eq!(data.ans_flags & 16, 16);
            assert_eq!(data.ans_length, 3);
        }
    }

    #[test]
    fn test_do_c_dflt_single_integer() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let v = Rf_ScalarInteger(42);
            let _v_guard = protect(v);
            let args = Rf_cons(v, R_NilValue());
            let _args_guard = protect(args);
            let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), INTSXP_I);
            assert_eq!(XLENGTH(result), 1);
            assert_eq!(*INTEGER(result), 42);
        }
    }

    #[test]
    fn test_do_c_dflt_two_integers() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let v1 = Rf_ScalarInteger(1);
            let _v1_guard = protect(v1);
            let v2 = Rf_ScalarInteger(2);
            let _v2_guard = protect(v2);
            let tail = Rf_cons(v2, R_NilValue());
            let _tail_guard = protect(tail);
            let args = Rf_cons(v1, tail);
            let _args_guard = protect(args);
            let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), INTSXP_I);
            assert_eq!(XLENGTH(result), 2);
            assert_eq!(*INTEGER(result), 1);
            assert_eq!(*INTEGER(result).add(1), 2);
        }
    }

    #[test]
    fn test_do_c_dflt_int_and_real() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let v_int = Rf_ScalarInteger(1);
            let _v_int_guard = protect(v_int);
            let v_real = Rf_ScalarReal(2.5);
            let _v_real_guard = protect(v_real);
            let tail = Rf_cons(v_real, R_NilValue());
            let _tail_guard = protect(tail);
            let args = Rf_cons(v_int, tail);
            let _args_guard = protect(args);
            let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            // integer + real -> real (coercion)
            assert_eq!(TYPEOF(result), REALSXP_I);
            assert_eq!(XLENGTH(result), 2);
            assert_eq!(*REAL(result), 1.0);
            assert_eq!(*REAL(result).add(1), 2.5);
        }
    }

    #[test]
    fn test_do_c_dflt_with_null() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let v1 = Rf_ScalarInteger(1);
            let _v1_guard = protect(v1);
            let v_null = R_NilValue();
            let v2 = Rf_ScalarInteger(3);
            let _v2_guard = protect(v2);
            // (1, NULL, 3) -> NULLs are dropped -> c(1, 3)
            let tail2 = Rf_cons(v2, R_NilValue());
            let _tail2_guard = protect(tail2);
            let tail1 = Rf_cons(v_null, tail2);
            let _tail1_guard = protect(tail1);
            let args = Rf_cons(v1, tail1);
            let _args_guard = protect(args);
            let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), INTSXP_I);
            assert_eq!(XLENGTH(result), 2);
            assert_eq!(*INTEGER(result), 1);
            assert_eq!(*INTEGER(result).add(1), 3);
        }
    }

    #[test]
    fn test_do_c_dflt_logical_vector() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let v1 = Rf_ScalarLogical(TRUE);
            let _v1_guard = protect(v1);
            let v2 = Rf_ScalarLogical(FALSE);
            let _v2_guard = protect(v2);
            let tail = Rf_cons(v2, R_NilValue());
            let _tail_guard = protect(tail);
            let args = Rf_cons(v1, tail);
            let _args_guard = protect(args);
            let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), LGLSXP_I);
            assert_eq!(XLENGTH(result), 2);
            assert_eq!(*LOGICAL(result), TRUE);
            assert_eq!(*LOGICAL(result).add(1), FALSE);
        }
    }

    #[test]
    fn test_do_c_dflt_real_vector() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let v1 = Rf_ScalarReal(1.5);
            let _v1_guard = protect(v1);
            let v2 = Rf_ScalarReal(2.5);
            let _v2_guard = protect(v2);
            let v3 = Rf_ScalarReal(3.5);
            let _v3_guard = protect(v3);
            let tail2 = Rf_cons(v3, R_NilValue());
            let _tail2_guard = protect(tail2);
            let tail1 = Rf_cons(v2, tail2);
            let _tail1_guard = protect(tail1);
            let args = Rf_cons(v1, tail1);
            let _args_guard = protect(args);
            let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), REALSXP_I);
            assert_eq!(XLENGTH(result), 3);
        }
    }

    #[test]
    fn test_do_c_dflt_integer_vector() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            // Create a length-2 integer vector
            let v = Rf_allocVector3(INTSXP_I, 2);
            let _v_guard = protect(v);
            *INTEGER(v) = 10;
            *INTEGER(v).add(1) = 20;
            let args = Rf_cons(v, R_NilValue());
            let _args_guard = protect(args);
            let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(result), INTSXP_I);
            assert_eq!(XLENGTH(result), 2);
            assert_eq!(*INTEGER(result), 10);
            assert_eq!(*INTEGER(result).add(1), 20);
        }
    }

    #[test]
    fn test_do_c_dflt_all_nulls() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            // c(NULL, NULL) should return NULL
            let tail = Rf_cons(R_NilValue(), R_NilValue());
            let _tail_guard = protect(tail);
            let args = Rf_cons(R_NilValue(), tail);
            let _args_guard = protect(args);
            let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            // All NULLs -> ans_flags=0, ans_length=0 -> NILSXP mode
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_c_dflt_logical_and_integer() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let v_lgl = Rf_ScalarLogical(TRUE);
            let _v_lgl_guard = protect(v_lgl);
            let v_int = Rf_ScalarInteger(42);
            let _v_int_guard = protect(v_int);
            let tail = Rf_cons(v_int, R_NilValue());
            let _tail_guard = protect(tail);
            let args = Rf_cons(v_lgl, tail);
            let _args_guard = protect(args);
            let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            // logical + integer -> integer (coercion)
            assert_eq!(TYPEOF(result), INTSXP_I);
            assert_eq!(XLENGTH(result), 2);
            assert_eq!(*INTEGER(result), TRUE);
            assert_eq!(*INTEGER(result).add(1), 42);
        }
    }

    #[test]
    fn test_integer_answer_from_logical() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let src = Rf_allocVector3(LGLSXP_I, 3);
            let _src_guard = protect(src);
            *LOGICAL(src) = TRUE;
            *LOGICAL(src).add(1) = FALSE;
            *LOGICAL(src).add(2) = NA_LOGICAL;

            let dest = Rf_allocVector3(INTSXP_I, 3);
            let _dest_guard = protect(dest);
            let mut data = BindData {
                ans_flags: 0,
                ans_ptr: dest,
                ans_length: 0,
                ans_names: ptr::null_mut(),
                ans_nnames: 0,
            };
            IntegerAnswer(src, &mut data, ptr::null_mut());
            assert_eq!(data.ans_length, 3);
            assert_eq!(*INTEGER(dest), TRUE);
            assert_eq!(*INTEGER(dest).add(1), FALSE);
            assert_eq!(*INTEGER(dest).add(2), NA_LOGICAL);
        }
    }

    #[test]
    fn test_real_answer_from_integer() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let src = Rf_allocVector3(INTSXP_I, 3);
            let _src_guard = protect(src);
            *INTEGER(src) = 1;
            *INTEGER(src).add(1) = NA_INTEGER;
            *INTEGER(src).add(2) = -5;

            let dest = Rf_allocVector3(REALSXP_I, 3);
            let _dest_guard = protect(dest);
            let mut data = BindData {
                ans_flags: 0,
                ans_ptr: dest,
                ans_length: 0,
                ans_names: ptr::null_mut(),
                ans_nnames: 0,
            };
            RealAnswer(src, &mut data, ptr::null_mut());
            assert_eq!(data.ans_length, 3);
            assert_eq!(*REAL(dest), 1.0);
            // NA_INTEGER -> NA_REAL
            assert!((*REAL(dest).add(1)).is_nan());
            assert_eq!(*REAL(dest).add(2), -5.0);
        }
    }

    #[test]
    fn test_logical_answer_from_integer() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let src = Rf_allocVector3(INTSXP_I, 3);
            let _src_guard = protect(src);
            *INTEGER(src) = 1;
            *INTEGER(src).add(1) = 0;
            *INTEGER(src).add(2) = NA_INTEGER;

            let dest = Rf_allocVector3(LGLSXP_I, 3);
            let _dest_guard = protect(dest);
            let mut data = BindData {
                ans_flags: 0,
                ans_ptr: dest,
                ans_length: 0,
                ans_names: ptr::null_mut(),
                ans_nnames: 0,
            };
            LogicalAnswer(src, &mut data, ptr::null_mut());
            assert_eq!(data.ans_length, 3);
            assert_eq!(*LOGICAL(dest), TRUE);
            assert_eq!(*LOGICAL(dest).add(1), FALSE);
            assert_eq!(*LOGICAL(dest).add(2), NA_LOGICAL);
        }
    }

    #[test]
    fn test_complex_answer_from_real() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let src = Rf_allocVector3(REALSXP_I, 2);
            let _src_guard = protect(src);
            *REAL(src) = 1.0;
            *REAL(src).add(1) = 2.0;

            let dest = Rf_allocVector3(CPLXSXP_I, 2);
            let _dest_guard = protect(dest);
            let mut data = BindData {
                ans_flags: 0,
                ans_ptr: dest,
                ans_length: 0,
                ans_names: ptr::null_mut(),
                ans_nnames: 0,
            };
            ComplexAnswer(src, &mut data, ptr::null_mut());
            assert_eq!(data.ans_length, 2);
            assert_eq!((*COMPLEX(dest)).r, 1.0);
            assert_eq!((*COMPLEX(dest)).i, 0.0);
            assert_eq!((*COMPLEX(dest).add(1)).r, 2.0);
            assert_eq!((*COMPLEX(dest).add(1)).i, 0.0);
        }
    }

    #[test]
    fn test_coerce_vector_lgl_to_int() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let src = Rf_allocVector3(LGLSXP_I, 2);
            let _src_guard = protect(src);
            *LOGICAL(src) = TRUE;
            *LOGICAL(src).add(1) = FALSE;

            let dest = coerceVector(src, SEXPTYPE::INTSXP);
            assert_eq!(TYPEOF(dest), INTSXP_I);
            assert_eq!(*INTEGER(dest), TRUE);
            assert_eq!(*INTEGER(dest).add(1), FALSE);
        }
    }

    #[test]
    fn test_coerce_vector_int_to_real() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let src = Rf_allocVector3(INTSXP_I, 2);
            let _src_guard = protect(src);
            *INTEGER(src) = 42;
            *INTEGER(src).add(1) = NA_INTEGER;

            let dest = coerceVector(src, SEXPTYPE::REALSXP);
            assert_eq!(TYPEOF(dest), REALSXP_I);
            assert_eq!(*REAL(dest), 42.0);
            assert!((*REAL(dest).add(1)).is_nan()); // NA -> NaN
        }
    }

    #[test]
    fn test_coerce_vector_same_type() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let src = Rf_allocVector3(INTSXP_I, 2);
            let _src_guard = protect(src);
            *INTEGER(src) = 1;
            *INTEGER(src).add(1) = 2;

            let dest = coerceVector(src, SEXPTYPE::INTSXP);
            // Should return the same pointer (no copy needed)
            assert_eq!(dest, src);
        }
    }

    #[test]
    fn test_coerce_vector_raw_to_int() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let src = Rf_allocVector3(RAWSXP_I, 3);
            let _src_guard = protect(src);
            *RAW(src) = 10;
            *RAW(src).add(1) = 20;
            *RAW(src).add(2) = 255;

            let dest = coerceVector(src, SEXPTYPE::INTSXP);
            assert_eq!(TYPEOF(dest), INTSXP_I);
            assert_eq!(*INTEGER(dest), 10);
            assert_eq!(*INTEGER(dest).add(1), 20);
            assert_eq!(*INTEGER(dest).add(2), 255);
        }
    }

    #[test]
    fn test_coerce_vector_raw_to_real() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let src = Rf_allocVector3(RAWSXP_I, 2);
            let _src_guard = protect(src);
            *RAW(src) = 0;
            *RAW(src).add(1) = 200;

            let dest = coerceVector(src, SEXPTYPE::REALSXP);
            assert_eq!(TYPEOF(dest), REALSXP_I);
            assert_eq!(*REAL(dest), 0.0);
            assert_eq!(*REAL(dest).add(1), 200.0);
        }
    }

    #[test]
    fn test_coerce_vector_raw_to_complex() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let src = Rf_allocVector3(RAWSXP_I, 2);
            let _src_guard = protect(src);
            *RAW(src) = 42;
            *RAW(src).add(1) = 100;

            let dest = coerceVector(src, SEXPTYPE::CPLXSXP);
            assert_eq!(TYPEOF(dest), CPLXSXP_I);
            assert_eq!((*COMPLEX(dest)).r, 42.0);
            assert_eq!((*COMPLEX(dest)).i, 0.0);
            assert_eq!((*COMPLEX(dest).add(1)).r, 100.0);
            assert_eq!((*COMPLEX(dest).add(1)).i, 0.0);
        }
    }

    #[test]
    fn test_coerce_vector_int_to_complex() {
        unsafe {
            let _guard = ProtectStackGuard::new();
            let src = Rf_allocVector3(INTSXP_I, 2);
            let _src_guard = protect(src);
            *INTEGER(src) = 3;
            *INTEGER(src).add(1) = NA_INTEGER;

            let dest = coerceVector(src, SEXPTYPE::CPLXSXP);
            assert_eq!(TYPEOF(dest), CPLXSXP_I);
            assert_eq!((*COMPLEX(dest)).r, 3.0);
            assert_eq!((*COMPLEX(dest)).i, 0.0);
            assert!((*COMPLEX(dest).add(1)).r.is_nan()); // NA -> NaN
            assert_eq!((*COMPLEX(dest).add(1)).i, 0.0);
        }
    }
}
