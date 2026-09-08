#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(unused_imports)]

//! Shared support layer — SEXP type constants, local symbol shims, type and
//! named-fraction predicates, attribute/coercion/duplicate helpers ported from
//! R's C runtime.

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::mainutils::subscript::{
    OneIndex, get1index, int_arraySubscript, makeSubscript, mat2indsub, strmat2intmat, vectorIndex,
};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::envir::defineVar;
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory_ext::{allocList, allocSExp};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's NA_REAL sentinel (specific NaN bit pattern).
pub(crate) const NA_REAL: c_double = crate::sexp::ffi::NA_REAL;

/// Maximum value for R_xlen_t.
pub(crate) const R_XLEN_T_MAX: R_xlen_t = i64::MAX;

/// Raw SEXPTYPE integer constants for use in match expressions.
/// These match the SEXPTYPE values defined in ffi.rs.
pub(crate) const NILSXP: c_int = 0;
pub(crate) const SYMSXP: c_int = 1;
pub(crate) const LISTSXP: c_int = 2;
pub(crate) const CLOSXP: c_int = 3;
pub(crate) const ENVSXP: c_int = 4;
pub(crate) const PROMSXP: c_int = 5;
pub(crate) const LANGSXP: c_int = 6;
pub(crate) const SPECIALSXP: c_int = 7;
pub(crate) const BUILTINSXP: c_int = 8;
pub(crate) const CHARSXP: c_int = 9;
pub(crate) const LGLSXP: c_int = 10;
pub(crate) const INTSXP: c_int = 13;
pub(crate) const REALSXP: c_int = 14;
pub(crate) const CPLXSXP: c_int = 15;
pub(crate) const STRSXP: c_int = 16;
pub(crate) const DOTSXP: c_int = 17;
pub(crate) const ANYSXP: c_int = 18;
pub(crate) const VECSXP: c_int = 19;
pub(crate) const EXPRSXP: c_int = 20;
pub(crate) const BCODESXP: c_int = 21;
pub(crate) const EXTPTRSXP: c_int = 22;
pub(crate) const WEAKREFSXP: c_int = 23;
pub(crate) const RAWSXP: c_int = 24;
pub(crate) const OBJSXP: c_int = 25;
pub(crate) const FUNSXP: c_int = 99;

// ---------------------------------------------------------------------------
// Local symbol helpers
// ---------------------------------------------------------------------------

/// Get the "dim" symbol.
#[inline]
pub(crate) unsafe fn sym_Dim() -> SEXP {
    unsafe { Rf_install(c"dim".as_ptr()) }
}

/// Get the "names" symbol.
#[inline]
pub(crate) unsafe fn sym_Names() -> SEXP {
    unsafe { Rf_install(c"names".as_ptr()) }
}

/// Get the "dimnames" symbol.
#[inline]
pub(crate) unsafe fn sym_DimNames() -> SEXP {
    unsafe { Rf_install(c"dimnames".as_ptr()) }
}

/// Get the "class" symbol.
#[inline]
pub(crate) unsafe fn sym_Class() -> SEXP {
    unsafe { Rf_install(c"class".as_ptr()) }
}

/// Get the "use.names" symbol (for subscript name passing).
#[inline]
pub(crate) unsafe fn sym_UseNames() -> SEXP {
    unsafe { Rf_install(c"use.names".as_ptr()) }
}

// ---------------------------------------------------------------------------
// Local type-checking helpers
// ---------------------------------------------------------------------------

#[inline]
pub(crate) unsafe fn isNull(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() }
}

#[inline]
pub(crate) unsafe fn isVector(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == LGLSXP
            || t == INTSXP
            || t == REALSXP
            || t == CPLXSXP
            || t == STRSXP
            || t == VECSXP
            || t == EXPRSXP
            || t == RAWSXP
    }
}

#[inline]
pub(crate) unsafe fn isVectorList(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == VECSXP || t == EXPRSXP
    }
}

#[inline]
pub(crate) unsafe fn isPairList(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == LISTSXP || t == NILSXP
    }
}

#[inline]
pub(crate) unsafe fn isList(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == LISTSXP }
}

#[inline]
pub(crate) unsafe fn isLanguage(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == LANGSXP }
}

#[inline]
pub(crate) unsafe fn isExpression(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == EXPRSXP }
}

#[inline]
pub(crate) unsafe fn isNewList(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == VECSXP }
}

#[inline]
pub(crate) unsafe fn isObject(x: SEXP) -> bool {
    unsafe { OBJECT(x) != 0 }
}

#[inline]
pub(crate) unsafe fn isMatrix(x: SEXP) -> bool {
    unsafe {
        let dim = getAttrib(x, sym_Dim());
        !isNull(dim) && LENGTH(dim) == 2
    }
}

#[inline]
pub(crate) unsafe fn isArray(x: SEXP) -> bool {
    unsafe {
        let dim = getAttrib(x, sym_Dim());
        !isNull(dim) && LENGTH(dim) >= 2
    }
}

#[inline]
pub(crate) unsafe fn isString(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == STRSXP }
}

#[inline]
pub(crate) unsafe fn isInteger(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == INTSXP }
}

#[inline]
pub(crate) unsafe fn isReal(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == REALSXP }
}

#[inline]
pub(crate) fn R_FINITE(x: c_double) -> bool {
    x.is_finite()
}

#[inline]
pub(crate) fn ISNA(x: c_double) -> bool {
    x.is_nan() && x.to_bits() != 0x7ff8000000000000u64
}

// ---------------------------------------------------------------------------
// Local helper stubs (functions not yet available in the codebase)
// ---------------------------------------------------------------------------

/// Check if an object has the S4 bit set.
#[inline]
pub(crate) unsafe fn IS_S4_OBJECT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let s4_mask: u16 = 1 << 4;
        (((*x).sxpinfo.gp() & s4_mask) != 0) as c_int
    }
}

/// Set the S4 bit on an object.
#[inline]
pub(crate) unsafe fn SET_S4_OBJECT(x: SEXP) {
    unsafe {
        if !x.is_null() {
            let s4_mask: u16 = 1 << 4;
            let gp = (*x).sxpinfo.gp() | s4_mask;
            (*x).sxpinfo.set_gp(gp);
        }
    }
}

/// Unset the S4 bit on an object.
#[inline]
pub(crate) unsafe fn UNSET_S4_OBJECT(x: SEXP) {
    unsafe {
        if !x.is_null() {
            let s4_mask: u16 = 1 << 4;
            let gp = (*x).sxpinfo.gp() & !s4_mask;
            (*x).sxpinfo.set_gp(gp);
        }
    }
}

/// Check if an object may be shared.
#[inline]
pub(crate) unsafe fn MAYBE_SHARED(x: SEXP) -> bool {
    unsafe {
        if x.is_null() {
            return true;
        } // conservative for null
        let info = (*x).sxpinfo;
        info.named() >= 2
    }
}

/// Check if an object may be referenced.
#[inline]
pub(crate) unsafe fn MAYBE_REFERENCED(x: SEXP) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        let info = (*x).sxpinfo;
        info.named() >= 1
    }
}

/// Mark an object as not mutable (NAMEDMAX).
#[inline]
pub(crate) unsafe fn MARK_NOT_MUTABLE(x: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).sxpinfo.set_named(2);
        }
    }
}

/// Set NAMED to 0 (setter-clear).
#[inline]
pub(crate) unsafe fn SETTER_CLEAR_NAMED(x: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).sxpinfo.set_named(0);
        }
    }
}

/// Raise NAMED level.
#[inline]
pub(crate) unsafe fn RAISE_NAMED(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() && (v as u8) > (*x).sxpinfo.named() {
            (*x).sxpinfo.set_named(v as u8);
        }
    }
}

/// Increment NAMED.
#[inline]
pub(crate) unsafe fn INCREMENT_NAMED(x: SEXP) {
    unsafe {
        if !x.is_null() {
            let n = (*x).sxpinfo.named();
            if n < 2 {
                (*x).sxpinfo.set_named(n + 1);
            }
        }
    }
}

/// Check if an object is growable (has truelength > length).
#[inline]
pub(crate) unsafe fn IS_GROWABLE(_x: SEXP) -> bool {
    // Simplified: always false since we don't fully implement truelength yet.
    false
}

/// Set the growable bit on an object.
#[inline]
pub(crate) unsafe fn SET_GROWABLE_BIT(x: SEXP) {
    unsafe {
        if !x.is_null() {
            let gp = (*x).sxpinfo.gp();
            (*x).sxpinfo.set_gp(gp | (1u16 << 5));
        }
    }
}

/// Set true length of a vector.
#[inline]
pub(crate) unsafe fn SET_TRUELENGTH(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() {
            (*x).data.vecsxp.truelength = v as R_xlen_t;
        }
    }
}

/// Get true length of a vector.
#[inline]
pub(crate) unsafe fn XTRUELENGTH(x: SEXP) -> R_xlen_t {
    unsafe {
        // Simplified: return XLENGTH
        XLENGTH(x)
    }
}

/// SETCADR: set the CAR of the CDR.
#[inline]
pub(crate) unsafe fn SETCADR(x: SEXP, v: SEXP) {
    unsafe {
        SETCAR(CDR(x), v);
    }
}

/// SET_TYPEOF: set the type of an SEXP.
#[inline]
pub(crate) unsafe fn SET_TYPEOF(x: SEXP, v: c_int) {
    unsafe {
        (*x).sxpinfo.set_type(SEXPTYPE(v));
    }
}

/// Set the standard vector length (not marking as immutable).
#[inline]
pub(crate) unsafe fn SET_STDVEC_LENGTH(x: SEXP, v: R_xlen_t) {
    unsafe {
        if !x.is_null() {
            (*x).data.vecsxp.length = v;
        }
    }
}

/// ENSURE_NAMEDMAX: set NAMED to NAMEDMAX.
#[inline]
pub(crate) unsafe fn ENSURE_NAMEDMAX(x: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).sxpinfo.set_named(2);
        }
    }
}

/// Check if the call is an assignment call.
#[inline]
pub(crate) unsafe fn IS_ASSIGNMENT_CALL(call: SEXP) -> bool {
    unsafe {
        if isNull(call) {
            return true;
        }
        let t = TYPEOF(call);
        t == LANGSXP || t == SYMSXP
    }
}

/// R_FixupRHS: fix up RHS for assignment (duplicate if needed).
pub(crate) unsafe fn R_FixupRHS(x: SEXP, y: SEXP) -> SEXP {
    unsafe {
        if MAYBE_SHARED(y) {
            shallow_duplicate(y)
        } else {
            y
        }
    }
}

/// PairToVectorList: convert a pairlist to a vector list.
pub(crate) unsafe fn PairToVectorList(x: SEXP) -> SEXP {
    unsafe {
        let len = Rf_length(x);
        let ans = Rf_allocVector3(VECSXP, len as R_xlen_t);
        let _ans_guard = protect(ans);
        let mut src = x;
        let mut i: R_xlen_t = 0;
        while !isNull(src) && i < len as R_xlen_t {
            SET_VECTOR_ELT(ans, i, CAR(src));
            src = CDR(src);
            i += 1;
        }
        ans
    }
}

/// VectorToPairList: convert a vector list to a pairlist.
pub unsafe fn VectorToPairList(x: SEXP) -> SEXP {
    unsafe {
        let len = Rf_length(x);
        let names = getAttrib(x, sym_Names());
        let mut result = R_NilValue();
        let mut i: R_xlen_t = len as R_xlen_t;
        while i > 0 {
            i -= 1;
            let cell = Rf_cons(VECTOR_ELT(x, i), result);
            if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == STRSXP
                && XLENGTH(names) > i
            {
                let name = STRING_ELT(names, i);
                if !name.is_null() {
                    let chars = CHAR(name);
                    if !chars.is_null() && *chars != 0 {
                        SETTAG(cell, Rf_install(chars));
                    }
                }
            }
            result = cell;
        }
        result
    }
}

/// coerceVector: coerce a vector to a different type.
pub(crate) unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    unsafe { crate::mainutils::coerce::coerceVector(x, type_) }
}

/// getAttrib: get an attribute from an object.
pub(crate) unsafe fn getAttrib(x: SEXP, what: SEXP) -> SEXP {
    unsafe { crate::eval::attrib_core::getAttrib(x, what) }
}

/// setAttrib: set an attribute on an object.
pub(crate) unsafe fn setAttrib(x: SEXP, what: SEXP, value: SEXP) {
    unsafe {
        crate::eval::attrib_core::setAttrib(x, what, value);
    }
}

/// shallow_duplicate: create a shallow copy.
pub(crate) unsafe fn shallow_duplicate(x: SEXP) -> SEXP {
    unsafe {
        if isNull(x) {
            return R_NilValue();
        }
        crate::mainutils::duplicate::shallow_duplicate(x)
    }
}

/// copyMostAttrib: copy most attributes from src to dest.
///
/// Copies all attributes except dim, dimnames, and names (which are
/// handled separately by EnlargeVector). Also copies the OBJECT flag
/// and S4 object bit.
pub(crate) unsafe fn copyMostAttrib(src: SEXP, dest: SEXP) {
    unsafe {
        use crate::eval::attrib_core::{R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol};
        if isNull(src) || isNull(dest) {
            return;
        }
        let src_attr = ATTRIB(src);
        if isNull(src_attr) {
            return;
        }
        // Copy every attribute except dim, dimnames and names: those are
        // handled separately (EnlargeVector already fixed up `names`, and
        // grown vectors lose their shape). Overwriting the whole attribute
        // list here would clobber the enlarged names attribute.
        let mut attr = src_attr;
        while !isNull(attr) {
            let tag = TAG(attr);
            if tag != R_NamesSymbol() && tag != R_DimSymbol() && tag != R_DimNamesSymbol() {
                setAttrib(dest, tag, CAR(attr));
            }
            attr = CDR(attr);
        }
        SET_OBJECT(dest, OBJECT(src));
        if IS_S4_OBJECT(src) != 0 {
            SET_S4_OBJECT(dest);
        } else {
            UNSET_S4_OBJECT(dest);
        }
    }
}

/// listAppend: append pairlist s to pairlist t.
pub(crate) unsafe fn listAppend(t: SEXP, s: SEXP) -> SEXP {
    unsafe {
        if isNull(t) {
            return s;
        }
        if isNull(s) {
            return t;
        }
        let mut p = t;
        while !isNull(CDR(p)) {
            p = CDR(p);
        }
        SETCDR(p, s);
        t
    }
}

/// asInteger: coerce to integer.
pub(crate) unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asInteger(x) }
}

/// nrows: get number of rows of a matrix.
pub(crate) unsafe fn nrows(x: SEXP) -> c_int {
    unsafe {
        let dim = getAttrib(x, sym_Dim());
        if isNull(dim) {
            return 0;
        }
        INTEGER(dim).read() as c_int
    }
}

/// ncols: get number of columns of a matrix.
pub(crate) unsafe fn ncols(x: SEXP) -> c_int {
    unsafe {
        let dim = getAttrib(x, sym_Dim());
        if isNull(dim) {
            return 0;
        }
        if LENGTH(dim) < 2 {
            return 1;
        }
        INTEGER(dim).add(1).read() as c_int
    }
}

/// installTrChar: install a symbol from a CHARSXP.
pub(crate) unsafe fn installTrChar(input: SEXP) -> SEXP {
    unsafe {
        if isNull(input) {
            return R_NilValue();
        }
        let c = CHAR(input);
        if c.is_null() {
            return R_NilValue();
        }
        Rf_install(c)
    }
}

pub(crate) unsafe fn ScalarInteger(x: c_int) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_ScalarInteger(x) }
}

pub(crate) unsafe fn ScalarReal(x: c_double) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_ScalarReal(x) }
}

/// ScalarString: create a length-1 character vector from a CHARSXP.
pub(crate) unsafe fn ScalarString(x: SEXP) -> SEXP {
    unsafe {
        let s = Rf_allocVector3(STRSXP, 1);
        let _s_guard = protect(s);
        SET_STRING_ELT(s, 0, x);
        s
    }
}

/// list2: create a 2-element list.
pub(crate) unsafe fn list2(a: SEXP, b: SEXP) -> SEXP {
    unsafe {
        let cdr = Rf_cons(b, R_NilValue());
        Rf_cons(a, cdr)
    }
}

/// nthcdr: walk n steps down a pairlist.
pub(crate) unsafe fn nthcdr(x: SEXP, mut n: c_int) -> SEXP {
    unsafe {
        let mut result = x;
        while n > 0 && !isNull(result) {
            result = CDR(result);
            n -= 1;
        }
        result
    }
}

/// checkArity: check function arity.
#[inline]
pub(crate) unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe { crate::mainutils::relop::checkArity(op, args) }
}

/// GetArrayDimnames: get dimnames attribute.
pub(crate) unsafe fn GetArrayDimnames(x: SEXP) -> SEXP {
    unsafe { getAttrib(x, sym_DimNames()) }
}

/// fixSubset3Args: prepare args for $ assignment.
pub(crate) unsafe fn fixSubset3Args(call: SEXP, args: SEXP, env: SEXP, nlist: *mut SEXP) -> SEXP {
    unsafe { crate::mainutils::subset::fixSubset3Args(call, args, env, nlist) }
}

/// NonNullStringMatch: match non-null strings.
pub(crate) unsafe fn NonNullStringMatch(s: SEXP, t: SEXP) -> c_int {
    unsafe { crate::mainutils::match_mod::NonNullStringMatch(s, t) }
}

/// R_CurrentExpression: stub returning nil.
pub(crate) unsafe fn R_CurrentExpression() -> SEXP {
    unsafe { R_NilValue() }
}

/// NA_STRING: get the NA string.
pub(crate) unsafe fn NA_STRING() -> SEXP {
    unsafe { crate::sexp::globals::R_NaString() }
}

/// R_BlankString: get the blank string.
pub(crate) unsafe fn R_BlankString() -> SEXP {
    unsafe { Rf_mkChar(b" \0".as_ptr() as *const c_char) }
}

/// DispatchOrEval: dispatch or evaluate a call.
pub(crate) unsafe fn DispatchOrEval(
    _call: SEXP,
    _op: SEXP,
    _generic: *const c_char,
    _args: SEXP,
    _rho: SEXP,
    _ans: *mut SEXP,
    _fallback: c_int,
    _supplied: c_int,
) -> c_int {
    0 // FALSE
}

/// R_getS4DataSlot: get S4 data slot.
///
/// For S4 objects, returns the `.Data` attribute which holds the
/// underlying data. For non-S4 objects, returns the input unchanged.
pub unsafe fn R_getS4DataSlot(x: SEXP, _type_: c_int) -> SEXP {
    unsafe {
        if isNull(x) || IS_S4_OBJECT(x) == 0 {
            return x;
        }
        let data_sym = Rf_install(b".Data\x00".as_ptr() as *const c_char);
        let slot = getAttrib(x, data_sym);
        if isNull(slot) { x } else { slot }
    }
}
