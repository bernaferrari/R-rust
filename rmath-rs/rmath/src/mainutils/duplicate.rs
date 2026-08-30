#![allow(unused_variables)]
#![allow(unused_assignments)]
//! Object duplication system for R objects.
//!
//! Ported from R's src/main/duplicate.c. Provides deep and shallow duplication
//! of R objects, vector/matrix copy with recycling, and cycle detection for
//! complex assignment operations.

#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::{Rf_allocVector3, Rf_cons};
#[cfg(feature = "altrep")]
use crate::sexp::constructors::Rf_isVector;
use crate::sexp::ffi::{R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory::with_arena;

// ---------------------------------------------------------------------------
// GP bit constants
// ---------------------------------------------------------------------------

/// DDVAL bit mask (gp bit 10).
const DDVAL_MASK: u16 = 1 << 10;

/// S4 object bit (gp bit 11).
const S4_OBJECT_MASK: u16 = 1 << 4;

/// JIT-related gp bits (bit 0 = NOJIT, bit 1 = MAYBEJIT).
const NOJIT_MASK: u16 = 1 << 0;
const MAYBEJIT_MASK: u16 = 1 << 1;

/// RTRACE bit in sxpinfo (bit 26 in type_and_flags).
const RTRACE_MASK: u32 = 1 << 26;

/// GROWABLE_BIT in gp (bit 5).
const GROWABLE_BIT_MASK: u16 = 1 << 5;

// ---------------------------------------------------------------------------
// Local helpers and entry points
// ---------------------------------------------------------------------------

#[cfg(feature = "altrep")]
unsafe fn ALTREP_DUPLICATE_EX(s: SEXP, deep: c_int) -> SEXP {
    unsafe {
        let _ = deep;
        crate::mainutils::duplicate::Rf_duplicate(s)
    }
}

#[cfg(feature = "altrep")]
unsafe fn R_tryWrap(x: SEXP) -> SEXP {
    x
}

unsafe fn DispatchGroup(
    _s: SEXP,
    _code: *const c_char,
    _call: SEXP,
    _op: *const c_char,
    _args: SEXP,
    _env: SEXP,
) -> c_int {
    0
}

/// Check if an object has no references (NAMED == 0).
#[inline]
unsafe fn NO_REFERENCES(x: SEXP) -> c_int {
    unsafe { crate::mainutils::relop::NO_REFERENCES(x) }
}

/// Check if an object is an S4 object.
#[inline]
unsafe fn IS_S4_OBJECT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (((*x).sxpinfo.gp() & S4_OBJECT_MASK) != 0) as c_int
    }
}

/// Set the S4 object flag.
#[inline]
unsafe fn SET_S4_OBJECT(x: SEXP) {
    unsafe {
        if !x.is_null() {
            let gp = (*x).sxpinfo.gp() | S4_OBJECT_MASK;
            (*x).sxpinfo.set_gp(gp);
        }
    }
}

/// Unset the S4 object flag.
#[inline]
unsafe fn UNSET_S4_OBJECT(x: SEXP) {
    unsafe {
        if !x.is_null() {
            let gp = (*x).sxpinfo.gp() & !S4_OBJECT_MASK;
            (*x).sxpinfo.set_gp(gp);
        }
    }
}

/// Check the NOJIT gp bit.
#[inline]
unsafe fn NOJIT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (((*x).sxpinfo.gp() & NOJIT_MASK) != 0) as c_int
    }
}

/// Check the MAYBEJIT gp bit.
#[inline]
unsafe fn MAYBEJIT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (((*x).sxpinfo.gp() & MAYBEJIT_MASK) != 0) as c_int
    }
}

/// Set the NOJIT gp bit.
#[inline]
unsafe fn SET_NOJIT(x: SEXP) {
    unsafe {
        if !x.is_null() {
            let gp = (*x).sxpinfo.gp() | NOJIT_MASK;
            (*x).sxpinfo.set_gp(gp);
        }
    }
}

/// Set the MAYBEJIT gp bit.
#[inline]
unsafe fn SET_MAYBEJIT(x: SEXP) {
    unsafe {
        if !x.is_null() {
            let gp = (*x).sxpinfo.gp() | MAYBEJIT_MASK;
            (*x).sxpinfo.set_gp(gp);
        }
    }
}

/// Check the RTRACE bit.
#[inline]
unsafe fn RTRACE(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (((*x).sxpinfo.type_and_flags & RTRACE_MASK) != 0) as c_int
    }
}

/// Set the RTRACE bit.
#[inline]
unsafe fn SET_RTRACE(x: SEXP, _v: c_int) {
    unsafe {
        if !x.is_null() {
            (*x).sxpinfo.type_and_flags |= RTRACE_MASK;
        }
    }
}

/// Set NAMED to maximum (2).
#[inline]
unsafe fn ENSURE_NAMEDMAX(x: SEXP) {
    unsafe {
        SET_NAMED(x, 2);
    }
}

/// Check if the GROWABLE_BIT is set.
#[inline]
unsafe fn GROWABLE_BIT_SET(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (((*x).sxpinfo.gp() & GROWABLE_BIT_MASK) != 0) as c_int
    }
}

/// Check ALTREP bit (same as R's ALTREP() macro on the sxpinfo alt flag).
#[inline]
#[cfg(feature = "altrep")]
unsafe fn ALTREP_CHECK(x: SEXP) -> c_int {
    unsafe { ALTREP(x) }
}

/// Raise a typed error for SEXPTYPEs this port cannot duplicate/copy yet.
unsafe fn UNIMPLEMENTED_TYPE(routine: *const c_char, s: SEXP) -> ! {
    unsafe {
        let routine = if routine.is_null() {
            "duplicate"
        } else {
            CStr::from_ptr(routine).to_str().unwrap_or("duplicate")
        };
        let sexptype = if s.is_null() { -1 } else { TYPEOF(s) };
        std::panic::panic_any(crate::sexp::context::RError {
            message: format!("{routine}: unsupported SEXPTYPE {sexptype}"),
        });
    }
}

/// Set the DDVAL flag on a symbol.
#[inline]
unsafe fn SET_DDVAL(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() {
            let gp = if v != 0 {
                (*x).sxpinfo.gp() | DDVAL_MASK
            } else {
                (*x).sxpinfo.gp() & !DDVAL_MASK
            };
            (*x).sxpinfo.set_gp(gp);
        }
    }
}

/// Check if a type is pairlist-like.
#[inline]
unsafe fn isPairList(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        (t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP || t == SEXPTYPE::DOTSXP) as c_int
    }
}

/// Check if a type is a vector list (VECSXP, EXPRSXP).
#[inline]
unsafe fn isVectorList(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        (t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP) as c_int
    }
}

/// Get nrows from dim attribute.
unsafe fn nrows(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let dim = ATTRIB(x);
        if dim.is_null() || dim == R_NilValue() {
            return 0;
        }
        // dim should be an integer vector; first element is nrows
        if TYPEOF(dim) != SEXPTYPE::INTSXP {
            return 0;
        }
        let len = LENGTH(dim);
        if len < 2 {
            return if len == 1 { INTEGER_ELT(dim, 0) } else { 0 };
        }
        INTEGER_ELT(dim, 0)
    }
}

/// Get ncols from dim attribute.
unsafe fn ncols(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let dim = ATTRIB(x);
        if dim.is_null() || dim == R_NilValue() {
            return 0;
        }
        if TYPEOF(dim) != SEXPTYPE::INTSXP {
            return 0;
        }
        let len = LENGTH(dim);
        if len < 2 {
            return 0;
        }
        INTEGER_ELT(dim, 1)
    }
}

// ---------------------------------------------------------------------------
// Internal macros / inline helpers
// ---------------------------------------------------------------------------

/// Copy true length from `from` to `to`, unless GROWABLE_BIT_SET(from).
#[inline]
unsafe fn COPY_TRUELENGTH(to: SEXP, from: SEXP) {
    unsafe {
        if GROWABLE_BIT_SET(from) == 0 {
            SET_TRUELENGTH(to, TRUELENGTH(from));
        }
    }
}

/// Duplicate attributes from `from` to `to`.
/// If `from` has non-nil attributes, they are deep or shallow duplicated
/// based on the `deep` flag.
#[inline]
unsafe fn DUPLICATE_ATTRIB(to: SEXP, from: SEXP, deep: c_int) {
    unsafe {
        let a = ATTRIB(from);
        if !a.is_null() && a != R_NilValue() {
            SET_ATTRIB(to, duplicate1(a, deep));
            SET_OBJECT(to, OBJECT(from));
            if IS_S4_OBJECT(from) != 0 {
                SET_S4_OBJECT(to);
            } else {
                UNSET_S4_OBJECT(to);
            }
        }
    }
}

/// Copy tag from `from` to `to`, if it is non-nil.
#[inline]
unsafe fn COPY_TAG(to: SEXP, from: SEXP) {
    unsafe {
        let tag = TAG(from);
        if !tag.is_null() && tag != R_NilValue() {
            SETTAG(to, tag);
        }
    }
}

/// Generic function to duplicate an atomic vector.
/// Handles the memcpy for the data and copies attributes.
unsafe fn duplicate_atomic_vector(
    elem_size: usize,
    to: *mut SEXP,
    from: SEXP,
    deep: c_int,
) -> SEXP {
    unsafe {
        let n = XLENGTH(from);
        let new_vec = Rf_allocVector3(TYPEOF(from), n);
        *to = new_vec;
        if n > 0 {
            let from_data = DATAPTR(from);
            let to_data = DATAPTR(new_vec);
            if !from_data.is_null() && !to_data.is_null() {
                let total_bytes = (n as usize) * elem_size;
                ptr::copy_nonoverlapping(from_data as *const u8, to_data as *mut u8, total_bytes);
            }
        }
        DUPLICATE_ATTRIB(new_vec, from, deep);
        COPY_TRUELENGTH(new_vec, from);
        new_vec
    }
}

// ---------------------------------------------------------------------------
// FILL_MATRIX_ITERATE macro equivalent
// ---------------------------------------------------------------------------

/// Iterator for filling a matrix from a vector with re-use.
///
/// This is the Rust equivalent of R's `FILL_MATRIX_ITERATE` macro.
/// Calls `f(didx, sidx)` for each destination/source index pair.
///
/// Parameters:
/// - `dstart`: starting destination index
/// - `drows`: number of destination rows
/// - `srows`: number of source rows
/// - `cols`: number of columns
/// - `nsrc`: source length (for recycling)
/// - `f`: callback receiving (didx, sidx)
unsafe fn fill_matrix_iterate<F>(
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
    mut f: F,
) where
    F: FnMut(R_xlen_t, R_xlen_t),
{
    let mut i: R_xlen_t = 0;
    let mut sidx: R_xlen_t = 0;
    while i < srows {
        sidx = i;
        let mut j: R_xlen_t = 0;
        let mut didx: R_xlen_t = dstart + i;
        while j < cols {
            if sidx >= nsrc {
                sidx -= nsrc;
            }
            f(didx, sidx);
            j += 1;
            sidx += srows;
            if sidx >= nsrc {
                sidx -= nsrc;
            }
            didx += drows;
        }
        i += 1;
    }
}

/// Iterator for filling a matrix by-row.
///
/// This is the Rust equivalent of R's `FILL_MATRIX_BYROW_ITERATE` macro.
/// Calls `f(didx, sidx)` for each destination/source index pair.
unsafe fn fill_matrix_byrow_iterate<F>(
    dstart: R_xlen_t,
    drows: R_xlen_t,
    dcols: R_xlen_t,
    nsrc: R_xlen_t,
    mut f: F,
) where
    F: FnMut(R_xlen_t, R_xlen_t),
{
    let mut i: R_xlen_t = 0;
    let mut sidx: R_xlen_t = 0;
    while i < drows {
        let mut j: R_xlen_t = 0;
        let mut didx: R_xlen_t = dstart + i;
        while j < dcols {
            if sidx >= nsrc {
                sidx -= nsrc;
            }
            f(didx, sidx);
            j += 1;
            sidx += 1;
            if sidx >= nsrc {
                sidx -= nsrc;
            }
            didx += drows;
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// Core duplicate functions
// ---------------------------------------------------------------------------

/// Core recursive duplication function.
///
/// `deep`: if nonzero, performs deep copy; if zero, performs shallow copy
/// (shared subtrees for pairlists/vectors, but new atomic vectors are copied).
unsafe fn duplicate1(s: SEXP, deep: c_int) -> SEXP {
    unsafe {
        if s.is_null() {
            return ptr::null_mut();
        }

        // ALTREP: try class-specific duplicate when the alt bit is set
        #[cfg(feature = "altrep")]
        if ALTREP_CHECK(s) != 0 {
            let ans = ALTREP_DUPLICATE_EX(s, deep);
            if !ans.is_null() {
                return ans;
            }
        }

        let mut t: SEXP = ptr::null_mut();

        match SEXPTYPE(TYPEOF(s)) {
            SEXPTYPE::NILSXP
            | SEXPTYPE::SYMSXP
            | SEXPTYPE::ENVSXP
            | SEXPTYPE::SPECIALSXP
            | SEXPTYPE::BUILTINSXP
            | SEXPTYPE::BCODESXP
            | SEXPTYPE::WEAKREFSXP => {
                return s;
            }
            SEXPTYPE::CLOSXP => {
                t = with_arena(|arena| arena.alloc_node(SEXPTYPE::CLOSXP));
                SET_FORMALS(t, FORMALS(s));
                SET_BODY(t, BODY(s));
                SET_CLOENV(t, CLOENV(s));
                DUPLICATE_ATTRIB(t, s, deep);
                if NOJIT(s) != 0 {
                    SET_NOJIT(t);
                }
                if MAYBEJIT(s) != 0 {
                    SET_MAYBEJIT(t);
                }
            }
            SEXPTYPE::LISTSXP => {
                t = duplicate_list(s, deep);
            }
            SEXPTYPE::LANGSXP => {
                t = duplicate_list(s, deep);
                (*t).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                DUPLICATE_ATTRIB(t, s, deep);
            }
            SEXPTYPE::DOTSXP => {
                t = duplicate_list(s, deep);
                (*t).sxpinfo.set_type(SEXPTYPE::DOTSXP);
                DUPLICATE_ATTRIB(t, s, deep);
            }
            SEXPTYPE::CHARSXP => {
                return s;
            }
            SEXPTYPE::EXPRSXP | SEXPTYPE::VECSXP => {
                let n = XLENGTH(s);
                t = Rf_allocVector3(TYPEOF(s), n);
                for i in 0..n {
                    SET_VECTOR_ELT(t, i, duplicate_child(VECTOR_ELT(s, i), deep));
                }
                DUPLICATE_ATTRIB(t, s, deep);
                COPY_TRUELENGTH(t, s);
            }
            SEXPTYPE::LGLSXP => {
                let mut result: SEXP = ptr::null_mut();
                t = duplicate_atomic_vector(std::mem::size_of::<c_int>(), &mut result, s, deep);
            }
            SEXPTYPE::INTSXP => {
                let mut result: SEXP = ptr::null_mut();
                t = duplicate_atomic_vector(std::mem::size_of::<c_int>(), &mut result, s, deep);
            }
            SEXPTYPE::REALSXP => {
                let mut result: SEXP = ptr::null_mut();
                t = duplicate_atomic_vector(std::mem::size_of::<c_double>(), &mut result, s, deep);
            }
            SEXPTYPE::CPLXSXP => {
                let mut result: SEXP = ptr::null_mut();
                t = duplicate_atomic_vector(std::mem::size_of::<Rcomplex>(), &mut result, s, deep);
            }
            SEXPTYPE::RAWSXP => {
                let mut result: SEXP = ptr::null_mut();
                t = duplicate_atomic_vector(std::mem::size_of::<Rbyte>(), &mut result, s, deep);
            }
            SEXPTYPE::STRSXP => {
                let n = XLENGTH(s);
                t = Rf_allocVector3(TYPEOF(s), n);
                for i in 0..n {
                    SET_STRING_ELT(t, i, STRING_ELT(s, i));
                }
                DUPLICATE_ATTRIB(t, s, deep);
                COPY_TRUELENGTH(t, s);
            }
            SEXPTYPE::PROMSXP => {
                return s;
            }
            SEXPTYPE::OBJSXP => {
                t = crate::mainutils::objects::R_allocObject();
                if !t.is_null() {
                    DUPLICATE_ATTRIB(t, s, deep);
                } else {
                    UNIMPLEMENTED_TYPE(b"duplicate\0".as_ptr() as *const c_char, s);
                }
            }
            _ => {
                UNIMPLEMENTED_TYPE(b"duplicate\0".as_ptr() as *const c_char, s);
            }
        }

        // Copy OBJECT and S4 flags if types match
        if TYPEOF(t) == TYPEOF(s) {
            SET_OBJECT(t, OBJECT(s));
            if IS_S4_OBJECT(s) != 0 {
                SET_S4_OBJECT(t);
            } else {
                UNSET_S4_OBJECT(t);
            }
        }

        t
    }
}

/// Deep duplicate an SEXP.
pub unsafe fn duplicate(s: SEXP) -> SEXP {
    unsafe { duplicate1(s, 1) }
}

/// Alias for duplicate (R API).
pub unsafe fn Rf_duplicate(s: SEXP) -> SEXP {
    unsafe { duplicate(s) }
}

/// Shallow duplicate an SEXP.
pub unsafe fn shallow_duplicate(s: SEXP) -> SEXP {
    unsafe { duplicate1(s, 0) }
}

/// Lazy duplicate: just set NAMEDMAX on the input.
/// Returns the input unchanged (no copy is made).
pub unsafe fn lazy_duplicate(s: SEXP) -> SEXP {
    unsafe {
        if s.is_null() {
            return s;
        }
        match SEXPTYPE(TYPEOF(s)) {
            SEXPTYPE::NILSXP
            | SEXPTYPE::SYMSXP
            | SEXPTYPE::ENVSXP
            | SEXPTYPE::SPECIALSXP
            | SEXPTYPE::BUILTINSXP
            | SEXPTYPE::CHARSXP
            | SEXPTYPE::PROMSXP => {
                // Immutable types - nothing to do
            }
            SEXPTYPE::CLOSXP
            | SEXPTYPE::LISTSXP
            | SEXPTYPE::LANGSXP
            | SEXPTYPE::DOTSXP
            | SEXPTYPE::EXPRSXP
            | SEXPTYPE::VECSXP
            | SEXPTYPE::LGLSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::RAWSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::OBJSXP => {
                ENSURE_NAMEDMAX(s);
            }
            _ => {} // intentionally unhandled: SEXPTYPE does not require NAMEDMAX enforcement
        }
        s
    }
}

/// Helper: call duplicate1 or lazy_duplicate based on deep flag.
unsafe fn duplicate_child(s: SEXP, deep: c_int) -> SEXP {
    unsafe {
        if deep != 0 {
            duplicate1(s, 1)
        } else {
            lazy_duplicate(s)
        }
    }
}

// ---------------------------------------------------------------------------
// Cycle detection
// ---------------------------------------------------------------------------

/// Detect cycles that would be created by assigning `child` as a
/// component of `s` in a complex assignment.
pub unsafe fn R_cycle_detected(s: SEXP, child: SEXP) -> c_int {
    unsafe {
        if s == child {
            match SEXPTYPE(TYPEOF(child)) {
                SEXPTYPE::NILSXP
                | SEXPTYPE::SYMSXP
                | SEXPTYPE::ENVSXP
                | SEXPTYPE::SPECIALSXP
                | SEXPTYPE::BUILTINSXP => {
                    return 0; // OK cycle
                }
                _ => {
                    return 1; // Bad cycle
                }
            }
        }

        // Check attributes
        let attr = ATTRIB(child);
        if !attr.is_null() && attr != R_NilValue() && R_cycle_detected(s, attr) != 0 {
            return 1;
        }

        // Check pairlist
        if isPairList(child) != 0 {
            let mut el = child;
            while !el.is_null() && el != R_NilValue() {
                if s == el || R_cycle_detected(s, CAR(el)) != 0 {
                    return 1;
                }
                let el_attr = ATTRIB(el);
                if !el_attr.is_null()
                    && el_attr != R_NilValue()
                    && R_cycle_detected(s, el_attr) != 0
                {
                    return 1;
                }
                el = CDR(el);
            }
        } else if isVectorList(child) != 0 {
            let len = LENGTH(child);
            for i in 0..len {
                if R_cycle_detected(s, VECTOR_ELT(child, i as R_xlen_t)) != 0 {
                    return 1;
                }
            }
        }

        0
    }
}

// ---------------------------------------------------------------------------
// Pairlist duplication
// ---------------------------------------------------------------------------

/// Duplicate a pairlist (LISTSXP/LANGSXP/DOTSXP).
unsafe fn duplicate_list(s: SEXP, deep: c_int) -> SEXP {
    unsafe {
        let mut val: SEXP = R_NilValue();

        // First pass: build the skeleton list
        let mut sp = s;
        while !sp.is_null() && sp != R_NilValue() {
            val = Rf_cons(R_NilValue(), val);
            sp = CDR(sp);
        }

        // Second pass: fill in CAR, TAG, and ATTRIB
        sp = s;
        let mut vp = val;
        while !sp.is_null() && sp != R_NilValue() {
            SETCAR(vp, duplicate_child(CAR(sp), deep));
            COPY_TAG(vp, sp);
            DUPLICATE_ATTRIB(vp, sp, deep);
            sp = CDR(sp);
            vp = CDR(vp);
        }

        val
    }
}

// ---------------------------------------------------------------------------
// copyVector
// ---------------------------------------------------------------------------

/// Copy the contents of vector `t` into vector `s`.
///
/// Both vectors must have the same type. The source `t` is recycled
/// into the destination `s` if it is shorter.
pub unsafe fn copyVector(s: SEXP, t: SEXP) {
    unsafe {
        let sT = TYPEOF(s);
        let tT = TYPEOF(t);
        if sT != tT {
            return; // In real R this would error
        }
        let ns = XLENGTH(s);
        let nt = XLENGTH(t);

        match SEXPTYPE(sT) {
            SEXPTYPE::STRSXP => {
                xcopyStringWithRecycle(s, t, 0, ns, nt);
            }
            SEXPTYPE::LGLSXP => {
                xcopyLogicalWithRecycle(LOGICAL(s), LOGICAL(t), 0, ns, nt);
            }
            SEXPTYPE::INTSXP => {
                xcopyIntegerWithRecycle(INTEGER(s), INTEGER(t), 0, ns, nt);
            }
            SEXPTYPE::REALSXP => {
                xcopyRealWithRecycle(REAL(s), REAL(t), 0, ns, nt);
            }
            SEXPTYPE::CPLXSXP => {
                xcopyComplexWithRecycle(COMPLEX(s), COMPLEX(t), 0, ns, nt);
            }
            SEXPTYPE::EXPRSXP | SEXPTYPE::VECSXP => {
                xcopyVectorWithRecycle(s, t, 0, ns, nt);
            }
            SEXPTYPE::RAWSXP => {
                xcopyRawWithRecycle(RAW(s), RAW(t), 0, ns, nt);
            }
            _ => {
                UNIMPLEMENTED_TYPE(b"copyVector\0".as_ptr() as *const c_char, s);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// copyListMatrix (legacy, no longer used by R)
// ---------------------------------------------------------------------------

/// Copy list matrix contents (legacy function).
#[allow(clippy::never_loop)]
pub unsafe fn copyListMatrix(s: SEXP, t: SEXP, byrow: c_int) {
    unsafe {
        let nr = nrows(s);
        let nc = ncols(s);
        let ns = (nr as R_xlen_t) * (nc as R_xlen_t);

        let mut pt = t;
        if byrow != 0 {
            let nR = nr as R_xlen_t;
            let tmp = Rf_allocVector3(SEXPTYPE::VECSXP, ns);
            for i in 0..nr {
                for j in 0..nc {
                    let idx = (i as R_xlen_t) + (j as R_xlen_t) * nR;
                    SET_VECTOR_ELT(tmp, idx, duplicate(CAR(pt)));
                    pt = CDR(pt);
                    if pt.is_null() || pt == R_NilValue() {
                        pt = t;
                    }
                }
            }
            let mut sp = s;
            for i in 0..ns {
                SETCAR(sp, VECTOR_ELT(tmp, i));
                sp = CDR(sp);
            }
        } else {
            let mut sp = s;
            for _ in 0..ns {
                SETCAR(sp, duplicate(CAR(pt)));
                sp = CDR(sp);
                pt = CDR(pt);
                if pt.is_null() || pt == R_NilValue() {
                    pt = t;
                }
                if sp.is_null() || sp == R_NilValue() {
                    break;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// copyMatrix
// ---------------------------------------------------------------------------

/// Helper for VECTOR_ELT_LD: returns lazy_duplicate of the element.
unsafe fn VECTOR_ELT_LD(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe { lazy_duplicate(VECTOR_ELT(x, i)) }
}

/// Copy matrix contents from `t` into `s`.
///
/// If `byrow` is nonzero, fills by rows; otherwise fills by columns
/// (which is equivalent to copyVector).
pub unsafe fn copyMatrix(s: SEXP, t: SEXP, byrow: c_int) {
    unsafe {
        let nr = nrows(s) as R_xlen_t;
        let nc = ncols(s) as R_xlen_t;
        let nt = XLENGTH(t);

        if byrow != 0 {
            match SEXPTYPE(TYPEOF(s)) {
                SEXPTYPE::STRSXP => {
                    fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                        SET_STRING_ELT(s, didx, STRING_ELT(t, sidx));
                    });
                }
                SEXPTYPE::LGLSXP => {
                    fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                        *LOGICAL(s).add(didx as usize) = *LOGICAL(t).add(sidx as usize);
                    });
                }
                SEXPTYPE::INTSXP => {
                    fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                        *INTEGER(s).add(didx as usize) = *INTEGER(t).add(sidx as usize);
                    });
                }
                SEXPTYPE::REALSXP => {
                    fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                        *REAL(s).add(didx as usize) = *REAL(t).add(sidx as usize);
                    });
                }
                SEXPTYPE::CPLXSXP => {
                    fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                        *COMPLEX(s).add(didx as usize) = *COMPLEX(t).add(sidx as usize);
                    });
                }
                SEXPTYPE::EXPRSXP | SEXPTYPE::VECSXP => {
                    fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                        SET_VECTOR_ELT(s, didx, VECTOR_ELT_LD(t, sidx));
                    });
                }
                SEXPTYPE::RAWSXP => {
                    fill_matrix_byrow_iterate(0, nr, nc, nt, |didx, sidx| {
                        *RAW(s).add(didx as usize) = *RAW(t).add(sidx as usize);
                    });
                }
                _ => {
                    UNIMPLEMENTED_TYPE(b"copyMatrix\0".as_ptr() as *const c_char, s);
                }
            }
        } else {
            copyVector(s, t);
        }
    }
}

// ---------------------------------------------------------------------------
// xcopy*WithRecycle functions
// ---------------------------------------------------------------------------

/// Copy complex data with recycling.
pub unsafe fn xcopyComplexWithRecycle(
    dst: *mut Rcomplex,
    src: *const Rcomplex,
    dstart: R_xlen_t,
    n: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        if dst.is_null() || src.is_null() || n == 0 {
            return;
        }
        if nsrc >= n {
            for i in 0..n {
                *dst.add((dstart + i) as usize) = *src.add(i as usize);
            }
            return;
        }
        if nsrc == 1 {
            let val = *src;
            for i in 0..n {
                *dst.add((dstart + i) as usize) = val;
            }
            return;
        }
        let mut sidx: R_xlen_t = 0;
        for i in 0..n {
            if sidx == nsrc {
                sidx = 0;
            }
            *dst.add((dstart + i) as usize) = *src.add(sidx as usize);
            sidx += 1;
        }
    }
}

/// Copy integer data with recycling.
pub unsafe fn xcopyIntegerWithRecycle(
    dst: *mut c_int,
    src: *const c_int,
    dstart: R_xlen_t,
    n: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        if dst.is_null() || src.is_null() || n == 0 {
            return;
        }
        if nsrc >= n {
            for i in 0..n {
                *dst.add((dstart + i) as usize) = *src.add(i as usize);
            }
            return;
        }
        if nsrc == 1 {
            let val = *src;
            for i in 0..n {
                *dst.add((dstart + i) as usize) = val;
            }
            return;
        }
        let mut sidx: R_xlen_t = 0;
        for i in 0..n {
            if sidx == nsrc {
                sidx = 0;
            }
            *dst.add((dstart + i) as usize) = *src.add(sidx as usize);
            sidx += 1;
        }
    }
}

/// Copy logical data with recycling.
pub unsafe fn xcopyLogicalWithRecycle(
    dst: *mut c_int,
    src: *const c_int,
    dstart: R_xlen_t,
    n: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        xcopyIntegerWithRecycle(dst, src, dstart, n, nsrc);
    }
}

/// Copy raw data with recycling.
pub unsafe fn xcopyRawWithRecycle(
    dst: *mut Rbyte,
    src: *const Rbyte,
    dstart: R_xlen_t,
    n: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        if dst.is_null() || src.is_null() || n == 0 {
            return;
        }
        if nsrc >= n {
            for i in 0..n {
                *dst.add((dstart + i) as usize) = *src.add(i as usize);
            }
            return;
        }
        if nsrc == 1 {
            let val = *src;
            for i in 0..n {
                *dst.add((dstart + i) as usize) = val;
            }
            return;
        }
        let mut sidx: R_xlen_t = 0;
        for i in 0..n {
            if sidx == nsrc {
                sidx = 0;
            }
            *dst.add((dstart + i) as usize) = *src.add(sidx as usize);
            sidx += 1;
        }
    }
}

/// Copy real (double) data with recycling.
pub unsafe fn xcopyRealWithRecycle(
    dst: *mut c_double,
    src: *const c_double,
    dstart: R_xlen_t,
    n: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        if dst.is_null() || src.is_null() || n == 0 {
            return;
        }
        if nsrc >= n {
            for i in 0..n {
                *dst.add((dstart + i) as usize) = *src.add(i as usize);
            }
            return;
        }
        if nsrc == 1 {
            let val = *src;
            for i in 0..n {
                *dst.add((dstart + i) as usize) = val;
            }
            return;
        }
        let mut sidx: R_xlen_t = 0;
        for i in 0..n {
            if sidx == nsrc {
                sidx = 0;
            }
            *dst.add((dstart + i) as usize) = *src.add(sidx as usize);
            sidx += 1;
        }
    }
}

/// Copy string vector elements with recycling.
pub unsafe fn xcopyStringWithRecycle(
    dst: SEXP,
    src: SEXP,
    dstart: R_xlen_t,
    n: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        if dst.is_null() || src.is_null() || n == 0 {
            return;
        }
        if nsrc >= n {
            for i in 0..n {
                SET_STRING_ELT(dst, dstart + i, STRING_ELT(src, i));
            }
            return;
        }
        if nsrc == 1 {
            let val = STRING_ELT(src, 0);
            for i in 0..n {
                SET_STRING_ELT(dst, dstart + i, val);
            }
            return;
        }
        let mut sidx: R_xlen_t = 0;
        for i in 0..n {
            if sidx == nsrc {
                sidx = 0;
            }
            SET_STRING_ELT(dst, dstart + i, STRING_ELT(src, sidx));
            sidx += 1;
        }
    }
}

/// Copy generic vector elements with recycling.
pub unsafe fn xcopyVectorWithRecycle(
    dst: SEXP,
    src: SEXP,
    dstart: R_xlen_t,
    n: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        if dst.is_null() || src.is_null() || n == 0 {
            return;
        }
        if nsrc >= n {
            for i in 0..n {
                SET_VECTOR_ELT(dst, dstart + i, VECTOR_ELT_LD(src, i));
            }
            return;
        }
        if nsrc == 1 {
            let val = VECTOR_ELT_LD(src, 0);
            for i in 0..n {
                SET_VECTOR_ELT(dst, dstart + i, val);
            }
            return;
        }
        let mut sidx: R_xlen_t = 0;
        for i in 0..n {
            if sidx == nsrc {
                sidx = 0;
            }
            SET_VECTOR_ELT(dst, dstart + i, VECTOR_ELT_LD(src, sidx));
            sidx += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// xfill*MatrixWithRecycle functions
// ---------------------------------------------------------------------------

/// Fill complex matrix with recycling.
pub unsafe fn xfillComplexMatrixWithRecycle(
    dst: *mut Rcomplex,
    src: *mut Rcomplex,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        fill_matrix_iterate(dstart, drows, srows, cols, nsrc, |didx, sidx| {
            *dst.add(didx as usize) = *src.add(sidx as usize);
        });
    }
}

/// Fill integer matrix with recycling.
pub unsafe fn xfillIntegerMatrixWithRecycle(
    dst: *mut c_int,
    src: *mut c_int,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        fill_matrix_iterate(dstart, drows, srows, cols, nsrc, |didx, sidx| {
            *dst.add(didx as usize) = *src.add(sidx as usize);
        });
    }
}

/// Fill logical matrix with recycling.
pub unsafe fn xfillLogicalMatrixWithRecycle(
    dst: *mut c_int,
    src: *mut c_int,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        xfillIntegerMatrixWithRecycle(dst, src, dstart, drows, srows, cols, nsrc);
    }
}

/// Fill raw matrix with recycling.
pub unsafe fn xfillRawMatrixWithRecycle(
    dst: *mut Rbyte,
    src: *mut Rbyte,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        fill_matrix_iterate(dstart, drows, srows, cols, nsrc, |didx, sidx| {
            *dst.add(didx as usize) = *src.add(sidx as usize);
        });
    }
}

/// Fill real matrix with recycling.
pub unsafe fn xfillRealMatrixWithRecycle(
    dst: *mut c_double,
    src: *mut c_double,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        fill_matrix_iterate(dstart, drows, srows, cols, nsrc, |didx, sidx| {
            *dst.add(didx as usize) = *src.add(sidx as usize);
        });
    }
}

/// Fill string matrix with recycling.
pub unsafe fn xfillStringMatrixWithRecycle(
    dst: SEXP,
    src: SEXP,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        fill_matrix_iterate(dstart, drows, srows, cols, nsrc, |didx, sidx| {
            SET_STRING_ELT(dst, didx, STRING_ELT(src, sidx));
        });
    }
}

/// Fill generic vector matrix with recycling.
pub unsafe fn xfillVectorMatrixWithRecycle(
    dst: SEXP,
    src: SEXP,
    dstart: R_xlen_t,
    drows: R_xlen_t,
    srows: R_xlen_t,
    cols: R_xlen_t,
    nsrc: R_xlen_t,
) {
    unsafe {
        fill_matrix_iterate(dstart, drows, srows, cols, nsrc, |didx, sidx| {
            SET_VECTOR_ELT(dst, didx, VECTOR_ELT(src, sidx));
        });
    }
}

// ---------------------------------------------------------------------------
// duplicate_attr: duplicate before attribute modification
// ---------------------------------------------------------------------------

/// Threshold for trying ALTREP wrapping (stub: always falls through).
#[cfg(feature = "altrep")]
const WRAP_THRESHOLD: R_xlen_t = 64;

/// Internal: duplicate for attribute modification.
///
/// For large vectors, tries ALTREP wrapping first (stub: always falls through).
/// Falls back to `duplicate` or `shallow_duplicate`.
unsafe fn duplicate_attr(x: SEXP, deep: c_int) -> SEXP {
    unsafe {
        if x.is_null() {
            return x;
        }
        // Check if vector and large enough
        #[cfg(feature = "altrep")]
        if Rf_isVector(x) != 0 && XLENGTH(x) >= WRAP_THRESHOLD {
            let val = R_tryWrap(x);
            if !val.is_null() && val != x {
                if deep != 0 {
                    let attr = ATTRIB(val);
                    if !attr.is_null() && attr != R_NilValue() {
                        SET_ATTRIB(val, duplicate(attr));
                    }
                }
                return val;
            }
        }
        if deep != 0 {
            duplicate(x)
        } else {
            shallow_duplicate(x)
        }
    }
}

/// Shallow duplicate before attribute modification.
pub unsafe fn R_shallow_duplicate_attr(x: SEXP) -> SEXP {
    unsafe { duplicate_attr(x, 0) }
}

/// Deep duplicate before attribute modification.
pub unsafe fn R_duplicate_attr(x: SEXP) -> SEXP {
    unsafe { duplicate_attr(x, 1) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::constructors::Rf_ScalarInteger;
    use crate::sexp::memory::RArena;

    /// Helper to create an integer vector with values.
    unsafe fn make_int_vector(values: &[c_int]) -> SEXP {
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::INTSXP, values.len() as R_xlen_t);
            for (i, &val) in values.iter().enumerate() {
                *INTEGER(v).add(i) = val;
            }
            v
        }
    }

    /// Helper to create a real vector with values.
    unsafe fn make_real_vector(values: &[c_double]) -> SEXP {
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::REALSXP, values.len() as R_xlen_t);
            for (i, &val) in values.iter().enumerate() {
                *REAL(v).add(i) = val;
            }
            v
        }
    }

    #[test]
    fn test_duplicate_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let d = duplicate(R_NilValue());
            assert_eq!(d, R_NilValue());
        }
    }

    #[test]
    fn test_duplicate_integer_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = make_int_vector(&[1, 2, 3]);
            let d = duplicate(v);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(d), 3);
            assert_ne!(d, v); // Should be a new allocation
            assert_eq!(*INTEGER(d).add(0), 1);
            assert_eq!(*INTEGER(d).add(1), 2);
            assert_eq!(*INTEGER(d).add(2), 3);
        }
    }

    #[test]
    fn test_duplicate_real_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = make_real_vector(&[1.5, 2.5, 3.5]);
            let d = duplicate(v);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::REALSXP);
            assert_eq!(LENGTH(d), 3);
            assert_ne!(d, v);
            assert!((*REAL(d).add(0) - 1.5).abs() < 1e-10);
            assert!((*REAL(d).add(1) - 2.5).abs() < 1e-10);
            assert!((*REAL(d).add(2) - 3.5).abs() < 1e-10);
        }
    }

    #[test]
    fn test_shallow_duplicate_integer() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = make_int_vector(&[10, 20]);
            let d = shallow_duplicate(v);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(d).add(0), 10);
            assert_eq!(*INTEGER(d).add(1), 20);
        }
    }

    #[test]
    fn test_lazy_duplicate() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = make_int_vector(&[5, 6, 7]);
            let d = lazy_duplicate(v);
            assert_eq!(d, v); // Same pointer - no copy
            assert_eq!(NAMED(d), 2); // NAMEDMAX
        }
    }

    #[test]
    fn test_duplicate_pairlist() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let car = Rf_ScalarInteger(1);
            // Create a proper nil-terminated pairlist: (1)
            let list = Rf_cons(car, R_NilValue());

            let d = duplicate(list);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::LISTSXP);
            assert_ne!(d, list); // New allocation
            // CAR should be a duplicate of the original scalar
            let d_car = CAR(d);
            assert_eq!(TYPEOF(d_car), SEXPTYPE::INTSXP);
            // CDR should be nil
            assert_eq!(CDR(d), R_NilValue());
        }
    }

    #[test]
    fn test_duplicate_closure() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut arena = RArena::new();
            let formals = arena.alloc_node(SEXPTYPE::NILSXP);
            let body = Rf_ScalarInteger(42);
            let env = arena.alloc_node(SEXPTYPE::ENVSXP);

            // Create closure using mkCLOSXP
            let c = crate::mainutils::dstruct::mkCLOSXP(formals, body, env);
            let d = duplicate(c);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::CLOSXP);
            assert_ne!(d, c);
            // Formals, body, env are shared (not deep-copied)
            assert_eq!(FORMALS(d), formals);
            assert_eq!(BODY(d), body);
            assert_eq!(CLOENV(d), env);
        }
    }

    #[test]
    fn test_cycle_detected_self() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(1);
            // Self-reference is a cycle for most types
            assert_ne!(R_cycle_detected(v, v), 0);
        }
    }

    #[test]
    fn test_cycle_detected_nil_ok() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // NILSXP self-reference is OK
            assert_eq!(R_cycle_detected(R_NilValue(), R_NilValue()), 0);
        }
    }

    #[test]
    fn test_cycle_detected_sym_ok() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut arena = RArena::new();
            let sym = arena.alloc_node(SEXPTYPE::SYMSXP);
            // SYMSXP self-reference is OK
            assert_eq!(R_cycle_detected(sym, sym), 0);
        }
    }

    #[test]
    fn test_cycle_detected_no_cycle() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let a = Rf_ScalarInteger(1);
            let b = Rf_ScalarInteger(2);
            assert_eq!(R_cycle_detected(a, b), 0);
        }
    }

    #[test]
    fn test_copy_vector_int() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dst = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
            let src = make_int_vector(&[10, 20, 30]);
            copyVector(dst, src);
            assert_eq!(*INTEGER(dst).add(0), 10);
            assert_eq!(*INTEGER(dst).add(1), 20);
            assert_eq!(*INTEGER(dst).add(2), 30);
        }
    }

    #[test]
    fn test_copy_vector_real() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dst = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
            let src = make_real_vector(&[1.0, 2.0, 3.0]);
            copyVector(dst, src);
            assert!((*REAL(dst).add(0) - 1.0).abs() < 1e-10);
            assert!((*REAL(dst).add(1) - 2.0).abs() < 1e-10);
            assert!((*REAL(dst).add(2) - 3.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_copy_vector_with_recycle() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dst = Rf_allocVector3(SEXPTYPE::INTSXP, 6);
            let src = make_int_vector(&[1, 2]);
            copyVector(dst, src);
            assert_eq!(*INTEGER(dst).add(0), 1);
            assert_eq!(*INTEGER(dst).add(1), 2);
            assert_eq!(*INTEGER(dst).add(2), 1);
            assert_eq!(*INTEGER(dst).add(3), 2);
            assert_eq!(*INTEGER(dst).add(4), 1);
            assert_eq!(*INTEGER(dst).add(5), 2);
        }
    }

    #[test]
    fn test_xcopy_real_no_recycle() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dst = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
            let src = make_real_vector(&[1.0, 2.0, 3.0]);
            xcopyRealWithRecycle(REAL(dst), REAL(src), 0, 3, 3);
            assert!((*REAL(dst).add(0) - 1.0).abs() < 1e-10);
            assert!((*REAL(dst).add(1) - 2.0).abs() < 1e-10);
            assert!((*REAL(dst).add(2) - 3.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_xcopy_real_with_recycle() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dst = Rf_allocVector3(SEXPTYPE::REALSXP, 5);
            let src = make_real_vector(&[10.0, 20.0]);
            xcopyRealWithRecycle(REAL(dst), REAL(src), 0, 5, 2);
            assert!((*REAL(dst).add(0) - 10.0).abs() < 1e-10);
            assert!((*REAL(dst).add(1) - 20.0).abs() < 1e-10);
            assert!((*REAL(dst).add(2) - 10.0).abs() < 1e-10);
            assert!((*REAL(dst).add(3) - 20.0).abs() < 1e-10);
            assert!((*REAL(dst).add(4) - 10.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_xcopy_real_scalar_recycle() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dst = Rf_allocVector3(SEXPTYPE::REALSXP, 4);
            let src = make_real_vector(&[42.0]);
            xcopyRealWithRecycle(REAL(dst), REAL(src), 0, 4, 1);
            for i in 0..4 {
                assert!((*REAL(dst).add(i) - 42.0).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn test_xcopy_int_with_recycle() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dst = Rf_allocVector3(SEXPTYPE::INTSXP, 5);
            let src = make_int_vector(&[7, 8, 9]);
            xcopyIntegerWithRecycle(INTEGER(dst), INTEGER(src), 0, 5, 3);
            assert_eq!(*INTEGER(dst).add(0), 7);
            assert_eq!(*INTEGER(dst).add(1), 8);
            assert_eq!(*INTEGER(dst).add(2), 9);
            assert_eq!(*INTEGER(dst).add(3), 7);
            assert_eq!(*INTEGER(dst).add(4), 8);
        }
    }

    #[test]
    fn test_xcopy_null_pointers() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Should not crash
            xcopyRealWithRecycle(ptr::null_mut(), ptr::null(), 0, 0, 0);
            xcopyIntegerWithRecycle(ptr::null_mut(), ptr::null(), 0, 0, 0);
        }
    }

    #[test]
    fn test_shallow_duplicate_attr() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = make_int_vector(&[1, 2, 3]);
            let d = R_shallow_duplicate_attr(v);
            assert!(!d.is_null());
        }
    }

    #[test]
    fn test_deep_duplicate_attr() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = make_int_vector(&[1, 2, 3]);
            let d = R_duplicate_attr(v);
            assert!(!d.is_null());
        }
    }

    #[test]
    fn test_duplicate_raw_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::RAWSXP, 4);
            *RAW(v).add(0) = 0xAA;
            *RAW(v).add(1) = 0xBB;
            *RAW(v).add(2) = 0xCC;
            *RAW(v).add(3) = 0xDD;
            let d = duplicate(v);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::RAWSXP);
            assert_eq!(LENGTH(d), 4);
            assert_eq!(*RAW(d).add(0), 0xAA);
            assert_eq!(*RAW(d).add(1), 0xBB);
            assert_eq!(*RAW(d).add(2), 0xCC);
            assert_eq!(*RAW(d).add(3), 0xDD);
        }
    }

    #[test]
    fn test_duplicate_logical_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::LGLSXP, 2);
            *LOGICAL(v).add(0) = 1;
            *LOGICAL(v).add(1) = 0;
            let d = duplicate(v);
            assert_eq!(*LOGICAL(d).add(0), 1);
            assert_eq!(*LOGICAL(d).add(1), 0);
        }
    }

    #[test]
    fn test_duplicate_complex_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::CPLXSXP, 2);
            *COMPLEX(v).add(0) = Rcomplex { r: 1.0, i: 2.0 };
            *COMPLEX(v).add(1) = Rcomplex { r: 3.0, i: 4.0 };
            let d = duplicate(v);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::CPLXSXP);
            assert!(((*COMPLEX(d).add(0)).r - 1.0).abs() < 1e-10);
            assert!(((*COMPLEX(d).add(0)).i - 2.0).abs() < 1e-10);
            assert!(((*COMPLEX(d).add(1)).r - 3.0).abs() < 1e-10);
            assert!(((*COMPLEX(d).add(1)).i - 4.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_duplicate_string_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s1 = Rf_allocVector3(SEXPTYPE::CHARSXP, 0); // placeholder CHARSXP
            let s2 = Rf_allocVector3(SEXPTYPE::CHARSXP, 0);
            let v = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
            SET_STRING_ELT(v, 0, s1);
            SET_STRING_ELT(v, 1, s2);
            let d = duplicate(v);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(d), 2);
            assert_eq!(STRING_ELT(d, 0), s1); // CHARSXP is shared, not copied
            assert_eq!(STRING_ELT(d, 1), s2);
        }
    }

    #[test]
    fn test_duplicate_vecsxp() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let elem1 = Rf_ScalarInteger(10);
            let elem2 = Rf_ScalarInteger(20);
            let v = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            SET_VECTOR_ELT(v, 0, elem1);
            SET_VECTOR_ELT(v, 1, elem2);
            let d = duplicate(v);
            assert!(!d.is_null());
            assert_eq!(TYPEOF(d), SEXPTYPE::VECSXP);
            assert_eq!(LENGTH(d), 2);
        }
    }

    #[test]
    fn test_duplicate_unsupported_type_errors() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let extptr = crate::sexp::memory_ext::allocSExp(SEXPTYPE::EXTPTRSXP);
            let err = std::panic::catch_unwind(|| {
                let _ = duplicate(extptr);
            })
            .expect_err("unsupported duplicate type should raise an RError");
            let message = err
                .downcast_ref::<crate::sexp::context::RError>()
                .map(|err| err.message.as_str())
                .unwrap_or("");
            assert!(message.contains("duplicate: unsupported SEXPTYPE"));
        }
    }
}
