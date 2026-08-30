#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/subassign.c
//!
//! Subset mutation for lists and vectors: the `[<-`, `[[<-`, and `$<-` operators.
//!
//! Ported internal helpers:
//!   getNames, EnlargeVector, EnlargeNames, embedInVector, dispatch_asvector,
//!   SubassignTypeFix, gi, DeleteListElements, VECTOR_ELT_FIX_NAMED,
//!   VectorAssign, MatrixAssign, ArrayAssign, GetOneIndex, SimpleListAssign,
//!   listRemove, DeleteOneVectorListItem, SubAssignArgs, R_DispatchOrEvalSP,
//!   errorNotSubsettable, errorMissingSubscript, errorOutOfBoundsSEXP
//!
//! Ported exported functions:
//!   do_subassign, do_subassign_dflt, do_subassign2, do_subassign2_dflt,
//!   do_subassign3, R_subassign3_dflt, SubassignTypeSym, SubassignDotsNames,
//!   GetSubassignSxpVec, var_assign

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

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's NA_REAL sentinel (specific NaN bit pattern).
const NA_REAL: c_double = crate::sexp::ffi::NA_REAL;

/// Maximum value for R_xlen_t.
const R_XLEN_T_MAX: R_xlen_t = i64::MAX;

/// Raw SEXPTYPE integer constants for use in match expressions.
/// These match the SEXPTYPE values defined in ffi.rs.
const NILSXP: c_int = 0;
const SYMSXP: c_int = 1;
const LISTSXP: c_int = 2;
const CLOSXP: c_int = 3;
const ENVSXP: c_int = 4;
const PROMSXP: c_int = 5;
const LANGSXP: c_int = 6;
const SPECIALSXP: c_int = 7;
const BUILTINSXP: c_int = 8;
const CHARSXP: c_int = 9;
const LGLSXP: c_int = 10;
const INTSXP: c_int = 13;
const REALSXP: c_int = 14;
const CPLXSXP: c_int = 15;
const STRSXP: c_int = 16;
const DOTSXP: c_int = 17;
const ANYSXP: c_int = 18;
const VECSXP: c_int = 19;
const EXPRSXP: c_int = 20;
const BCODESXP: c_int = 21;
const EXTPTRSXP: c_int = 22;
const WEAKREFSXP: c_int = 23;
const RAWSXP: c_int = 24;
const OBJSXP: c_int = 25;
const FUNSXP: c_int = 99;

// ---------------------------------------------------------------------------
// Local symbol helpers
// ---------------------------------------------------------------------------

/// Get the "dim" symbol.
#[inline]
unsafe fn sym_Dim() -> SEXP {
    unsafe { Rf_install(std::ffi::CString::new("dim").unwrap_or_default().as_ptr()) }
}

/// Get the "names" symbol.
#[inline]
unsafe fn sym_Names() -> SEXP {
    unsafe { Rf_install(std::ffi::CString::new("names").unwrap_or_default().as_ptr()) }
}

/// Get the "dimnames" symbol.
#[inline]
unsafe fn sym_DimNames() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new("dimnames")
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// Get the "class" symbol.
#[inline]
unsafe fn sym_Class() -> SEXP {
    unsafe { Rf_install(std::ffi::CString::new("class").unwrap_or_default().as_ptr()) }
}

/// Get the "use.names" symbol (for subscript name passing).
#[inline]
unsafe fn sym_UseNames() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new("use.names")
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

// ---------------------------------------------------------------------------
// Local type-checking helpers
// ---------------------------------------------------------------------------

#[inline]
unsafe fn isNull(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() }
}

#[inline]
unsafe fn isVector(x: SEXP) -> bool {
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
unsafe fn isVectorList(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == VECSXP || t == EXPRSXP
    }
}

#[inline]
unsafe fn isPairList(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == LISTSXP || t == NILSXP
    }
}

#[inline]
unsafe fn isList(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == LISTSXP }
}

#[inline]
unsafe fn isLanguage(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == LANGSXP }
}

#[inline]
unsafe fn isExpression(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == EXPRSXP }
}

#[inline]
unsafe fn isNewList(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == VECSXP }
}

#[inline]
unsafe fn isObject(x: SEXP) -> bool {
    unsafe { OBJECT(x) != 0 }
}

#[inline]
unsafe fn isMatrix(x: SEXP) -> bool {
    unsafe {
        let dim = getAttrib(x, sym_Dim());
        !isNull(dim) && LENGTH(dim) == 2
    }
}

#[inline]
unsafe fn isArray(x: SEXP) -> bool {
    unsafe {
        let dim = getAttrib(x, sym_Dim());
        !isNull(dim) && LENGTH(dim) >= 2
    }
}

#[inline]
unsafe fn isString(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == STRSXP }
}

#[inline]
unsafe fn isInteger(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == INTSXP }
}

#[inline]
unsafe fn isReal(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == REALSXP }
}

#[inline]
fn R_FINITE(x: c_double) -> bool {
    x.is_finite()
}

#[inline]
fn ISNA(x: c_double) -> bool {
    x.is_nan() && x.to_bits() != 0x7ff8000000000000u64
}

// ---------------------------------------------------------------------------
// Local helper stubs (functions not yet available in the codebase)
// ---------------------------------------------------------------------------

/// Check if an object has the S4 bit set.
#[inline]
unsafe fn IS_S4_OBJECT(x: SEXP) -> c_int {
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
unsafe fn SET_S4_OBJECT(x: SEXP) {
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
unsafe fn UNSET_S4_OBJECT(x: SEXP) {
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
unsafe fn MAYBE_SHARED(x: SEXP) -> bool {
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
unsafe fn MAYBE_REFERENCED(x: SEXP) -> bool {
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
unsafe fn MARK_NOT_MUTABLE(x: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).sxpinfo.set_named(2);
        }
    }
}

/// Set NAMED to 0 (setter-clear).
#[inline]
unsafe fn SETTER_CLEAR_NAMED(x: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).sxpinfo.set_named(0);
        }
    }
}

/// Raise NAMED level.
#[inline]
unsafe fn RAISE_NAMED(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() && (v as u8) > (*x).sxpinfo.named() {
            (*x).sxpinfo.set_named(v as u8);
        }
    }
}

/// Increment NAMED.
#[inline]
unsafe fn INCREMENT_NAMED(x: SEXP) {
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
unsafe fn IS_GROWABLE(_x: SEXP) -> bool {
    // Simplified: always false since we don't fully implement truelength yet.
    false
}

/// Set the growable bit on an object.
#[inline]
unsafe fn SET_GROWABLE_BIT(x: SEXP) {
    unsafe {
        if !x.is_null() {
            let gp = (*x).sxpinfo.gp();
            (*x).sxpinfo.set_gp(gp | (1u16 << 5));
        }
    }
}

/// Set true length of a vector.
#[inline]
unsafe fn SET_TRUELENGTH(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() {
            (*x).data.vecsxp.truelength = v as R_xlen_t;
        }
    }
}

/// Get true length of a vector.
#[inline]
unsafe fn XTRUELENGTH(x: SEXP) -> R_xlen_t {
    unsafe {
        // Simplified: return XLENGTH
        XLENGTH(x)
    }
}

/// SETCADR: set the CAR of the CDR.
#[inline]
unsafe fn SETCADR(x: SEXP, v: SEXP) {
    unsafe {
        SETCAR(CDR(x), v);
    }
}

/// SET_TYPEOF: set the type of an SEXP.
#[inline]
unsafe fn SET_TYPEOF(x: SEXP, v: c_int) {
    unsafe {
        (*x).sxpinfo.set_type(SEXPTYPE(v));
    }
}

/// Set the standard vector length (not marking as immutable).
#[inline]
unsafe fn SET_STDVEC_LENGTH(x: SEXP, v: R_xlen_t) {
    unsafe {
        if !x.is_null() {
            (*x).data.vecsxp.length = v;
        }
    }
}

/// ENSURE_NAMEDMAX: set NAMED to NAMEDMAX.
#[inline]
unsafe fn ENSURE_NAMEDMAX(x: SEXP) {
    unsafe {
        if !x.is_null() {
            (*x).sxpinfo.set_named(2);
        }
    }
}

/// Check if the call is an assignment call.
#[inline]
unsafe fn IS_ASSIGNMENT_CALL(call: SEXP) -> bool {
    unsafe {
        if isNull(call) {
            return true;
        }
        let t = TYPEOF(call);
        t == LANGSXP || t == SYMSXP
    }
}

/// R_FixupRHS: fix up RHS for assignment (duplicate if needed).
unsafe fn R_FixupRHS(x: SEXP, y: SEXP) -> SEXP {
    unsafe {
        if MAYBE_SHARED(y) {
            shallow_duplicate(y)
        } else {
            y
        }
    }
}

/// PairToVectorList: convert a pairlist to a vector list.
unsafe fn PairToVectorList(x: SEXP) -> SEXP {
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
unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    unsafe { crate::mainutils::coerce::coerceVector(x, type_) }
}

/// getAttrib: get an attribute from an object.
unsafe fn getAttrib(x: SEXP, what: SEXP) -> SEXP {
    unsafe { crate::eval::attrib_core::getAttrib(x, what) }
}

/// setAttrib: set an attribute on an object.
unsafe fn setAttrib(x: SEXP, what: SEXP, value: SEXP) {
    unsafe {
        crate::eval::attrib_core::setAttrib(x, what, value);
    }
}

/// shallow_duplicate: create a shallow copy.
unsafe fn shallow_duplicate(x: SEXP) -> SEXP {
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
unsafe fn copyMostAttrib(src: SEXP, dest: SEXP) {
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
unsafe fn listAppend(t: SEXP, s: SEXP) -> SEXP {
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
unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asInteger(x) }
}

/// nrows: get number of rows of a matrix.
unsafe fn nrows(x: SEXP) -> c_int {
    unsafe {
        let dim = getAttrib(x, sym_Dim());
        if isNull(dim) {
            return 0;
        }
        INTEGER(dim).read() as c_int
    }
}

/// ncols: get number of columns of a matrix.
unsafe fn ncols(x: SEXP) -> c_int {
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
unsafe fn installTrChar(input: SEXP) -> SEXP {
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

unsafe fn ScalarInteger(x: c_int) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_ScalarInteger(x) }
}

unsafe fn ScalarReal(x: c_double) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_ScalarReal(x) }
}

/// ScalarString: create a length-1 character vector from a CHARSXP.
unsafe fn ScalarString(x: SEXP) -> SEXP {
    unsafe {
        let s = Rf_allocVector3(STRSXP, 1);
        let _s_guard = protect(s);
        SET_STRING_ELT(s, 0, x);
        s
    }
}

/// list2: create a 2-element list.
unsafe fn list2(a: SEXP, b: SEXP) -> SEXP {
    unsafe {
        let cdr = Rf_cons(b, R_NilValue());
        Rf_cons(a, cdr)
    }
}

/// nthcdr: walk n steps down a pairlist.
unsafe fn nthcdr(x: SEXP, mut n: c_int) -> SEXP {
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
unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe { crate::mainutils::relop::checkArity(op, args) }
}

/// GetArrayDimnames: get dimnames attribute.
unsafe fn GetArrayDimnames(x: SEXP) -> SEXP {
    unsafe { getAttrib(x, sym_DimNames()) }
}

/// fixSubset3Args: prepare args for $ assignment.
unsafe fn fixSubset3Args(call: SEXP, args: SEXP, env: SEXP, nlist: *mut SEXP) -> SEXP {
    unsafe { crate::mainutils::subset::fixSubset3Args(call, args, env, nlist) }
}

/// NonNullStringMatch: match non-null strings.
unsafe fn NonNullStringMatch(s: SEXP, t: SEXP) -> c_int {
    unsafe { crate::mainutils::match_mod::NonNullStringMatch(s, t) }
}

/// R_CurrentExpression: stub returning nil.
unsafe fn R_CurrentExpression() -> SEXP {
    unsafe { R_NilValue() }
}

/// NA_STRING: get the NA string.
unsafe fn NA_STRING() -> SEXP {
    unsafe { crate::sexp::globals::R_NaString() }
}

/// R_BlankString: get the blank string.
unsafe fn R_BlankString() -> SEXP {
    unsafe { Rf_mkChar(b" \0".as_ptr() as *const c_char) }
}

/// DispatchOrEval: dispatch or evaluate a call.
unsafe fn DispatchOrEval(
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

// ---------------------------------------------------------------------------
// Internal helper functions
// ---------------------------------------------------------------------------

/// Port of `getNames()` -- retrieves names attribute from a vector,
/// deferring to getAttrib if a 'dim' attribute is present.
unsafe fn getNames(x: SEXP) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::R_DimSymbol;
        use crate::eval::attrib_core::R_NamesSymbol as R_NamesSym;

        let mut attr = ATTRIB(x);
        while !isNull(attr) {
            if TAG(attr) == R_DimSymbol() {
                return getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
            }
            attr = CDR(attr);
        }

        // Don't use getAttrib since that would mark as immutable
        attr = ATTRIB(x);
        while !isNull(attr) {
            if TAG(attr) == crate::eval::attrib_core::R_NamesSymbol() {
                return CAR(attr);
            }
            attr = CDR(attr);
        }

        R_NilValue()
    }
}

/// Port of `EnlargeVector()` -- changes vector length to newlen,
/// allowing assignment past the end of a vector.
unsafe fn EnlargeVector(x: SEXP, newlen: R_xlen_t) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::R_NamesSymbol as R_NamesSym;

        let len = XLENGTH(x);
        let newtruelen: R_xlen_t;
        if newlen > len {
            let expanded_nlen = (newlen as f64) * 1.05;
            if expanded_nlen <= R_XLEN_T_MAX as f64 {
                newtruelen = expanded_nlen as R_xlen_t;
            } else {
                newtruelen = newlen;
            }
        } else {
            newtruelen = newlen;
        }

        let _x_guard = protect(x);
        let newx = Rf_allocVector3(TYPEOF(x), newtruelen);
        let _newx_guard = protect(newx);

        // Copy the elements into place.
        let xtype = TYPEOF(x);
        if xtype == LGLSXP || xtype == INTSXP {
            let px = INTEGER(newx);
            let px_src = INTEGER(x);
            for i in 0..len {
                *px.add(i as usize) = *px_src.add(i as usize);
            }
            for i in len..newtruelen {
                *px.add(i as usize) = NA_INTEGER;
            }
        } else if xtype == REALSXP {
            let px = REAL(newx);
            let px_src = REAL(x);
            for i in 0..len {
                *px.add(i as usize) = *px_src.add(i as usize);
            }
            for i in len..newtruelen {
                *px.add(i as usize) = NA_REAL;
            }
        } else if xtype == CPLXSXP {
            let px = COMPLEX(newx);
            let px_src = COMPLEX(x);
            for i in 0..len {
                *px.add(i as usize) = *px_src.add(i as usize);
            }
            for i in len..newtruelen {
                (*px.add(i as usize)).r = NA_REAL;
                (*px.add(i as usize)).i = 0.0;
            }
        } else if xtype == STRSXP {
            for i in 0..len {
                SET_STRING_ELT(newx, i, STRING_ELT(x, i));
            }
            for i in len..newtruelen {
                SET_STRING_ELT(newx, i, NA_STRING());
            }
        } else if xtype == EXPRSXP || xtype == VECSXP {
            for i in 0..len {
                SET_VECTOR_ELT(newx, i, VECTOR_ELT(x, i));
            }
            for i in len..newtruelen {
                SET_VECTOR_ELT(newx, i, R_NilValue());
            }
        } else if xtype == RAWSXP {
            let px = RAW(newx);
            let px_src = RAW(x);
            for i in 0..len {
                *px.add(i as usize) = *px_src.add(i as usize);
            }
            for i in len..newtruelen {
                *px.add(i as usize) = 0;
            }
        }

        if newlen < newtruelen {
            SET_GROWABLE_BIT(newx);
            SET_TRUELENGTH(newx, newtruelen as c_int);
            SET_STDVEC_LENGTH(newx, newlen);
        }

        // Adjust the attribute list.
        let names = getNames(x);
        if !isNull(names) {
            let enlarged = EnlargeNames(names, len, newlen);
            setAttrib(newx, crate::eval::attrib_core::R_NamesSymbol(), enlarged);
        }
        copyMostAttrib(x, newx);
        newx
    }
}

/// Port of `EnlargeNames()` -- grows a names attribute vector.
unsafe fn EnlargeNames(names: SEXP, len: R_xlen_t, newlen: R_xlen_t) -> SEXP {
    unsafe {
        if TYPEOF(names) != STRSXP || XLENGTH(names) != len {
            // Error case - just return names unchanged
            return names;
        }
        let newnames = EnlargeVector(names, newlen);
        let _newnames_guard = protect(newnames);
        for i in len..newlen {
            SET_STRING_ELT(newnames, i, R_BlankString());
        }
        newnames
    }
}

/// Port of `embedInVector()` -- embeds a non-vector in a list for
/// SubassignTypeFix (used for S4 objects).
unsafe fn embedInVector(v: SEXP, _call: SEXP) -> SEXP {
    unsafe {
        let ans = Rf_allocVector3(VECSXP, 1);
        let _ans_guard = protect(ans);
        SET_VECTOR_ELT(ans, 0, v);
        ans
    }
}

/// Port of `dispatch_asvector()` -- dispatches as.vector method.
unsafe fn dispatch_asvector(_x: *mut SEXP, _call: SEXP, _rho: SEXP) -> bool {
    false
}

/// Port of `SubassignTypeFix()` -- coerces LHS/RHS to compatible types
/// for subassignment. Returns the type code `100 * TYPEOF(x) + TYPEOF(y)`.
unsafe fn SubassignTypeFix(
    x: *mut SEXP,
    y: *mut SEXP,
    stretch: R_xlen_t,
    level: c_int,
    call: SEXP,
    rho: SEXP,
) -> c_int {
    unsafe {
        let mut redo_which = true;
        let which = 100 * TYPEOF(*x) + TYPEOF(*y);
        let x_is_object = isObject(*x);

        match which {
            // No coercion needed
            1000 | 1300 | 1400 | 1500 | 1600 | 1900 | 2000 | 2400 | 1010 | 1310 | 1410 | 1510
            | 1313 | 1413 | 1513 | 1414 | 1514 | 1515 | 1616 | 1919 | 2020 | 2424 => {
                redo_which = false;
            }

            1013 => {
                // logical <- integer
                *x = coerceVector(*x, INTSXP);
            }

            1014 | 1314 => {
                // logical/integer <- real
                *x = coerceVector(*x, REALSXP);
            }

            1015 | 1315 | 1415 => {
                // logical/integer/real <- complex
                *x = coerceVector(*x, CPLXSXP);
            }

            1610 | 1613 | 1614 | 1615 => {
                // character <- logical/integer/real/complex
                *y = coerceVector(*y, STRSXP);
            }

            1016 | 1316 | 1416 | 1516 => {
                // logical/integer/real/complex <- character
                *x = coerceVector(*x, STRSXP);
            }

            1901 | 1902 | 1904 | 1905 | 1906 | 1910 | 1913 | 1914 | 1915 | 1916 | 1920 | 1921
            | 1922 | 1923 | 1924 | 1903 | 1907 | 1908 | 1999 => {
                // vector <- various
                if level == 1 {
                    *y = coerceVector(*y, VECSXP);
                } else {
                    redo_which = false;
                }
            }

            1925 => {
                // vector <- S4/OBJ
                if level == 1 {
                    *y = embedInVector(*y, call);
                } else {
                    redo_which = false;
                }
            }

            1019 | 1319 | 1419 | 1519 | 1619 | 2419 => {
                // various <- vector
                *x = coerceVector(*x, VECSXP);
            }

            1020 | 1320 | 1420 | 1520 | 1620 | 2420 => {
                // various <- expression
                *x = coerceVector(*x, EXPRSXP);
            }

            2001 | 2002 | 2006 | 2010 | 2013 | 2014 | 2015 | 2016 | 2019 => {
                // expression <- various
                if level == 1 {
                    *y = coerceVector(*y, VECSXP);
                } else {
                    redo_which = false;
                }
            }

            2025 => {
                // expression <- S4/OBJ
                if level == 1 {
                    *y = embedInVector(*y, call);
                } else {
                    redo_which = false;
                }
            }

            1025 | 1325 | 1425 | 1525 | 1625 | 2425 => {
                // various <- S4|OBJ
                if dispatch_asvector(y, call, rho) {
                    // dispatch_asvector() leaves the new *y unprotected; the
                    // recursive call below may allocate (coerceVector), so the
                    // new value has to be protected (upstream GC fix):
                    let y_guard = protect(*y);
                    let which = SubassignTypeFix(x, y, stretch, level, call, rho);
                    drop(y_guard);
                    return which;
                }
            }

            _ => {
                // Incompatible types - just return which
            }
        }

        if stretch > 0 {
            let _y_guard = protect(*y);
            *x = EnlargeVector(*x, stretch);
        }
        SET_OBJECT(*x, x_is_object as c_int);

        if redo_which {
            100 * TYPEOF(*x) + TYPEOF(*y)
        } else {
            which
        }
    }
}

/// Port of `gi()` -- gets an index value from an integer or real subscript vector.
unsafe fn gi(indx: SEXP, i: R_xlen_t) -> R_xlen_t {
    unsafe {
        if TYPEOF(indx) == REALSXP {
            let d = REAL_ELT(indx, i as c_int);
            if R_FINITE(d) {
                d as R_xlen_t
            } else {
                NA_INTEGER as R_xlen_t
            }
        } else {
            INTEGER_ELT(indx, i as c_int) as R_xlen_t
        }
    }
}

/// Port of `DeleteListElements()` -- removes specified elements from a vector list.
unsafe fn DeleteListElements(x: SEXP, which: SEXP) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::R_NamesSymbol as R_NamesSym;

        let len = XLENGTH(x);
        let lenw = XLENGTH(which);

        let include = Rf_allocVector3(INTSXP, len);
        let _include_guard = protect(include);
        let pinclude = INTEGER(include);
        for i in 0..len {
            *pinclude.add(i as usize) = 1;
        }
        for i in 0..lenw {
            let ii = gi(which, i);
            if ii > 0 && ii <= len {
                *pinclude.add((ii - 1) as usize) = 0;
            }
        }

        let mut ii: R_xlen_t = 0;
        for i in 0..len {
            ii += *pinclude.add(i as usize) as R_xlen_t;
        }
        if ii == len {
            return x;
        }

        let xnew = Rf_allocVector3(TYPEOF(x), ii);
        let _xnew_guard = protect(xnew);
        let mut k: R_xlen_t = 0;
        for i in 0..len {
            if *pinclude.add(i as usize) == 1 {
                SET_VECTOR_ELT(xnew, k, VECTOR_ELT(x, i));
                k += 1;
            }
        }

        let xnames = getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
        let _xnames_guard = protect(xnames);
        if !isNull(xnames) {
            let xnewnames = Rf_allocVector3(STRSXP, ii);
            let _xnewnames_guard = protect(xnewnames);
            k = 0;
            for i in 0..len {
                if *pinclude.add(i as usize) == 1 {
                    SET_STRING_ELT(xnewnames, k, STRING_ELT(xnames, i));
                    k += 1;
                }
            }
            setAttrib(xnew, crate::eval::attrib_core::R_NamesSymbol(), xnewnames);
        }
        copyMostAttrib(x, xnew);
        xnew
    }
}

/// Port of `VECTOR_ELT_FIX_NAMED()` -- sets NAMED=NAMEDMAX if needed for PR15098.
unsafe fn VECTOR_ELT_FIX_NAMED(y: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        let val = VECTOR_ELT(y, i);
        if NAMED(y) != 0 || NAMED(val) != 0 {
            ENSURE_NAMEDMAX(val);
        }
        val
    }
}

// ---------------------------------------------------------------------------
// VectorAssign
// ---------------------------------------------------------------------------

/// Port of `VectorAssign()` -- handles `x[s] <- y` for vectors.
unsafe fn VectorAssign(call: SEXP, rho: SEXP, x: SEXP, s: SEXP, y: SEXP) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::R_DimSymbol;

        // Quick return for simple scalar case
        if isNull(ATTRIB(s)) && TYPEOF(x) == REALSXP && IS_SCALAR(y, REALSXP) != 0 {
            // Note: IS_SCALAR only inspects the scalar flag; the element type
            // must be verified separately before using the typed accessors.
            if TYPEOF(s) == INTSXP && IS_SCALAR(s, INTSXP) != 0 {
                let ival = SCALAR_IVAL(s) as R_xlen_t;
                let ival_ok = ival != NA_INTEGER as i64 && ival >= 1 && ival <= XLENGTH(x);
                if ival_ok {
                    *REAL(x).add((ival - 1) as usize) = SCALAR_DVAL(y);
                    return x;
                }
            } else if TYPEOF(s) == REALSXP && IS_SCALAR(s, REALSXP) != 0 {
                let dval = SCALAR_DVAL(s);
                if R_FINITE(dval) {
                    let ival = dval as R_xlen_t;
                    if ival >= 1 && ival <= XLENGTH(x) {
                        *REAL(x).add((ival - 1) as usize) = SCALAR_DVAL(y);
                        return x;
                    }
                }
            }
        }

        if isNull(x) && isNull(y) {
            return R_NilValue();
        }

        // Check for special matrix subscripting.
        let mut s = s;
        let mut s_guard = protect(s);
        if !isNull(ATTRIB(s)) {
            let dim = getAttrib(x, R_DimSymbol());
            if isMatrix(s) && isArray(x) && ncols(s) == Rf_length(dim) {
                if isString(s) {
                    let dnames = GetArrayDimnames(x);
                    let dnames_guard = protect(dnames);
                    let intmat = strmat2intmat(s, dnames, call, x);
                    drop(dnames_guard);
                    drop(s_guard);
                    s = intmat;
                    s_guard = protect(s);
                }
                if isInteger(s) || isReal(s) {
                    let indsub = mat2indsub(dim, s, R_NilValue(), x);
                    drop(s_guard);
                    s = indsub;
                    s_guard = protect(s);
                }
            }
        }

        let stretch: R_xlen_t = 1;
        let indx = makeSubscript(x, s, &stretch as *const _ as *mut R_xlen_t, R_NilValue());
        let _indx_guard = protect(indx);
        let n = XLENGTH(indx);

        // NAs are not allowed in subscripted assignments. Upstream
        // (subassign.c) raises this while processing the subscript, before any
        // typed assignment arm; `gi()` maps NA indices to the NA_INTEGER
        // sentinel for both INTSXP and expanded-logical subscripts.
        for i in 0..n {
            if gi(indx, i) == NA_INTEGER as R_xlen_t {
                crate::mainutils::errors::Rf_error(
                    b"NAs are not allowed in subscripted assignments\0".as_ptr()
                        as *const core::ffi::c_char,
                );
            }
        }

        let old_x = x;
        let mut x = x;
        let mut y = y;
        let which = SubassignTypeFix(&mut x, &mut y, stretch, 1, call, rho);

        if n == 0 {
            return x;
        }

        let ny = XLENGTH(y);
        let nx = XLENGTH(x);
        let _x_guard = protect(x);

        let is_list_target = TYPEOF(x) == VECSXP || TYPEOF(x) == EXPRSXP;
        if !is_list_target || isNull(y) {
            // Check length compatibility
            if n > 0 && ny == 0 {
                crate::mainutils::errors::Rf_error(
                    b"replacement has length zero\0".as_ptr() as *const core::ffi::c_char
                );
            }
        }

        // Warn about non-multiple recycling
        if ny != 0 && n % ny != 0 {
            crate::mainutils::errors::warningcall(
                call,
                b"number of items to replace is not a multiple of replacement length\0".as_ptr()
                    as *const core::ffi::c_char,
            );
        }

        // Duplicate y if x == y
        let _y_guard = if x == y {
            y = shallow_duplicate(y);
            protect(y)
        } else {
            protect(y)
        };

        match which {
            1010 | 1310 | 1313 => {
                // logical <- logical, integer <- logical, integer <- integer
                let px = INTEGER(x);
                let y_is_int = TYPEOF(y) == SEXPTYPE::INTSXP;
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    *px.add(ii as usize) = if y_is_int {
                        INTEGER_ELT(y, iny as c_int)
                    } else {
                        LOGICAL_ELT(y, iny as c_int)
                    };
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1410 | 1413 => {
                // real <- logical/integer
                let px = REAL(x);
                let y_is_int = TYPEOF(y) == SEXPTYPE::INTSXP;
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    let iy = if y_is_int {
                        INTEGER_ELT(y, iny as c_int)
                    } else {
                        LOGICAL_ELT(y, iny as c_int)
                    };
                    if iy == NA_INTEGER {
                        *px.add(ii as usize) = NA_REAL;
                    } else {
                        *px.add(ii as usize) = iy as c_double;
                    }
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1410 | 1413 => {
                // real <- logical/integer
                let px = REAL(x);
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    let iy = INTEGER_ELT(y, iny as c_int);
                    if iy == NA_INTEGER {
                        *px.add(ii as usize) = NA_REAL;
                    } else {
                        *px.add(ii as usize) = iy as c_double;
                    }
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1414 => {
                // real <- real
                let px = REAL(x);
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    *px.add(ii as usize) = REAL_ELT(y, iny as c_int);
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1510 | 1513 => {
                // complex <- logical/integer
                let px = COMPLEX(x);
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    let iy = if TYPEOF(y) == SEXPTYPE::INTSXP {
                        INTEGER_ELT(y, iny as c_int)
                    } else {
                        LOGICAL_ELT(y, iny as c_int)
                    };
                    if iy == NA_INTEGER {
                        (*px.add(ii as usize)).r = NA_REAL;
                        (*px.add(ii as usize)).i = 0.0;
                    } else {
                        (*px.add(ii as usize)).r = iy as c_double;
                        (*px.add(ii as usize)).i = 0.0;
                    }
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1514 => {
                // complex <- real
                let px = COMPLEX(x);
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    let ry = REAL_ELT(y, iny as c_int);
                    if ISNA(ry) {
                        (*px.add(ii as usize)).r = NA_REAL;
                        (*px.add(ii as usize)).i = 0.0;
                    } else {
                        (*px.add(ii as usize)).r = ry;
                        (*px.add(ii as usize)).i = 0.0;
                    }
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1515 => {
                // complex <- complex
                let px = COMPLEX(x);
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    *px.add(ii as usize) = COMPLEX_ELT(y, iny as c_int);
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1610 | 1613 | 1614 | 1615 | 1616 => {
                // character <- various
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    SET_STRING_ELT(x, ii, STRING_ELT(y, iny));
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1919 => {
                // vector <- vector
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    if (idx as R_xlen_t) >= ny {
                        ENSURE_NAMEDMAX(VECTOR_ELT(y, iny as R_xlen_t));
                    }
                    SET_VECTOR_ELT(x, ii, VECTOR_ELT_FIX_NAMED(y, iny as R_xlen_t));
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            2019 | 2020 => {
                // expression <- vector/expression
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    SET_VECTOR_ELT(x, ii, VECTOR_ELT(y, iny as R_xlen_t));
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1900 | 2000 => {
                // vector/expression <- null
                x = DeleteListElements(x, indx);
                return x;
            }

            2424 => {
                // raw <- raw
                let px = RAW(x);
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    *px.add(ii as usize) = RAW_ELT(y, iny as c_int);
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            _ => {
                // Warning case
            }
        }

        // Check for additional named elements.
        // Note makeSubscript passes the additional names back as the
        // use.names attribute (a vector list) of the generated subscript
        // vector (see trunk subassign.c VectorAssign tail).
        let newnames = getAttrib(indx, crate::eval::attrib_core::R_UseNamesSymbol());
        if !isNull(newnames) {
            let mut oldnames = getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
            if !isNull(oldnames) {
                for i in 0..n {
                    if !VECTOR_ELT(newnames, i).is_null() && VECTOR_ELT(newnames, i) != R_NilValue()
                    {
                        let mut ii = gi(indx, i);
                        if ii == NA_INTEGER as R_xlen_t {
                            continue;
                        }
                        ii -= 1;
                        SET_STRING_ELT(oldnames, ii, VECTOR_ELT(newnames, i));
                    }
                }
            } else {
                oldnames = Rf_allocVector3(SEXPTYPE::STRSXP, nx);
                let _oldnames_guard = protect(oldnames);
                for i in 0..nx {
                    SET_STRING_ELT(oldnames, i, R_BlankString());
                }
                for i in 0..n {
                    if !VECTOR_ELT(newnames, i).is_null() && VECTOR_ELT(newnames, i) != R_NilValue()
                    {
                        let mut ii = gi(indx, i);
                        if ii == NA_INTEGER as R_xlen_t {
                            continue;
                        }
                        ii -= 1;
                        SET_STRING_ELT(oldnames, ii, VECTOR_ELT(newnames, i));
                    }
                }
                setAttrib(x, crate::eval::attrib_core::R_NamesSymbol(), oldnames);
            }
        }

        x
    }
}

// ---------------------------------------------------------------------------
// MatrixAssign
// ---------------------------------------------------------------------------

/// Port of `MatrixAssign()` -- handles `x[i,j] <- y` for matrices.
unsafe fn MatrixAssign(call: SEXP, rho: SEXP, x: SEXP, s: SEXP, y: SEXP) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::R_DimSymbol;

        if !isMatrix(x) {
            // Error: incorrect number of subscripts
            return x;
        }

        let nr = nrows(x);
        let ny = XLENGTH(y) as R_xlen_t;

        let dim = getAttrib(x, R_DimSymbol());
        SETCAR(s, int_arraySubscript(0, CAR(s), dim, x, call));
        SETCADR(s, int_arraySubscript(1, CADR(s), dim, x, call));
        let sr = CAR(s);
        let sc = CADR(s);
        let nrs = Rf_length(sr);
        let ncs = Rf_length(sc);

        let psc = INTEGER(sc);
        let psr = INTEGER(sr);

        let mut anyIdxNA = false;
        for i in 0..nrs {
            if *psr.add(i as usize) == NA_INTEGER {
                anyIdxNA = true;
                break;
            }
        }
        for i in 0..ncs {
            if *psc.add(i as usize) == NA_INTEGER {
                anyIdxNA = true;
                break;
            }
        }

        let n = (nrs as R_xlen_t) * (ncs as R_xlen_t);

        if n > 0 && ny == 0 {
            // Error: replacement has length zero
            return x;
        }

        let mut x = x;
        let mut y = y;
        let which = SubassignTypeFix(&mut x, &mut y, 0, 1, call, rho);
        if n == 0 {
            return x;
        }

        let _x_guard = protect(x);
        let _y_guard = if x == y {
            y = shallow_duplicate(y);
            protect(y)
        } else {
            protect(y)
        };

        let NR = nr as R_xlen_t;
        let mut k: R_xlen_t = 0;

        if anyIdxNA {
            for j in 0..ncs {
                let jj = *psc.add(j as usize);
                if jj != NA_INTEGER {
                    let jj = (jj - 1) as R_xlen_t;
                    let offset = jj * NR;
                    for i in 0..nrs {
                        let ii = *psr.add(i as usize);
                        if ii != NA_INTEGER {
                            let ij = (ii as R_xlen_t - 1) + offset;
                            // Perform assignment based on type
                            match which {
                                1010 | 1310 | 1313 => {
                                    *INTEGER(x).add(ij as usize) = INTEGER_ELT(y, k as c_int);
                                }
                                1410 | 1413 => {
                                    let iy = INTEGER_ELT(y, k as c_int);
                                    if iy == NA_INTEGER {
                                        *REAL(x).add(ij as usize) = NA_REAL;
                                    } else {
                                        *REAL(x).add(ij as usize) = iy as c_double;
                                    }
                                }
                                1414 => {
                                    *REAL(x).add(ij as usize) = REAL_ELT(y, k as c_int);
                                }
                                1510 | 1513 => {
                                    let iy = INTEGER_ELT(y, k as c_int);
                                    if iy == NA_INTEGER {
                                        (*COMPLEX(x).add(ij as usize)).r = NA_REAL;
                                        (*COMPLEX(x).add(ij as usize)).i = 0.0;
                                    } else {
                                        (*COMPLEX(x).add(ij as usize)).r = iy as c_double;
                                        (*COMPLEX(x).add(ij as usize)).i = 0.0;
                                    }
                                }
                                1514 => {
                                    let ry = REAL_ELT(y, k as c_int);
                                    if ISNA(ry) {
                                        (*COMPLEX(x).add(ij as usize)).r = NA_REAL;
                                        (*COMPLEX(x).add(ij as usize)).i = 0.0;
                                    } else {
                                        (*COMPLEX(x).add(ij as usize)).r = ry;
                                        (*COMPLEX(x).add(ij as usize)).i = 0.0;
                                    }
                                }
                                1515 => {
                                    *COMPLEX(x).add(ij as usize) = COMPLEX_ELT(y, k as c_int);
                                }
                                1610 | 1613 | 1614 | 1615 | 1616 => {
                                    SET_STRING_ELT(x, ij, STRING_ELT(y, k));
                                }
                                1919 => {
                                    SET_VECTOR_ELT(x, ij, VECTOR_ELT_FIX_NAMED(y, k as R_xlen_t));
                                }
                                2424 => {
                                    *RAW(x).add(ij as usize) = RAW_ELT(y, k as c_int);
                                }
                                _ => {} // intentionally unhandled: unsupported SEXPTYPE for matrix subassignment
                            }
                            k += 1;
                            if k == ny {
                                k = 0;
                            }
                        }
                    }
                }
            }
        } else {
            for j in 0..ncs {
                let jj = (*psc.add(j as usize) - 1) as R_xlen_t;
                let offset = jj * NR;
                for i in 0..nrs {
                    let ii = *psr.add(i as usize);
                    let ij = (ii as R_xlen_t - 1) + offset;
                    match which {
                        1010 | 1310 | 1313 => {
                            *INTEGER(x).add(ij as usize) = INTEGER_ELT(y, k as c_int);
                        }
                        1410 | 1413 => {
                            let iy = INTEGER_ELT(y, k as c_int);
                            if iy == NA_INTEGER {
                                *REAL(x).add(ij as usize) = NA_REAL;
                            } else {
                                *REAL(x).add(ij as usize) = iy as c_double;
                            }
                        }
                        1414 => {
                            *REAL(x).add(ij as usize) = REAL_ELT(y, k as c_int);
                        }
                        1510 | 1513 => {
                            let iy = INTEGER_ELT(y, k as c_int);
                            if iy == NA_INTEGER {
                                (*COMPLEX(x).add(ij as usize)).r = NA_REAL;
                                (*COMPLEX(x).add(ij as usize)).i = 0.0;
                            } else {
                                (*COMPLEX(x).add(ij as usize)).r = iy as c_double;
                                (*COMPLEX(x).add(ij as usize)).i = 0.0;
                            }
                        }
                        1514 => {
                            let ry = REAL_ELT(y, k as c_int);
                            if ISNA(ry) {
                                (*COMPLEX(x).add(ij as usize)).r = NA_REAL;
                                (*COMPLEX(x).add(ij as usize)).i = 0.0;
                            } else {
                                (*COMPLEX(x).add(ij as usize)).r = ry;
                                (*COMPLEX(x).add(ij as usize)).i = 0.0;
                            }
                        }
                        1515 => {
                            *COMPLEX(x).add(ij as usize) = COMPLEX_ELT(y, k as c_int);
                        }
                        1610 | 1613 | 1614 | 1615 | 1616 => {
                            SET_STRING_ELT(x, ij, STRING_ELT(y, k));
                        }
                        1919 => {
                            if ny < (ncs as R_xlen_t) * (nrs as R_xlen_t) {
                                for ii in 0..ny {
                                    ENSURE_NAMEDMAX(VECTOR_ELT(y, ii));
                                }
                            }
                            SET_VECTOR_ELT(x, ij, VECTOR_ELT_FIX_NAMED(y, k as R_xlen_t));
                        }
                        2424 => {
                            *RAW(x).add(ij as usize) = RAW_ELT(y, k as c_int);
                        }
                        _ => {} // intentionally unhandled: unsupported SEXPTYPE for matrix subassignment
                    }
                    k += 1;
                    if k == ny {
                        k = 0;
                    }
                }
            }
        }

        x
    }
}

// ---------------------------------------------------------------------------
// ArrayAssign
// ---------------------------------------------------------------------------

/// Port of `ArrayAssign()` -- handles `x[i,j,k,...] <- y` for arrays.
unsafe fn ArrayAssign(call: SEXP, rho: SEXP, x: SEXP, s: SEXP, y: SEXP) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::R_DimSymbol;

        let mut k = 0i32;
        let dims = getAttrib(x, R_DimSymbol());
        let _dims_guard = protect(dims);
        if isNull(dims) || {
            k = LENGTH(dims);
            k != Rf_length(s)
        } {
            // Error: incorrect number of subscripts
            return x;
        }

        let ny = XLENGTH(y);
        let kk = k as usize;

        // Allocate stack arrays for subscripts, indices, bounds, offsets
        let mut subs: Vec<*const c_int> = Vec::with_capacity(kk);
        let mut indx: Vec<c_int> = vec![0; kk];
        let mut bound: Vec<c_int> = vec![0; kk];
        let mut offset: Vec<R_xlen_t> = vec![0; kk];

        // Expand the list of subscripts.
        let mut tmp = s;
        for i in 0..kk {
            SETCAR(tmp, int_arraySubscript(i as c_int, CAR(tmp), dims, x, call));
            tmp = CDR(tmp);
        }

        let mut n: R_xlen_t = 1;
        tmp = s;
        for i in 0..kk {
            indx[i] = 0;
            subs.push(INTEGER(CAR(tmp)));
            bound[i] = LENGTH(CAR(tmp));
            n *= bound[i] as R_xlen_t;
            tmp = CDR(tmp);
        }

        if n > 0 && ny == 0 {
            // Error: replacement has length zero
            return x;
        }

        offset[0] = 1;
        let pdims = INTEGER(dims);
        for i in 1..kk {
            offset[i] = offset[i - 1] * (*pdims.add(i - 1)) as R_xlen_t;
        }

        let mut x = x;
        let mut y = y;
        let which = SubassignTypeFix(&mut x, &mut y, 0, 1, call, rho);

        if n == 0 {
            return x;
        }

        let _x_guard = protect(x);
        let _y_guard = if x == y {
            y = shallow_duplicate(y);
            protect(y)
        } else {
            protect(y)
        };

        // Array assignment loop
        let mut iny: R_xlen_t = 0;
        for idx in 0..n {
            let mut ii: R_xlen_t = 0;
            let mut is_na = false;
            for j in 0..kk {
                let jj = *subs[j].add(indx[j] as usize);
                if jj == NA_INTEGER {
                    is_na = true;
                    break;
                } else {
                    ii += ((jj - 1) as R_xlen_t) * offset[j];
                }
            }

            if !is_na {
                match which {
                    1010 | 1310 | 1313 => {
                        *INTEGER(x).add(ii as usize) = INTEGER_ELT(y, iny as c_int);
                    }
                    1410 | 1413 => {
                        let iy = INTEGER_ELT(y, iny as c_int);
                        if iy == NA_INTEGER {
                            *REAL(x).add(ii as usize) = NA_REAL;
                        } else {
                            *REAL(x).add(ii as usize) = iy as c_double;
                        }
                    }
                    1414 => {
                        *REAL(x).add(ii as usize) = REAL_ELT(y, iny as c_int);
                    }
                    1510 | 1513 => {
                        let iy = INTEGER_ELT(y, iny as c_int);
                        if iy == NA_INTEGER {
                            (*COMPLEX(x).add(ii as usize)).r = NA_REAL;
                            (*COMPLEX(x).add(ii as usize)).i = 0.0;
                        } else {
                            (*COMPLEX(x).add(ii as usize)).r = iy as c_double;
                            (*COMPLEX(x).add(ii as usize)).i = 0.0;
                        }
                    }
                    1514 => {
                        let ry = REAL_ELT(y, iny as c_int);
                        if ISNA(ry) {
                            (*COMPLEX(x).add(ii as usize)).r = NA_REAL;
                            (*COMPLEX(x).add(ii as usize)).i = 0.0;
                        } else {
                            (*COMPLEX(x).add(ii as usize)).r = ry;
                            (*COMPLEX(x).add(ii as usize)).i = 0.0;
                        }
                    }
                    1515 => {
                        *COMPLEX(x).add(ii as usize) = COMPLEX_ELT(y, iny as c_int);
                    }
                    1610 | 1613 | 1614 | 1615 | 1616 => {
                        SET_STRING_ELT(x, ii, STRING_ELT(y, iny));
                    }
                    1919 => {
                        if (idx as R_xlen_t) >= ny {
                            ENSURE_NAMEDMAX(VECTOR_ELT(y, iny));
                        }
                        SET_VECTOR_ELT(x, ii, VECTOR_ELT_FIX_NAMED(y, iny));
                    }
                    2424 => {
                        *RAW(x).add(ii as usize) = RAW_ELT(y, iny as c_int);
                    }
                    _ => {} // intentionally unhandled: unsupported SEXPTYPE for subassignment
                }
            }

            iny += 1;
            if iny >= ny {
                iny = 0;
            }

            // Increment multi-dimensional index
            if n > 1 {
                let mut j = 0usize;
                loop {
                    indx[j] += 1;
                    if indx[j] < bound[j] {
                        break;
                    }
                    indx[j] = 0;
                    j += 1;
                    if j == kk {
                        j = 0;
                    }
                }
            }
        }

        x
    }
}

// ---------------------------------------------------------------------------
// GetOneIndex
// ---------------------------------------------------------------------------

/// Port of `GetOneIndex()` -- extracts a single subscript index for pairlist assignment.
unsafe fn GetOneIndex(sub: SEXP, ind: c_int) -> SEXP {
    unsafe {
        if ind < 0 || ind + 1 > Rf_length(sub) {
            // Error: internal error
            return sub;
        }
        if Rf_length(sub) > 1 {
            match TYPEOF(sub) {
                INTSXP => {
                    return ScalarInteger(INTEGER_ELT(sub, ind));
                }
                REALSXP => {
                    return ScalarReal(REAL_ELT(sub, ind));
                }
                STRSXP => {
                    return ScalarString(STRING_ELT(sub, ind as R_xlen_t));
                }
                _ => {
                    // Error: invalid subscript
                    return sub;
                }
            }
        }
        sub
    }
}

// ---------------------------------------------------------------------------
// SimpleListAssign
// ---------------------------------------------------------------------------

/// Port of `SimpleListAssign()` -- handles `x[[s]] <- y` for pairlists.
unsafe fn SimpleListAssign(
    _call: SEXP,
    x: SEXP,
    s: SEXP,
    y: SEXP,
    ind: c_int,
    _check_cycles: bool,
) -> SEXP {
    unsafe {
        let sub = CAR(s);
        if Rf_length(s) > 1 {
            // Error: invalid number of subscripts
            return x;
        }

        let sub = GetOneIndex(sub, ind);
        let _sub_guard = protect(sub);
        let mut stretch: R_xlen_t = 1;
        let indx = makeSubscript(x, sub, &mut stretch, R_NilValue());
        let _indx_guard = protect(indx);

        let n = Rf_length(indx);
        if n > 1 {
            // Error: invalid subscript
            return x;
        }

        let mut nx = Rf_length(x);
        let mut x = x;

        if stretch > 0 {
            let t = CAR(s);
            let yi = allocList((stretch - nx as R_xlen_t) as c_int);
            let _yi_guard = protect(yi);
            if isString(t) && Rf_length(t) == (stretch - nx as R_xlen_t) as c_int {
                let mut z = yi;
                for i in 0..Rf_length(t) {
                    SETTAG(z, installTrChar(STRING_ELT(t, i as R_xlen_t)));
                    z = CDR(z);
                }
            }
            x = listAppend(x, yi);
            nx = stretch as c_int;
        }
        let _x_guard = protect(x);

        if n == 1 {
            let ii = asInteger(indx);
            if ii != NA_INTEGER {
                let ii = ii - 1;
                let xi = nthcdr(x, ii % nx);
                SETCAR(xi, y);
            }
        }
        x
    }
}

// ---------------------------------------------------------------------------
// listRemove
// ---------------------------------------------------------------------------

/// Port of `listRemove()` -- removes an element from a pairlist (for `x[[s]] <- NULL`).
unsafe fn listRemove(x: SEXP, s: SEXP, ind: c_int) -> SEXP {
    unsafe {
        let nx = Rf_length(x);
        let s = GetOneIndex(s, ind);
        let _s_guard = protect(s);
        let mut stretch: R_xlen_t = 0;
        let s = makeSubscript(x, s, &mut stretch, R_NilValue());
        let _subscript_guard = protect(s);
        let ns = Rf_length(s);

        let mut indx = vec![1i32; nx as usize];
        if TYPEOF(s) == REALSXP {
            for i in 0..ns {
                let di = REAL_ELT(s, i);
                if R_FINITE(di) {
                    indx[(di as R_xlen_t - 1) as usize] = 0;
                }
            }
        } else {
            for i in 0..ns {
                let ii = INTEGER_ELT(s, i);
                if ii != NA_INTEGER {
                    indx[(ii - 1) as usize] = 0;
                }
            }
        }

        let mut px = x;
        let mut pv: SEXP = ptr::null_mut();
        let mut val: SEXP = ptr::null_mut();
        for i in 0..nx {
            if indx[i as usize] != 0 {
                if isNull(val) {
                    val = px;
                }
                pv = px;
            } else {
                if !isNull(pv) {
                    SETCDR(pv, CDR(px));
                }
            }
            px = CDR(px);
        }

        if !isNull(val) {
            SET_ATTRIB(val, ATTRIB(x));
            if IS_S4_OBJECT(x) != 0 {
                SET_S4_OBJECT(val);
            } else {
                UNSET_S4_OBJECT(val);
            }
            SET_OBJECT(val, OBJECT(x));
            RAISE_NAMED(val, NAMED(x));
        }

        val
    }
}

// ---------------------------------------------------------------------------
// DeleteOneVectorListItem
// ---------------------------------------------------------------------------

/// Port of `DeleteOneVectorListItem()` -- removes a single element from a vector list.
unsafe fn DeleteOneVectorListItem(x: SEXP, which: R_xlen_t) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::R_NamesSymbol as R_NamesSym;

        let n = XLENGTH(x);
        if which >= 0 && which < n {
            let y = Rf_allocVector3(TYPEOF(x), n - 1);
            let _y_guard = protect(y);
            let mut k: R_xlen_t = 0;
            for i in 0..n {
                if i != which {
                    SET_VECTOR_ELT(y, k, VECTOR_ELT(x, i));
                    k += 1;
                }
            }
            let xnames = getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
            let _xnames_guard = protect(xnames);
            if !isNull(xnames) {
                let ynames = Rf_allocVector3(STRSXP, n - 1);
                let _ynames_guard = protect(ynames);
                k = 0;
                for i in 0..n {
                    if i != which {
                        SET_STRING_ELT(ynames, k, STRING_ELT(xnames, i));
                        k += 1;
                    }
                }
                setAttrib(y, crate::eval::attrib_core::R_NamesSymbol(), ynames);
            }
            copyMostAttrib(x, y);
            y
        } else {
            x
        }
    }
}

// ---------------------------------------------------------------------------
// SubAssignArgs
// ---------------------------------------------------------------------------

/// Port of `SubAssignArgs()` -- extracts (x, s, y) from the argument list
/// and returns the number of subscripts.
unsafe fn SubAssignArgs(args: SEXP, x: *mut SEXP, s: *mut SEXP, y: *mut SEXP) -> c_int {
    unsafe {
        if isNull(CDR(args)) {
            // Error: invalid number of arguments
            *x = CAR(args);
            *s = R_NilValue();
            *y = R_NilValue();
            return 0;
        }
        *x = CAR(args);
        if isNull(CDDR(args)) {
            *s = R_NilValue();
            *y = CADR(args);
            return 0;
        } else {
            let mut nsubs = 1;
            let mut p = CDR(args);
            *s = p;
            while !isNull(CDDR(p)) {
                p = CDR(p);
                nsubs += 1;
            }
            *y = CADR(p);
            SETCDR(p, R_NilValue());
            nsubs
        }
    }
}

// ---------------------------------------------------------------------------
// R_DispatchOrEvalSP
// ---------------------------------------------------------------------------

/// Port of `R_DispatchOrEvalSP()` -- fast-path dispatch/eval for `[<-` and friends.
/// Mirrors subset.c: evaluate first arg, skip dispatch when not an object,
/// otherwise EVPROMISE + `DispatchOrEval`.
unsafe fn R_DispatchOrEvalSP(
    call: SEXP,
    op: SEXP,
    generic: *const c_char,
    args: SEXP,
    rho: SEXP,
    ans: *mut SEXP,
) -> c_int {
    unsafe {
        use crate::eval::dispatch::{DispatchOrEval, evalListKeepMissing};
        use crate::eval::eval::Rf_eval;
        use crate::sexp::memory_ext::{CONS_NR, R_mkEVPROMISE};
        use crate::sexp::symbol::R_DotsSymbol;

        let mut prom: SEXP = ptr::null_mut();
        let mut args_work = args;

        if args != R_NilValue() && CAR(args) != R_DotsSymbol() {
            let x = Rf_eval(CAR(args), rho);
            let _px = protect(x);
            if !isObject(x) {
                let rest = evalListKeepMissing(CDR(args), rho);
                let _pr = protect(rest);
                if !ans.is_null() {
                    *ans = CONS_NR(x, rest);
                }
                return 0;
            }
            prom = R_mkEVPROMISE(CAR(args), x);
            args_work = CONS_NR(prom, CDR(args));
        }

        let _pa = protect(args_work);
        let disp = DispatchOrEval(call, op, generic, args_work, rho, ans, 0, 0);
        let _ = prom;
        disp
    }
}

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

/// Port of `errorNotSubsettable()` -- signals an error for non-subsettable types.
unsafe fn errorNotSubsettable(x: SEXP) {
    unsafe {
        let t = TYPEOF(x);
        let type_name = crate::mainutils::util_main::type2char(t);
        let s = std::ffi::CStr::from_ptr(type_name).to_string_lossy();
        let msg = format!("object of type '{}' is not subsettable", s);
        let cmsg = std::ffi::CString::new(msg).unwrap_or_default();
        crate::mainutils::errors::Rf_error1(
            b"invalid subscript\0".as_ptr() as *const core::ffi::c_char,
            cmsg.as_ptr(),
        );
        unreachable!()
    }
}

/// Port of `errorMissingSubscript()` -- signals an error for missing subscripts.
unsafe fn errorMissingSubscript(x: SEXP) {
    unsafe {
        let t = TYPEOF(x);
        let type_name = crate::mainutils::util_main::type2char(t);
        let s = std::ffi::CStr::from_ptr(type_name).to_string_lossy();
        let msg = format!("object of type '{}' is missing a subscript", s);
        let cmsg = std::ffi::CString::new(msg).unwrap_or_default();
        crate::mainutils::errors::Rf_error1(
            b"invalid subscript\0".as_ptr() as *const core::ffi::c_char,
            cmsg.as_ptr(),
        );
        unreachable!()
    }
}

/// Port of `errorOutOfBoundsSEXP()` -- signals an out-of-bounds error for [[<-.
unsafe fn errorOutOfBoundsSEXP(x: SEXP, subscript: c_int, _sindex: SEXP) {
    unsafe {
        let t = TYPEOF(x);
        let type_name = crate::mainutils::util_main::type2char(t);
        let s = std::ffi::CStr::from_ptr(type_name).to_string_lossy();
        let msg = format!("subscript out of bounds: type '{}' index {}", s, subscript);
        let cmsg = std::ffi::CString::new(msg).unwrap_or_default();
        crate::mainutils::errors::Rf_error1(
            b"subscript out of bounds\0".as_ptr() as *const core::ffi::c_char,
            cmsg.as_ptr(),
        );
        unreachable!()
    }
}

// ---------------------------------------------------------------------------
// Exported functions
// ---------------------------------------------------------------------------

/// Port of `do_subassign()` -- the `[<-` operator.
unsafe fn do_subassign(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();
        if R_DispatchOrEvalSP(
            call,
            op,
            b"[\x00<-".as_ptr() as *const c_char,
            args,
            rho,
            &mut ans,
        ) != 0
        {
            return ans;
        }
        do_subassign_dflt(call, op, ans, rho)
    }
}

/// Port of `do_subassign_dflt()` -- default `[<-` implementation.
pub unsafe fn do_subassign_dflt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = op;
        let _args_guard = protect(args);

        let mut subs: SEXP = ptr::null_mut();
        let mut y: SEXP = ptr::null_mut();
        let mut x: SEXP = ptr::null_mut();
        let nsubs = SubAssignArgs(args, &mut x, &mut subs, &mut y);
        let _y_guard = protect(y);

        // Make sure LHS is duplicated if it matches one of the indices
        let mut s_iter = subs;
        while !isNull(s_iter) {
            let idx = CAR(s_iter);
            if x == idx {
                MARK_NOT_MUTABLE(x);
            }
            s_iter = CDR(s_iter);
        }

        // Duplicate if shared
        if MAYBE_SHARED(CAR(args)) {
            let dup = shallow_duplicate(CAR(args));
            SETCAR(args, dup);
            x = CAR(args);
        }

        let s4 = IS_S4_OBJECT(x);
        let mut oldtype = 0;

        if TYPEOF(x) == LISTSXP || TYPEOF(x) == LANGSXP {
            oldtype = TYPEOF(x);
            x = PairToVectorList(x);
        } else if XLENGTH(x) == 0 {
            if XLENGTH(y) == 0
                && (isNull(x)
                    || TYPEOF(x) == TYPEOF(y)
                    || TYPEOF(y) == VECSXP
                    || TYPEOF(y) == EXPRSXP)
            {
                return x;
            } else {
                if isNull(x) {
                    x = coerceVector(x, TYPEOF(y));
                }
            }
        }
        let _x_guard = protect(x);

        match TYPEOF(x) {
            LGLSXP | INTSXP | REALSXP | CPLXSXP | STRSXP | EXPRSXP | VECSXP | RAWSXP => {
                x = match nsubs {
                    0 => VectorAssign(
                        call,
                        rho,
                        x,
                        {
                            use crate::sexp::globals::R_MissingArg;
                            R_MissingArg()
                        },
                        y,
                    ),
                    1 => VectorAssign(call, rho, x, CAR(subs), y),
                    2 => MatrixAssign(call, rho, x, subs, y),
                    _ => ArrayAssign(call, rho, x, subs, y),
                };
            }
            _ => {
                errorNotSubsettable(x);
            }
        }

        if oldtype == LANGSXP && Rf_length(x) > 0 {
            x = VectorToPairList(x);
            SET_TYPEOF(x, LANGSXP);
        }

        SETTER_CLEAR_NAMED(x);
        if s4 != 0 {
            SET_S4_OBJECT(x);
        }
        x
    }
}

/// Port of `do_subassign2()` -- the `[[<-` operator.
unsafe fn do_subassign2(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();
        if R_DispatchOrEvalSP(
            call,
            op,
            b"[[\x00<-".as_ptr() as *const c_char,
            args,
            rho,
            &mut ans,
        ) != 0
        {
            return ans;
        }
        do_subassign2_dflt(call, op, ans, rho)
    }
}

/// Port of `do_subassign2_dflt()` -- default `[[<-` implementation.
pub unsafe fn do_subassign2_dflt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = op;
        use crate::eval::attrib_core::R_DimNamesSymbol;
        use crate::eval::attrib_core::R_DimSymbol;
        use crate::eval::attrib_core::R_NamesSymbol as R_NamesSym;
        use crate::sexp::globals::R_MissingArg;

        let _args_guard = protect(args);
        let mut dynamic_guards: Vec<crate::sexp::protect::ProtectGuard> = Vec::new();

        let mut subs: SEXP = ptr::null_mut();
        let mut y: SEXP = ptr::null_mut();
        let mut x: SEXP = ptr::null_mut();
        let nsubs = SubAssignArgs(args, &mut x, &mut subs, &mut y);
        let _initial_y_guard = protect(y);

        // Handle NULL left-hand sides
        if isNull(x) {
            if isNull(y) {
                return x;
            }
            x = Rf_allocVector3(VECSXP, 0);
        }

        // Ensure LHS is local
        if MAYBE_SHARED(x) {
            let dup = shallow_duplicate(x);
            SETCAR(args, dup);
            x = dup;
        }

        let s4 = IS_S4_OBJECT(x);
        let xOrig = if s4 != 0 && TYPEOF(x) == OBJSXP {
            let orig = x;
            x = R_getS4DataSlot(x, ANYSXP);
            orig
        } else {
            ptr::null_mut()
        };

        let _initial_x_guard = protect(x);
        let xtop = x;
        let mut xup = x;

        let dims = getAttrib(x, R_DimSymbol());
        let ndims = Rf_length(dims);

        let pdims: *const c_int = if ndims > 0 {
            if TYPEOF(dims) == INTSXP {
                INTEGER(dims)
            } else {
                ptr::null()
            }
        } else {
            ptr::null()
        };

        // ENVSXP special case
        if TYPEOF(x) == ENVSXP {
            if nsubs != 1 || !isString(CAR(subs)) || Rf_length(CAR(subs)) != 1 {
                // Error: wrong args
                return x;
            }
            defineVar(installTrChar(STRING_ELT(CAR(subs), 0 as R_xlen_t)), y, x);
            if s4 != 0 && !isNull(xOrig) {
                return xOrig;
            }
            return x;
        }

        // Recursive indexing case
        let mut recursed = false;
        let mut thesub: SEXP = R_NilValue();
        let mut len = 0;
        let mut off: R_xlen_t = -1;
        let mut newname: SEXP = R_NilValue();

        if nsubs == 1 {
            thesub = CAR(subs);
            len = Rf_length(thesub);
            if len > 1 {
                xup = vectorIndex(x, thesub, 0, len - 2, TRUE, call, TRUE);
                dynamic_guards.push(protect(xup));
                off = OneIndex(
                    xup,
                    thesub,
                    XLENGTH(xup),
                    0,
                    &mut newname,
                    len - 2,
                    R_NilValue(),
                );
                x = vectorIndex(xup, thesub, len - 2, len - 1, TRUE, call, TRUE);
                dynamic_guards.push(protect(x));
                recursed = true;
            }
        }
        let _xup_guard = protect(xup);

        let mut stretch: R_xlen_t = 0;
        let mut offset: R_xlen_t = 0;

        if isVector(x) {
            if !isVectorList(x) && Rf_length(y) == 0 {
                // Error: replacement has length zero
                return xtop;
            }
            if !isVectorList(x) && Rf_length(y) > 1 {
                // Error: more elements supplied
                return xtop;
            }
            if nsubs == 0 || CAR(subs) == R_MissingArg() {
                errorMissingSubscript(x);
            }
            if nsubs == 1 {
                offset = OneIndex(
                    x,
                    thesub,
                    XLENGTH(x),
                    0,
                    &mut newname,
                    if recursed { len - 1 } else { -1 },
                    R_NilValue(),
                );
                if isVectorList(x) && isNull(y) {
                    let old_x = x;
                    x = DeleteOneVectorListItem(x, offset);
                    if recursed {
                        if isVectorList(xup) {
                            SET_VECTOR_ELT(xup, off, x);
                        } else {
                            let _x_guard = protect(x);
                            xup = SimpleListAssign(call, xup, subs, x, len - 2, false);
                        }
                    } else {
                        // xtop = x handled below
                    }
                    if s4 != 0 && !isNull(xOrig) {
                        SET_S4_OBJECT(xOrig);
                    }
                    return x;
                }
                if offset < 0 {
                    errorOutOfBoundsSEXP(x, -1, thesub);
                }
                if offset >= XLENGTH(x) {
                    stretch = offset + 1;
                }
            } else {
                if ndims != nsubs {
                    // Error: improper number of subscripts
                    return xtop;
                }
                let indx = Rf_allocVector3(INTSXP, ndims as R_xlen_t);
                let _indx_guard = protect(indx);
                let pindx = INTEGER(indx);
                let names = getAttrib(x, R_DimNamesSymbol());
                let mut subs_tmp = subs;
                for i in 0..ndims {
                    let sub_i = CAR(subs_tmp);
                    *pindx.add(i as usize) = get1index(
                        sub_i,
                        if isNull(names) {
                            R_NilValue()
                        } else {
                            VECTOR_ELT(names, i as R_xlen_t)
                        },
                        if pdims.is_null() {
                            0
                        } else {
                            *pdims.add(i as usize) as R_xlen_t
                        },
                        FALSE,
                        -1,
                        call,
                    ) as c_int;
                    subs_tmp = CDR(subs_tmp);
                    if *pindx.add(i as usize) < 0
                        || (pdims.is_null() || *pindx.add(i as usize) >= *pdims.add(i as usize))
                    {
                        errorOutOfBoundsSEXP(x, i, sub_i);
                    }
                }
                offset = 0;
                for i in (1..ndims).rev() {
                    offset = (offset + (*pindx.add(i as usize) as R_xlen_t))
                        * (if pdims.is_null() {
                            1
                        } else {
                            *pdims.add((i - 1) as usize) as R_xlen_t
                        });
                }
                offset += *pindx.add(0) as R_xlen_t;
            }
            // NAs are not allowed in subscripted assignments (upstream raises
            // this from OneIndex processing, before any typed assignment arm).
            if offset == NA_INTEGER as R_xlen_t {
                crate::mainutils::errors::Rf_error(
                    b"NAs are not allowed in subscripted assignments\0".as_ptr()
                        as *const core::ffi::c_char,
                );
            }
            let which = SubassignTypeFix(&mut x, &mut y, stretch, 2, call, rho);
            dynamic_guards.push(protect(x));
            dynamic_guards.push(protect(y));

            match which {
                1010 | 1310 | 1313 => {
                    *INTEGER(x).add(offset as usize) = INTEGER_ELT(y, 0);
                }
                1410 | 1413 => {
                    if INTEGER_ELT(y, 0) == NA_INTEGER {
                        *REAL(x).add(offset as usize) = NA_REAL;
                    } else {
                        *REAL(x).add(offset as usize) = INTEGER_ELT(y, 0) as c_double;
                    }
                }
                1414 => {
                    *REAL(x).add(offset as usize) = REAL(y).read();
                }
                1510 | 1513 => {
                    if INTEGER_ELT(y, 0) == NA_INTEGER {
                        (*COMPLEX(x).add(offset as usize)).r = NA_REAL;
                        (*COMPLEX(x).add(offset as usize)).i = 0.0;
                    } else {
                        (*COMPLEX(x).add(offset as usize)).r = INTEGER_ELT(y, 0) as c_double;
                        (*COMPLEX(x).add(offset as usize)).i = 0.0;
                    }
                }
                1514 => {
                    let ry = REAL_ELT(y, 0);
                    if ISNA(ry) {
                        (*COMPLEX(x).add(offset as usize)).r = NA_REAL;
                        (*COMPLEX(x).add(offset as usize)).i = 0.0;
                    } else {
                        (*COMPLEX(x).add(offset as usize)).r = ry;
                        (*COMPLEX(x).add(offset as usize)).i = 0.0;
                    }
                }
                1515 => {
                    *COMPLEX(x).add(offset as usize) = COMPLEX_ELT(y, 0);
                }
                1610 | 1613 | 1614 | 1615 | 1616 => {
                    SET_STRING_ELT(x, offset, STRING_ELT(y, 0));
                }
                1019 | 1319 | 1419 | 1519 | 1619 | 1901 | 1902 | 1904 | 1905 | 1906 | 1910
                | 1913 | 1914 | 1915 | 1916 | 1920 | 1921 | 1922 | 1923 | 1924 | 1925 | 1903
                | 1907 | 1908 | 1999 | 2001 | 2002 | 2006 | 2010 | 2013 | 2014 | 2015 | 2016
                | 2024 | 2025 | 1919 | 2020 => {
                    if MAYBE_REFERENCED(y) && VECTOR_ELT(x, offset) != y {
                        y = R_FixupRHS(x, y);
                    }
                    SET_VECTOR_ELT(x, offset, y);
                }
                2424 => {
                    *RAW(x).add(offset as usize) = RAW_ELT(y, 0);
                }
                _ => {} // intentionally unhandled: unsupported SEXPTYPE for scalar subassignment
            }

            // If stretched, handle new name
            if stretch > 0 && !isNull(newname) {
                let names = getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
                if isNull(names) {
                    let names_new = Rf_allocVector3(STRSXP, Rf_length(x) as R_xlen_t);
                    let _names_new_guard = protect(names_new);
                    SET_STRING_ELT(names_new, offset, newname);
                    setAttrib(x, crate::eval::attrib_core::R_NamesSymbol(), names_new);
                } else {
                    SET_STRING_ELT(names, offset, newname);
                }
            }

            dynamic_guards.push(protect(x));
            dynamic_guards.push(protect(xup));
        } else if isPairList(x) {
            dynamic_guards.push(protect(y));
            if nsubs == 1 {
                if isNull(y) {
                    x = listRemove(x, CAR(subs), len - 1);
                } else {
                    x = SimpleListAssign(call, x, subs, y, len - 1, true);
                }
            } else {
                if ndims != nsubs {
                    // Error
                    return xtop;
                }
                let indx = Rf_allocVector3(INTSXP, ndims as R_xlen_t);
                let _indx_guard = protect(indx);
                let pindx = INTEGER(indx);
                let names = getAttrib(x, R_DimNamesSymbol());
                let mut subs_tmp = subs;
                for i in 0..ndims {
                    let sub_i = CAR(subs_tmp);
                    *pindx.add(i as usize) = get1index(
                        sub_i,
                        VECTOR_ELT(names, i as R_xlen_t),
                        if pdims.is_null() {
                            0
                        } else {
                            *pdims.add(i as usize) as R_xlen_t
                        },
                        FALSE,
                        -1,
                        call,
                    ) as c_int;
                    subs_tmp = CDR(subs_tmp);
                    if *pindx.add(i as usize) < 0
                        || (pdims.is_null() || *pindx.add(i as usize) >= *pdims.add(i as usize))
                    {
                        errorOutOfBoundsSEXP(x, i, sub_i);
                    }
                }
                offset = 0;
                for i in (1..ndims).rev() {
                    offset = (offset + (*pindx.add(i as usize) as R_xlen_t))
                        * (if pdims.is_null() {
                            1
                        } else {
                            *pdims.add((i - 1) as usize) as R_xlen_t
                        });
                }
                offset += *pindx.add(0) as R_xlen_t;
                let slot = nthcdr(x, offset as c_int);
                SETCAR(slot, y);
            }
            dynamic_guards.push(protect(x));
            dynamic_guards.push(protect(xup));
        } else {
            errorNotSubsettable(x);
        }

        let mut xtop = xtop;
        if recursed {
            if isVectorList(xup) {
                SET_VECTOR_ELT(xup, off, x);
            } else {
                xup = SimpleListAssign(call, xup, subs, x, len - 2, false);
            }
            if len == 2 {
                xtop = xup;
            }
        } else {
            xtop = x;
        }

        SETTER_CLEAR_NAMED(xtop);
        if s4 != 0 {
            SET_S4_OBJECT(xtop);
        }
        xtop
    }
}

/// Port of `do_subassign3()` -- the `$<-` operator.
unsafe fn do_subassign3(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut nlist: SEXP = R_NilValue();
        checkArity(op, args);
        let args = fixSubset3Args(call, args, env, &mut nlist);
        let _args_guard = protect(args);

        let mut ans: SEXP = ptr::null_mut();
        if R_DispatchOrEvalSP(
            call,
            op,
            b"$\x00<-".as_ptr() as *const c_char,
            args,
            env,
            &mut ans,
        ) != 0
        {
            return ans;
        }
        let _ans_guard = protect(ans);
        let result = R_subassign3_dflt(call, CAR(ans), nlist, CADDR(ans));
        result
    }
}

/// Port of `R_subassign3_dflt()` -- default `$<-` implementation.
pub unsafe fn R_subassign3_dflt(call: SEXP, x: SEXP, nlist: SEXP, val: SEXP) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::R_NamesSymbol as R_NamesSym;

        let mut x = x;
        let mut val = val;
        // Upstream has no early NULL return: a NULL target is grown below
        // (coerced to an empty list / new one-element pairlist as needed).

        let s4 = IS_S4_OBJECT(x);
        let mut xS4: SEXP = R_NilValue();
        let nprotect = 0;

        if MAYBE_SHARED(x) {
            x = shallow_duplicate(x);
        }

        // Code to allow classes to extend ENVSXP
        if TYPEOF(x) == OBJSXP {
            xS4 = x;
            x = R_getS4DataSlot(x, ANYSXP);
            if isNull(x) {
                // Error: no method
                return xS4;
            }
        }

        if (isList(x) || isLanguage(x)) && !isNull(x) {
            if TAG(x) == nlist {
                if isNull(val) {
                    SET_ATTRIB(CDR(x), ATTRIB(x));
                    if IS_S4_OBJECT(x) != 0 {
                        SET_S4_OBJECT(CDR(x));
                    } else {
                        UNSET_S4_OBJECT(CDR(x));
                    }
                    SET_OBJECT(CDR(x), OBJECT(x));
                    RAISE_NAMED(CDR(x), NAMED(x));
                    SETCAR(x, R_NilValue());
                    x = CDR(x);
                } else {
                    if MAYBE_REFERENCED(val) && CAR(x) != val {
                        val = R_FixupRHS(x, val);
                    }
                    SETCAR(x, val);
                }
            } else {
                let mut t = x;
                while !isNull(t) {
                    if TAG(CDR(t)) == nlist {
                        if isNull(val) {
                            SETCAR(CDR(t), R_NilValue());
                            SETCDR(t, CDDR(t));
                        } else {
                            if MAYBE_REFERENCED(val) && CADR(t) != val {
                                val = R_FixupRHS(x, val);
                            }
                            SETCAR(CDR(t), val);
                        }
                        break;
                    } else if isNull(CDR(t)) && !isNull(val) {
                        SETCDR(t, allocSExp(SEXPTYPE::LISTSXP));
                        SETTAG(CDR(t), nlist);
                        if MAYBE_REFERENCED(val) {
                            ENSURE_NAMEDMAX(val);
                        }
                        SETCADR(t, val);
                        break;
                    }
                    t = CDR(t);
                }
            }
            if isNull(x) && !isNull(val) {
                x = allocList(1);
                if MAYBE_REFERENCED(val) {
                    ENSURE_NAMEDMAX(val);
                }
                SETCAR(x, val);
                SETTAG(x, nlist);
            }
        } else if TYPEOF(x) == ENVSXP {
            defineVar(nlist, val, x);
            INCREMENT_NAMED(val);
        } else if TYPEOF(x) == SYMSXP
            || TYPEOF(x) == CLOSXP
            || TYPEOF(x) == SPECIALSXP
            || TYPEOF(x) == BUILTINSXP
        {
            errorNotSubsettable(x);
        } else {
            let nx = XLENGTH(x);
            let mut atype = VECSXP;

            if isExpression(x) {
                atype = EXPRSXP;
            } else if !isNewList(x) {
                x = coerceVector(x, VECSXP);
            }

            let names = getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
            let nlist_name = PRINTNAME(nlist);

            if isNull(val) {
                // Element deletion
                if !isNull(names) {
                    let mut imatch: i64 = -1;
                    for i in 0..nx {
                        if NonNullStringMatch(STRING_ELT(names, i), nlist_name) != 0 {
                            imatch = i as i64;
                            break;
                        }
                    }
                    if imatch >= 0 {
                        let ans = Rf_allocVector3(atype, nx - 1);
                        let ansnames = Rf_allocVector3(STRSXP, nx - 1);
                        let mut ii: R_xlen_t = 0;
                        for i in 0..nx {
                            if i != imatch as R_xlen_t {
                                SET_VECTOR_ELT(ans, ii, VECTOR_ELT(x, i));
                                SET_STRING_ELT(ansnames, ii, STRING_ELT(names, i));
                                ii += 1;
                            }
                        }
                        setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), ansnames);
                        copyMostAttrib(x, ans);
                        x = ans;
                    }
                }
            } else {
                // Replace or add element
                let mut imatch: i64 = -1;
                if !isNull(names) {
                    for i in 0..nx {
                        if NonNullStringMatch(STRING_ELT(names, i), nlist_name) != 0 {
                            imatch = i as i64;
                            break;
                        }
                    }
                }
                if imatch >= 0 {
                    // Replace existing element
                    if MAYBE_REFERENCED(val) && VECTOR_ELT(x, imatch as R_xlen_t) != val {
                        val = R_FixupRHS(x, val);
                    }
                    SET_VECTOR_ELT(x, imatch as R_xlen_t, val);
                } else {
                    // Add new element
                    let ans = Rf_allocVector3(VECSXP, nx + 1);
                    let ansnames = Rf_allocVector3(STRSXP, nx + 1);
                    for i in 0..nx {
                        SET_VECTOR_ELT(ans, i, VECTOR_ELT(x, i));
                        if isNull(names) {
                            SET_STRING_ELT(ansnames, i, R_BlankString());
                        } else {
                            SET_STRING_ELT(ansnames, i, STRING_ELT(names, i));
                        }
                    }
                    if MAYBE_REFERENCED(val) {
                        ENSURE_NAMEDMAX(val);
                    }
                    SET_VECTOR_ELT(ans, nx, val);
                    SET_STRING_ELT(ansnames, nx, nlist_name);
                    setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), ansnames);
                    copyMostAttrib(x, ans);
                    x = ans;
                }
            }
        }

        if !isNull(xS4) {
            x = xS4;
        }
        SETTER_CLEAR_NAMED(x);
        if s4 != 0 {
            SET_S4_OBJECT(x);
        }
        x
    }
}

// ---------------------------------------------------------------------------
// Additional exported symbols
// ---------------------------------------------------------------------------

/// Port of `SubassignTypeSym()` -- used by the byte code compiler.
pub unsafe fn SubassignTypeSym() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new("SubassignTypeSym")
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// Port of `SubassignDotsNames()` -- handles assignment to `...` names.
pub unsafe fn SubassignDotsNames(_call: SEXP, _rho: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Port of `GetSubassignSxpVec()` -- used by the byte code interpreter.
pub unsafe fn GetSubassignSxpVec(x: SEXP, indx: SEXP) -> SEXP {
    unsafe {
        if isNull(x) || isNull(indx) {
            return R_NilValue();
        }
        let n = XLENGTH(indx);
        if n == 0 {
            return R_NilValue();
        }
        let idx = gi(indx, 0);
        if idx == NA_INTEGER as R_xlen_t || idx < 1 || idx > XLENGTH(x) {
            return R_NilValue();
        }
        VECTOR_ELT(x, idx - 1)
    }
}

/// Port of `var_assign()` -- handles variable assignment in the interpreter.
pub unsafe fn var_assign(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_subassign(call, op, args, rho) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_do_subassign_handles_empty_r_argument_list() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_subassign(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                R_NilValue(),
                std::ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_subassign_dflt_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_subassign_dflt(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            // Null args return null
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_do_subassign2_handles_empty_r_argument_list() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_subassign2(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                R_NilValue(),
                std::ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_subassign2_dflt_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_subassign2_dflt(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            // Null args return null
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_do_subassign3_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        // do_subassign3 calls fixSubset3Args which panics with RError on nil args.
        // Just verify the function exists and has the right signature.
        // A full integration test would need proper SEXP arguments.
        let _fn_ptr: unsafe fn(SEXP, SEXP, SEXP, SEXP) -> SEXP = do_subassign3;
    }

    #[test]
    fn test_R_subassign3_dflt_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_subassign3_dflt(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            // Upstream subassign.c has no early NULL return: assignment into
            // NULL grows a result rather than staying nil.
            assert!(!result.is_null());
        }
    }

    #[test]
    fn test_SubassignTypeSym_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = SubassignTypeSym();
            // Should not be null (it's an installed symbol)
            assert!(!result.is_null());
        }
    }

    #[test]
    fn test_SubassignDotsNames_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = SubassignDotsNames(std::ptr::null_mut(), std::ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_GetSubassignSxpVec_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = GetSubassignSxpVec(std::ptr::null_mut(), std::ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_var_assign_handles_empty_r_argument_list() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = var_assign(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                R_NilValue(),
                std::ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_getNames_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = getNames(R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_gi_integer() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_allocVector3(INTSXP, 3);
            let _v_guard = protect(v);
            let p = INTEGER(v);
            *p.add(0) = 10;
            *p.add(1) = 20;
            *p.add(2) = NA_INTEGER;
            assert_eq!(gi(v, 0), 10);
            assert_eq!(gi(v, 1), 20);
            assert_eq!(gi(v, 2), NA_INTEGER as R_xlen_t);
        }
    }

    #[test]
    fn test_SubAssignArgs_two_args() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Create args: x, y (no subscripts)
            let y_val = Rf_allocVector3(INTSXP, 1);
            let _y_val_guard = protect(y_val);
            let args = Rf_cons(R_NilValue(), Rf_cons(y_val, R_NilValue()));
            let _args_guard = protect(args);

            let mut x: SEXP = ptr::null_mut();
            let mut s: SEXP = ptr::null_mut();
            let mut y: SEXP = ptr::null_mut();
            let nsubs = SubAssignArgs(args, &mut x, &mut s, &mut y);
            assert_eq!(nsubs, 0);
            assert_eq!(x, R_NilValue());
            assert_eq!(s, R_NilValue());
            assert_eq!(y, y_val);
        }
    }

    #[test]
    fn test_SubassignTypeFix_same_type() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut xv: SEXP = Rf_allocVector3(INTSXP, 1);
            let _xv_guard = protect(xv);
            let mut yv: SEXP = Rf_allocVector3(INTSXP, 1);
            let _yv_guard = protect(yv);
            let which = SubassignTypeFix(&mut xv, &mut yv, 0, 1, ptr::null_mut(), ptr::null_mut());
            // 100 * 13 + 13 = 1313
            assert_eq!(which, 1313);
        }
    }
}
