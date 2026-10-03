#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! SEXP constructor functions matching R's allocation API.
//!
//! These are the Rust equivalents of R's allocVector, cons, allocList, etc.
//! They use the thread-local arena allocator for memory management.

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use super::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use super::globals::R_NilValue;
use super::memory::{self};

// ---------------------------------------------------------------------------
// FFI-compatible constructor functions
// ---------------------------------------------------------------------------

unsafe fn require_allocation(value: SEXP) -> SEXP {
    if !value.is_null() {
        return value;
    }
    let bounded = unsafe {
        memory::with_arena(|arena| {
            let budget = arena.budget();
            budget.max_bytes > 0 || budget.max_nodes > 0
        })
    };
    if value.is_null() && bounded {
        std::panic::panic_any(super::context::RError {
            message: "R allocation failed: memory or node budget exceeded".into(),
        });
    }
    value
}

unsafe fn alloc_vector3_inner(sexptype: SEXPTYPE, length: R_xlen_t) -> SEXP {
    unsafe {
        // GNU allocVector also accepts pairlists/calls, but those need a
        // chain of nodes rather than a vector header with integer lengths in
        // pointer slots. Keep this compatibility dispatch outside RArena's
        // vector-only safe constructors.
        if length < 0 {
            super::context::r_error("negative length vectors are not allowed");
        }
        if sexptype == SEXPTYPE::NILSXP {
            return R_NilValue();
        }
        if sexptype == SEXPTYPE::LISTSXP || sexptype == SEXPTYPE::LANGSXP {
            let n = c_int::try_from(length)
                .unwrap_or_else(|_| super::context::r_error("invalid length for pairlist"));
            let list = memory::with_arena(|arena| {
                let list = arena.alloc_list_chain(n);
                if n > 0 && !list.is_null() && sexptype == SEXPTYPE::LANGSXP {
                    (*list).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
                list
            });
            return if n == 0 {
                list
            } else {
                require_allocation(list)
            };
        }
        require_allocation(memory::with_arena(|arena| {
            arena.alloc_vector(sexptype, length)
        }))
    }
}

unsafe fn alloc_vector_inner(sexptype: SEXPTYPE, length: c_int) -> SEXP {
    unsafe { alloc_vector3_inner(sexptype, length as R_xlen_t) }
}

pub unsafe fn Rf_allocVector3<T: Into<SEXPTYPE>>(sexptype: T, length: R_xlen_t) -> SEXP {
    unsafe { alloc_vector3_inner(sexptype.into(), length) }
}

pub unsafe fn Rf_allocVector<T: Into<SEXPTYPE>>(sexptype: T, length: c_int) -> SEXP {
    unsafe { alloc_vector_inner(sexptype.into(), length) }
}

pub unsafe fn Rf_cons(car: SEXP, cdr: SEXP) -> SEXP {
    unsafe {
        require_allocation(memory::with_arena(|arena| {
            arena.cons(car, cdr, ptr::null_mut())
        }))
    }
}

/// Allocate and finish a call graph before deferred collection can observe it.
/// Every input becomes reachable from the fresh head within the same lend.
unsafe fn language(items: &[SEXP]) -> SEXP {
    unsafe {
        require_allocation(memory::with_arena(|arena| {
            let mut head = R_NilValue();
            for &item in items.iter().rev() {
                head = arena.cons(item, head, ptr::null_mut());
                if head.is_null() {
                    return head;
                }
            }
            (*head).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            head
        }))
    }
}

/// Create a two-element call.
pub unsafe fn Rf_lang2(car: SEXP, cdr: SEXP) -> SEXP {
    unsafe { language(&[car, cdr]) }
}

/// Create a three-element call.
pub unsafe fn Rf_lang3(car: SEXP, cdr: SEXP, tag: SEXP) -> SEXP {
    unsafe { language(&[car, cdr, tag]) }
}

/// Create a four-element call.
pub unsafe fn Rf_lang4(car: SEXP, a2: SEXP, a3: SEXP, a4: SEXP) -> SEXP {
    unsafe { language(&[car, a2, a3, a4]) }
}

/// Create a five-element call.
pub unsafe fn Rf_lang5(car: SEXP, a2: SEXP, a3: SEXP, a4: SEXP, a5: SEXP) -> SEXP {
    unsafe { language(&[car, a2, a3, a4, a5]) }
}
/// Allocate a pairlist chain of n NILSXP elements.
pub unsafe fn Rf_allocList(n: c_int) -> SEXP {
    let value = unsafe { memory::with_arena(|arena| arena.alloc_list_chain(n)) };
    if n == 0 {
        value
    } else {
        unsafe { require_allocation(value) }
    }
}

/// Create a CHARSXP from a C string.
pub unsafe fn Rf_mkChar(s: *const c_char) -> SEXP {
    unsafe {
        if s.is_null() {
            return ptr::null_mut();
        }
        let len = std::ffi::CStr::from_ptr(s).to_bytes();
        require_allocation(memory::with_arena(|arena| arena.alloc_charsxp(len)))
    }
}

pub unsafe fn persistent_mkChar(s: *const c_char) -> SEXP {
    if s.is_null() {
        return ptr::null_mut();
    }
    let bytes = unsafe { std::ffi::CStr::from_ptr(s) }.to_bytes();
    super::instance::with_required_current_instance(|owner| unsafe {
        super::symbol::persistent_charsxp_from_bytes_in(owner, bytes)
    })
}

/// Create a CHARSXP from a C string with known length.
pub unsafe fn Rf_mkCharLen(s: *const c_char, len: c_int) -> SEXP {
    unsafe {
        if s.is_null() || len < 0 {
            return ptr::null_mut();
        }
        let bytes = std::slice::from_raw_parts(s as *const u8, len as usize);
        require_allocation(memory::with_arena(|arena| arena.alloc_charsxp(bytes)))
    }
}

/// Create a scalar STRSXP from a C string.
pub unsafe fn Rf_mkString(s: *const c_char) -> SEXP {
    unsafe {
        if s.is_null() {
            return ptr::null_mut();
        }
        let bytes = std::ffi::CStr::from_ptr(s).to_bytes();
        require_allocation(memory::with_arena(|arena| {
            let charsxp = arena.alloc_charsxp(bytes);
            if charsxp.is_null() {
                return ptr::null_mut();
            }
            let strsxp = arena.alloc_vector(SEXPTYPE::STRSXP, 1);
            if !strsxp.is_null() && arena.set_reference_element(strsxp, 0, charsxp).is_none() {
                return ptr::null_mut();
            }
            strsxp
        }))
    }
}

/// Allocate and initialize a scalar before allocation notifications run.
/// The private initializer only writes its fresh payload; it cannot reenter R.
unsafe fn scalar(sexptype: SEXPTYPE, initialize: impl FnOnce(SEXP)) -> SEXP {
    unsafe {
        require_allocation(memory::with_arena(|arena| {
            let value = arena.alloc_vector(sexptype, 1);
            if !value.is_null() {
                initialize(value);
            }
            value
        }))
    }
}

/// Create a scalar STRSXP containing NA_STRING (R's NA_character_).
pub unsafe fn Rf_mkNAString() -> SEXP {
    unsafe { Rf_ScalarString(super::globals::R_NaString()) }
}

/// Create a scalar logical value.
pub unsafe fn Rf_ScalarLogical(x: c_int) -> SEXP {
    unsafe {
        scalar(SEXPTYPE::LGLSXP, |value| {
            (*value).gengc_next_node.cast::<c_int>().write(x)
        })
    }
}

/// Create a scalar integer value.
pub unsafe fn Rf_ScalarInteger(x: c_int) -> SEXP {
    unsafe {
        scalar(SEXPTYPE::INTSXP, |value| {
            (*value).gengc_next_node.cast::<c_int>().write(x)
        })
    }
}

/// Create a scalar real value.
pub unsafe fn Rf_ScalarReal(x: c_double) -> SEXP {
    unsafe {
        scalar(SEXPTYPE::REALSXP, |value| {
            (*value).gengc_next_node.cast::<c_double>().write(x)
        })
    }
}

/// Create a scalar complex value.
pub unsafe fn Rf_ScalarComplex(x: super::ffi::Rcomplex) -> SEXP {
    unsafe {
        scalar(SEXPTYPE::CPLXSXP, |value| {
            (*value)
                .gengc_next_node
                .cast::<super::ffi::Rcomplex>()
                .write(x)
        })
    }
}

/// Create a scalar string from a CHARSXP.
pub unsafe fn Rf_ScalarString(x: SEXP) -> SEXP {
    unsafe {
        require_allocation(memory::with_arena(|arena| {
            let value = arena.alloc_vector(SEXPTYPE::STRSXP, 1);
            if !value.is_null() && arena.set_reference_element(value, 0, x).is_none() {
                return ptr::null_mut();
            }
            value
        }))
    }
}

/// Create a scalar raw value.
pub unsafe fn Rf_ScalarRaw(x: super::ffi::Rbyte) -> SEXP {
    unsafe {
        scalar(SEXPTYPE::RAWSXP, |value| {
            (*value)
                .gengc_next_node
                .cast::<super::ffi::Rbyte>()
                .write(x)
        })
    }
}

// ---------------------------------------------------------------------------
// Type checking functions
// ---------------------------------------------------------------------------

/// Check if an SEXP is NULL. Re-export from accessors.
pub(crate) use crate::sexp::accessors::Rf_isNull;

/// Get the length of an SEXP.
pub unsafe fn Rf_length(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return 0;
        }
        // For pairlist, count the length
        let t = (*x).sxpinfo.type_of();
        if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP || t == SEXPTYPE::DOTSXP {
            let mut count = 0i32;
            let mut current = x;
            while !current.is_null() && current != R_NilValue() {
                count += 1;
                current = crate::sexp::accessors::CDR(current);
            }
            count
        } else if t == SEXPTYPE::ENVSXP {
            let mut count = 0i32;
            let mut walk = |mut frame: SEXP| {
                while !frame.is_null() && frame != R_NilValue() {
                    let tag = crate::sexp::accessors::TAG(frame);
                    if !tag.is_null() && tag != R_NilValue() {
                        count += 1;
                    }
                    frame = crate::sexp::accessors::CDR(frame);
                }
            };
            walk(crate::sexp::accessors::FRAME(x));
            let hashtab = crate::sexp::accessors::HASHTAB(x);
            if !hashtab.is_null()
                && hashtab != R_NilValue()
                && (*hashtab).sxpinfo.type_of() == SEXPTYPE::VECSXP
            {
                let n = crate::sexp::accessors::XLENGTH(hashtab);
                for i in 0..n {
                    walk(crate::sexp::accessors::VECTOR_ELT(hashtab, i));
                }
            }
            count
        } else if matches!(
            t,
            SEXPTYPE::CHARSXP
                | SEXPTYPE::LGLSXP
                | SEXPTYPE::INTSXP
                | SEXPTYPE::REALSXP
                | SEXPTYPE::CPLXSXP
                | SEXPTYPE::STRSXP
                | SEXPTYPE::VECSXP
                | SEXPTYPE::EXPRSXP
                | SEXPTYPE::RAWSXP
        ) {
            crate::sexp::accessors::LENGTH(x)
        } else {
            1
        }
    }
}

/// Check if an SEXP is a symbol.
pub unsafe fn Rf_isSymbol(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::SYMSXP) as c_int
    }
}

/// Check if an SEXP is a list (pairlist).
pub unsafe fn Rf_isList(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::LISTSXP) as c_int
    }
}

/// Check if an SEXP is an integer vector.
pub unsafe fn Rf_isInteger(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::INTSXP) as c_int
    }
}

/// Check if an SEXP is a real (double) vector.
pub unsafe fn Rf_isReal(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::REALSXP) as c_int
    }
}

/// Check if an SEXP is a complex vector.
pub unsafe fn Rf_isComplex(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::CPLXSXP) as c_int
    }
}

/// Check if an SEXP is a logical vector.
pub unsafe fn Rf_isLogical(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::LGLSXP) as c_int
    }
}

/// Check if an SEXP is a character (string) vector.
pub unsafe fn Rf_isString(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::STRSXP) as c_int
    }
}

/// Check if an SEXP is a raw vector.
pub unsafe fn Rf_isRaw(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::RAWSXP) as c_int
    }
}

/// Check if an SEXP is a vector (any atomic or generic vector type).
pub unsafe fn Rf_isVector(x: SEXP) -> c_int {
    crate::sexp::object::raw_is_vector(x) as c_int
}

/// Check if an SEXP is an atomic vector.
pub unsafe fn Rf_isVectorAtomic(x: SEXP) -> c_int {
    crate::sexp::object::raw_is_atomic_vector(x) as c_int
}

/// Check if an SEXP is a function.
pub unsafe fn Rf_isFunction(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = (*x).sxpinfo.type_of().0;
        (t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP) as c_int
    }
}

/// Check if an SEXP is an environment.
pub unsafe fn Rf_isEnvironment(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::ENVSXP) as c_int
    }
}

#[cfg(test)]
#[path = "constructor_lifetime_tests.rs"]
mod constructor_lifetime_tests;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[test]
    fn vector_compatibility_constructor_allocates_real_pairlist_headers() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            for kind in [SEXPTYPE::LISTSXP, SEXPTYPE::LANGSXP] {
                assert_eq!(Rf_allocVector3(kind, 0), R_NilValue());
                let value = Rf_allocVector3(kind, 3);
                assert_eq!((*value).sxpinfo.type_of(), kind);
                let mut cell = value;
                for _ in 0..3 {
                    assert!(!cell.is_null());
                    assert_ne!(cell, R_NilValue());
                    assert!(crate::sexp::accessors::CAR(cell).is_null());
                    cell = crate::sexp::accessors::CDR(cell);
                }
                assert_eq!(cell, R_NilValue());
            }
            assert_eq!(Rf_allocVector3(SEXPTYPE::NILSXP, 3), R_NilValue());
            for (kind, length) in [(SEXPTYPE::LISTSXP, i64::MAX), (SEXPTYPE::REALSXP, -1)] {
                let error = std::panic::catch_unwind(|| Rf_allocVector3(kind, length)).unwrap_err();
                assert!(
                    error
                        .downcast_ref::<crate::sexp::context::RError>()
                        .is_some()
                );
            }
        });
    }

    use super::super::ffi::*;
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn test_alloc_vector_real() {
        let _session = RSession::new();
        unsafe {
            let v = Rf_allocVector(SEXPTYPE::REALSXP, 3);
            assert!(!v.is_null());
            assert_eq!((*v).sxpinfo.type_of(), SEXPTYPE::REALSXP);
            assert_eq!((*v).vecsxp_length(), 3);
        }
    }

    #[test]
    fn test_alloc_vector_int() {
        let _session = RSession::new();
        unsafe {
            let v = Rf_allocVector(SEXPTYPE::INTSXP, 2);
            assert!(!v.is_null());
            assert_eq!((*v).sxpinfo.type_of(), SEXPTYPE::INTSXP);
        }
    }

    #[test]
    fn test_alloc_vector_logical() {
        let _session = RSession::new();
        unsafe {
            let v = Rf_allocVector(SEXPTYPE::LGLSXP, 1);
            assert!(!v.is_null());
            assert_eq!((*v).sxpinfo.type_of(), SEXPTYPE::LGLSXP);
        }
    }

    #[test]
    fn test_alloc_vector_string() {
        let _session = RSession::new();
        unsafe {
            let v = Rf_allocVector(SEXPTYPE::STRSXP, 2);
            assert!(!v.is_null());
            assert_eq!((*v).sxpinfo.type_of(), SEXPTYPE::STRSXP);
        }
    }

    #[test]
    fn test_alloc_vector_raw() {
        let _session = RSession::new();
        unsafe {
            let v = Rf_allocVector(SEXPTYPE::RAWSXP, 4);
            assert!(!v.is_null());
            assert_eq!((*v).sxpinfo.type_of(), SEXPTYPE::RAWSXP);
        }
    }

    #[test]
    fn test_scalar_integer() {
        let _session = RSession::new();
        unsafe {
            let s = Rf_ScalarInteger(42);
            assert!(!s.is_null());
            assert_eq!((*s).sxpinfo.type_of(), SEXPTYPE::INTSXP);
            assert_eq!((*s).vecsxp_length(), 1);
            let data = (*s).gengc_next_node as *mut c_int;
            assert_eq!(*data, 42);
        }
    }

    #[test]
    fn test_scalar_real() {
        let _session = RSession::new();
        unsafe {
            let s = Rf_ScalarReal(3.14);
            assert!(!s.is_null());
            let data = (*s).gengc_next_node as *mut c_double;
            assert!((*data - 3.14).abs() < 1e-10);
        }
    }

    #[test]
    fn test_scalar_logical() {
        let _session = RSession::new();
        unsafe {
            let s = Rf_ScalarLogical(1);
            assert!(!s.is_null());
            let data = (*s).gengc_next_node as *mut c_int;
            assert_eq!(*data, 1);
        }
    }

    #[test]
    fn test_cons() {
        let _session = RSession::new();
        unsafe {
            let car = Rf_ScalarInteger(1);
            let cdr = Rf_ScalarInteger(2);
            let cell = Rf_cons(car, cdr);
            assert!(!cell.is_null());
            assert_eq!((*cell).sxpinfo.type_of(), SEXPTYPE::LISTSXP);
            assert_eq!(crate::sexp::accessors::CAR(cell), car);
            assert_eq!(crate::sexp::accessors::CDR(cell), cdr);
        }
    }

    #[test]
    fn test_mk_string() {
        let _session = RSession::new();
        unsafe {
            let s = Rf_mkString(b"hello\0".as_ptr() as *const c_char);
            assert!(!s.is_null());
            assert_eq!((*s).sxpinfo.type_of(), SEXPTYPE::STRSXP);
            assert_eq!((*s).vecsxp_length(), 1);
        }
    }

    #[test]
    fn test_mk_char() {
        let _session = RSession::new();
        unsafe {
            let cs = Rf_mkChar(b"test\0".as_ptr() as *const c_char);
            assert!(!cs.is_null());
            assert_eq!((*cs).sxpinfo.type_of(), SEXPTYPE::CHARSXP);
        }
    }

    #[test]
    fn test_is_null() {
        let _session = RSession::new();
        unsafe {
            assert_eq!(Rf_isNull(ptr::null_mut()), 1);
            assert_eq!(Rf_isNull(R_NilValue()), 1);
            let s = Rf_ScalarInteger(1);
            assert_eq!(Rf_isNull(s), 0);
        }
    }

    #[test]
    fn test_is_type_checks() {
        let _session = RSession::new();
        unsafe {
            let iv = Rf_allocVector(SEXPTYPE::INTSXP, 1);
            assert_eq!(Rf_isInteger(iv), 1);
            assert_eq!(Rf_isReal(iv), 0);

            let rv = Rf_allocVector(SEXPTYPE::REALSXP, 1);
            assert_eq!(Rf_isReal(rv), 1);
            assert_eq!(Rf_isInteger(rv), 0);

            assert_eq!(Rf_isVector(iv), 1);
            assert_eq!(Rf_isVectorAtomic(iv), 1);
        }
    }
}

pub unsafe fn persistent_cons(car: SEXP, cdr: SEXP) -> SEXP {
    unsafe { super::memory_ext::cons_raw(car, cdr) }
}

pub unsafe fn persistent_scalar_integer(val: c_int) -> SEXP {
    super::instance::with_required_current_instance(|owner| unsafe {
        (*owner)
            .persistent_nodes
            .allocate_integer(val, false)
            .unwrap_or(ptr::null_mut())
    })
}

pub unsafe fn persistent_scalar_logical(val: c_int) -> SEXP {
    super::instance::with_required_current_instance(|owner| unsafe {
        (*owner)
            .persistent_nodes
            .allocate_integer(val, true)
            .unwrap_or(ptr::null_mut())
    })
}

pub unsafe fn persistent_scalar_real(val: c_double) -> SEXP {
    super::instance::with_required_current_instance(|owner| unsafe {
        (*owner)
            .persistent_nodes
            .allocate_real(val)
            .unwrap_or(ptr::null_mut())
    })
}

pub unsafe fn persistent_mkstring(s: *const c_char) -> SEXP {
    if s.is_null() {
        return ptr::null_mut();
    }
    let bytes = unsafe { std::ffi::CStr::from_ptr(s) }.to_bytes();
    super::instance::with_required_current_instance(|owner| unsafe {
        let chars = super::symbol::persistent_charsxp_from_bytes_in(owner, bytes);
        if chars.is_null() {
            return chars;
        }
        (*owner)
            .persistent_nodes
            .allocate_string(chars)
            .unwrap_or(ptr::null_mut())
    })
}
