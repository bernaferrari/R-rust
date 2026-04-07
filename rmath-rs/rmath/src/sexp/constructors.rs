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

/// Allocate a vector of the given type and length.
/// This is the equivalent of R's `Rf_allocVector3`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_allocVector3(sexptype: c_int, length: R_xlen_t) -> SEXP {
    memory::with_arena(|arena| arena.alloc_vector(SEXPTYPE(sexptype), length))
}

/// Allocate a vector (c_int length version).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_allocVector(sexptype: c_int, length: c_int) -> SEXP {
    unsafe { Rf_allocVector3(sexptype, length as R_xlen_t) }
}

/// Create a cons cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_cons(car: SEXP, cdr: SEXP) -> SEXP {
    memory::with_arena(|arena| arena.cons(car, cdr, ptr::null_mut()))
}

/// Create a tagged cons cell (LANGSXP).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_lang2(car: SEXP, cdr: SEXP) -> SEXP {
    unsafe {
        let cell = Rf_cons(car, cdr);
        if !cell.is_null() {
            (*cell).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        cell
    }
}

/// Create a lang3 (3-element call).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_lang3(car: SEXP, cdr: SEXP, tag: SEXP) -> SEXP {
    unsafe {
        let cdr_cell = Rf_cons(cdr, tag);
        let cell = Rf_cons(car, cdr_cell);
        if !cell.is_null() {
            (*cell).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        cell
    }
}

/// Allocate a pairlist chain of n NILSXP elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_allocList(n: c_int) -> SEXP {
    memory::with_arena(|arena| arena.alloc_list_chain(n))
}

/// Create a CHARSXP from a C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_mkChar(s: *const c_char) -> SEXP {
    unsafe {
        if s.is_null() {
            return ptr::null_mut();
        }
        let len = std::ffi::CStr::from_ptr(s).to_bytes();
        memory::with_arena(|arena| arena.alloc_charsxp(len))
    }
}

/// Create a CHARSXP from a C string with known length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_mkCharLen(s: *const c_char, len: c_int) -> SEXP {
    unsafe {
        if s.is_null() || len < 0 {
            return ptr::null_mut();
        }
        let bytes = std::slice::from_raw_parts(s as *const u8, len as usize);
        memory::with_arena(|arena| arena.alloc_charsxp(bytes))
    }
}

/// Create a scalar STRSXP from a C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_mkString(s: *const c_char) -> SEXP {
    unsafe {
        if s.is_null() {
            return ptr::null_mut();
        }
        let charsxp = Rf_mkChar(s);
        if charsxp.is_null() {
            return ptr::null_mut();
        }
        let strsxp = Rf_allocVector(SEXPTYPE::STRSXP.0, 1);
        if strsxp.is_null() {
            return ptr::null_mut();
        }
        // Store CHARSXP pointer as the first element
        let data = (*strsxp).gengc_next_node as *mut SEXP;
        *data = charsxp;
        strsxp
    }
}

/// Create a scalar logical value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_ScalarLogical(x: c_int) -> SEXP {
    unsafe {
        let s = Rf_allocVector(SEXPTYPE::LGLSXP.0, 1);
        if !s.is_null() {
            let data = (*s).gengc_next_node as *mut c_int;
            *data = x;
        }
        s
    }
}

/// Create a scalar integer value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_ScalarInteger(x: c_int) -> SEXP {
    unsafe {
        let s = Rf_allocVector(SEXPTYPE::INTSXP.0, 1);
        if !s.is_null() {
            let data = (*s).gengc_next_node as *mut c_int;
            *data = x;
        }
        s
    }
}

/// Create a scalar real value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_ScalarReal(x: c_double) -> SEXP {
    unsafe {
        let s = Rf_allocVector(SEXPTYPE::REALSXP.0, 1);
        if !s.is_null() {
            let data = (*s).gengc_next_node as *mut c_double;
            *data = x;
        }
        s
    }
}

/// Create a scalar complex value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_ScalarComplex(x: super::ffi::Rcomplex) -> SEXP {
    unsafe {
        let s = Rf_allocVector(SEXPTYPE::CPLXSXP.0, 1);
        if !s.is_null() {
            let data = (*s).gengc_next_node as *mut super::ffi::Rcomplex;
            *data = x;
        }
        s
    }
}

/// Create a scalar string from a CHARSXP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_ScalarString(x: SEXP) -> SEXP {
    unsafe {
        let s = Rf_allocVector(SEXPTYPE::STRSXP.0, 1);
        if !s.is_null() {
            let data = (*s).gengc_next_node as *mut SEXP;
            *data = x;
        }
        s
    }
}

/// Create a scalar raw value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_ScalarRaw(x: super::ffi::Rbyte) -> SEXP {
    unsafe {
        let s = Rf_allocVector(SEXPTYPE::RAWSXP.0, 1);
        if !s.is_null() {
            let data = (*s).gengc_next_node as *mut super::ffi::Rbyte;
            *data = x;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// Type checking functions
// ---------------------------------------------------------------------------

/// Check if an SEXP is NULL. Re-export from accessors.
pub use super::accessors::Rf_isNull;

/// Get the length of an SEXP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_length(x: SEXP) -> c_int {
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
                current = (*current).data.listsxp.cdrval;
            }
            count
        } else {
            (*x).vecsxp_length() as c_int
        }
    }
}

/// Check if an SEXP is a symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_isSymbol(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::SYMSXP) as c_int
    }
}

/// Check if an SEXP is a list (pairlist).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_isList(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::LISTSXP) as c_int
    }
}

/// Check if an SEXP is an integer vector.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_isInteger(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::INTSXP) as c_int
    }
}

/// Check if an SEXP is a real (double) vector.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_isReal(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::REALSXP) as c_int
    }
}

/// Check if an SEXP is a complex vector.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_isComplex(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::CPLXSXP) as c_int
    }
}

/// Check if an SEXP is a logical vector.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_isLogical(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::LGLSXP) as c_int
    }
}

/// Check if an SEXP is a character (string) vector.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_isString(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::STRSXP) as c_int
    }
}

/// Check if an SEXP is a raw vector.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_isRaw(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::RAWSXP) as c_int
    }
}

/// Check if an SEXP is a vector (any atomic or generic vector type).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_isVector(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (*x).sxpinfo.type_of().is_vector_type() as c_int
    }
}

/// Check if an SEXP is an atomic vector.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_isVectorAtomic(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (*x).sxpinfo.type_of().is_atomic_type() as c_int
    }
}

/// Check if an SEXP is a function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_isFunction(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = (*x).sxpinfo.type_of().0;
        (t == SEXPTYPE::CLOSXP.0 || t == SEXPTYPE::BUILTINSXP.0 || t == SEXPTYPE::SPECIALSXP.0)
            as c_int
    }
}

/// Check if an SEXP is an environment.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_isEnvironment(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.type_of() == SEXPTYPE::ENVSXP) as c_int
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::ffi::*;
    use super::*;

    #[test]
    fn test_alloc_vector_real() {
        unsafe {
            let v = Rf_allocVector(SEXPTYPE::REALSXP.0, 3);
            assert!(!v.is_null());
            assert_eq!((*v).sxpinfo.type_of(), SEXPTYPE::REALSXP);
            assert_eq!((*v).vecsxp_length(), 3);
        }
    }

    #[test]
    fn test_alloc_vector_int() {
        unsafe {
            let v = Rf_allocVector(SEXPTYPE::INTSXP.0, 2);
            assert!(!v.is_null());
            assert_eq!((*v).sxpinfo.type_of(), SEXPTYPE::INTSXP);
        }
    }

    #[test]
    fn test_alloc_vector_logical() {
        unsafe {
            let v = Rf_allocVector(SEXPTYPE::LGLSXP.0, 1);
            assert!(!v.is_null());
            assert_eq!((*v).sxpinfo.type_of(), SEXPTYPE::LGLSXP);
        }
    }

    #[test]
    fn test_alloc_vector_string() {
        unsafe {
            let v = Rf_allocVector(SEXPTYPE::STRSXP.0, 2);
            assert!(!v.is_null());
            assert_eq!((*v).sxpinfo.type_of(), SEXPTYPE::STRSXP);
        }
    }

    #[test]
    fn test_alloc_vector_raw() {
        unsafe {
            let v = Rf_allocVector(SEXPTYPE::RAWSXP.0, 4);
            assert!(!v.is_null());
            assert_eq!((*v).sxpinfo.type_of(), SEXPTYPE::RAWSXP);
        }
    }

    #[test]
    fn test_scalar_integer() {
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
        unsafe {
            let s = Rf_ScalarReal(3.14);
            assert!(!s.is_null());
            let data = (*s).gengc_next_node as *mut c_double;
            assert!((*data - 3.14).abs() < 1e-10);
        }
    }

    #[test]
    fn test_scalar_logical() {
        unsafe {
            let s = Rf_ScalarLogical(1);
            assert!(!s.is_null());
            let data = (*s).gengc_next_node as *mut c_int;
            assert_eq!(*data, 1);
        }
    }

    #[test]
    fn test_cons() {
        unsafe {
            let car = Rf_ScalarInteger(1);
            let cdr = Rf_ScalarInteger(2);
            let cell = Rf_cons(car, cdr);
            assert!(!cell.is_null());
            assert_eq!((*cell).sxpinfo.type_of(), SEXPTYPE::LISTSXP);
            assert_eq!((*cell).data.listsxp.carval, car);
            assert_eq!((*cell).data.listsxp.cdrval, cdr);
        }
    }

    #[test]
    fn test_mk_string() {
        unsafe {
            let s = Rf_mkString(std::ffi::CString::new("hello").unwrap().as_ptr());
            assert!(!s.is_null());
            assert_eq!((*s).sxpinfo.type_of(), SEXPTYPE::STRSXP);
            assert_eq!((*s).vecsxp_length(), 1);
        }
    }

    #[test]
    fn test_mk_char() {
        unsafe {
            let cs = Rf_mkChar(std::ffi::CString::new("test").unwrap().as_ptr());
            assert!(!cs.is_null());
            assert_eq!((*cs).sxpinfo.type_of(), SEXPTYPE::CHARSXP);
        }
    }

    #[test]
    fn test_is_null() {
        unsafe {
            assert_eq!(Rf_isNull(ptr::null_mut()), 1);
            assert_eq!(Rf_isNull(R_NilValue()), 1);
            let s = Rf_ScalarInteger(1);
            assert_eq!(Rf_isNull(s), 0);
        }
    }

    #[test]
    fn test_is_type_checks() {
        unsafe {
            let iv = Rf_allocVector(SEXPTYPE::INTSXP.0, 1);
            assert_eq!(Rf_isInteger(iv), 1);
            assert_eq!(Rf_isReal(iv), 0);

            let rv = Rf_allocVector(SEXPTYPE::REALSXP.0, 1);
            assert_eq!(Rf_isReal(rv), 1);
            assert_eq!(Rf_isInteger(rv), 0);

            assert_eq!(Rf_isVector(iv), 1);
            assert_eq!(Rf_isVectorAtomic(iv), 1);
        }
    }

    #[test]
    fn test_length_vector() {
        unsafe {
            let v = Rf_allocVector(SEXPTYPE::REALSXP.0, 5);
            assert_eq!(Rf_length(v), 5);
            assert_eq!(Rf_length(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_length_pairlist() {
        unsafe {
            let list = Rf_allocList(3);
            assert_eq!(Rf_length(list), 3);
        }
    }
}
