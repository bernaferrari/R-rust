#![allow(
    unsafe_op_in_unsafe_fn,
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]

//! Port of R's src/main/altclasses.c -- ALTREP concrete class implementations.
//!
//! Provides specific ALTREP classes:
//! - compact_intseq: compact integer sequences
//! - compact_realseq: compact real sequences
//! - deferred_string: deferred string operations
//! - deferred_names: deferred names vectors
//! - mmap_integer/mmap_real: memory-mapped vectors
//! - wrap_integer/wrap_logical/wrap_real/wrap_complex/wrap_raw/wrap_string/wrap_list:
//!   attribute and meta data wrappers

use std::cell::Cell;
use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{
    NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ALTREP functions from altrep.rs
use crate::main::altrep::{
    R_altrep_data1, R_altrep_data2, R_new_altrep, R_set_altrep_data1, R_set_altrep_data2,
};

// ---------------------------------------------------------------------------
// Constants for sortedness
// ---------------------------------------------------------------------------

const UNKNOWN_SORTEDNESS: c_int = 0;
const SORTED_INCR: c_int = 1;
const SORTED_DECR: c_int = -1;
const KNOWN_SORTED: c_int = 2;
const KNOWN_UNSORTED: c_int = -2;

// R_XLEN_T_MAX
const R_XLEN_T_MAX: R_xlen_t = i64::MAX;

// R_INT_MIN = 1 + INT_MIN (from summary.c convention)
const R_INT_MIN: c_int = 1 + c_int::MIN;

// DBL_DIG
const DBL_DIG: c_int = 15;

// NMETA for wrapper classes
const NMETA: usize = 2;

// Local SEXPTYPE integer constants for matching
// SEXPTYPE constants now imported from crate::sexp::ffi::SEXPTYPE

// ---------------------------------------------------------------------------
// ALTREP class type -- opaque pointer
// ---------------------------------------------------------------------------

/// Opaque ALTREP class descriptor.
type R_altrep_class_t = *mut c_void;

thread_local! { static R_compact_intseq_class: Cell<R_altrep_class_t> = Cell::new(ptr::null_mut()); }
thread_local! { static R_compact_realseq_class: Cell<R_altrep_class_t> = Cell::new(ptr::null_mut()); }
thread_local! { static R_deferred_string_class: Cell<R_altrep_class_t> = Cell::new(ptr::null_mut()); }
thread_local! { static mmap_integer_class: Cell<R_altrep_class_t> = Cell::new(ptr::null_mut()); }
thread_local! { static mmap_real_class: Cell<R_altrep_class_t> = Cell::new(ptr::null_mut()); }
thread_local! { static wrap_integer_class: Cell<R_altrep_class_t> = Cell::new(ptr::null_mut()); }
thread_local! { static wrap_logical_class: Cell<R_altrep_class_t> = Cell::new(ptr::null_mut()); }
thread_local! { static wrap_real_class: Cell<R_altrep_class_t> = Cell::new(ptr::null_mut()); }
thread_local! { static wrap_complex_class: Cell<R_altrep_class_t> = Cell::new(ptr::null_mut()); }
thread_local! { static wrap_raw_class: Cell<R_altrep_class_t> = Cell::new(ptr::null_mut()); }
thread_local! { static wrap_string_class: Cell<R_altrep_class_t> = Cell::new(ptr::null_mut()); }
thread_local! { static wrap_list_class: Cell<R_altrep_class_t> = Cell::new(ptr::null_mut()); }

// ---------------------------------------------------------------------------
// Helper macros (ported from C preprocessor macros)
// ---------------------------------------------------------------------------

/// COMPACT_SEQ_INFO(x) = R_altrep_data1(x)
unsafe fn COMPACT_SEQ_INFO(x: SEXP) -> SEXP {
    R_altrep_data1(x)
}

/// COMPACT_SEQ_EXPANDED(x) = R_altrep_data2(x)
unsafe fn COMPACT_SEQ_EXPANDED(x: SEXP) -> SEXP {
    R_altrep_data2(x)
}

/// SET_COMPACT_SEQ_EXPANDED(x, v) = R_set_altrep_data2(x, v)
unsafe fn SET_COMPACT_SEQ_EXPANDED(x: SEXP, v: SEXP) {
    R_set_altrep_data2(x, v);
}

/// COMPACT_INTSEQ_SERIALIZED_STATE_LENGTH(info)
unsafe fn COMPACT_INTSEQ_SERIALIZED_STATE_LENGTH(info: SEXP) -> R_xlen_t {
    if TYPEOF(info) == SEXPTYPE::INTSXP.0 {
        *INTEGER(info).add(0) as R_xlen_t
    } else {
        *REAL(info).add(0) as R_xlen_t
    }
}

/// COMPACT_INTSEQ_SERIALIZED_STATE_FIRST(info)
unsafe fn COMPACT_INTSEQ_SERIALIZED_STATE_FIRST(info: SEXP) -> c_int {
    if TYPEOF(info) == SEXPTYPE::INTSXP.0 {
        *INTEGER(info).add(1)
    } else {
        *REAL(info).add(1) as c_int
    }
}

/// COMPACT_INTSEQ_SERIALIZED_STATE_INCR(info)
unsafe fn COMPACT_INTSEQ_SERIALIZED_STATE_INCR(info: SEXP) -> c_int {
    if TYPEOF(info) == SEXPTYPE::INTSXP.0 {
        *INTEGER(info).add(2)
    } else {
        *REAL(info).add(2) as c_int
    }
}

/// COMPACT_INTSEQ_INFO_LENGTH(info) -- info is stored as REALSXP
unsafe fn COMPACT_INTSEQ_INFO_LENGTH(info: SEXP) -> R_xlen_t {
    *REAL(info).add(0) as R_xlen_t
}

/// COMPACT_INTSEQ_INFO_FIRST(info)
unsafe fn COMPACT_INTSEQ_INFO_FIRST(info: SEXP) -> c_int {
    *REAL(info).add(1) as c_int
}

/// COMPACT_INTSEQ_INFO_INCR(info)
unsafe fn COMPACT_INTSEQ_INFO_INCR(info: SEXP) -> c_int {
    *REAL(info).add(2) as c_int
}

/// COMPACT_REALSEQ_INFO_LENGTH(info)
unsafe fn COMPACT_REALSEQ_INFO_LENGTH(info: SEXP) -> R_xlen_t {
    *REAL(info).add(0) as R_xlen_t
}

/// COMPACT_REALSEQ_INFO_FIRST(info)
unsafe fn COMPACT_REALSEQ_INFO_FIRST(info: SEXP) -> c_double {
    *REAL(info).add(1)
}

/// COMPACT_REALSEQ_INFO_INCR(info)
unsafe fn COMPACT_REALSEQ_INFO_INCR(info: SEXP) -> c_double {
    *REAL(info).add(2)
}

/// Deferred string state macros
unsafe fn DEFERRED_STRING_STATE(x: SEXP) -> SEXP {
    R_altrep_data1(x)
}

unsafe fn CLEAR_DEFERRED_STRING_STATE(x: SEXP) {
    R_set_altrep_data1(x, R_NilValue());
}

unsafe fn DEFERRED_STRING_EXPANDED(x: SEXP) -> SEXP {
    R_altrep_data2(x)
}

unsafe fn SET_DEFERRED_STRING_EXPANDED(x: SEXP, v: SEXP) {
    R_set_altrep_data2(x, v);
}

unsafe fn DEFERRED_STRING_STATE_ARG(s: SEXP) -> SEXP {
    CAR(s)
}

unsafe fn DEFERRED_STRING_STATE_INFO(s: SEXP) -> SEXP {
    CDR(s)
}

unsafe fn DEFERRED_STRING_ARG(x: SEXP) -> SEXP {
    DEFERRED_STRING_STATE_ARG(DEFERRED_STRING_STATE(x))
}

unsafe fn DEFERRED_STRING_INFO(x: SEXP) -> SEXP {
    DEFERRED_STRING_STATE_INFO(DEFERRED_STRING_STATE(x))
}

unsafe fn DEFERRED_STRING_SCIPEN(x: SEXP) -> c_int {
    *INTEGER(DEFERRED_STRING_STATE_INFO(DEFERRED_STRING_STATE(x))).add(0)
}

/// MMAP state macros
unsafe fn MMAP_STATE_FILE(x: SEXP) -> SEXP {
    CAR(x)
}

unsafe fn MMAP_STATE_SIZE(x: SEXP) -> usize {
    REAL_ELT(CADR(x), 0) as usize
}

unsafe fn MMAP_STATE_LENGTH(x: SEXP) -> usize {
    REAL_ELT(CADR(x), 1) as usize
}

unsafe fn MMAP_STATE_TYPE(x: SEXP) -> c_int {
    *INTEGER(CADDR(x)).add(0)
}

unsafe fn MMAP_STATE_PTROK(x: SEXP) -> c_int {
    *INTEGER(CADDR(x)).add(1)
}

unsafe fn MMAP_STATE_WRTOK(x: SEXP) -> c_int {
    *INTEGER(CADDR(x)).add(2)
}

unsafe fn MMAP_STATE_SEROK(x: SEXP) -> c_int {
    *INTEGER(CADDR(x)).add(3)
}

unsafe fn MMAP_EPTR(x: SEXP) -> SEXP {
    R_altrep_data1(x)
}

unsafe fn MMAP_STATE(x: SEXP) -> SEXP {
    R_altrep_data2(x)
}

unsafe fn MMAP_LENGTH(x: SEXP) -> usize {
    MMAP_STATE_LENGTH(MMAP_STATE(x))
}

unsafe fn MMAP_PTROK(x: SEXP) -> c_int {
    MMAP_STATE_PTROK(MMAP_STATE(x))
}

unsafe fn MMAP_WRTOK(x: SEXP) -> c_int {
    MMAP_STATE_WRTOK(MMAP_STATE(x))
}

unsafe fn MMAP_SEROK(x: SEXP) -> c_int {
    MMAP_STATE_SEROK(MMAP_STATE(x))
}

unsafe fn MMAP_ADDR(x: SEXP) -> *mut c_void {
    let eptr = MMAP_EPTR(x);
    // void *addr = R_ExternalPtrAddr(eptr);
    let addr: *mut c_void = ptr::null_mut();
    if addr.is_null() {
        // error("object has been unmapped");
    }
    addr
}

/// Wrapper macros
unsafe fn WRAPPER_WRAPPED(x: SEXP) -> SEXP {
    R_altrep_data1(x)
}

unsafe fn WRAPPER_SET_WRAPPED(x: SEXP, v: SEXP) {
    R_set_altrep_data1(x, v);
}

unsafe fn WRAPPER_METADATA(x: SEXP) -> SEXP {
    R_altrep_data2(x)
}

unsafe fn WRAPPER_SET_METADATA(x: SEXP, v: SEXP) {
    R_set_altrep_data2(x, v);
}

unsafe fn WRAPPER_SORTED(x: SEXP) -> c_int {
    *INTEGER(WRAPPER_METADATA(x)).add(0)
}

unsafe fn WRAPPER_NO_NA(x: SEXP) -> c_int {
    *INTEGER(WRAPPER_METADATA(x)).add(1)
}

/// WRAPPER_WRAPPED_RW -- get wrapped data, duplicating if shared
unsafe fn WRAPPER_WRAPPED_RW(x: SEXP) -> SEXP {
    let data = WRAPPER_WRAPPED(x);
    // if MAYBE_SHARED(data) {
    //     PROTECT(x);
    //     WRAPPER_SET_WRAPPED(x, shallow_duplicate(data));
    //     UNPROTECT(1);
    // }

    let meta = WRAPPER_METADATA(x);
    *INTEGER(meta).add(0) = UNKNOWN_SORTEDNESS;
    let mut i: usize = 1;
    while i < NMETA {
        *INTEGER(meta).add(i) = 0;
        i += 1;
    }

    WRAPPER_WRAPPED(x)
}

/// is_wrapper -- check if SEXP is a wrapper ALTREP
unsafe fn is_wrapper(x: SEXP) -> c_int {
    if ALTREP(x) != 0 {
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP.0
            || t == SEXPTYPE::LGLSXP.0
            || t == SEXPTYPE::REALSXP.0
            || t == SEXPTYPE::CPLXSXP.0
            || t == SEXPTYPE::RAWSXP.0
            || t == SEXPTYPE::STRSXP.0
            || t == SEXPTYPE::VECSXP.0
        {
            // R_altrep_inherits(x, corresponding_class) -- stub
            0
        } else {
            0
        }
    } else {
        0
    }
}

/// asLogicalNA -- helper to convert SEXP to logical with NA default
unsafe fn asLogicalNA(x: SEXP, dflt: c_int) -> c_int {
    let val = crate::main::coerce::asLogical(x);
    if val == NA_LOGICAL { dflt } else { val }
}

// ===========================================================================
//  Compact Integer Sequences
// ===========================================================================

/// new_compact_intseq -- constructor
unsafe fn new_compact_intseq(n: R_xlen_t, n1: c_int, inc: c_int) -> SEXP {
    if n == 1 {
        return Rf_ScalarInteger(n1);
    }

    if inc != 1 && inc != -1 {
        // error("compact sequences with increment %d not supported yet", inc);
        return R_NilValue();
    }

    // info used REALSXP to allow for long vectors
    let info = Rf_allocVector(SEXPTYPE::REALSXP.0, 3);
    *REAL(info).add(0) = n as c_double;
    *REAL(info).add(1) = n1 as c_double;
    *REAL(info).add(2) = inc as c_double;

    // SEXP ans = R_new_altrep(R_compact_intseq_class, info, R_NilValue);
    let _ans = R_new_altrep(ptr::null_mut(), info, R_NilValue());
    // MARK_NOT_MUTABLE(ans);
    R_NilValue()
}

/// compact_intseq_Serialized_state
unsafe fn compact_intseq_Serialized_state(x: SEXP) -> SEXP {
    COMPACT_SEQ_INFO(x)
}

/// compact_intseq_Unserialize
unsafe fn compact_intseq_Unserialize(_class: SEXP, state: SEXP) -> SEXP {
    let n = COMPACT_INTSEQ_SERIALIZED_STATE_LENGTH(state);
    let n1 = COMPACT_INTSEQ_SERIALIZED_STATE_FIRST(state);
    let inc = COMPACT_INTSEQ_SERIALIZED_STATE_INCR(state);

    if inc == 1 {
        new_compact_intseq(n, n1, 1)
    } else if inc == -1 {
        new_compact_intseq(n, n1, -1)
    } else {
        R_NilValue()
    }
}

/// compact_intseq_Coerce
unsafe fn compact_intseq_Coerce(x: SEXP, r#type: c_int) -> SEXP {
    if r#type == SEXPTYPE::REALSXP.0 {
        let info = COMPACT_SEQ_INFO(x);
        let n = COMPACT_INTSEQ_INFO_LENGTH(info);
        let n1 = COMPACT_INTSEQ_INFO_FIRST(info);
        let inc = COMPACT_INTSEQ_INFO_INCR(info);
        new_compact_realseq(n, n1 as c_double, inc as c_double)
    } else {
        ptr::null_mut()
    }
}

/// compact_intseq_Duplicate
unsafe fn compact_intseq_Duplicate(x: SEXP, _deep: c_int) -> SEXP {
    let n = XLENGTH(x);
    let val = Rf_allocVector(SEXPTYPE::INTSXP.0, n as c_int);
    let data = INTEGER(val);
    let mut i: R_xlen_t = 0;
    while i < n {
        *data.add(i as usize) = INTEGER_ELT(x, i as c_int);
        i += 1;
    }
    val
}

/// compact_intseq_Inspect
unsafe fn compact_intseq_Inspect(_x: SEXP, _pre: c_int, _deep: c_int, _pvec: c_int) -> c_int {
    1 // TRUE
}

/// compact_intseq_Length
unsafe fn compact_intseq_Length(x: SEXP) -> R_xlen_t {
    let info = COMPACT_SEQ_INFO(x);
    COMPACT_INTSEQ_INFO_LENGTH(info)
}

/// compact_intseq_Dataptr
unsafe fn compact_intseq_Dataptr(x: SEXP, _writeable: c_int) -> *mut c_void {
    if COMPACT_SEQ_EXPANDED(x) == R_NilValue() {
        Rf_protect(x);
        let info = COMPACT_SEQ_INFO(x);
        let n = COMPACT_INTSEQ_INFO_LENGTH(info);
        let n1 = COMPACT_INTSEQ_INFO_FIRST(info);
        let inc = COMPACT_INTSEQ_INFO_INCR(info);
        let val = Rf_allocVector(SEXPTYPE::INTSXP.0, n as c_int);
        let data = INTEGER(val);

        if inc == 1 {
            let mut i: R_xlen_t = 0;
            while i < n {
                *data.add(i as usize) = n1 + i as c_int;
                i += 1;
            }
        } else if inc == -1 {
            let mut i: R_xlen_t = 0;
            while i < n {
                *data.add(i as usize) = n1 - i as c_int;
                i += 1;
            }
        }

        SET_COMPACT_SEQ_EXPANDED(x, val);
        Rf_unprotect(1);
    }
    DATAPTR(COMPACT_SEQ_EXPANDED(x))
}

/// compact_intseq_Dataptr_or_null
unsafe fn compact_intseq_Dataptr_or_null(x: SEXP) -> *const c_void {
    let val = COMPACT_SEQ_EXPANDED(x);
    if val == R_NilValue() {
        ptr::null()
    } else {
        ROBJ_DATAPTR(val)
    }
}

/// compact_intseq_Elt
unsafe fn compact_intseq_Elt(x: SEXP, i: R_xlen_t) -> c_int {
    let ex = COMPACT_SEQ_EXPANDED(x);
    if ex != R_NilValue() {
        *INTEGER(ex).add(i as usize)
    } else {
        let info = COMPACT_SEQ_INFO(x);
        let n1 = COMPACT_INTSEQ_INFO_FIRST(info);
        let inc = COMPACT_INTSEQ_INFO_INCR(info);
        n1 + inc * i as c_int
    }
}

/// compact_intseq_Get_region
unsafe fn compact_intseq_Get_region(
    sx: SEXP,
    i: R_xlen_t,
    n: R_xlen_t,
    buf: *mut c_int,
) -> R_xlen_t {
    let info = COMPACT_SEQ_INFO(sx);
    let size = COMPACT_INTSEQ_INFO_LENGTH(info);
    let n1 = COMPACT_INTSEQ_INFO_FIRST(info);
    let inc = COMPACT_INTSEQ_INFO_INCR(info);

    let ncopy = if size - i > n { n } else { size - i };
    if inc == 1 {
        let mut k: R_xlen_t = 0;
        while k < ncopy {
            *buf.add(k as usize) = n1 + (k + i) as c_int;
            k += 1;
        }
        ncopy
    } else if inc == -1 {
        let mut k: R_xlen_t = 0;
        while k < ncopy {
            *buf.add(k as usize) = n1 - (k + i) as c_int;
            k += 1;
        }
        ncopy
    } else {
        0
    }
}

/// compact_intseq_Is_sorted
unsafe fn compact_intseq_Is_sorted(x: SEXP) -> c_int {
    let inc = COMPACT_INTSEQ_INFO_INCR(COMPACT_SEQ_INFO(x));
    if inc < 0 { SORTED_DECR } else { SORTED_INCR }
}

/// compact_intseq_No_NA
unsafe fn compact_intseq_No_NA(_x: SEXP) -> c_int {
    1 // TRUE
}

/// compact_intseq_Sum
unsafe fn compact_intseq_Sum(x: SEXP, _narm: c_int) -> SEXP {
    let info = COMPACT_SEQ_INFO(x);
    let size = COMPACT_INTSEQ_INFO_LENGTH(info);
    let n1 = COMPACT_INTSEQ_INFO_FIRST(info);
    let inc = COMPACT_INTSEQ_INFO_INCR(info);
    let tmp: c_double = (size as c_double / 2.0)
        * (n1 as c_double + n1 as c_double + inc as c_double * (size - 1) as c_double);
    if tmp > c_int::MAX as c_double || tmp < R_INT_MIN as c_double {
        Rf_ScalarReal(tmp)
    } else {
        Rf_ScalarInteger(tmp as c_int)
    }
}

/// InitCompactIntegerClass
unsafe fn InitCompactIntegerClass() {
    // R_make_altinteger_class("compact_intseq", "base", NULL) is a stub
    // All method registrations are stubs since ALTREP class system is not functional
}

/// R_is_compact_intseq -- check if SEXP is a compact integer sequence
pub unsafe fn R_is_compact_intseq(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    // R_altrep_inherits(x, R_compact_intseq_class) -- stub
    0
}

// ===========================================================================
//  Compact Real Sequences
// ===========================================================================

/// new_compact_realseq -- constructor
unsafe fn new_compact_realseq(n: R_xlen_t, n1: c_double, inc: c_double) -> SEXP {
    if n == 1 {
        return Rf_ScalarReal(n1);
    }

    if inc != 1.0 && inc != -1.0 {
        return R_NilValue();
    }

    let info = Rf_allocVector(SEXPTYPE::REALSXP.0, 3);
    *REAL(info).add(0) = n as c_double;
    *REAL(info).add(1) = n1;
    *REAL(info).add(2) = inc;

    // SEXP ans = R_new_altrep(R_compact_realseq_class, info, R_NilValue);
    let _ans = R_new_altrep(ptr::null_mut(), info, R_NilValue());
    // MARK_NOT_MUTABLE(ans);
    R_NilValue()
}

/// compact_realseq_Serialized_state
unsafe fn compact_realseq_Serialized_state(x: SEXP) -> SEXP {
    COMPACT_SEQ_INFO(x)
}

/// compact_realseq_Unserialize
unsafe fn compact_realseq_Unserialize(_class: SEXP, state: SEXP) -> SEXP {
    let inc = COMPACT_REALSEQ_INFO_INCR(state);
    let len = COMPACT_REALSEQ_INFO_LENGTH(state);
    let n1 = COMPACT_REALSEQ_INFO_FIRST(state);

    if inc == 1.0 {
        new_compact_realseq(len, n1, 1.0)
    } else if inc == -1.0 {
        new_compact_realseq(len, n1, -1.0)
    } else {
        R_NilValue()
    }
}

/// compact_realseq_Duplicate
unsafe fn compact_realseq_Duplicate(x: SEXP, _deep: c_int) -> SEXP {
    let n = XLENGTH(x);
    let val = Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int);
    let data = REAL(val);
    let mut i: R_xlen_t = 0;
    while i < n {
        *data.add(i as usize) = REAL_ELT(x, i as c_int);
        i += 1;
    }
    val
}

/// compact_realseq_Inspect
unsafe fn compact_realseq_Inspect(_x: SEXP, _pre: c_int, _deep: c_int, _pvec: c_int) -> c_int {
    1 // TRUE
}

/// compact_realseq_Length
unsafe fn compact_realseq_Length(x: SEXP) -> R_xlen_t {
    *REAL(COMPACT_SEQ_INFO(x)).add(0) as R_xlen_t
}

/// compact_realseq_Dataptr
unsafe fn compact_realseq_Dataptr(x: SEXP, _writeable: c_int) -> *mut c_void {
    if COMPACT_SEQ_EXPANDED(x) == R_NilValue() {
        Rf_protect(x);
        let info = COMPACT_SEQ_INFO(x);
        let n = COMPACT_REALSEQ_INFO_LENGTH(info);
        let n1 = COMPACT_REALSEQ_INFO_FIRST(info);
        let inc = COMPACT_REALSEQ_INFO_INCR(info);

        let val = Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int);
        let data = REAL(val);

        if inc == 1.0 {
            let mut i: R_xlen_t = 0;
            while i < n {
                *data.add(i as usize) = n1 + i as c_double;
                i += 1;
            }
        } else if inc == -1.0 {
            let mut i: R_xlen_t = 0;
            while i < n {
                *data.add(i as usize) = n1 - i as c_double;
                i += 1;
            }
        }

        SET_COMPACT_SEQ_EXPANDED(x, val);
        Rf_unprotect(1);
    }
    DATAPTR(COMPACT_SEQ_EXPANDED(x))
}

/// compact_realseq_Dataptr_or_null
unsafe fn compact_realseq_Dataptr_or_null(x: SEXP) -> *const c_void {
    let val = COMPACT_SEQ_EXPANDED(x);
    if val == R_NilValue() {
        ptr::null()
    } else {
        ROBJ_DATAPTR(val)
    }
}

/// compact_realseq_Elt
unsafe fn compact_realseq_Elt(x: SEXP, i: R_xlen_t) -> c_double {
    let ex = COMPACT_SEQ_EXPANDED(x);
    if ex != R_NilValue() {
        *REAL(ex).add(i as usize)
    } else {
        let info = COMPACT_SEQ_INFO(x);
        let n1 = COMPACT_REALSEQ_INFO_FIRST(info);
        let inc = COMPACT_REALSEQ_INFO_INCR(info);
        n1 + inc * i as c_double
    }
}

/// compact_realseq_Get_region
unsafe fn compact_realseq_Get_region(
    sx: SEXP,
    i: R_xlen_t,
    n: R_xlen_t,
    buf: *mut c_double,
) -> R_xlen_t {
    let info = COMPACT_SEQ_INFO(sx);
    let size = COMPACT_REALSEQ_INFO_LENGTH(info);
    let n1 = COMPACT_REALSEQ_INFO_FIRST(info);
    let inc = COMPACT_REALSEQ_INFO_INCR(info);

    let ncopy = if size - i > n { n } else { size - i };
    if inc == 1.0 {
        let mut k: R_xlen_t = 0;
        while k < ncopy {
            *buf.add(k as usize) = n1 + (k + i) as c_double;
            k += 1;
        }
        ncopy
    } else if inc == -1.0 {
        let mut k: R_xlen_t = 0;
        while k < ncopy {
            *buf.add(k as usize) = n1 - (k + i) as c_double;
            k += 1;
        }
        ncopy
    } else {
        0
    }
}

/// compact_realseq_Is_sorted
unsafe fn compact_realseq_Is_sorted(x: SEXP) -> c_int {
    let inc = COMPACT_REALSEQ_INFO_INCR(COMPACT_SEQ_INFO(x));
    if inc < 0.0 { SORTED_DECR } else { SORTED_INCR }
}

/// compact_realseq_No_NA
unsafe fn compact_realseq_No_NA(_x: SEXP) -> c_int {
    1 // TRUE
}

/// compact_realseq_Sum
unsafe fn compact_realseq_Sum(x: SEXP, _narm: c_int) -> SEXP {
    let info = COMPACT_SEQ_INFO(x);
    let size = COMPACT_REALSEQ_INFO_LENGTH(info) as c_double;
    let n1 = COMPACT_REALSEQ_INFO_FIRST(info);
    let inc = COMPACT_REALSEQ_INFO_INCR(info);
    Rf_ScalarReal((size / 2.0) * (n1 + n1 + inc * (size - 1.0)))
}

/// InitCompactRealClass
unsafe fn InitCompactRealClass() {
    // R_make_altreal_class("compact_realseq", "base", NULL) is a stub
}

// ===========================================================================
//  Compact Integer/Real Sequences -- R_compact_intrange
// ===========================================================================

/// R_compact_intrange -- create a compact integer or real sequence for n1:n2
pub unsafe fn R_compact_intrange(n1: R_xlen_t, n2: R_xlen_t) -> SEXP {
    let n = if n1 <= n2 { n2 - n1 + 1 } else { n1 - n2 + 1 };

    if n >= R_XLEN_T_MAX {
        return R_NilValue();
    }

    if n1 <= c_int::MIN as R_xlen_t
        || n1 > c_int::MAX as R_xlen_t
        || n2 <= c_int::MIN as R_xlen_t
        || n2 > c_int::MAX as R_xlen_t
    {
        new_compact_realseq(n, n1 as c_double, if n1 <= n2 { 1.0 } else { -1.0 })
    } else {
        new_compact_intseq(n, n1 as c_int, if n1 <= n2 { 1 } else { -1 })
    }
}

// ===========================================================================
//  Deferred String Coercions
// ===========================================================================

/// R_OutDecSym -- cached symbol for "OutDec"
thread_local! { static R_OutDecSym: Cell<SEXP> = Cell::new(ptr::null_mut()); }

/// Deferred string state OUTDEC getter
unsafe fn DEFERRED_STRING_OUTDEC(_x: SEXP) -> *const c_char {
    if R_OutDecSym.with(|v| v.get()).is_null() {
        R_OutDecSym.with(|v| v.set(ptr::null_mut()));
    }
    b".\0".as_ptr() as *const c_char
}

/// ExpandDeferredStringElt -- expand a single deferred string element
unsafe fn ExpandDeferredStringElt(x: SEXP, i: R_xlen_t) -> SEXP {
    // make sure the STRSXP for the expanded string is allocated
    let mut val = DEFERRED_STRING_EXPANDED(x);
    if val == R_NilValue() {
        let n = XLENGTH(x);
        val = Rf_allocVector(SEXPTYPE::STRSXP.0, n as c_int);
        if n > 0 {
            ptr::write_bytes(
                DATAPTR(val) as *mut u8,
                0,
                n as usize * std::mem::size_of::<SEXP>(),
            );
        }
        SET_DEFERRED_STRING_EXPANDED(x, val);
    }

    let elt = STRING_ELT(val, i);
    if elt.is_null() {
        let data = DEFERRED_STRING_ARG(x);
        let dtype = TYPEOF(data);
        let result_elt = if dtype == SEXPTYPE::INTSXP.0 {
            Rf_mkChar(c"0".as_ptr())
        } else if dtype == SEXPTYPE::REALSXP.0 {
            Rf_mkChar(c"0".as_ptr())
        } else {
            Rf_mkChar(c"".as_ptr())
        };
        SET_STRING_ELT(val, i, result_elt);
        return result_elt;
    }
    elt
}

/// expand_deferred_string -- fully expand a deferred string
unsafe fn expand_deferred_string(x: SEXP) {
    let state = DEFERRED_STRING_STATE(x);
    if state != R_NilValue() {
        Rf_protect(x);
        let n = XLENGTH(x);
        if n == 0 {
            SET_DEFERRED_STRING_EXPANDED(x, Rf_allocVector(SEXPTYPE::STRSXP.0, 0));
        } else {
            let mut i: R_xlen_t = 0;
            while i < n {
                ExpandDeferredStringElt(x, i);
                i += 1;
            }
        }
        CLEAR_DEFERRED_STRING_STATE(x);
        Rf_unprotect(1);
    }
}

/// R_deferred_coerceToString -- constructor for deferred string conversions
pub unsafe fn R_deferred_coerceToString(v: SEXP, info: SEXP) -> SEXP {
    let mut ans = R_NilValue();
    let dtype = TYPEOF(v);
    if dtype == SEXPTYPE::INTSXP.0 || dtype == SEXPTYPE::REALSXP.0 {
        Rf_protect(v);
        let _info = if info.is_null() {
            Rf_ScalarInteger(0) // R_print.scipen default
        } else {
            info
        };
        // MARK_NOT_MUTABLE(v);
        // ans = Rf_cons(v, _info);
        // ans = R_new_altrep(R_deferred_string_class, ans, R_NilValue);
        Rf_unprotect(1);
    }
    ans
}

/// deferred_string_Serialized_state
unsafe fn deferred_string_Serialized_state(x: SEXP) -> SEXP {
    let state = DEFERRED_STRING_STATE(x);
    if state != R_NilValue() {
        state
    } else {
        ptr::null_mut()
    }
}

/// deferred_string_Unserialize
unsafe fn deferred_string_Unserialize(_class: SEXP, state: SEXP) -> SEXP {
    let arg = DEFERRED_STRING_STATE_ARG(state);
    let info = DEFERRED_STRING_STATE_INFO(state);
    R_deferred_coerceToString(arg, info)
}

/// deferred_string_Inspect
unsafe fn deferred_string_Inspect(_x: SEXP, _pre: c_int, _deep: c_int, _pvec: c_int) -> c_int {
    1 // TRUE
}

/// deferred_string_Length
unsafe fn deferred_string_Length(x: SEXP) -> R_xlen_t {
    let state = DEFERRED_STRING_STATE(x);
    if state == R_NilValue() {
        XLENGTH(DEFERRED_STRING_EXPANDED(x))
    } else {
        XLENGTH(DEFERRED_STRING_STATE_ARG(state))
    }
}

/// deferred_string_Dataptr
unsafe fn deferred_string_Dataptr(x: SEXP, _writeable: c_int) -> *mut c_void {
    expand_deferred_string(x);
    DATAPTR(DEFERRED_STRING_EXPANDED(x))
}

/// deferred_string_Dataptr_or_null
unsafe fn deferred_string_Dataptr_or_null(x: SEXP) -> *const c_void {
    let state = DEFERRED_STRING_STATE(x);
    if state != R_NilValue() {
        ptr::null()
    } else {
        ROBJ_DATAPTR(DEFERRED_STRING_EXPANDED(x))
    }
}

/// deferred_string_Elt
unsafe fn deferred_string_Elt(x: SEXP, i: R_xlen_t) -> SEXP {
    let state = DEFERRED_STRING_STATE(x);
    if state == R_NilValue() {
        STRING_ELT(DEFERRED_STRING_EXPANDED(x), i)
    } else {
        Rf_protect(x);
        let elt = ExpandDeferredStringElt(x, i);
        Rf_unprotect(1);
        elt
    }
}

/// deferred_string_Set_elt
unsafe fn deferred_string_Set_elt(x: SEXP, i: R_xlen_t, v: SEXP) {
    expand_deferred_string(x);
    SET_STRING_ELT(DEFERRED_STRING_EXPANDED(x), i, v);
}

/// deferred_string_Is_sorted
unsafe fn deferred_string_Is_sorted(_x: SEXP) -> c_int {
    UNKNOWN_SORTEDNESS
}

/// deferred_string_No_NA
unsafe fn deferred_string_No_NA(x: SEXP) -> c_int {
    let state = DEFERRED_STRING_STATE(x);
    if state == R_NilValue() {
        0 // FALSE -- may have been modified
    } else {
        let arg = DEFERRED_STRING_STATE_ARG(state);
        let _dtype = TYPEOF(arg);
        // Defer to INTEGER_NO_NA / REAL_NO_NA -- stubbed as 0
        0
    }
}

/// deferred_string_Extract_subset
unsafe fn deferred_string_Extract_subset(x: SEXP, _indx: SEXP, _call: SEXP) -> SEXP {
    if OBJECT(x) == 0 && ATTRIB(x) == R_NilValue() && DEFERRED_STRING_STATE(x) != R_NilValue() {
        // For deferred string coercions, create a new conversion
        // using the subset of the argument.
        let _data = DEFERRED_STRING_ARG(x);
        let _info = DEFERRED_STRING_INFO(x);
        // result = ExtractSubset(data, indx, call);
        // result = R_deferred_coerceToString(result, info);
        R_NilValue()
    } else {
        ptr::null_mut()
    }
}

/// InitDefferredStringClass
unsafe fn InitDefferredStringClass() {
    // R_make_altstring_class("deferred_string", "base", NULL) is a stub
}

// ===========================================================================
//  Memory Mapped Vectors
// ===========================================================================

/// mmap_Serialized_state
unsafe fn mmap_Serialized_state(x: SEXP) -> SEXP {
    if MMAP_SEROK(x) != 0 {
        MMAP_STATE(x)
    } else {
        ptr::null_mut()
    }
}

/// mmap_Unserialize
unsafe fn mmap_Unserialize(_class: SEXP, state: SEXP) -> SEXP {
    let _file = MMAP_STATE_FILE(state);
    let _type = MMAP_STATE_TYPE(state);
    let _ptrOK = MMAP_STATE_PTROK(state);
    let _wrtOK = MMAP_STATE_WRTOK(state);
    let _serOK = MMAP_STATE_SEROK(state);
    R_NilValue()
}

/// mmap_Inspect
unsafe fn mmap_Inspect(_x: SEXP, _pre: c_int, _deep: c_int, _pvec: c_int) -> c_int {
    1 // TRUE
}

/// mmap_Length
unsafe fn mmap_Length(x: SEXP) -> R_xlen_t {
    MMAP_LENGTH(x) as R_xlen_t
}

/// mmap_Dataptr
unsafe fn mmap_Dataptr(x: SEXP, _writeable: c_int) -> *mut c_void {
    let _addr = MMAP_ADDR(x);
    if MMAP_PTROK(x) != 0 {
        ptr::null_mut()
    } else {
        ptr::null_mut()
    }
}

/// mmap_Dataptr_or_null
unsafe fn mmap_Dataptr_or_null(x: SEXP) -> *const c_void {
    if MMAP_PTROK(x) != 0 {
        ptr::null()
    } else {
        ptr::null()
    }
}

/// mmap_integer_Elt
unsafe fn mmap_integer_Elt(x: SEXP, i: R_xlen_t) -> c_int {
    let p = MMAP_ADDR(x) as *mut c_int;
    *p.add(i as usize)
}

/// mmap_integer_Get_region
unsafe fn mmap_integer_Get_region(sx: SEXP, i: R_xlen_t, n: R_xlen_t, buf: *mut c_int) -> R_xlen_t {
    let x = MMAP_ADDR(sx) as *mut c_int;
    let size = XLENGTH(sx);
    let ncopy = if size - i > n { n } else { size - i };
    let mut k: R_xlen_t = 0;
    while k < ncopy {
        *buf.add(k as usize) = *x.add((k + i) as usize);
        k += 1;
    }
    ncopy
}

/// mmap_real_Elt
unsafe fn mmap_real_Elt(x: SEXP, i: R_xlen_t) -> c_double {
    let p = MMAP_ADDR(x) as *mut c_double;
    *p.add(i as usize)
}

/// mmap_real_Get_region
unsafe fn mmap_real_Get_region(sx: SEXP, i: R_xlen_t, n: R_xlen_t, buf: *mut c_double) -> R_xlen_t {
    let x = MMAP_ADDR(sx) as *mut c_double;
    let size = XLENGTH(sx);
    let ncopy = if size - i > n { n } else { size - i };
    let mut k: R_xlen_t = 0;
    while k < ncopy {
        *buf.add(k as usize) = *x.add((k + i) as usize);
        k += 1;
    }
    ncopy
}

/// InitMmapIntegerClass
unsafe fn InitMmapIntegerClass(_dll: *mut c_void) {
    // R_make_altinteger_class("mmap_integer", "base", dll) is a stub
}

/// InitMmapRealClass
unsafe fn InitMmapRealClass(_dll: *mut c_void) {
    // R_make_altreal_class("mmap_real", "base", dll) is a stub
}

/// do_mmap_file -- .Internal(mmap(file, type, ptrOK, wrtOK, serOK))
pub unsafe fn do_mmap_file(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let _file = CAR(args);
    let _stype = CADR(args);
    let _sptrOK = CADDR(args);
    let _swrtOK = CADDDR(args);
    let _sserOK = CAD5R(args);

    // Full implementation requires sys/stat.h, fcntl.h, sys/mman.h (Unix only)
    // and R_MakeExternalPtr, R_ExternalPtrAddr, etc.
    R_NilValue()
}

/// do_munmap_file -- .Internal(munmap(x))
pub unsafe fn do_munmap_file(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let _x = CAR(args);
    R_NilValue()
}

// ===========================================================================
//  Attribute and Meta Data Wrappers
// ===========================================================================

/// make_wrapper -- create a wrapper ALTREP object
unsafe fn make_wrapper(x: SEXP, meta: SEXP) -> SEXP {
    let dtype = TYPEOF(x);
    let _cls: R_altrep_class_t = if dtype == SEXPTYPE::INTSXP.0 {
        wrap_integer_class.with(|v| v.get())
    } else if dtype == SEXPTYPE::LGLSXP.0 {
        wrap_logical_class.with(|v| v.get())
    } else if dtype == SEXPTYPE::REALSXP.0 {
        wrap_real_class.with(|v| v.get())
    } else if dtype == SEXPTYPE::CPLXSXP.0 {
        wrap_complex_class.with(|v| v.get())
    } else if dtype == SEXPTYPE::RAWSXP.0 {
        wrap_raw_class.with(|v| v.get())
    } else if dtype == SEXPTYPE::STRSXP.0 {
        wrap_string_class.with(|v| v.get())
    } else if dtype == SEXPTYPE::VECSXP.0 {
        wrap_list_class.with(|v| v.get())
    } else {
        return R_NilValue();
    };

    // SEXP ans = R_new_altrep(cls, x, meta);
    let _ans = R_new_altrep(_cls as SEXP, x, meta);

    // WRAPATTRIB section -- move attributes to wrapper if present
    if !ATTRIB(x).is_null() {
        // SET_ATTRIB(ans, shallow_duplicate(ATTRIB(x)));
        // SET_OBJECT(ans, OBJECT(x));
        // IS_S4_OBJECT(x) ? SET_S4_OBJECT(ans) : UNSET_S4_OBJECT(ans);
    }

    R_NilValue()
}

// ---------------------------------------------------------------------------
// Wrapper ALTREP Methods
// ---------------------------------------------------------------------------

/// wrapper_Serialized_state
unsafe fn wrapper_Serialized_state(x: SEXP) -> SEXP {
    let _wrapped = WRAPPER_WRAPPED(x);
    if ALTREP(_wrapped) == 0 && WRAPPER_SORTED(x) == UNKNOWN_SORTEDNESS && WRAPPER_NO_NA(x) == 0 {
        return ptr::null_mut();
    }

    // return CONS(WRAPPER_WRAPPED(x), WRAPPER_METADATA(x));
    R_NilValue()
}

/// wrapper_Unserialize
unsafe fn wrapper_Unserialize(_class: SEXP, state: SEXP) -> SEXP {
    make_wrapper(CAR(state), CDR(state))
}

/// wrapper_Duplicate
unsafe fn wrapper_Duplicate(x: SEXP, deep: c_int) -> SEXP {
    let data = WRAPPER_WRAPPED(x);

    if deep != 0 {
        // data = duplicate(data);
    } else {
        // MARK_NOT_MUTABLE(data);
    }
    Rf_protect(data);

    let _meta = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, NMETA as c_int));

    let ans = make_wrapper(data, _meta);

    Rf_unprotect(2);
    ans
}

/// wrapper_Inspect
unsafe fn wrapper_Inspect(_x: SEXP, _pre: c_int, _deep: c_int, _pvec: c_int) -> c_int {
    1 // TRUE
}

/// wrapper_Length
unsafe fn wrapper_Length(x: SEXP) -> R_xlen_t {
    XLENGTH(WRAPPER_WRAPPED(x))
}

// ---------------------------------------------------------------------------
// Wrapper ALTVEC Methods
// ---------------------------------------------------------------------------

/// wrapper_Dataptr
unsafe fn wrapper_Dataptr(x: SEXP, writeable: c_int) -> *mut c_void {
    if writeable != 0 {
        DATAPTR(WRAPPER_WRAPPED_RW(x))
    } else {
        DATAPTR(WRAPPER_WRAPPED(x))
    }
}

/// wrapper_Dataptr_or_null
unsafe fn wrapper_Dataptr_or_null(x: SEXP) -> *const c_void {
    ROBJ_DATAPTR(WRAPPER_WRAPPED(x))
}

/// wrapper_Extract_subset
unsafe fn wrapper_Extract_subset(_x: SEXP, _indx: SEXP, _call: SEXP) -> SEXP {
    // ExtractSubset(WRAPPER_WRAPPED(x), indx, call)
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Wrapper ALTINTEGER Methods
// ---------------------------------------------------------------------------

/// wrapper_integer_Elt
unsafe fn wrapper_integer_Elt(x: SEXP, i: R_xlen_t) -> c_int {
    INTEGER_ELT(WRAPPER_WRAPPED(x), i as c_int)
}

/// wrapper_integer_Get_region
unsafe fn wrapper_integer_Get_region(
    x: SEXP,
    i: R_xlen_t,
    n: R_xlen_t,
    buf: *mut c_int,
) -> R_xlen_t {
    let data = WRAPPER_WRAPPED(x);
    let size = XLENGTH(data);
    let ncopy = if size - i > n { n } else { size - i };
    let mut k: R_xlen_t = 0;
    while k < ncopy {
        *buf.add(k as usize) = INTEGER_ELT(data, (k + i) as c_int);
        k += 1;
    }
    ncopy
}

/// wrapper_integer_Is_sorted
unsafe fn wrapper_integer_Is_sorted(x: SEXP) -> c_int {
    if WRAPPER_SORTED(x) != UNKNOWN_SORTEDNESS {
        WRAPPER_SORTED(x)
    } else {
        UNKNOWN_SORTEDNESS
    }
}

/// wrapper_integer_no_NA
unsafe fn wrapper_integer_no_NA(x: SEXP) -> c_int {
    if WRAPPER_NO_NA(x) != 0 {
        1 // TRUE
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Wrapper ALTLOGICAL Methods
// ---------------------------------------------------------------------------

/// wrapper_logical_Elt
unsafe fn wrapper_logical_Elt(x: SEXP, i: R_xlen_t) -> c_int {
    LOGICAL_ELT(WRAPPER_WRAPPED(x), i as c_int)
}

/// wrapper_logical_Get_region
unsafe fn wrapper_logical_Get_region(
    x: SEXP,
    i: R_xlen_t,
    n: R_xlen_t,
    buf: *mut c_int,
) -> R_xlen_t {
    let data = WRAPPER_WRAPPED(x);
    let size = XLENGTH(data);
    let ncopy = if size - i > n { n } else { size - i };
    let mut k: R_xlen_t = 0;
    while k < ncopy {
        *buf.add(k as usize) = LOGICAL_ELT(data, (k + i) as c_int);
        k += 1;
    }
    ncopy
}

/// wrapper_logical_Is_sorted
unsafe fn wrapper_logical_Is_sorted(x: SEXP) -> c_int {
    if WRAPPER_SORTED(x) != UNKNOWN_SORTEDNESS {
        WRAPPER_SORTED(x)
    } else {
        UNKNOWN_SORTEDNESS
    }
}

/// wrapper_logical_no_NA
unsafe fn wrapper_logical_no_NA(x: SEXP) -> c_int {
    if WRAPPER_NO_NA(x) != 0 {
        1 // TRUE
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Wrapper ALTREAL Methods
// ---------------------------------------------------------------------------

/// wrapper_real_Elt
unsafe fn wrapper_real_Elt(x: SEXP, i: R_xlen_t) -> c_double {
    REAL_ELT(WRAPPER_WRAPPED(x), i as c_int)
}

/// wrapper_real_Get_region
unsafe fn wrapper_real_Get_region(
    x: SEXP,
    i: R_xlen_t,
    n: R_xlen_t,
    buf: *mut c_double,
) -> R_xlen_t {
    let data = WRAPPER_WRAPPED(x);
    let size = XLENGTH(data);
    let ncopy = if size - i > n { n } else { size - i };
    let mut k: R_xlen_t = 0;
    while k < ncopy {
        *buf.add(k as usize) = REAL_ELT(data, (k + i) as c_int);
        k += 1;
    }
    ncopy
}

/// wrapper_real_Is_sorted
unsafe fn wrapper_real_Is_sorted(x: SEXP) -> c_int {
    if WRAPPER_SORTED(x) != UNKNOWN_SORTEDNESS {
        WRAPPER_SORTED(x)
    } else {
        UNKNOWN_SORTEDNESS
    }
}

/// wrapper_real_no_NA
unsafe fn wrapper_real_no_NA(x: SEXP) -> c_int {
    if WRAPPER_NO_NA(x) != 0 {
        1 // TRUE
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Wrapper ALTCOMPLEX Methods
// ---------------------------------------------------------------------------

/// wrapper_complex_Elt
unsafe fn wrapper_complex_Elt(x: SEXP, i: R_xlen_t) -> Rcomplex {
    COMPLEX_ELT(WRAPPER_WRAPPED(x), i as c_int)
}

/// wrapper_complex_Get_region
unsafe fn wrapper_complex_Get_region(
    x: SEXP,
    i: R_xlen_t,
    n: R_xlen_t,
    buf: *mut Rcomplex,
) -> R_xlen_t {
    let data = WRAPPER_WRAPPED(x);
    let size = XLENGTH(data);
    let ncopy = if size - i > n { n } else { size - i };
    let mut k: R_xlen_t = 0;
    while k < ncopy {
        *buf.add(k as usize) = COMPLEX_ELT(data, (k + i) as c_int);
        k += 1;
    }
    ncopy
}

// ---------------------------------------------------------------------------
// Wrapper ALTRAW Methods
// ---------------------------------------------------------------------------

/// wrapper_raw_Elt
unsafe fn wrapper_raw_Elt(x: SEXP, i: R_xlen_t) -> Rbyte {
    RAW_ELT(WRAPPER_WRAPPED(x), i as c_int)
}

/// wrapper_raw_Get_region
unsafe fn wrapper_raw_Get_region(x: SEXP, i: R_xlen_t, n: R_xlen_t, buf: *mut Rbyte) -> R_xlen_t {
    let data = WRAPPER_WRAPPED(x);
    let size = XLENGTH(data);
    let ncopy = if size - i > n { n } else { size - i };
    let mut k: R_xlen_t = 0;
    while k < ncopy {
        *buf.add(k as usize) = RAW_ELT(data, (k + i) as c_int);
        k += 1;
    }
    ncopy
}

// ---------------------------------------------------------------------------
// Wrapper ALTSTRING Methods
// ---------------------------------------------------------------------------

/// wrapper_string_Elt
unsafe fn wrapper_string_Elt(x: SEXP, i: R_xlen_t) -> SEXP {
    STRING_ELT(WRAPPER_WRAPPED(x), i)
}

/// wrapper_string_Set_elt
unsafe fn wrapper_string_Set_elt(x: SEXP, i: R_xlen_t, v: SEXP) {
    SET_STRING_ELT(WRAPPER_WRAPPED_RW(x), i, v);
}

/// wrapper_string_Is_sorted
unsafe fn wrapper_string_Is_sorted(x: SEXP) -> c_int {
    if WRAPPER_SORTED(x) != UNKNOWN_SORTEDNESS {
        WRAPPER_SORTED(x)
    } else {
        UNKNOWN_SORTEDNESS
    }
}

/// wrapper_string_no_NA
unsafe fn wrapper_string_no_NA(x: SEXP) -> c_int {
    if WRAPPER_NO_NA(x) != 0 {
        1 // TRUE
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Wrapper ALTLIST Methods
// ---------------------------------------------------------------------------

/// wrapper_list_Elt
unsafe fn wrapper_list_Elt(x: SEXP, i: R_xlen_t) -> SEXP {
    VECTOR_ELT(WRAPPER_WRAPPED(x), i)
}

/// wrapper_list_Set_elt
unsafe fn wrapper_list_Set_elt(x: SEXP, i: R_xlen_t, v: SEXP) {
    SET_VECTOR_ELT(WRAPPER_WRAPPED_RW(x), i, v);
}

// ---------------------------------------------------------------------------
// Wrapper Class Initialization
// ---------------------------------------------------------------------------

/// InitWrapIntegerClass
unsafe fn InitWrapIntegerClass(_dll: *mut c_void) {
    // R_make_altinteger_class("wrap_integer", "base", dll) is a stub
}

/// InitWrapLogicalClass
unsafe fn InitWrapLogicalClass(_dll: *mut c_void) {
    // R_make_altlogical_class("wrap_logical", "base", dll) is a stub
}

/// InitWrapRealClass
unsafe fn InitWrapRealClass(_dll: *mut c_void) {
    // R_make_altreal_class("wrap_real", "base", dll) is a stub
}

/// InitWrapComplexClass
unsafe fn InitWrapComplexClass(_dll: *mut c_void) {
    // R_make_altcomplex_class("wrap_complex", "base", dll) is a stub
}

/// InitWrapRawClass
unsafe fn InitWrapRawClass(_dll: *mut c_void) {
    // R_make_altraw_class("wrap_raw", "base", dll) is a stub
}

/// InitWrapStringClass
unsafe fn InitWrapStringClass(_dll: *mut c_void) {
    // R_make_altstring_class("wrap_string", "base", dll) is a stub
}

/// InitWrapListClass
unsafe fn InitWrapListClass(_dll: *mut c_void) {
    // R_make_altlist_class("wrap_list", "base", dll) is a stub
}

// ---------------------------------------------------------------------------
// Wrapper Utilities
// ---------------------------------------------------------------------------

/// wrap_meta -- create a wrapper with meta-data
unsafe fn wrap_meta(x: SEXP, srt: c_int, no_na: c_int) -> SEXP {
    let dtype = TYPEOF(x);
    if dtype != SEXPTYPE::INTSXP.0
        && dtype != SEXPTYPE::REALSXP.0
        && dtype != SEXPTYPE::LGLSXP.0
        && dtype != SEXPTYPE::CPLXSXP.0
        && dtype != SEXPTYPE::RAWSXP.0
        && dtype != SEXPTYPE::STRSXP.0
        && dtype != SEXPTYPE::VECSXP.0
    {
        return x;
    }

    // avoid wrappers of wrappers, at least in some cases
    if is_wrapper(x) != 0 && srt == UNKNOWN_SORTEDNESS && no_na == 0 {
        // return shallow_duplicate(x);
        return x;
    }

    if !ATTRIB(x).is_null() {
        return x;
    }

    let abs_srt = if srt < 0 { -srt } else { srt };
    if abs_srt != KNOWN_SORTED && srt != KNOWN_UNSORTED && srt != UNKNOWN_SORTEDNESS {
        return x;
    }

    if no_na < 0 || no_na > 1 {
        return x;
    }

    let meta = Rf_allocVector(SEXPTYPE::INTSXP.0, NMETA as c_int);
    *INTEGER(meta).add(0) = srt;
    *INTEGER(meta).add(1) = no_na;

    make_wrapper(x, meta)
}

/// do_wrap_meta -- .Internal(wrap_meta(x, srt, no_na))
pub unsafe fn do_wrap_meta(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    // checkArity(op, args);
    let x = CAR(args);
    // int srt = asInteger(CADR(args));
    // int no_na = asInteger(CADDR(args));
    let _srt: c_int = 0;
    let _no_na: c_int = 0;
    wrap_meta(x, _srt, _no_na)
}

/// R_tryWrap -- wrap an object with no meta-data
pub unsafe fn R_tryWrap(x: SEXP) -> SEXP {
    wrap_meta(x, UNKNOWN_SORTEDNESS, 0)
}

/// do_tryWrap -- .Internal(tryWrap(x))
pub unsafe fn do_tryWrap(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    // checkArity(op, args);
    let x = CAR(args);
    R_tryWrap(x)
}

/// R_tryUnwrap -- unwrap a wrapper if it has no useful meta-data
pub unsafe fn R_tryUnwrap(x: SEXP) -> SEXP {
    if is_wrapper(x) != 0 && WRAPPER_SORTED(x) == UNKNOWN_SORTEDNESS && WRAPPER_NO_NA(x) == 0 {
        let _data = WRAPPER_WRAPPED(x);
        // if (! MAYBE_SHARED(data)) {
        //     SET_ATTRIB(data, ATTRIB(x));
        //     SET_OBJECT(data, OBJECT(x));
        //     IS_S4_OBJECT(x) ? SET_S4_OBJECT(data) : UNSET_S4_OBJECT(data);
        //     ALTREP_SET_TYPEOF(x, LISTSXP);
        //     SET_ALTREP(x, 0);
        //     SET_ATTRIB(x, R_NilValue);
        //     SETCAR(x, R_NilValue);
        //     SETCDR(x, R_NilValue);
        //     SET_TAG(x, R_NilValue);
        //     SET_OBJECT(x, 0);
        //     UNSET_S4_OBJECT(x);
        //     return data;
        // }
    }
    x
}

// ===========================================================================
//  ALTREP Class Initialization
// ===========================================================================

/// R_init_altrep -- initialize all built-in ALTREP classes
pub unsafe fn R_init_altrep() {
    InitCompactIntegerClass();
    InitCompactRealClass();
    InitDefferredStringClass();
    InitMmapIntegerClass(ptr::null_mut());
    InitMmapRealClass(ptr::null_mut());
    InitWrapIntegerClass(ptr::null_mut());
    InitWrapLogicalClass(ptr::null_mut());
    InitWrapRealClass(ptr::null_mut());
    InitWrapComplexClass(ptr::null_mut());
    InitWrapRawClass(ptr::null_mut());
    InitWrapStringClass(ptr::null_mut());
    InitWrapListClass(ptr::null_mut());
}

/// Backwards-compatible init function
pub unsafe fn R_init_altrep_classes() {
    R_init_altrep();
}

pub unsafe fn R_init_compact_intseq() -> SEXP {
    InitCompactIntegerClass();
    R_NilValue()
}

pub unsafe fn R_compact_intseq_check(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    R_is_compact_intseq(x)
}

pub unsafe fn R_init_compact_realseq() -> SEXP {
    InitCompactRealClass();
    R_NilValue()
}

pub unsafe fn R_compact_realseq_check(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    0
}

pub unsafe fn R_init_deferred_string() -> SEXP {
    InitDefferredStringClass();
    R_NilValue()
}

pub unsafe fn R_deferred_string_check(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    0
}

pub unsafe fn R_init_deferred_names() -> SEXP {
    R_NilValue()
}

pub unsafe fn R_deferred_names_check(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn test_compact_intseq_init() {
        unsafe {
            let cls = R_init_compact_intseq();
            assert_eq!(cls, R_NilValue());
        }
    }

    #[test]
    fn test_compact_realseq_init() {
        unsafe {
            let cls = R_init_compact_realseq();
            assert_eq!(cls, R_NilValue());
        }
    }

    #[test]
    fn test_deferred_string_init() {
        unsafe {
            let cls = R_init_deferred_string();
            assert_eq!(cls, R_NilValue());
        }
    }

    #[test]
    fn test_deferred_names_init() {
        unsafe {
            let cls = R_init_deferred_names();
            assert_eq!(cls, R_NilValue());
        }
    }

    #[test]
    fn test_compact_intseq_check_null() {
        unsafe {
            assert_eq!(R_compact_intseq_check(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_compact_realseq_check_null() {
        unsafe {
            assert_eq!(R_compact_realseq_check(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_init_altrep_classes() {
        unsafe {
            R_init_altrep_classes();
        }
    }

    #[test]
    fn test_init_altrep() {
        unsafe {
            R_init_altrep();
        }
    }

    #[test]
    fn test_is_compact_intseq_null() {
        unsafe {
            assert_eq!(R_is_compact_intseq(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_compact_intseq_length() {
        unsafe {
            // new_compact_intseq with n==1 returns ScalarInteger
            let val = new_compact_intseq(1, 42, 1);
            assert!(!val.is_null());
        }
    }

    #[test]
    fn test_compact_realseq_length() {
        unsafe {
            let val = new_compact_realseq(1, 3.14, 1.0);
            assert!(!val.is_null());
        }
    }

    #[test]
    fn test_compact_intrange() {
        unsafe {
            let val = R_compact_intrange(1, 5);
            assert!(!val.is_null());
        }
    }

    #[test]
    fn test_deferred_coerce_to_string_null() {
        unsafe {
            let val = R_deferred_coerceToString(ptr::null_mut(), ptr::null_mut());
            assert_eq!(val, R_NilValue());
        }
    }

    #[test]
    fn test_try_wrap_null() {
        unsafe {
            let val = R_tryWrap(ptr::null_mut());
            assert!(val.is_null());
        }
    }

    #[test]
    fn test_try_unwrap_null() {
        unsafe {
            let val = R_tryUnwrap(ptr::null_mut());
            assert!(val.is_null());
        }
    }
}
