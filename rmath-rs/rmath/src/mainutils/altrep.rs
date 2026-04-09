#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/altrep.c — ALTREP (alternative representations).
//!
//! ALTREP provides a mechanism for lazy/delayed computation of R vectors.
//!
//! # Storage Layout
//!
//! An ALTREP object is stored as a VECSXP with 2 elements:
//!   - data1 (slot 0): the class descriptor SEXP (or R_NilValue for built-in classes)
//!   - data2 (slot 1): instance-specific data (e.g., expanded cache)
//!
//! The ALT bit in sxpinfo identifies the object as ALTREP.

use std::os::raw::c_int;

use crate::sexp::accessors::*;
use crate::sexp::ffi::{NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory::with_arena;

// ---------------------------------------------------------------------------
// ALTREP data1/data2 accessors
// ---------------------------------------------------------------------------

/// Get the ALTREP data1 field.
///
/// In R, data1 stores the class descriptor. For built-in classes
/// (compact_intseq, compact_realseq), it may hold a named SEXP.
pub unsafe fn R_altrep_data1(x: SEXP) -> SEXP {
    if x.is_null() || ALTREP(x) == 0 {
        return unsafe { R_NilValue() };
    }
    unsafe {
        let data_ptr = (*x).gengc_next_node as *mut SEXP;
        if data_ptr.is_null() {
            return R_NilValue();
        }
        *data_ptr
    }
}

/// Get the ALTREP data2 field.
///
/// In R, data2 stores instance-specific data. For compact sequences,
/// this is the expanded/materialized vector (or R_NilValue if not yet expanded).
pub unsafe fn R_altrep_data2(x: SEXP) -> SEXP {
    if x.is_null() || ALTREP(x) == 0 {
        return unsafe { R_NilValue() };
    }
    unsafe {
        let data_ptr = (*x).gengc_next_node as *mut SEXP;
        if data_ptr.is_null() {
            return R_NilValue();
        }
        let data2_ptr = data_ptr.add(1);
        *data2_ptr
    }
}

/// Set the ALTREP data1 field.
pub unsafe fn R_set_altrep_data1(x: SEXP, v: SEXP) {
    if x.is_null() || ALTREP(x) == 0 {
        return;
    }
    unsafe {
        let data_ptr = (*x).gengc_next_node as *mut SEXP;
        if data_ptr.is_null() {
            return;
        }
        *data_ptr = v;
    }
}

/// Set the ALTREP data2 field.
pub unsafe fn R_set_altrep_data2(x: SEXP, v: SEXP) {
    if x.is_null() || ALTREP(x) == 0 {
        return;
    }
    unsafe {
        let data_ptr = (*x).gengc_next_node as *mut SEXP;
        if data_ptr.is_null() {
            return;
        }
        let data2_ptr = data_ptr.add(1);
        *data2_ptr = v;
    }
}

// ---------------------------------------------------------------------------
// ALTREP constructors
// ---------------------------------------------------------------------------

/// Create a new ALTREP object.
///
/// Allocates a VECSXP(2) with the ALT bit set, storing `data1` in
/// slot 0 and `data2` in slot 1. The class descriptor is stored as
/// the ATTRIB of the object. This follows R's internal layout.
pub unsafe fn R_new_altrep(class_def: SEXP, data1: SEXP, data2: SEXP) -> SEXP {
    with_arena(|arena| {
        let vec = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
        if vec.is_null() {
            return unsafe { R_NilValue() };
        }

        unsafe {
            (*vec).sxpinfo.set_alt(true);

            // Store class descriptor as ATTRIB
            (*vec).attrib = class_def;

            let slots = (*vec).gengc_next_node as *mut SEXP;
            if slots.is_null() {
                return R_NilValue();
            }

            *slots = data1;
            *slots.add(1) = data2;
        }

        vec
    })
}

/// Create a compact integer sequence ALTREP.
///
/// For n <= 1, returns a simple scalar integer vector.
/// For n > 1, returns an ALTREP object that computes elements on demand.
pub unsafe fn R_compact_intseq(from: R_xlen_t, to: R_xlen_t) -> SEXP {
    let n = if to >= from {
        to - from + 1
    } else {
        from - to + 1
    };

    if n <= 1 {
        return with_arena(|arena| {
            let vec = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
            if vec.is_null() {
                return unsafe { R_NilValue() };
            }
            unsafe {
                let data_ptr = (*vec).gengc_next_node as *mut c_int;
                if !data_ptr.is_null() {
                    *data_ptr = from as c_int;
                }
            }
            vec
        });
    }

    let inc = if to >= from {
        1 as R_xlen_t
    } else {
        -1 as R_xlen_t
    };
    let class_sym = unsafe { R_NilValue() };

    let info = with_arena(|arena| {
        let vec = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        if vec.is_null() {
            return unsafe { R_NilValue() };
        }
        unsafe {
            let data_ptr = (*vec).gengc_next_node as *mut c_int;
            if data_ptr.is_null() {
                return R_NilValue();
            }
            *data_ptr = from as c_int;
            *data_ptr.add(1) = to as c_int;
            *data_ptr.add(2) = inc as c_int;
        }
        vec
    });

    let altrep = unsafe { R_new_altrep(class_sym, info, R_NilValue()) };
    if !altrep.is_null() {
        unsafe {
            (*altrep).sxpinfo.set_type(SEXPTYPE::INTSXP);
            (*altrep).data.vecsxp.length = n;
            (*altrep).data.vecsxp.truelength = n;
        }
    }
    altrep
}

/// Create a compact real sequence ALTREP.
///
/// Creates a lazy real sequence [from, from+by, from+2*by, ...] with the
/// given length. For length <= 1, returns a simple scalar real vector.
pub unsafe fn R_compact_realseq(from: f64, by: f64, length: R_xlen_t) -> SEXP {
    if length <= 1 {
        return with_arena(|arena| {
            let vec = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
            if vec.is_null() {
                return unsafe { R_NilValue() };
            }
            unsafe {
                let data_ptr = (*vec).gengc_next_node as *mut f64;
                if !data_ptr.is_null() {
                    *data_ptr = from;
                }
            }
            vec
        });
    }

    let class_sym = unsafe { R_NilValue() };

    let info = with_arena(|arena| {
        let vec = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        if vec.is_null() {
            return unsafe { R_NilValue() };
        }
        unsafe {
            let data_ptr = (*vec).gengc_next_node as *mut f64;
            if data_ptr.is_null() {
                return R_NilValue();
            }
            *data_ptr = from;
            *data_ptr.add(1) = from + by * (length as f64 - 1.0);
            *data_ptr.add(2) = by;
        }
        vec
    });

    let altrep = unsafe { R_new_altrep(class_sym, info, R_NilValue()) };
    if !altrep.is_null() {
        unsafe {
            (*altrep).sxpinfo.set_type(SEXPTYPE::REALSXP);
            (*altrep).data.vecsxp.length = length;
            (*altrep).data.vecsxp.truelength = length;
        }
    }
    altrep
}

// ---------------------------------------------------------------------------
// ALTREP class structure
// ---------------------------------------------------------------------------

pub const R_ALTREP_CLASS_TYPE: c_int = 255;

/// Get the ALTREP class (stored as ATTRIB).
pub unsafe fn R_altrep_class(x: SEXP) -> SEXP {
    if x.is_null() || ALTREP(x) == 0 {
        return unsafe { R_NilValue() };
    }
    unsafe { (*x).attrib }
}

/// Get the ALTREP length.
pub unsafe fn R_altrep_length(x: SEXP) -> R_xlen_t {
    if x.is_null() || ALTREP(x) == 0 {
        return 0;
    }
    unsafe { XLENGTH(x) }
}

// ---------------------------------------------------------------------------
// ALTREP realization
// ---------------------------------------------------------------------------

/// Realize (materialize) an ALTREP object.
///
/// For compact sequences, this expands the sequence into a contiguous vector.
/// For other ALTREP types, returns the object unchanged.
pub unsafe fn R_altrep_realize(x: SEXP) -> SEXP {
    if x.is_null() {
        return unsafe { R_NilValue() };
    }
    if ALTREP(x) == 0 {
        return x;
    }

    let data2 = unsafe { R_altrep_data2(x) };
    if !data2.is_null() {
        return data2;
    }

    let tp = unsafe { TYPEOF(x) };
    match tp {
        t if t == SEXPTYPE::INTSXP.0 => unsafe { compact_intseq_expand(x) },
        t if t == SEXPTYPE::REALSXP.0 => unsafe { compact_realseq_expand(x) },
        _ => x,
    }
}

/// Expand a compact integer sequence into a contiguous INTSXP.
unsafe fn compact_intseq_expand(x: SEXP) -> SEXP {
    let data1 = unsafe { R_altrep_data1(x) };
    if data1.is_null() {
        return x;
    }

    unsafe {
        let data_ptr = (*data1).gengc_next_node as *mut c_int;
        if data_ptr.is_null() {
            return x;
        }
        let n1 = *data_ptr;
        let _n2 = *data_ptr.add(1);
        let inc = *data_ptr.add(2);
        let len = XLENGTH(x);

        let expanded = with_arena(|arena| {
            let vec = arena.alloc_vector(SEXPTYPE::INTSXP, len);
            if vec.is_null() {
                return R_NilValue();
            }
            let out = (*vec).gengc_next_node as *mut c_int;
            if out.is_null() {
                return R_NilValue();
            }
            for i in 0..len as isize {
                *out.add(i as usize) = n1 + (i as c_int) * inc;
            }
            vec
        });

        if !expanded.is_null() {
            R_set_altrep_data2(x, expanded);
        }
        expanded
    }
}

/// Expand a compact real sequence into a contiguous REALSXP.
unsafe fn compact_realseq_expand(x: SEXP) -> SEXP {
    let data1 = unsafe { R_altrep_data1(x) };
    if data1.is_null() {
        return x;
    }

    unsafe {
        let data_ptr = (*data1).gengc_next_node as *mut f64;
        if data_ptr.is_null() {
            return x;
        }
        let n1 = *data_ptr;
        let _n2 = *data_ptr.add(1);
        let by = *data_ptr.add(2);
        let len = XLENGTH(x);

        let expanded = with_arena(|arena| {
            let vec = arena.alloc_vector(SEXPTYPE::REALSXP, len);
            if vec.is_null() {
                return R_NilValue();
            }
            let out = (*vec).gengc_next_node as *mut f64;
            if out.is_null() {
                return R_NilValue();
            }
            for i in 0..len as isize {
                *out.add(i as usize) = n1 + (i as f64) * by;
            }
            vec
        });

        if !expanded.is_null() {
            R_set_altrep_data2(x, expanded);
        }
        expanded
    }
}

/// Duplicate an ALTREP.
pub unsafe fn R_altrep_duplicate(x: SEXP, _deep: c_int) -> SEXP {
    if x.is_null() {
        return unsafe { R_NilValue() };
    }
    let realized = unsafe { R_altrep_realize(x) };
    if realized.is_null() {
        return unsafe { R_NilValue() };
    }
    let len = unsafe { XLENGTH(realized) };
    let tp = unsafe { TYPEOF(realized) };

    with_arena(|arena| {
        let dup = arena.alloc_vector(SEXPTYPE(tp), len);
        if dup.is_null() {
            return unsafe { R_NilValue() };
        }
        unsafe {
            let src_data = (*realized).gengc_next_node as *const u8;
            let dst_data = (*dup).gengc_next_node as *mut u8;
            if !src_data.is_null() && !dst_data.is_null() {
                let type_size: usize = match tp as u32 {
                    13 => std::mem::size_of::<c_int>(),
                    14 => std::mem::size_of::<f64>(),
                    15 => std::mem::size_of::<crate::sexp::ffi::Rcomplex>(),
                    16 => 1,
                    _ => std::mem::size_of::<SEXP>(),
                };
                std::ptr::copy_nonoverlapping(src_data, dst_data, (len as usize) * type_size);
            }
        }
        dup
    })
}

/// Inspect an ALTREP.
pub unsafe fn R_altrep_inspect(_x: SEXP, _pre: c_int, _deep: c_int) -> c_int {
    0
}

// ---------------------------------------------------------------------------
// ALTREP type-specific accessors
// ---------------------------------------------------------------------------

/// Get integer element from ALTREP integer vector.
pub unsafe fn ALTINTEGER_ELT(x: SEXP, i: R_xlen_t) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    // Directly compute from data1 without going through realize (avoids arena borrow conflict)
    if ALTREP(x) != 0 {
        let data1 = R_altrep_data1(x);
        if !data1.is_null() {
            let tp = TYPEOF(data1);
            if tp == SEXPTYPE::INTSXP.0 {
                let data_ptr = (*data1).gengc_next_node as *const c_int;
                if !data_ptr.is_null() {
                    let n1 = *data_ptr;
                    let inc = *data_ptr.add(2);
                    return n1 + (i as c_int) * inc;
                }
            }
        }
    }
    // Fallback: try realize
    let realized = unsafe { R_altrep_realize(x) };
    if realized.is_null() {
        return NA_INTEGER;
    }
    unsafe {
        let data_ptr = (*realized).gengc_next_node as *const c_int;
        if data_ptr.is_null() || i < 0 || i >= XLENGTH(realized) {
            return NA_INTEGER;
        }
        *data_ptr.add(i as usize)
    }
}

/// Set integer element in ALTREP integer vector.
pub unsafe fn ALTINTEGER_SET_ELT(x: SEXP, i: R_xlen_t, v: c_int) {
    if x.is_null() {
        return;
    }
    let realized = unsafe { R_altrep_realize(x) };
    if realized.is_null() {
        return;
    }
    unsafe {
        let data_ptr = (*realized).gengc_next_node as *mut c_int;
        if !data_ptr.is_null() && i >= 0 && i < XLENGTH(realized) {
            *data_ptr.add(i as usize) = v;
        }
    }
}

/// Get real element from ALTREP real vector.
pub unsafe fn ALTREAL_ELT(x: SEXP, i: R_xlen_t) -> f64 {
    if x.is_null() {
        return crate::sexp::ffi::NA_REAL;
    }
    // Directly compute from data1 without going through realize (avoids arena borrow conflict)
    if ALTREP(x) != 0 {
        let data1 = R_altrep_data1(x);
        if !data1.is_null() {
            let tp = TYPEOF(data1);
            if tp == SEXPTYPE::REALSXP.0 {
                let data_ptr = (*data1).gengc_next_node as *const f64;
                if !data_ptr.is_null() {
                    let n1 = *data_ptr;
                    let by = *data_ptr.add(2);
                    return n1 + (i as f64) * by;
                }
            }
        }
    }
    let realized = unsafe { R_altrep_realize(x) };
    if realized.is_null() {
        return crate::sexp::ffi::NA_REAL;
    }
    unsafe {
        let data_ptr = (*realized).gengc_next_node as *const f64;
        if data_ptr.is_null() || i < 0 || i >= XLENGTH(realized) {
            return crate::sexp::ffi::NA_REAL;
        }
        *data_ptr.add(i as usize)
    }
}

/// Set real element in ALTREP real vector.
pub unsafe fn ALTREAL_SET_ELT(x: SEXP, i: R_xlen_t, v: f64) {
    if x.is_null() {
        return;
    }
    let realized = unsafe { R_altrep_realize(x) };
    if realized.is_null() {
        return;
    }
    unsafe {
        let data_ptr = (*realized).gengc_next_node as *mut f64;
        if !data_ptr.is_null() && i >= 0 && i < XLENGTH(realized) {
            *data_ptr.add(i as usize) = v;
        }
    }
}

/// Get logical element from ALTREP logical vector.
pub unsafe fn ALTLOGICAL_ELT(x: SEXP, i: R_xlen_t) -> c_int {
    if x.is_null() {
        return crate::sexp::ffi::NA_LOGICAL;
    }
    let realized = unsafe { R_altrep_realize(x) };
    if realized.is_null() {
        return crate::sexp::ffi::NA_LOGICAL;
    }
    unsafe {
        let data_ptr = (*realized).gengc_next_node as *const c_int;
        if data_ptr.is_null() || i < 0 || i >= XLENGTH(realized) {
            return crate::sexp::ffi::NA_LOGICAL;
        }
        *data_ptr.add(i as usize)
    }
}

/// Set logical element in ALTREP logical vector.
pub unsafe fn ALTLOGICAL_SET_ELT(x: SEXP, i: R_xlen_t, v: c_int) {
    if x.is_null() {
        return;
    }
    let realized = unsafe { R_altrep_realize(x) };
    if realized.is_null() {
        return;
    }
    unsafe {
        let data_ptr = (*realized).gengc_next_node as *mut c_int;
        if !data_ptr.is_null() && i >= 0 && i < XLENGTH(realized) {
            *data_ptr.add(i as usize) = v;
        }
    }
}

/// Get raw element from ALTREP raw vector.
pub unsafe fn ALTRAW_ELT(x: SEXP, i: R_xlen_t) -> u8 {
    if x.is_null() {
        return 0;
    }
    let realized = unsafe { R_altrep_realize(x) };
    if realized.is_null() {
        return 0;
    }
    unsafe {
        let data_ptr = (*realized).gengc_next_node as *const u8;
        if data_ptr.is_null() || i < 0 || i >= XLENGTH(realized) {
            return 0;
        }
        *data_ptr.add(i as usize)
    }
}

/// Set raw element in ALTREP raw vector.
pub unsafe fn ALTRAW_SET_ELT(x: SEXP, i: R_xlen_t, v: u8) {
    if x.is_null() {
        return;
    }
    let realized = unsafe { R_altrep_realize(x) };
    if realized.is_null() {
        return;
    }
    unsafe {
        let data_ptr = (*realized).gengc_next_node as *mut u8;
        if !data_ptr.is_null() && i >= 0 && i < XLENGTH(realized) {
            *data_ptr.add(i as usize) = v;
        }
    }
}

/// Get string element from ALTREP string vector.
pub unsafe fn ALTSTRING_ELT(x: SEXP, i: R_xlen_t) -> SEXP {
    if x.is_null() {
        return unsafe { R_NilValue() };
    }
    let realized = unsafe { R_altrep_realize(x) };
    if realized.is_null() {
        return unsafe { R_NilValue() };
    }
    unsafe {
        let data_ptr = (*realized).gengc_next_node as *const SEXP;
        if data_ptr.is_null() || i < 0 || i >= XLENGTH(realized) {
            return R_NilValue();
        }
        *data_ptr.add(i as usize)
    }
}

/// Set string element in ALTREP string vector.
pub unsafe fn ALTSTRING_SET_ELT(x: SEXP, i: R_xlen_t, v: SEXP) {
    if x.is_null() {
        return;
    }
    let realized = unsafe { R_altrep_realize(x) };
    if realized.is_null() {
        return;
    }
    unsafe {
        let data_ptr = (*realized).gengc_next_node as *mut SEXP;
        if !data_ptr.is_null() && i >= 0 && i < XLENGTH(realized) {
            *data_ptr.add(i as usize) = v;
        }
    }
}

// ---------------------------------------------------------------------------
// ALTREP method registration (stubs - no dynamic dispatch needed)
// ---------------------------------------------------------------------------

pub unsafe fn R_set_altrep_finalizer(_class: SEXP, _finalizer: Option<unsafe extern "C" fn(SEXP)>) {
}

pub unsafe fn R_set_altrep_duplicate_method(
    _class: SEXP,
    _method: Option<unsafe extern "C" fn(SEXP, c_int) -> SEXP>,
) {
}

pub unsafe fn R_set_altrep_inspect_method(
    _class: SEXP,
    _method: Option<unsafe extern "C" fn(SEXP, c_int, c_int) -> c_int>,
) {
}

pub unsafe fn R_set_altrep_length_method(
    _class: SEXP,
    _method: Option<unsafe extern "C" fn(SEXP) -> R_xlen_t>,
) {
}

pub unsafe fn R_set_altrep_coerce_method(
    _class: SEXP,
    _method: Option<unsafe extern "C" fn(SEXP, c_int) -> SEXP>,
) {
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_altrep_class_null() {
        unsafe {
            let result = R_altrep_class(std::ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_altrep_length_null() {
        unsafe {
            assert_eq!(R_altrep_length(std::ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_altrep_data_null() {
        unsafe {
            assert_eq!(R_altrep_data1(std::ptr::null_mut()), R_NilValue());
            assert_eq!(R_altrep_data2(std::ptr::null_mut()), R_NilValue());
        }
    }

    #[test]
    fn test_new_altrep_creates_altrep_object() {
        unsafe {
            let data1 = R_NilValue();
            let data2 = R_NilValue();
            let altrep = R_new_altrep(R_NilValue(), data1, data2);
            assert!(!altrep.is_null(), "R_new_altrep should return non-null");
            assert_eq!(ALTREP(altrep), 1, "ALT bit should be set");
            assert_eq!(R_altrep_data1(altrep), R_NilValue());
            assert_eq!(R_altrep_data2(altrep), R_NilValue());
        }
    }

    #[test]
    fn test_compact_intseq_scalar() {
        unsafe {
            let seq = R_compact_intseq(42, 42);
            assert!(!seq.is_null());
            assert_eq!(TYPEOF(seq), SEXPTYPE::INTSXP.0);
            let data_ptr = (*seq).gengc_next_node as *const c_int;
            assert_eq!(*data_ptr, 42);
        }
    }

    #[test]
    fn test_compact_realseq_scalar() {
        unsafe {
            let seq = R_compact_realseq(3.14, 1.0, 1);
            assert!(!seq.is_null());
            assert_eq!(TYPEOF(seq), SEXPTYPE::REALSXP.0);
            let data_ptr = (*seq).gengc_next_node as *const f64;
            assert!((*data_ptr - 3.14).abs() < 1e-10);
        }
    }

    #[test]
    fn test_altrep_set_data() {
        unsafe {
            let altrep = R_new_altrep(R_NilValue(), R_NilValue(), R_NilValue());
            assert!(!altrep.is_null());

            let sentinel_value = 999;
            let sentinel = with_arena(|arena| {
                let v = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
                if !v.is_null() {
                    let d = (*v).gengc_next_node as *mut c_int;
                    if !d.is_null() {
                        *d = sentinel_value;
                    }
                }
                v
            });

            R_set_altrep_data2(altrep, sentinel);
            let retrieved = R_altrep_data2(altrep);
            assert!(!retrieved.is_null());
            let d = (*retrieved).gengc_next_node as *const c_int;
            assert_eq!(*d, sentinel_value);
        }
    }

    #[test]
    fn test_altinteger_elt_null() {
        unsafe {
            assert_eq!(ALTINTEGER_ELT(std::ptr::null_mut(), 0), NA_INTEGER);
        }
    }

    #[test]
    fn test_altreal_elt_null() {
        unsafe {
            assert!(ALTREAL_ELT(std::ptr::null_mut(), 0).is_nan());
        }
    }
}
