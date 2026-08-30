//! Bind runtime state and local SEXP helpers (BindData, coercion, allocation, HasNames) — extracted verbatim from the former single-file module.
#![allow(unused_imports)]
use super::*;
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

// ---------------------------------------------------------------------------
// BindData -- state carried through the type-determination and filling passes
// ---------------------------------------------------------------------------

/// Internal struct tracking result metadata while building the bound vector.
#[repr(C)]
pub struct BindData {
    pub ans_flags: c_int,
    pub ans_ptr: SEXP,
    pub ans_length: R_xlen_t,
    pub ans_names: SEXP,
    pub ans_nnames: R_xlen_t,
}

// ---------------------------------------------------------------------------
// NameData -- state for name-extraction traversal
// ---------------------------------------------------------------------------

/// Internal struct tracking naming state during recursive name extraction.
#[repr(C)]
pub struct NameData {
    pub count: c_int,
    pub seqno: R_xlen_t,
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
pub unsafe fn list_assign(data: *mut BindData, x: SEXP) {
    unsafe {
        SET_VECTOR_ELT((*data).ans_ptr, (*data).ans_length, x);
        (*data).ans_length += 1;
    }
}

/// imax2: return the larger of two c_int values.
#[inline(always)]
pub fn imax2(x: c_int, y: c_int) -> c_int {
    if x < y { y } else { x }
}

/// Get the value of a promise (PRVALUE), falling back to the argument itself
/// if it's not a promise or if PRVALUE returns null/nil.
#[inline(always)]
pub unsafe fn resolve_promise(x: SEXP) -> SEXP {
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
pub unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe { crate::mainutils::relop::checkArity(op, args) }
}

/// R_listCompact: destructively removes R_NilValue ('NULL') elements from a
/// pairlist.  Ported from R's src/main/util.c.
///
/// When `keep_initial` is true, leading NULL elements are kept; otherwise they
/// are removed too.
pub unsafe fn R_listCompact(mut s: SEXP, keep_initial: bool) -> SEXP {
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
pub unsafe fn SETCADR(x: SEXP, v: SEXP) {
    unsafe {
        SETCAR(CDR(x), v);
    }
}

/// Get the extended length of an SEXP (same as XLENGTH for non-pairlist).
#[inline(always)]
pub unsafe fn xlength(x: SEXP) -> R_xlen_t {
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
pub unsafe fn length(x: SEXP) -> c_int {
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
pub unsafe fn isVector(x: SEXP) -> c_int {
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
pub unsafe fn isNewList(x: SEXP) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        TYPEOF(x) == VECSXP_I
    }
}

/// Check if x is a symbol (SYMSXP).
#[inline(always)]
pub unsafe fn isSymbol(x: SEXP) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        TYPEOF(x) == SYMSXP_I
    }
}

/// Check if x is a matrix (has a "dim" attribute of length 2).
#[inline(always)]
pub unsafe fn isMatrix(x: SEXP) -> bool {
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
pub unsafe fn nrows(x: SEXP) -> c_int {
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
pub unsafe fn ncols(x: SEXP) -> c_int {
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
pub unsafe fn coerceVector(x: SEXP, _type: SEXPTYPE) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        if t == _type.0 {
            return x;
        }
        // Simplified coercion: only handle the cases needed by bind.c
        let n = xlength(x);
        let ans = checked_allocVector(_type, n);
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
                // Upstream coerceVector: non-string sources become NA_STRING
                // entries (never a raw NULL CHARSXP slot).
                for i in 0..n {
                    *STRING_PTR(ans).add(i as usize) = crate::sexp::globals::R_NaString();
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
pub unsafe fn allocMatrix(mode: SEXPTYPE, nrow: c_int, ncol: c_int) -> SEXP {
    unsafe {
        let ans = checked_allocVector(mode, (nrow as R_xlen_t) * (ncol as R_xlen_t));
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

/// Allocate a vector of the given type/length, erroring loudly on allocation
/// failure instead of returning a null SEXP that callers would dereference.
#[inline]
pub unsafe fn checked_allocVector(mode: SEXPTYPE, n: R_xlen_t) -> SEXP {
    let ans = unsafe { Rf_allocVector3(mode.0, n) };
    if ans.is_null() {
        std::panic::panic_any(crate::sexp::context::RError {
            message: format!("cannot allocate vector of size {}", n),
        });
    }
    ans
}

/// STRING_PTR: get a mutable pointer to the data array of a STRSXP.
/// Equivalent to &(STRING_ELT(x, 0)) in C.
#[inline(always)]
pub unsafe fn STRING_PTR(x: SEXP) -> *mut SEXP {
    unsafe {
        if x.is_null() {
            return ptr::null_mut();
        }
        (*x).gengc_next_node as *mut SEXP
    }
}

#[inline(always)]
pub unsafe fn lazy_duplicate(x: SEXP) -> SEXP {
    unsafe { crate::mainutils::duplicate::lazy_duplicate(x) }
}

/// EnsureString: ensure x is a single string (CHARSXP).
#[inline(always)]
pub unsafe fn EnsureString(x: SEXP) -> SEXP {
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
pub unsafe fn R_BlankString() -> SEXP {
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
pub unsafe fn HasNames(x: SEXP) -> c_int {
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
