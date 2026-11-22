#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/subset.c
//!
//! Vector and List Subsetting -- the three kinds of subscripting [, [[, and $.
//!
//! Ported public functions:
//!   do_subset()         -- the `[` subset operator
//!   do_subset_dflt()    -- default method for `[`
//!   do_subset2()        -- the `[[` subset operator
//!   do_subset2_dflt()   -- default method for `[[`
//!   dispatch_subset2()  -- dispatch [[ on objects
//!   do_subset3()        -- the `$` subset operator
//!   R_subset3_dflt()    -- default method for `$`
//!   fixSubset3Args()    -- prepare arguments for `$`
//!   ExtractSubset()     -- extract subset elements by index
//!
//! Ported static helper functions (module-private):
//!   VectorSubset(), MatrixSubset(), ArraySubset()
//!   ExtractArg(), ExtractDropArg(), ExtractExactArg()
//!   scalarIndex(), findASubIndex()
//!   errorcallNotSubsettable(),
//!   errorcallOutOfBounds(), errorcallOutOfBoundsSEXP()
//!   VECTOR_ELT_FIX_NAMED(), R_DispatchOrEvalSP()
//!   pstrmatch()

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::mainutils::subscript::{
    get1index, int_arraySubscript, make_subscript, mat2indsub, strmat2intmat,
};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::envir::R_findVarInFrame;
use crate::sexp::ffi::{FALSE, NA_INTEGER, NA_LOGICAL, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_MissingArg, R_NilValue, R_UnboundValue};
use crate::sexp::memory_ext::allocLang;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's NA_REAL sentinel (specific NaN bit pattern).
const NA_REAL: f64 = crate::sexp::ffi::NA_REAL;

/// Maximum value for R_xlen_t (for overflow checking).
const R_XLEN_T_MAX: R_xlen_t = i64::MAX;

// ---------------------------------------------------------------------------
// Local symbol install helpers (for attribute names used in subsetting)
// ---------------------------------------------------------------------------

/// Get the "dim" symbol (install if needed).
#[inline]
fn sym_Dim() -> SEXP {
    crate::sexp::symbol::install_str("dim")
}

/// Get the "names" symbol.
#[inline]
fn sym_Names() -> SEXP {
    crate::sexp::symbol::install_str("names")
}

/// Get the "dimnames" symbol.
#[inline]
fn sym_DimNames() -> SEXP {
    crate::sexp::symbol::install_str("dimnames")
}

/// Get the "class" symbol.
#[inline]
fn sym_Class() -> SEXP {
    crate::sexp::symbol::install_str("class")
}

/// Get the "srcref" symbol.
#[inline]
fn sym_Srcref() -> SEXP {
    crate::sexp::symbol::install_str("srcref")
}

/// Get the "tsp" symbol.
#[inline]
fn sym_Tsp() -> SEXP {
    crate::sexp::symbol::install_str("tsp")
}

/// Get the "tzone" symbol.
#[inline]
fn sym_Tzone() -> SEXP {
    crate::sexp::symbol::install_str("tzone")
}

/// Get the "drop" symbol.
#[inline]
fn sym_Drop() -> SEXP {
    crate::sexp::symbol::install_str("drop")
}

/// Get the "exact" symbol.
#[inline]
fn sym_Exact() -> SEXP {
    crate::sexp::symbol::install_str("exact")
}

/// Get the "contrasts" symbol.
#[inline]
fn sym_Contrasts() -> SEXP {
    crate::sexp::symbol::install_str("contrasts")
}

/// Get the "row.names" symbol.
#[inline]
fn sym_RowNames() -> SEXP {
    crate::sexp::symbol::install_str("row.names")
}

pub(crate) fn is_data_frame(x: SEXP) -> bool {
    let class = getAttrib(x, sym_Class());
    if isNull(class) || TYPEOF(class) != SEXPTYPE::STRSXP {
        return false;
    }
    for i in 0..XLENGTH(class) {
        let elt = STRING_ELT(class, i);
        if isNull(elt) {
            continue;
        }
        if char_bytes(elt) == b"data.frame" {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Local type-checking helpers
// ---------------------------------------------------------------------------

/// Check if x is NILSXP.
#[inline]
fn isNull(x: SEXP) -> bool {
    x.is_null() || x == R_NilValue()
}

/// Check if x is a pairlist (LISTSXP or LANGSXP but not DOTSXP).
#[inline]
fn isPairList(x: SEXP) -> bool {
    let t = TYPEOF(x);
    t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP
}

/// Check if x is NILSXP, LISTSXP, or LANGSXP (for $ operator).
#[inline]
fn isPairListOrNil(x: SEXP) -> bool {
    let t = TYPEOF(x);
    t == SEXPTYPE::NILSXP || t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP
}

/// Check if x is a vector list (VECSXP or EXPRSXP).
#[inline]
fn isVectorList(x: SEXP) -> bool {
    let t = TYPEOF(x);
    t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP
}

/// Check if x is an expression (EXPRSXP).
#[inline]
fn isExpression(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::EXPRSXP
}

/// Check if x is a language object (LANGSXP).
#[inline]
fn isLanguage(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::LANGSXP
}

/// Names of a pairlist/language object from cell tags, like names().
fn pairlist_tag_names(x: SEXP) -> SEXP {
    let n = xlength(x);
    if n <= 0 {
        return R_NilValue();
    }
    let names = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    if names.is_null() {
        return R_NilValue();
    }
    let _names_guard = protect(names);
    let mut cell = x;
    let mut i: R_xlen_t = 0;
    let mut any = false;
    while !cell.is_null() && cell != R_NilValue() && i < n {
        let tag = TAG(cell);
        if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
            let printname = PRINTNAME(tag);
            if !printname.is_null() && printname != R_NilValue() {
                SET_STRING_ELT(names, i, printname);
                any = true;
            } else {
                SET_STRING_ELT(names, i, mk_char(""));
            }
        } else {
            SET_STRING_ELT(names, i, mk_char(""));
        }
        i += 1;
        cell = CDR(cell);
    }
    if any { names } else { R_NilValue() }
}

/// Check if x is an environment.
#[inline]
fn isEnvironment(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::ENVSXP
}

/// Check if x is a vector type (any atomic or generic vector).
#[inline]
fn isVector(x: SEXP) -> bool {
    crate::sexp::object::raw_is_vector(x)
}

/// Check if x is an atomic vector.
#[inline]
fn isVectorAtomic(x: SEXP) -> bool {
    crate::sexp::object::raw_is_atomic_vector(x)
}

/// Check if x is a symbol.
#[inline]
fn isSymbol(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::SYMSXP
}

/// Check if x is a string vector.
#[inline]
fn isString(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::STRSXP
}

/// Check if x is a promise.
#[inline]
fn isPromise(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::PROMSXP
}

/// Check if x has the OBJECT bit set.
#[inline]
fn isObject(x: SEXP) -> bool {
    OBJECT(x) != 0
}

/// Check if x is a matrix (has "dim" attribute of length 2).
fn isMatrix(x: SEXP) -> bool {
    let dim = getAttrib(x, sym_Dim());
    !isNull(dim) && LENGTH(dim) == 2
}

/// Check if x is an array (has "dim" attribute of length >= 1).
fn isArray(x: SEXP) -> bool {
    let dim = getAttrib(x, sym_Dim());
    !isNull(dim) && LENGTH(dim) >= 1
}

/// Get the number of rows of x (assumes matrix).
fn nrows(x: SEXP) -> c_int {
    let dim = getAttrib(x, sym_Dim());
    if isNull(dim) {
        return LENGTH(x);
    }
    INTEGER_ELT(dim, 0)
}

/// Get the number of columns of x (assumes matrix).
fn ncols(x: SEXP) -> c_int {
    let dim = getAttrib(x, sym_Dim());
    if isNull(dim) {
        return 1;
    }
    if LENGTH(dim) >= 2 {
        INTEGER_ELT(dim, 1)
    } else {
        1
    }
}

/// Get extended length of x.
#[inline]
fn xlength(x: SEXP) -> R_xlen_t {
    if isPairList(x) || isLanguage(x) {
        crate::sexp::constructors::Rf_length(x) as R_xlen_t
    } else {
        XLENGTH(x)
    }
}

/// Get the length as c_int.
#[inline]
fn length_int(x: SEXP) -> c_int {
    crate::sexp::constructors::Rf_length(x)
}

// ---------------------------------------------------------------------------
// Local NAMED helpers
// ---------------------------------------------------------------------------

/// Raise the NAMED level of x to at least v (like RAISE_NAMED in C).
#[inline]
fn RAISE_NAMED(x: SEXP, v: c_int) {
    if !x.is_null() {
        let cur = NAMED(x);
        if v > cur {
            SET_NAMED(x, v);
        }
    }
}

/// Set NAMED to NAMEDMAX (2).
#[inline]
fn ENSURE_NAMEDMAX(x: SEXP) {
    if !x.is_null() {
        SET_NAMED(x, 2);
    }
}

// ---------------------------------------------------------------------------
// Local getAttrib / setAttrib wrappers (delegate to eval/attrib_core)
// ---------------------------------------------------------------------------

/// Get attribute -- delegates to the eval/attrib_core implementation.
use crate::sexp::attrib_core::getAttrib;

/// Set attribute -- delegates to the eval/attrib_core implementation.
fn setAttrib(x: SEXP, which: SEXP, value: SEXP) {
    crate::eval::attrib_core::setAttrib(x, which, value)
}

/// Raw CHAR bytes of a CHARSXP (GNU `CHAR(x)`).
fn char_bytes(x: SEXP) -> Vec<u8> {
    crate::mainutils::printutils::charsxp_bytes(x)
}

/// Checked STRSXP(1) for `text` (`Rf_mkString`); an interior NUL yields "".
fn mk_string(text: &str) -> SEXP {
    let text = if text.contains('\0') { "" } else { text };
    crate::mainutils::essentials::active_owner()
        .node_factory()
        .strings(&[text])
        .map(|value| value.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Lossy UTF-8 text of a CHARSXP's CHAR bytes.
fn char_lossy(x: SEXP) -> String {
    String::from_utf8_lossy(&char_bytes(x)).into_owned()
}

/// UTF-8 text of a CHARSXP's CHAR bytes, empty when invalid.
fn char_utf8(x: SEXP) -> String {
    String::from_utf8(char_bytes(x)).unwrap_or_default()
}

/// Checked CHARSXP for `text` on the active session heap. Like
/// `Rf_mkChar(CString::new(text).unwrap_or_default())`, an interior NUL
/// yields the empty string.
fn mk_char(text: &str) -> SEXP {
    let text = if text.contains('\0') { "" } else { text };
    crate::mainutils::essentials::active_owner()
        .node_factory()
        .character(text)
        .map(|value| value.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

// ---------------------------------------------------------------------------
// Internal error helpers (module-private)
// ---------------------------------------------------------------------------

/// Report "not subsettable" error.
fn errorcallNotSubsettable(_x: SEXP, _call: SEXP) {
    std::panic::panic_any(RError {
        message: "object of type is not subsettable".to_string(),
    });
}

thread_local! {
    static OOB_SUBSCRIPT: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

pub(crate) fn take_oob_subscript() -> i32 {
    OOB_SUBSCRIPT.with(|cell| cell.replace(0))
}

pub(crate) fn note_oob_subscript(one_based: i32) {
    OOB_SUBSCRIPT.with(|cell| cell.set(one_based));
}
/// Report out-of-bounds error (integer index). `subscript` is 0-based.
fn errorcallOutOfBounds(_x: SEXP, subscript: c_int, _index: R_xlen_t, _call: SEXP) {
    let _ = (_x, _index, _call);
    OOB_SUBSCRIPT.with(|cell| cell.set(subscript + 1));
    std::panic::panic_any(RError {
        message: "subscript out of bounds".to_string(),
    });
}

/// Report out-of-bounds error (SEXP index).
fn errorcallOutOfBoundsSEXP(_x: SEXP, _subscript: c_int, _sindex: SEXP, _call: SEXP) {
    let _ = (_x, _subscript, _sindex, _call);
    std::panic::panic_any(RError {
        message: "subscript out of bounds".to_string(),
    });
}

/// Report an error with a message (like errorcall in C).
fn errorcall(call: SEXP, msg: &str) {
    let _ = call;
    std::panic::panic_any(RError {
        message: msg.to_string(),
    });
}

/// Report an error (like error() in C).
fn r_error(msg: &str) {
    std::panic::panic_any(RError {
        message: msg.to_string(),
    });
}

// ---------------------------------------------------------------------------
// asLogical -- local implementation
// ---------------------------------------------------------------------------

/// Convert SEXP to logical value (NA -> NA_LOGICAL, length-0 -> NA_LOGICAL).
fn asLogical(x: SEXP) -> c_int {
    if isNull(x) {
        return NA_LOGICAL;
    }
    let len = LENGTH(x);
    if len == 0 {
        return NA_LOGICAL;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LGLSXP {
        LOGICAL_ELT(x, 0)
    } else if t == SEXPTYPE::INTSXP {
        let v = INTEGER_ELT(x, 0);
        if v == NA_INTEGER {
            NA_LOGICAL
        } else {
            if v != 0 { TRUE } else { FALSE }
        }
    } else if t == SEXPTYPE::REALSXP {
        let v = REAL_ELT(x, 0);
        if v.is_nan() {
            NA_LOGICAL
        } else {
            if v != 0.0 { TRUE } else { FALSE }
        }
    } else {
        NA_LOGICAL
    }
}

// ---------------------------------------------------------------------------
// list2 -- local helper to create a 2-element list
// ---------------------------------------------------------------------------

/// Create a 2-element list: list2(a, b).
fn list2(a: SEXP, b: SEXP) -> SEXP {
    let cdr = Rf_cons(b, R_NilValue());
    Rf_cons(a, cdr)
}

// ---------------------------------------------------------------------------
// nthcdr -- walk n steps down a pairlist
// ---------------------------------------------------------------------------

/// Return the n-th CDR of x (like nthcdr in C).
fn nthcdr(x: SEXP, mut n: c_int) -> SEXP {
    {
        let mut result = x;
        while n > 0 && !isNull(result) {
            result = CDR(result);
            n -= 1;
        }
        result
    }
}

// ---------------------------------------------------------------------------
// DropDims -- drop dimensions with extent 1
// ---------------------------------------------------------------------------

/// Port of upstream `DropDims()` from subset.c: drop all extent-1
/// dimensions. One surviving dimension becomes a plain vector carrying that
/// dimension's dimnames slot as vector names (`m[,1]` → named vector); no
/// surviving dimension becomes a scalar. Attribute-only mutation is safe
/// because callers pass freshly allocated subset results (dropping a
/// singleton dimension never reorders column-major data).
fn DropDims(x: SEXP) -> SEXP {
    if isNull(x) {
        return x;
    }
    let dim = getAttrib(x, sym_Dim());
    if isNull(dim) {
        return x;
    }
    let ndim = LENGTH(dim);
    if ndim < 2 {
        return x;
    }

    let mut kept: Vec<c_int> = Vec::with_capacity(ndim as usize);
    let mut kept_idx: Vec<c_int> = Vec::with_capacity(ndim as usize);
    for i in 0..ndim {
        let d = INTEGER_ELT(dim, i);
        if d != 1 {
            kept.push(d);
            kept_idx.push(i);
        }
    }

    let dimnames = getAttrib(x, sym_DimNames());
    let _dimnames_guard = protect(dimnames);
    let slot = |i: c_int| -> SEXP {
        if dimnames.is_null()
            || dimnames == R_NilValue()
            || TYPEOF(dimnames) != SEXPTYPE::VECSXP
            || i >= LENGTH(dimnames)
        {
            R_NilValue()
        } else {
            VECTOR_ELT(dimnames, i as R_xlen_t)
        }
    };

    if kept.is_empty() {
        setAttrib(x, sym_Dim(), R_NilValue());
        setAttrib(x, sym_DimNames(), R_NilValue());
        let mut chosen = R_NilValue();
        let mut found = 0;
        for i in 0..ndim {
            let candidate = slot(i);
            if !isNull(candidate) && candidate != R_NilValue() {
                found += 1;
                chosen = candidate;
            }
        }
        if found == 1 {
            setAttrib(x, sym_Names(), chosen);
        }
        return x;
    }
    if kept.len() == 1 {
        // Single surviving dimension: plain vector, with that
        // dimension's names carried over as vector names.
        let names = slot(kept_idx[0]);
        let _names_guard = protect(names);
        setAttrib(x, sym_Dim(), R_NilValue());
        setAttrib(x, sym_DimNames(), R_NilValue());
        if !isNull(names) && names != R_MissingArg() {
            setAttrib(x, sym_Names(), names);
        }
        return x;
    }

    let new_dim = Rf_allocVector3(SEXPTYPE::INTSXP, kept.len() as R_xlen_t);
    let _new_dim_guard = protect(new_dim);
    for (j, d) in kept.iter().enumerate() {
        SET_INTEGER_ELT(new_dim, j as c_int, *d);
    }
    let dim_names = getAttrib(dim, sym_Names());
    if !isNull(dim_names) && TYPEOF(dim_names) == SEXPTYPE::STRSXP {
        let new_names = Rf_allocVector3(SEXPTYPE::STRSXP, kept.len() as R_xlen_t);
        let _new_names = protect(new_names);
        for (j, i) in kept_idx.iter().enumerate() {
            SET_STRING_ELT(
                new_names,
                j as R_xlen_t,
                STRING_ELT(dim_names, *i as R_xlen_t),
            );
        }
        setAttrib(new_dim, sym_Names(), new_names);
    }
    setAttrib(x, sym_Dim(), new_dim);

    if !dimnames.is_null() && dimnames != R_NilValue() {
        let new_dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, kept.len() as R_xlen_t);
        let _new_dimnames_guard = protect(new_dimnames);
        let mut any = false;
        for (j, i) in kept_idx.iter().enumerate() {
            let slot_i = slot(*i);
            if !slot_i.is_null() && slot_i != R_NilValue() {
                any = true;
            }
            SET_VECTOR_ELT(new_dimnames, j as R_xlen_t, slot_i);
        }
        if any {
            let dimnames_names = getAttrib(dimnames, sym_Names());
            let _dimnames_names_guard = protect(dimnames_names);
            if !isNull(dimnames_names) && TYPEOF(dimnames_names) == SEXPTYPE::STRSXP {
                let new_names = Rf_allocVector3(SEXPTYPE::STRSXP, kept.len() as R_xlen_t);
                let _new_names = protect(new_names);
                for (j, i) in kept_idx.iter().enumerate() {
                    if (*i as R_xlen_t) < XLENGTH(dimnames_names) {
                        SET_STRING_ELT(
                            new_names,
                            j as R_xlen_t,
                            STRING_ELT(dimnames_names, *i as R_xlen_t),
                        );
                    }
                }
                setAttrib(new_dimnames, sym_Names(), new_names);
            }
            setAttrib(x, sym_DimNames(), new_dimnames);
        } else {
            setAttrib(x, sym_DimNames(), R_NilValue());
        }
    }
    x
}

// ---------------------------------------------------------------------------
// GetRowNames -- extract row.names from dimnames
// ---------------------------------------------------------------------------

/// Extract row names from a dimnames list.
fn GetRowNames(dimnames: SEXP) -> SEXP {
    if isNull(dimnames) {
        return R_NilValue();
    }
    if TYPEOF(dimnames) == SEXPTYPE::VECSXP {
        VECTOR_ELT(dimnames, 0)
    } else {
        CAR(dimnames)
    }
}

// ---------------------------------------------------------------------------
// GetArrayDimnames -- get dimnames attribute
// ---------------------------------------------------------------------------

/// Get the dimnames attribute of x.
fn GetArrayDimnames(x: SEXP) -> SEXP {
    getAttrib(x, sym_DimNames())
}

// ---------------------------------------------------------------------------
// installTrChar -- install a symbol from a CHARSXP
// ---------------------------------------------------------------------------

/// Install a symbol from a CHARSXP (translate character).
pub(crate) fn installTrChar(input: SEXP) -> SEXP {
    if isNull(input) {
        return R_NilValue();
    }
    match std::str::from_utf8(&char_bytes(input)) {
        Ok(name) => crate::sexp::symbol::install_str(name),
        Err(_) => ptr::null_mut(),
    }
}

// ---------------------------------------------------------------------------
// translateChar -- get the UTF-8 string from a CHARSXP
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// checkArity -- local stub (no-op for now)
// ---------------------------------------------------------------------------

/// Check that the number of arguments matches the function arity.
fn checkArity(op: SEXP, args: SEXP) {
    crate::mainutils::relop::checkArity(op, args)
}

// ---------------------------------------------------------------------------
// R_FINITE -- check if a double is finite
// ---------------------------------------------------------------------------

/// Check if a double value is finite (not NaN or Inf).
#[inline]
fn R_FINITE(x: f64) -> bool {
    x.is_finite()
}

// ---------------------------------------------------------------------------
// pmatch enum and pstrmatch helper
// ---------------------------------------------------------------------------

/// Partial matching result.
#[derive(Debug, Clone, Copy, PartialEq)]
enum pmatch {
    NO_MATCH,
    EXACT_MATCH,
    PARTIAL_MATCH,
}

/// Partially match a target (symbol or CHARSXP) against an input string.
///
/// Returns EXACT_MATCH if strings are identical, PARTIAL_MATCH if target
/// starts with the input prefix, NO_MATCH otherwise.
fn pstrmatch(target: SEXP, input: SEXP, slen: usize) -> pmatch {
    if isNull(target) {
        return pmatch::NO_MATCH;
    }

    let st = match TYPEOF(target) {
        t if t == SEXPTYPE::SYMSXP => PRINTNAME(target),
        t if t == SEXPTYPE::CHARSXP => target,
        _ => return pmatch::NO_MATCH,
    };
    if st.is_null() || input.is_null() {
        return pmatch::NO_MATCH;
    }

    let st_owned = char_bytes(st);
    let si_owned = char_bytes(input);
    let st_bytes = st_owned.as_slice();
    let si_bytes = si_owned.as_slice();

    // input must be non-empty
    if si_bytes.is_empty() {
        return pmatch::NO_MATCH;
    }

    // target must start with input
    if slen <= st_bytes.len() && st_bytes[..slen] == *si_bytes {
        if st_bytes.len() == slen {
            pmatch::EXACT_MATCH
        } else {
            pmatch::PARTIAL_MATCH
        }
    } else {
        pmatch::NO_MATCH
    }
}

// ---------------------------------------------------------------------------
// VECTOR_ELT_FIX_NAMED -- ensure NAMEDMAX on extracted list elements
// ---------------------------------------------------------------------------

/// If RHS (container or element) has NAMED > 0, set NAMED = NAMEDMAX.
fn VECTOR_ELT_FIX_NAMED(y: SEXP, i: R_xlen_t) -> SEXP {
    let val = VECTOR_ELT(y, i);
    if NAMED(y) != 0 || NAMED(val) != 0 {
        ENSURE_NAMEDMAX(val);
    }
    val
}

mod bracket;
mod bracket2;
mod dollar;
mod extract;
mod subassign;

pub use bracket::*;
pub use bracket2::*;
pub use dollar::*;
pub use extract::*;
pub use subassign::*;

#[cfg(test)]
mod tests;
