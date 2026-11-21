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
// Checked bridges
//
// The raw entry functions below uphold the same ambient contract the unsafe
// accessors they replace already required: callers run inside the active R
// session and keep their argument objects rooted across allocation. These
// shims concentrate that one boundary so the ported bodies stay checked.
// ---------------------------------------------------------------------------

/// The active session's owner token for this raw boundary.
pub(crate) fn owner() -> crate::sexp::owner::OwnerToken<'static> {
    // SAFETY: callers are translated entry points inside the active session;
    // the token's retain/exclusion contract matches the ambient contract the
    // raw allocators and accessors in this module already require.
    unsafe { crate::sexp::owner::OwnerToken::current() }
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// The session's checked node factory.
pub(crate) fn factory() -> crate::sexp::object::SessionNodeFactory<'static> {
    owner().node_factory()
}

/// Wrap a live, rooted argument pointer for checked reads and writes.
pub(crate) fn owned(pointer: SEXP) -> crate::sexp::object::Sexp<'static> {
    owner()
        .sexp(pointer)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked mutation authority over a live, rooted argument pointer.
pub(crate) fn owned_mut(pointer: SEXP) -> crate::sexp::object::SexpMut<'static> {
    crate::sexp::object::SexpMut::try_from_checked(owned(pointer))
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Intern a NUL-terminated static name, matching `Rf_install`.
pub(crate) fn install(name: &std::ffi::CStr) -> SEXP {
    // SAFETY: the names below are static C strings; the ambient session
    // contract of `Rf_install` is the one this boundary already upholds.
    unsafe { Rf_install(name.as_ptr()) }
}

/// Allocate a vector through the runtime's GC-retry/denial path.
pub(crate) fn alloc_vector<T: Into<SEXPTYPE>>(kind: T, length: R_xlen_t) -> SEXP {
    // SAFETY: callers rely on `Rf_allocVector3`'s allocation-denial error and
    // GC retry; the ambient session contract is unchanged.
    Rf_allocVector3(kind, length)
}

/// Walk a pairlist with checked `CDR` steps, matching `Rf_length`.
pub(crate) fn pairlist_length(mut node: crate::sexp::object::Sexp<'static>) -> c_int {
    let mut n = 0;
    while !node.is_nil() {
        node = node
            .try_cdr()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        n += 1;
    }
    n
}

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
pub(crate) fn sym_Dim() -> SEXP {
    install(c"dim")
}

/// Get the "names" symbol.
#[inline]
pub(crate) fn sym_Names() -> SEXP {
    install(c"names")
}

/// Get the "dimnames" symbol.
#[inline]
pub(crate) fn sym_DimNames() -> SEXP {
    install(c"dimnames")
}

/// Get the "class" symbol.
#[inline]
pub(crate) fn sym_Class() -> SEXP {
    install(c"class")
}

/// Get the "use.names" symbol (for subscript name passing).
#[inline]
pub(crate) fn sym_UseNames() -> SEXP {
    install(c"use.names")
}

// ---------------------------------------------------------------------------
// Local type-checking helpers
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn isNull(x: SEXP) -> bool {
    x.is_null() || x == R_NilValue()
}

#[inline]
pub(crate) fn isVector(x: SEXP) -> bool {
    {
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
pub(crate) fn isVectorList(x: SEXP) -> bool {
    {
        let t = TYPEOF(x);
        t == VECSXP || t == EXPRSXP
    }
}

#[inline]
pub(crate) fn isPairList(x: SEXP) -> bool {
    {
        let t = TYPEOF(x);
        // GNU Rf_isPairList: NILSXP / LISTSXP / LANGSXP / DOTSXP.
        t == LISTSXP || t == NILSXP || t == LANGSXP || t == DOTSXP
    }
}

#[inline]
pub(crate) fn isList(x: SEXP) -> bool {
    TYPEOF(x) == LISTSXP
}

#[inline]
pub(crate) fn isLanguage(x: SEXP) -> bool {
    TYPEOF(x) == LANGSXP
}

#[inline]
pub(crate) fn isExpression(x: SEXP) -> bool {
    TYPEOF(x) == EXPRSXP
}

#[inline]
pub(crate) fn isNewList(x: SEXP) -> bool {
    TYPEOF(x) == VECSXP
}

#[inline]
pub(crate) fn isObject(x: SEXP) -> bool {
    OBJECT(x) != 0
}

#[inline]
pub(crate) fn isMatrix(x: SEXP) -> bool {
    let dim = getAttrib(x, sym_Dim());
    !isNull(dim) && LENGTH(dim) == 2
}

#[inline]
pub(crate) fn isArray(x: SEXP) -> bool {
    let dim = getAttrib(x, sym_Dim());
    !isNull(dim) && LENGTH(dim) >= 2
}

#[inline]
pub(crate) fn isString(x: SEXP) -> bool {
    TYPEOF(x) == STRSXP
}

#[inline]
pub(crate) fn isInteger(x: SEXP) -> bool {
    TYPEOF(x) == INTSXP
}

#[inline]
pub(crate) fn isReal(x: SEXP) -> bool {
    TYPEOF(x) == REALSXP
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
pub(crate) fn IS_S4_OBJECT(x: SEXP) -> c_int {
    crate::sexp::accessors::checked_s4_object_flag(x)
}

/// Set the S4 bit on an object.
#[inline]
pub(crate) fn SET_S4_OBJECT(x: SEXP) {
    if crate::sexp::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
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
pub(crate) fn UNSET_S4_OBJECT(x: SEXP) {
    if crate::sexp::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
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
pub(crate) fn MAYBE_SHARED(x: SEXP) -> bool {
    x.is_null() || NAMED(x) >= 2
}

/// Check if an object may be referenced.
#[inline]
pub(crate) fn MAYBE_REFERENCED(x: SEXP) -> bool {
    NAMED(x) >= 1
}

/// Mark an object as not mutable (NAMEDMAX).
#[inline]
pub(crate) fn MARK_NOT_MUTABLE(x: SEXP) {
    if !x.is_null() {
        crate::sexp::accessors::SET_NAMED(x, 2);
    }
}

/// Set NAMED to 0 (setter-clear).
#[inline]
pub(crate) fn SETTER_CLEAR_NAMED(x: SEXP) {
    if crate::sexp::globals::immutable_singleton_projection(x).is_some() {
        return;
    }

    if !x.is_null() {
        crate::sexp::accessors::SET_NAMED(x, 0);
    }
}

/// Raise NAMED level.
#[inline]
pub(crate) fn RAISE_NAMED(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() && (v as u8) > (*x).sxpinfo.named() {
            crate::sexp::accessors::SET_NAMED(x, v);
        }
    }
}

/// Increment NAMED.
#[inline]
pub(crate) fn INCREMENT_NAMED(x: SEXP) {
    unsafe {
        if !x.is_null() {
            let n = (*x).sxpinfo.named();
            if n < 2 {
                crate::sexp::accessors::SET_NAMED(x, i32::from(n + 1));
            }
        }
    }
}

/// Check if an object is growable (has truelength > length).
#[inline]
pub(crate) fn IS_GROWABLE(x: SEXP) -> bool {
    unsafe { !x.is_null() && ((*x).sxpinfo.gp() & (1u16 << 5)) != 0 && XTRUELENGTH(x) > XLENGTH(x) }
}

/// Set the growable bit on an object.
#[inline]
pub(crate) fn SET_GROWABLE_BIT(x: SEXP) {
    if crate::sexp::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if !x.is_null() {
            let gp = (*x).sxpinfo.gp();
            (*x).sxpinfo.set_gp(gp | (1u16 << 5));
        }
    }
}

/// Set true length of a vector.
#[inline]
pub(crate) fn SET_TRUELENGTH(x: SEXP, v: c_int) {
    if crate::sexp::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if !x.is_null() {
            (*x).data.vector_mut().truelength = v as R_xlen_t;
        }
    }
}

/// Get true length of a vector.
#[inline]
pub(crate) fn XTRUELENGTH(x: SEXP) -> R_xlen_t {
    unsafe {
        if x.is_null() {
            0
        } else {
            (*x).data.vector().truelength
        }
    }
}

/// SETCADR: set the CAR of the CDR.
#[inline]
pub(crate) fn SETCADR(x: SEXP, v: SEXP) {
    if crate::sexp::globals::immutable_singleton_projection(x).is_some() {
        return;
    }

    SETCAR(CDR(x), v);
}

/// SET_TYPEOF: set the type of an SEXP.
#[inline]
pub(crate) fn SET_TYPEOF(x: SEXP, v: c_int) {
    if crate::sexp::globals::immutable_singleton_projection(x).is_some() {
        return;
    }

    crate::sexp::accessors::SET_TYPEOF(x, SEXPTYPE(v).as_c_int());
}

/// Set the standard vector length (not marking as immutable).
#[inline]
pub(crate) fn SET_STDVEC_LENGTH(x: SEXP, v: R_xlen_t) {
    if crate::sexp::globals::immutable_singleton_projection(x).is_some() {
        return;
    }
    unsafe {
        if !x.is_null() {
            (*x).data.vector_mut().length = v;
        }
    }
}

/// ENSURE_NAMEDMAX: set NAMED to NAMEDMAX.
#[inline]
pub(crate) fn ENSURE_NAMEDMAX(x: SEXP) {
    if !x.is_null() {
        crate::sexp::accessors::SET_NAMED(x, 2);
    }
}

#[cfg(test)]
mod singleton_tests {
    use super::*;

    #[test]
    fn legacy_named_helpers_preserve_shared_singletons() {
        unsafe {
            for node in [
                R_NilValue(),
                crate::sexp::globals::R_True(),
                crate::sexp::globals::R_False(),
            ] {
                MARK_NOT_MUTABLE(node);
                SETTER_CLEAR_NAMED(node);
                ENSURE_NAMEDMAX(node);
                RAISE_NAMED(node, 1);
                INCREMENT_NAMED(node);
                SET_TYPEOF(node, SEXPTYPE::REALSXP.0);
                SET_STDVEC_LENGTH(node, 99);
                assert_eq!(crate::sexp::accessors::NAMED(node), 2);
            }
            assert_eq!(
                crate::sexp::accessors::TYPEOF(R_NilValue()),
                SEXPTYPE::NILSXP
            );
            assert_eq!(
                crate::sexp::accessors::LOGICAL_ELT(crate::sexp::globals::R_True(), 0),
                1
            );
            assert_eq!(
                crate::sexp::accessors::XLENGTH(crate::sexp::globals::R_True()),
                1
            );
        }
    }
}

/// Check if the call is an assignment call.
#[inline]
pub(crate) fn IS_ASSIGNMENT_CALL(call: SEXP) -> bool {
    if isNull(call) {
        return true;
    }
    let t = TYPEOF(call);
    t == LANGSXP || t == SYMSXP
}

/// R_FixupRHS: fix up RHS for assignment (duplicate if needed).
pub(crate) fn R_FixupRHS(x: SEXP, y: SEXP) -> SEXP {
    if MAYBE_SHARED(y) {
        shallow_duplicate(y)
    } else {
        y
    }
}

/// PairToVectorList: convert a pairlist to a vector list.
/// GNU copies cell tags onto the `names` attribute so `formals(f)[i] <- …`
/// keeps formal names.
pub(crate) fn PairToVectorList(x: SEXP) -> SEXP {
    let fail = |error: crate::sexp::object::SexpError| -> ! {
        crate::sexp::context::r_error(error.to_string())
    };
    let factory = factory();
    let len = pairlist_length(owned(x));
    let ans = factory
        .wrap(alloc_vector(VECSXP, len as R_xlen_t))
        .unwrap_or_else(|e| fail(e));
    let mut ans = crate::sexp::object::SexpMut::try_from_checked(ans).unwrap_or_else(|e| fail(e));
    let names = factory
        .wrap(alloc_vector(STRSXP, len as R_xlen_t))
        .unwrap_or_else(|e| fail(e));
    let mut names =
        crate::sexp::object::SexpMut::try_from_checked(names).unwrap_or_else(|e| fail(e));
    let blank = factory.character("").unwrap_or_else(|e| fail(e));
    let mut src = owned(x);
    let mut i: R_xlen_t = 0;
    let mut any_name = false;
    while !src.is_nil() && i < len as R_xlen_t {
        let car = src.try_car().unwrap_or_else(|e| fail(e));
        ans.try_set_vector_elt(i, car).unwrap_or_else(|e| fail(e));
        let tag = src.try_tag().unwrap_or_else(|e| fail(e));
        if !tag.is_nil() && tag.typeof_() == SEXPTYPE::SYMSXP {
            let printname = tag.try_printname().unwrap_or_else(|e| fail(e));
            if !printname.is_nil() {
                names
                    .try_set_string_elt(i, printname)
                    .unwrap_or_else(|e| fail(e));
                any_name = true;
            } else {
                names
                    .try_set_string_elt(i, blank.clone())
                    .unwrap_or_else(|e| fail(e));
            }
        } else {
            names
                .try_set_string_elt(i, blank.clone())
                .unwrap_or_else(|e| fail(e));
        }
        src = src.try_cdr().unwrap_or_else(|e| fail(e));
        i += 1;
    }
    let ans = ans.freeze();
    let names = names.freeze();
    if any_name {
        setAttrib(ans.as_raw(), sym_Names(), names.as_raw());
    }
    ans.as_raw()
}

/// VectorToPairList: convert a vector list to a pairlist.
pub fn VectorToPairList(x: SEXP) -> SEXP {
    let fail = |error: crate::sexp::object::SexpError| -> ! {
        crate::sexp::context::r_error(error.to_string())
    };
    let factory = factory();
    let vector = owned(x);
    let len = XLENGTH(x);
    let names = getAttrib(x, sym_Names());
    let names = if names.is_null() {
        None
    } else {
        Some(owned(names))
    };
    let mut result = factory.nil();
    let mut i: R_xlen_t = len;
    while i > 0 {
        i -= 1;
        let value = vector.try_vector_elt(i).unwrap_or_else(|e| fail(e));
        let mut cell = factory
            .pairlist_cell(&value, &result, &factory.nil())
            .unwrap_or_else(|e| fail(e));
        if let Some(names) = &names {
            if names.typeof_() == SEXPTYPE::STRSXP && names.len() > i {
                let name = names.try_string_elt(i).unwrap_or_else(|e| fail(e));
                if !name.is_nil() {
                    let text = name.try_as_string().unwrap_or_else(|e| fail(e));
                    if !text.is_empty() {
                        let ctext = std::ffi::CString::new(text).unwrap_or_default();
                        let symbol = owned(install(&ctext));
                        let mut cell_mut =
                            crate::sexp::object::SexpMut::try_from_checked(cell.clone())
                                .unwrap_or_else(|e| fail(e));
                        cell_mut
                            .try_set_pairlist_tag(&symbol)
                            .unwrap_or_else(|e| fail(e));
                        cell = cell_mut.freeze();
                    }
                }
            }
        }
        result = cell;
    }
    result.as_raw()
}

/// coerceVector: coerce a vector to a different type.
pub(crate) fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::mainutils::coerce::coerceVector(x, type_)
}

/// getAttrib: get an attribute from an object.
pub(crate) fn getAttrib(x: SEXP, what: SEXP) -> SEXP {
    unsafe { crate::eval::attrib_core::getAttrib(x, what) }
}

/// setAttrib: set an attribute on an object.
pub(crate) fn setAttrib(x: SEXP, what: SEXP, value: SEXP) {
    unsafe {
        crate::eval::attrib_core::setAttrib(x, what, value);
    }
}

/// shallow_duplicate: create a shallow copy.
pub(crate) fn shallow_duplicate(x: SEXP) -> SEXP {
    if isNull(x) {
        return R_NilValue();
    }
    crate::mainutils::duplicate::shallow_duplicate(x)
}

/// copyMostAttrib: copy most attributes from src to dest.
///
/// Copies all attributes except dim, dimnames, and names (which are
/// handled separately by EnlargeVector). Also copies the OBJECT flag
/// and S4 object bit.
pub(crate) fn copyMostAttrib(src: SEXP, dest: SEXP) {
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
                // GNU copyMostAttrib uses installAttrib: this is an
                // internal metadata transfer, not a user `attr<-` call.
                // In particular, a temporary `[<-` result may have grown
                // while retaining its old `tsp`; class methods validate and
                // reject that resize after NextMethod returns.
                crate::sexp::attrib_core::installAttrib(dest, tag, CAR(attr));
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
pub(crate) fn listAppend(t: SEXP, s: SEXP) -> SEXP {
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
    owned_mut(p)
        .try_set_pairlist_cdr(&owned(s))
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    t
}

/// asInteger: coerce to integer.
pub(crate) fn asInteger(x: SEXP) -> c_int {
    crate::mainutils::coerce::asInteger(x)
}

/// nrows: get number of rows of a matrix.
pub(crate) fn nrows(x: SEXP) -> c_int {
    let dim = getAttrib(x, sym_Dim());
    if isNull(dim) {
        return 0;
    }
    owned(dim)
        .try_integer_elt(0)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// ncols: get number of columns of a matrix.
pub(crate) fn ncols(x: SEXP) -> c_int {
    let dim = getAttrib(x, sym_Dim());
    if isNull(dim) {
        return 0;
    }
    if LENGTH(dim) < 2 {
        return 1;
    }
    owned(dim)
        .try_integer_elt(1)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// installTrChar: install a symbol from a CHARSXP.
pub(crate) fn installTrChar(input: SEXP) -> SEXP {
    if isNull(input) {
        return R_NilValue();
    }
    let text = owned(input)
        .try_as_string()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let ctext = std::ffi::CString::new(text).unwrap_or_default();
    install(&ctext)
}

pub(crate) fn ScalarInteger(x: c_int) -> SEXP {
    factory()
        .allocate(|arena| {
            crate::sexp::builder::scalar_integer_in(arena, x).map(|value| value.as_raw())
        })
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        .as_raw()
}

pub(crate) fn ScalarReal(x: c_double) -> SEXP {
    factory()
        .allocate(|arena| {
            crate::sexp::builder::scalar_real_in(arena, x).map(|value| value.as_raw())
        })
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        .as_raw()
}

/// ScalarString: create a length-1 character vector from a CHARSXP.
pub(crate) fn ScalarString(x: SEXP) -> SEXP {
    let fail = |error: crate::sexp::object::SexpError| -> ! {
        crate::sexp::context::r_error(error.to_string())
    };
    let s = factory()
        .wrap(alloc_vector(STRSXP, 1))
        .unwrap_or_else(|e| fail(e));
    let mut s = crate::sexp::object::SexpMut::try_from_checked(s).unwrap_or_else(|e| fail(e));
    s.try_set_string_elt(0, owned(x))
        .unwrap_or_else(|e| fail(e));
    s.freeze().as_raw()
}

/// list2: create a 2-element list.
pub(crate) fn list2(a: SEXP, b: SEXP) -> SEXP {
    let fail = |error: crate::sexp::object::SexpError| -> ! {
        crate::sexp::context::r_error(error.to_string())
    };
    let factory = factory();
    let nil = factory.nil();
    let cdr = factory
        .pairlist_cell(&owned(b), &nil, &nil)
        .unwrap_or_else(|e| fail(e));
    factory
        .pairlist_cell(&owned(a), &cdr, &nil)
        .unwrap_or_else(|e| fail(e))
        .as_raw()
}

/// nthcdr: walk n steps down a pairlist.
pub(crate) fn nthcdr(x: SEXP, mut n: c_int) -> SEXP {
    let mut result = x;
    while n > 0 && !isNull(result) {
        result = CDR(result);
        n -= 1;
    }
    result
}

/// checkArity: check function arity.
#[inline]
pub(crate) fn checkArity(op: SEXP, args: SEXP) {
    crate::mainutils::relop::checkArity(op, args)
}

/// GetArrayDimnames: get dimnames attribute.
pub(crate) fn GetArrayDimnames(x: SEXP) -> SEXP {
    getAttrib(x, sym_DimNames())
}

/// fixSubset3Args: prepare args for $ assignment.
pub(crate) fn fixSubset3Args(call: SEXP, args: SEXP, env: SEXP, nlist: *mut SEXP) -> SEXP {
    unsafe { crate::mainutils::subset::fixSubset3Args(call, args, env, nlist) }
}

/// NonNullStringMatch: match non-null strings.
pub(crate) fn NonNullStringMatch(s: SEXP, t: SEXP) -> c_int {
    crate::mainutils::match_mod::NonNullStringMatch(s, t)
}

/// R_CurrentExpression: stub returning nil.
pub(crate) fn R_CurrentExpression() -> SEXP {
    R_NilValue()
}

/// NA_STRING: get the NA string.
pub(crate) fn NA_STRING() -> SEXP {
    crate::sexp::globals::R_NaString()
}

/// R_BlankString: get the blank string.
pub(crate) fn R_BlankString() -> SEXP {
    factory()
        .character("")
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        .as_raw()
}

/// DispatchOrEval: dispatch or evaluate a call.
pub(crate) fn DispatchOrEval(
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

/// GNU `R_getS4DataSlot`: restore the basic S3 data part, or extract an
/// object wrapper's `.Data`/`.xData` attribute.
pub fn R_getS4DataSlot(x: SEXP, type_: c_int) -> SEXP {
    unsafe {
        if x.is_null() {
            return x;
        }
        let owner = crate::sexp::owner::OwnerToken::current().expect("active S4 owner");
        let pin = owner.pin().expect("live S4 owner");
        let factory = owner.node_factory();
        let _source_root = factory.wrap(x).expect("S4 input belongs to active owner");
        if isNull(x) {
            return x;
        }
        let check_owner = || {
            if let Some(pin) = &pin {
                pin.require_live().expect("live S4 conversion owner");
            }
            factory
                .require_active()
                .expect("original S4 conversion owner");
        };
        let _source = protect(x);
        let s3_symbol = Rf_install(c".S3Class".as_ptr());
        check_owner();
        let s3_class = getAttrib(x, s3_symbol);
        check_owner();
        let _class = protect(s3_class);
        let value = if TYPEOF(x) != SEXPTYPE::OBJSXP || type_ == OBJSXP {
            if isNull(s3_class) && type_ == OBJSXP {
                return R_NilValue();
            }
            let result = crate::mainutils::duplicate::shallow_duplicate_if_shared(x);
            check_owner();
            let _result_root = factory
                .wrap(result)
                .expect("S4 result belongs to original owner");
            let _result = protect(result);
            let class_symbol = Rf_install(c"class".as_ptr());
            check_owner();
            setAttrib(result, class_symbol, s3_class);
            check_owner();
            if !isNull(s3_class) {
                setAttrib(result, s3_symbol, R_NilValue());
                check_owner();
            }
            UNSET_S4_OBJECT(result);
            if type_ == OBJSXP {
                return result;
            }
            result
        } else {
            let symbol = Rf_install(c".Data".as_ptr());
            check_owner();
            let value = getAttrib(x, symbol);
            check_owner();
            value
        };
        let _value = protect(value);
        let value = if isNull(value) {
            let symbol = Rf_install(c".xData".as_ptr());
            check_owner();
            let value = getAttrib(x, symbol);
            check_owner();
            value
        } else {
            value
        };
        let _value_root = factory
            .wrap(value)
            .expect("S4 data belongs to original owner");
        if !isNull(value) && (type_ == ANYSXP || TYPEOF(value) == type_) {
            value
        } else {
            R_NilValue()
        }
    }
}
