#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! SEXP accessor functions with C FFI compatibility.
//!
//! These replace the stub implementations in mainutils/inlined.rs
//! with real implementations that read from SexprecCore.

use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;

use super::ffi::{
    NA_INTEGER, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, SexprecCore, SexprecData,
};

// ---------------------------------------------------------------------------
// Header accessors
// ---------------------------------------------------------------------------

/// Get the SEXPTYPE tag of an SEXP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn TYPEOF(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0; // NILSXP
        }
        (*x).sxpinfo.type_of().0
    }
}

/// Get the length of a vector SEXP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn LENGTH(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (*x).vecsxp_length() as c_int
    }
}

/// Get the extended length of a vector SEXP (64-bit).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XLENGTH(x: SEXP) -> R_xlen_t {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (*x).vecsxp_length()
    }
}

/// Get the true length (allocated capacity) of a vector SEXP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn TRUELENGTH(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (*x).vecsxp_truelength() as c_int
    }
}

/// Set the true length of a vector SEXP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_TRUELENGTH(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() {
            (*x).set_vecsxp_truelength(v as R_xlen_t);
        }
    }
}

/// Get the attributes of an SEXP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ATTRIB(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).attrib
        }
    }
}

/// Set the attributes of an SEXP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_ATTRIB(x: SEXP, v: SEXP) {
    unsafe {
        if !x.is_null() {
            super::gengc::attrib_write_barrier(x, v);
            (*x).attrib = v;
        }
    }
}

/// Check if an SEXP has the OBJECT flag set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn OBJECT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (*x).sxpinfo.obj() as c_int
    }
}

/// Set the OBJECT flag on an SEXP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_OBJECT(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() {
            (*x).sxpinfo.set_obj(v != 0);
        }
    }
}

/// Get the namedness level (0, 1, or 2).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn NAMED(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (*x).sxpinfo.named() as c_int
    }
}

/// Set the namedness level.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_NAMED(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() {
            (*x).sxpinfo.set_named(v as u8);
        }
    }
}

/// Get the LEVELS (gp[0..1]) field.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn LEVELS(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        ((*x).sxpinfo.gp() & 0x03) as c_int
    }
}

/// Set the LEVELS field.
pub unsafe fn SETLEVELS(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() {
            let gp = ((*x).sxpinfo.gp() & !0x03) | ((v as u16) & 0x03);
            (*x).sxpinfo.set_gp(gp);
        }
    }
}

/// Get the scalar flag.
pub unsafe fn IS_SCALAR(x: SEXP, _type: c_int) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (*x).sxpinfo.scalar() as c_int
    }
}

/// Set the scalar flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_SCALAR(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() {
            (*x).sxpinfo.set_scalar(v != 0);
        }
    }
}

/// Check the ALT flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ALTREP(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (*x).sxpinfo.alt() as c_int
    }
}

/// Set the ALT flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_ALTREP(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() {
            (*x).sxpinfo.set_alt(v != 0);
        }
    }
}

/// Get the mark bit (for GC).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn MARK(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (*x).sxpinfo.mark() as c_int
    }
}

/// Set the mark bit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_MARK(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() {
            (*x).sxpinfo.set_mark(v != 0);
        }
    }
}

/// Get the type (same as TYPEOF but as a macro name).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_isNull(x: SEXP) -> c_int {
    unsafe { (TYPEOF(x) == SEXPTYPE::NILSXP.0) as c_int }
}

/// Check the type of a SEXP. Alias for TYPEOF.
pub unsafe fn TYPEOF_CHECK(x: SEXP) -> c_int {
    unsafe { TYPEOF(x) }
}

// ---------------------------------------------------------------------------
// List/cons cell accessors
// ---------------------------------------------------------------------------

/// Get the CAR of a cons cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CAR(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.listsxp.carval
        }
    }
}

/// Get the CDR of a cons cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CDR(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.listsxp.cdrval
        }
    }
}

/// Get the TAG of a cons cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn TAG(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.listsxp.tagval
        }
    }
}

/// Set the CAR of a cons cell.
pub unsafe fn SETCAR(x: SEXP, y: SEXP) {
    unsafe {
        if !x.is_null() {
            super::gengc::list_write_barrier(x, 0, y);
            (*x).data.listsxp.carval = y;
        }
    }
}

/// Set the CDR of a cons cell.
pub unsafe fn SETCDR(x: SEXP, y: SEXP) {
    unsafe {
        if !x.is_null() {
            super::gengc::list_write_barrier(x, 1, y);
            (*x).data.listsxp.cdrval = y;
        }
    }
}

/// Set the TAG of a cons cell.
pub unsafe fn SETTAG(x: SEXP, y: SEXP) {
    unsafe {
        if !x.is_null() {
            super::gengc::list_write_barrier(x, 2, y);
            (*x).data.listsxp.tagval = y;
        }
    }
}

/// Get the CAR of the CDR (CADR) — second element of a list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CADR(x: SEXP) -> SEXP {
    unsafe { CAR(CDR(x)) }
}

/// Get the CAR of the CDAR (CAAR).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CAAR(x: SEXP) -> SEXP {
    unsafe { CAR(CAR(x)) }
}

/// Get the CDR of the CDR (CDDR).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CDDR(x: SEXP) -> SEXP {
    unsafe { CDR(CDR(x)) }
}

/// Get the CDR of the CADR (CDAR).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CDAR(x: SEXP) -> SEXP {
    unsafe { CDR(CAR(x)) }
}

/// Get the CAR of the CDDR (CADDR).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CADDR(x: SEXP) -> SEXP {
    unsafe { CAR(CDR(CDR(x))) }
}

/// Get the CDR of the CDDR (CDDDR).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CDDDR(x: SEXP) -> SEXP {
    unsafe { CDR(CDR(CDR(x))) }
}

/// Get the CAR of the CDDDR (CADDDR).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CADDDR(x: SEXP) -> SEXP {
    unsafe { CAR(CDR(CDR(CDR(x)))) }
}

/// Get the CAR of the CADDDR (CAD5R).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CAD5R(x: SEXP) -> SEXP {
    unsafe { CAR(CDR(CDR(CDR(CDR(x))))) }
}

// ---------------------------------------------------------------------------
// Symbol accessors
// ---------------------------------------------------------------------------

/// Get the print name (CHARSXP) of a symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn PRINTNAME(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.symsxp.pname
        }
    }
}

/// Get the value of a symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SYMVALUE(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.symsxp.value
        }
    }
}

/// Get the internal value of a symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn INTERNAL(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.symsxp.internal
        }
    }
}

/// Set the print name of a symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_PRINTNAME(x: SEXP, v: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).data.symsxp.pname = v;
        }
    }
}

/// Set the value of a symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_SYMVALUE(x: SEXP, v: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).data.symsxp.value = v;
        }
    }
}

/// Set the internal value of a symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_INTERNAL(x: SEXP, v: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).data.symsxp.internal = v;
        }
    }
}

// ---------------------------------------------------------------------------
// Closure accessors
// ---------------------------------------------------------------------------

/// Get the formals of a closure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn FORMALS(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.closxp.formals
        }
    }
}

/// Get the body of a closure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn BODY(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.closxp.body
        }
    }
}

/// Get the environment of a closure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CLOENV(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.closxp.env
        }
    }
}

/// Set the formals of a closure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_FORMALS(x: SEXP, v: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).data.closxp.formals = v;
        }
    }
}

/// Set the body of a closure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_BODY(x: SEXP, v: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).data.closxp.body = v;
        }
    }
}

/// Set the environment of a closure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_CLOENV(x: SEXP, v: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).data.closxp.env = v;
        }
    }
}

// ---------------------------------------------------------------------------
// Environment accessors
// ---------------------------------------------------------------------------

/// Get the frame of an environment.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn FRAME(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.envsxp.frame
        }
    }
}

/// Get the enclosing environment.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ENCLOS(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.envsxp.enclos
        }
    }
}

/// Get the hash table of an environment.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn HASHTAB(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.envsxp.hashtab
        }
    }
}

/// Set the frame of an environment.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_FRAME(x: SEXP, v: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).data.envsxp.frame = v;
        }
    }
}

/// Set the enclosing environment.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_ENCLOS(x: SEXP, v: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).data.envsxp.enclos = v;
        }
    }
}

/// Set the hash table of an environment.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_HASHTAB(x: SEXP, v: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).data.envsxp.hashtab = v;
        }
    }
}

// ---------------------------------------------------------------------------
// Promise accessors
// ---------------------------------------------------------------------------

/// Get the value of a promise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn PRVALUE(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.promsxp.value
        }
    }
}

/// Get the expression of a promise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn PRCODE(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.promsxp.expr
        }
    }
}

/// Get the environment of a promise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn PRENV(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            ptr::null_mut()
        } else {
            (*x).data.promsxp.env
        }
    }
}

/// Set the value of a promise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_PRVALUE(x: SEXP, v: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).data.promsxp.value = v;
        }
    }
}

/// Set the expression of a promise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_PRCODE(x: SEXP, v: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).data.promsxp.expr = v;
        }
    }
}

/// Set the environment of a promise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_PRENV(x: SEXP, v: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).data.promsxp.env = v;
        }
    }
}

// ---------------------------------------------------------------------------
// Primitive function accessors
// ---------------------------------------------------------------------------

/// Get the offset of a primitive function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn PRIMOFFSET(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (*x).data.primsxp.offset
    }
}

/// Set the offset of a primitive function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_PRIMOFFSET(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() {
            (*x).data.primsxp.offset = v;
        }
    }
}

// ---------------------------------------------------------------------------
// Vector data accessors
// ---------------------------------------------------------------------------

/// Get a pointer to the data region of a vector SEXP.
///
/// For vector types, the data is stored in a separate allocation
/// tracked by the arena allocator. The data pointer is stored in
/// the gengc_next_node field for vector types.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn DATAPTR(x: SEXP) -> *mut c_void {
    unsafe {
        if x.is_null() {
            return ptr::null_mut();
        }
        // For vector types, data pointer is stored in gengc_next_node
        let t = (*x).sxpinfo.type_of();
        if t.is_vector_type() || t == SEXPTYPE::CHARSXP {
            (*x).gengc_next_node as *mut c_void
        } else {
            ptr::null_mut()
        }
    }
}

/// Set the data pointer for a vector SEXP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_DATAPTR(x: SEXP, v: *mut c_void) {
    unsafe {
        if !x.is_null() {
            (*x).gengc_next_node = v as SEXP;
        }
    }
}

/// Get the data pointer, returning a const pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ROBJ_DATAPTR(x: SEXP) -> *const c_void {
    unsafe { DATAPTR(x) }
}

/// Get a pointer to the logical vector data.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn LOGICAL(x: SEXP) -> *mut c_int {
    unsafe { DATAPTR(x) as *mut c_int }
}

/// Get a pointer to the integer vector data.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn INTEGER(x: SEXP) -> *mut c_int {
    unsafe { DATAPTR(x) as *mut c_int }
}

/// Get a pointer to the real (double) vector data.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn REAL(x: SEXP) -> *mut c_double {
    unsafe { DATAPTR(x) as *mut c_double }
}

/// Get a pointer to the complex vector data.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn COMPLEX(x: SEXP) -> *mut Rcomplex {
    unsafe { DATAPTR(x) as *mut Rcomplex }
}

/// Get a pointer to the raw byte vector data.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn RAW(x: SEXP) -> *mut super::ffi::Rbyte {
    unsafe { DATAPTR(x) as *mut super::ffi::Rbyte }
}

/// Get the character data of a CHARSXP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CHAR(x: SEXP) -> *const c_char {
    unsafe { DATAPTR(x) as *const c_char }
}

/// Get a mutable pointer to the character data of a CHARSXP.
pub unsafe fn CHAR_RW(x: SEXP) -> *mut c_char {
    unsafe { DATAPTR(x) as *mut c_char }
}

// ---------------------------------------------------------------------------
// String/list element accessors
// ---------------------------------------------------------------------------

/// Get the i-th element of a STRSXP (a CHARSXP).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn STRING_ELT(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        if x.is_null() {
            return ptr::null_mut();
        }
        let ptrs = DATAPTR(x) as *mut SEXP;
        *ptrs.add(i as usize)
    }
}

/// Set the i-th element of a STRSXP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_STRING_ELT(x: SEXP, i: R_xlen_t, val: SEXP) {
    unsafe {
        if !x.is_null() {
            let ptrs = DATAPTR(x) as *mut SEXP;
            *ptrs.add(i as usize) = val;
        }
    }
}

/// Get the i-th element of a VECSXP/EXPRSXP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn VECTOR_ELT(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        if x.is_null() {
            return ptr::null_mut();
        }
        let ptrs = DATAPTR(x) as *mut SEXP;
        *ptrs.add(i as usize)
    }
}

/// Set the i-th element of a VECSXP/EXPRSXP.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_VECTOR_ELT(x: SEXP, i: R_xlen_t, val: SEXP) {
    unsafe {
        if !x.is_null() {
            super::gengc::vector_write_barrier(x, i as usize, val);
            let ptrs = DATAPTR(x) as *mut SEXP;
            *ptrs.add(i as usize) = val;
        }
    }
}

// ---------------------------------------------------------------------------
// Element-level accessors
// ---------------------------------------------------------------------------

/// Get the i-th logical value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn LOGICAL_ELT(x: SEXP, i: c_int) -> c_int {
    unsafe {
        if x.is_null() || LOGICAL(x).is_null() {
            return NA_INTEGER;
        }
        *LOGICAL(x).add(i as usize)
    }
}

/// Set the i-th logical value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_LOGICAL_ELT(x: SEXP, i: c_int, v: c_int) {
    unsafe {
        if !x.is_null() && !LOGICAL(x).is_null() {
            *LOGICAL(x).add(i as usize) = v;
        }
    }
}

/// Get the i-th integer value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn INTEGER_ELT(x: SEXP, i: c_int) -> c_int {
    unsafe {
        if x.is_null() || INTEGER(x).is_null() {
            return NA_INTEGER;
        }
        *INTEGER(x).add(i as usize)
    }
}

/// Set the i-th integer value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_INTEGER_ELT(x: SEXP, i: c_int, v: c_int) {
    unsafe {
        if !x.is_null() && !INTEGER(x).is_null() {
            *INTEGER(x).add(i as usize) = v;
        }
    }
}

/// Get the i-th real value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn REAL_ELT(x: SEXP, i: c_int) -> c_double {
    unsafe {
        if x.is_null() || REAL(x).is_null() {
            return NA_REAL;
        }
        *REAL(x).add(i as usize)
    }
}

/// Set the i-th real value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_REAL_ELT(x: SEXP, i: c_int, v: c_double) {
    unsafe {
        if !x.is_null() && !REAL(x).is_null() {
            *REAL(x).add(i as usize) = v;
        }
    }
}

/// Get the i-th complex value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn COMPLEX_ELT(x: SEXP, i: c_int) -> Rcomplex {
    unsafe {
        if x.is_null() || COMPLEX(x).is_null() {
            return Rcomplex {
                r: NA_REAL,
                i: NA_REAL,
            };
        }
        *COMPLEX(x).add(i as usize)
    }
}

/// Set the i-th complex value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_COMPLEX_ELT(x: SEXP, i: c_int, v: Rcomplex) {
    unsafe {
        if !x.is_null() && !COMPLEX(x).is_null() {
            *COMPLEX(x).add(i as usize) = v;
        }
    }
}

/// Get the i-th raw byte value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn RAW_ELT(x: SEXP, i: c_int) -> super::ffi::Rbyte {
    unsafe {
        if x.is_null() || RAW(x).is_null() {
            return 0;
        }
        *RAW(x).add(i as usize)
    }
}

/// Set the i-th raw byte value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SET_RAW_ELT(x: SEXP, i: c_int, v: super::ffi::Rbyte) {
    unsafe {
        if !x.is_null() && !RAW(x).is_null() {
            *RAW(x).add(i as usize) = v;
        }
    }
}

// ---------------------------------------------------------------------------
// Scalar getters (for length-1 vectors)
// ---------------------------------------------------------------------------

/// Get the scalar logical value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SCALAR_LVAL(x: SEXP) -> c_int {
    unsafe { LOGICAL_ELT(x, 0) }
}

/// Get the scalar integer value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SCALAR_IVAL(x: SEXP) -> c_int {
    unsafe { INTEGER_ELT(x, 0) }
}

/// Get the scalar real value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SCALAR_DVAL(x: SEXP) -> c_double {
    unsafe { REAL_ELT(x, 0) }
}

// ---------------------------------------------------------------------------
// Helper methods on SexprecCore
// ---------------------------------------------------------------------------

impl SexprecCore {
    /// Get the vector length from the data union.
    #[inline]
    pub unsafe fn vecsxp_length(&self) -> R_xlen_t {
        unsafe { self.data.vecsxp.length }
    }

    /// Get the vector true length from the data union.
    #[inline]
    pub unsafe fn vecsxp_truelength(&self) -> R_xlen_t {
        unsafe { self.data.vecsxp.truelength }
    }

    /// Set the vector true length.
    #[inline]
    pub unsafe fn set_vecsxp_truelength(&mut self, v: R_xlen_t) {
        unsafe {
            self.data = SexprecData {
                vecsxp: super::ffi::Vecsxp {
                    length: self.data.vecsxp.length,
                    truelength: v,
                },
            };
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::ffi::*;
    use super::*;

    fn make_test_vector() -> Box<SexprecCore> {
        let mut node = Box::new(SexprecCore::new_vector(SEXPTYPE::REALSXP, 3));
        unsafe {
            node.data = SexprecData {
                vecsxp: Vecsxp {
                    length: 3,
                    truelength: 3,
                },
            };
        }
        node
    }

    #[test]
    fn test_typeof_null() {
        unsafe {
            assert_eq!(TYPEOF(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_typeof_vector() {
        let node = make_test_vector();
        unsafe {
            assert_eq!(TYPEOF(node.as_ref() as *const _ as SEXP), 14); // REALSXP
        }
    }

    #[test]
    fn test_length_vector() {
        let node = make_test_vector();
        unsafe {
            assert_eq!(LENGTH(node.as_ref() as *const _ as SEXP), 3);
            assert_eq!(XLENGTH(node.as_ref() as *const _ as SEXP), 3);
        }
    }

    #[test]
    fn test_length_null() {
        unsafe {
            assert_eq!(LENGTH(ptr::null_mut()), 0);
            assert_eq!(XLENGTH(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_attrib_null() {
        unsafe {
            assert!(ATTRIB(ptr::null_mut()).is_null());
        }
    }

    #[test]
    fn test_isnull() {
        let node = make_test_vector();
        unsafe {
            assert_eq!(Rf_isNull(ptr::null_mut()), 1);
            assert_eq!(Rf_isNull(node.as_ref() as *const _ as SEXP), 0);
        }
    }

    #[test]
    fn test_set_attrib() {
        let mut node = make_test_vector();
        unsafe {
            let ptr = node.as_mut() as *mut _ as SEXP;
            assert!(ATTRIB(ptr).is_null());
            SET_ATTRIB(ptr, ptr); // self-referential for test
            assert_eq!(ATTRIB(ptr), ptr);
            SET_ATTRIB(ptr, ptr::null_mut());
        }
    }

    #[test]
    fn test_named() {
        let mut node = make_test_vector();
        unsafe {
            let ptr = node.as_mut() as *mut _ as SEXP;
            assert_eq!(NAMED(ptr), 0);
            SET_NAMED(ptr, 2);
            assert_eq!(NAMED(ptr), 2);
        }
    }

    #[test]
    fn test_object_flag() {
        let mut node = make_test_vector();
        unsafe {
            let ptr = node.as_mut() as *mut _ as SEXP;
            assert_eq!(OBJECT(ptr), 0);
            SET_OBJECT(ptr, 1);
            assert_eq!(OBJECT(ptr), 1);
        }
    }

    #[test]
    fn test_set_truelength() {
        let mut node = make_test_vector();
        unsafe {
            let ptr = node.as_mut() as *mut _ as SEXP;
            assert_eq!(TRUELENGTH(ptr), 3);
            SET_TRUELENGTH(ptr, 10);
            assert_eq!(TRUELENGTH(ptr), 10);
        }
    }

    #[test]
    fn test_elt_null_returns_na() {
        unsafe {
            assert_eq!(LOGICAL_ELT(ptr::null_mut(), 0), NA_INTEGER);
            assert_eq!(INTEGER_ELT(ptr::null_mut(), 0), NA_INTEGER);
            assert!(REAL_ELT(ptr::null_mut(), 0).is_nan());
        }
    }

    #[test]
    fn test_dataptr_null() {
        unsafe {
            assert!(DATAPTR(ptr::null_mut()).is_null());
        }
    }
}
