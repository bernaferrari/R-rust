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
//!   errorcallNotSubsettable(), errorcallMissingSubs(),
//!   errorcallOutOfBounds(), errorcallOutOfBoundsSEXP()
//!   VECTOR_ELT_FIX_NAMED(), R_DispatchOrEvalSP()
//!   pstrmatch()

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::eval::eval::Rf_eval;
use crate::mainutils::subscript::{get1index, int_arraySubscript, makeSubscript};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::envir::R_findVarInFrame;
use crate::sexp::ffi::{FALSE, NA_INTEGER, NA_LOGICAL, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue};
use crate::sexp::memory_ext::allocLang;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

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

/// Get the "srcref" symbol.
#[inline]
unsafe fn sym_Srcref() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new("srcref")
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// Get the "tsp" symbol.
#[inline]
unsafe fn sym_Tsp() -> SEXP {
    unsafe { Rf_install(std::ffi::CString::new("tsp").unwrap_or_default().as_ptr()) }
}

/// Get the "drop" symbol.
#[inline]
unsafe fn sym_Drop() -> SEXP {
    unsafe { Rf_install(std::ffi::CString::new("drop").unwrap_or_default().as_ptr()) }
}

/// Get the "exact" symbol.
#[inline]
unsafe fn sym_Exact() -> SEXP {
    unsafe { Rf_install(std::ffi::CString::new("exact").unwrap_or_default().as_ptr()) }
}

/// Get the "row.names" symbol.
#[inline]
unsafe fn sym_RowNames() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new("row.names")
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

// ---------------------------------------------------------------------------
// Local type-checking helpers
// ---------------------------------------------------------------------------

/// Check if x is NILSXP.
#[inline]
unsafe fn isNull(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() }
}

/// Check if x is a pairlist (LISTSXP or LANGSXP but not DOTSXP).
#[inline]
unsafe fn isPairList(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP
    }
}

/// Check if x is NILSXP, LISTSXP, or LANGSXP (for $ operator).
#[inline]
unsafe fn isPairListOrNil(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::NILSXP || t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP
    }
}

/// Check if x is a vector list (VECSXP or EXPRSXP).
#[inline]
unsafe fn isVectorList(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP
    }
}

/// Check if x is an expression (EXPRSXP).
#[inline]
unsafe fn isExpression(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::EXPRSXP }
}

/// Check if x is a language object (LANGSXP).
#[inline]
unsafe fn isLanguage(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::LANGSXP }
}

/// Check if x is an environment.
#[inline]
unsafe fn isEnvironment(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::ENVSXP }
}

/// Check if x is a vector type (any atomic or generic vector).
#[inline]
unsafe fn isVector(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        matches!(t, 10 | 13 | 14 | 15 | 16 | 19 | 20 | 24)
    }
}

/// Check if x is an atomic vector.
#[inline]
unsafe fn isVectorAtomic(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        matches!(t, 10 | 13 | 14 | 15 | 16 | 24)
    }
}

/// Check if x is a symbol.
#[inline]
unsafe fn isSymbol(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::SYMSXP }
}

/// Check if x is a string vector.
#[inline]
unsafe fn isString(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::STRSXP }
}

/// Check if x is a promise.
#[inline]
unsafe fn isPromise(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::PROMSXP }
}

/// Check if x has the OBJECT bit set.
#[inline]
unsafe fn isObject(x: SEXP) -> bool {
    unsafe { OBJECT(x) != 0 }
}

/// Check if x is a matrix (has "dim" attribute of length 2).
unsafe fn isMatrix(x: SEXP) -> bool {
    unsafe {
        let dim = getAttrib(x, sym_Dim());
        !isNull(dim) && LENGTH(dim) == 2
    }
}

/// Check if x is an array (has "dim" attribute of length >= 1).
unsafe fn isArray(x: SEXP) -> bool {
    unsafe {
        let dim = getAttrib(x, sym_Dim());
        !isNull(dim) && LENGTH(dim) >= 1
    }
}

/// Get the number of rows of x (assumes matrix).
unsafe fn nrows(x: SEXP) -> c_int {
    unsafe {
        let dim = getAttrib(x, sym_Dim());
        if isNull(dim) {
            return LENGTH(x);
        }
        INTEGER_ELT(dim, 0)
    }
}

/// Get the number of columns of x (assumes matrix).
unsafe fn ncols(x: SEXP) -> c_int {
    unsafe {
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
}

/// Get extended length of x.
#[inline]
unsafe fn xlength(x: SEXP) -> R_xlen_t {
    unsafe { XLENGTH(x) }
}

/// Get the length as c_int.
#[inline]
unsafe fn length_int(x: SEXP) -> c_int {
    unsafe { LENGTH(x) }
}

// ---------------------------------------------------------------------------
// Local NAMED helpers
// ---------------------------------------------------------------------------

/// Raise the NAMED level of x to at least v (like RAISE_NAMED in C).
#[inline]
unsafe fn RAISE_NAMED(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() {
            let cur = NAMED(x);
            if v > cur {
                SET_NAMED(x, v);
            }
        }
    }
}

/// Set NAMED to NAMEDMAX (2).
#[inline]
unsafe fn ENSURE_NAMEDMAX(x: SEXP) {
    unsafe {
        if !x.is_null() {
            SET_NAMED(x, 2);
        }
    }
}

// ---------------------------------------------------------------------------
// Local getAttrib / setAttrib wrappers (delegate to eval/attrib_core)
// ---------------------------------------------------------------------------

/// Get attribute -- delegates to the eval/attrib_core implementation.
use crate::eval::attrib_core::getAttrib;

/// Set attribute -- delegates to the eval/attrib_core implementation.
use crate::eval::attrib_core::setAttrib;

// ---------------------------------------------------------------------------
// Internal error helpers (module-private)
// ---------------------------------------------------------------------------

/// Report "not subsettable" error.
unsafe fn errorcallNotSubsettable(_x: SEXP, _call: SEXP) {
    std::panic::panic_any(RError {
        message: "object of type is not subsettable".to_string(),
    });
}

/// Report missing subscript error.
unsafe fn errorcallMissingSubs(_x: SEXP, _call: SEXP) {
    std::panic::panic_any(RError {
        message: "subscript is missing".to_string(),
    });
}

/// Report out-of-bounds error (integer index).
unsafe fn errorcallOutOfBounds(_x: SEXP, _subscript: c_int, _index: R_xlen_t, _call: SEXP) {
    std::panic::panic_any(RError {
        message: format!("subscript out of bounds (dimension {})", _subscript),
    });
}

/// Report out-of-bounds error (SEXP index).
unsafe fn errorcallOutOfBoundsSEXP(_x: SEXP, _subscript: c_int, _sindex: SEXP, _call: SEXP) {
    std::panic::panic_any(RError {
        message: format!("subscript out of bounds (dimension {})", _subscript),
    });
}

/// Report an error with a message (like errorcall in C).
unsafe fn errorcall(call: SEXP, msg: &str) {
    let _ = call;
    std::panic::panic_any(RError {
        message: msg.to_string(),
    });
}

/// Report an error (like error() in C).
unsafe fn r_error(msg: &str) {
    std::panic::panic_any(RError {
        message: msg.to_string(),
    });
}

// ---------------------------------------------------------------------------
// asLogical -- local implementation
// ---------------------------------------------------------------------------

/// Convert SEXP to logical value (NA -> NA_LOGICAL, length-0 -> NA_LOGICAL).
unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// list2 -- local helper to create a 2-element list
// ---------------------------------------------------------------------------

/// Create a 2-element list: list2(a, b).
unsafe fn list2(a: SEXP, b: SEXP) -> SEXP {
    unsafe {
        let cdr = Rf_cons(b, R_NilValue());
        Rf_cons(a, cdr)
    }
}

// ---------------------------------------------------------------------------
// nthcdr -- walk n steps down a pairlist
// ---------------------------------------------------------------------------

/// Return the n-th CDR of x (like nthcdr in C).
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

// ---------------------------------------------------------------------------
// DropDims -- drop dimensions with extent 1
// ---------------------------------------------------------------------------

/// Drop dimensions of length 1 from result.
///
/// Walks the dim attribute and removes any dimension with extent 1,
/// adjusting the result accordingly. If all dimensions are 1, returns
/// a length-0 vector. If no dimensions are 1, returns input unchanged.
unsafe fn DropDims(x: SEXP) -> SEXP {
    unsafe {
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
        // Count dimensions to keep
        let mut keep_count = 0;
        for i in 0..ndim {
            if INTEGER_ELT(dim, i as c_int) != 1 {
                keep_count += 1;
            }
        }
        // If all dims are 1, return length-0 vector
        if keep_count == 0 {
            return Rf_allocVector3(TYPEOF(x), 0);
        }
        // If no dims to drop, return unchanged
        if keep_count == ndim {
            return x;
        }
        // Build new dim
        let new_dim = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP, keep_count as R_xlen_t));
        let mut new_len: R_xlen_t = 1;
        let mut j = 0;
        for i in 0..ndim {
            let d = INTEGER_ELT(dim, i as c_int);
            if d != 1 {
                *INTEGER(new_dim).add(j as usize) = d;
                new_len *= d as R_xlen_t;
                j += 1;
            }
        }
        // Reallocate x with new dimensions
        let new_x = Rf_protect(Rf_allocVector3(TYPEOF(x), new_len));
        // Copy data
        let xtype = TYPEOF(x);
        if xtype == SEXPTYPE::INTSXP || xtype == SEXPTYPE::LGLSXP {
            for i in 0..new_len {
                *INTEGER(new_x).add(i as usize) = *INTEGER(x).add(i as usize);
            }
        } else if xtype == SEXPTYPE::REALSXP {
            for i in 0..new_len {
                *REAL(new_x).add(i as usize) = *REAL(x).add(i as usize);
            }
        } else if xtype == SEXPTYPE::STRSXP {
            for i in 0..new_len {
                SET_STRING_ELT(new_x, i, STRING_ELT(x, i));
            }
        } else if xtype == SEXPTYPE::VECSXP || xtype == SEXPTYPE::EXPRSXP {
            for i in 0..new_len {
                SET_VECTOR_ELT(new_x, i, VECTOR_ELT(x, i));
            }
        }
        // Copy attributes except dim and dimnames
        let src_attr = ATTRIB(x);
        if !isNull(src_attr) {
            let mut new_attr_list = R_NilValue();
            let mut prev: SEXP = R_NilValue();
            let mut a = src_attr;
            while !isNull(a) {
                let tag = TAG(a);
                if tag == sym_Dim() || tag == sym_DimNames() {
                    a = CDR(a);
                    continue;
                }
                let new_pair =
                    Rf_cons(crate::mainutils::duplicate::duplicate(CAR(a)), R_NilValue());
                SETTAG(new_pair, tag);
                if isNull(new_attr_list) {
                    new_attr_list = new_pair;
                } else {
                    SETCDR(prev, new_pair);
                }
                prev = new_pair;
                a = CDR(a);
            }
            // Add new dim
            let dim_pair = Rf_cons(new_dim, R_NilValue());
            SETTAG(dim_pair, sym_Dim());
            SETCDR(prev, dim_pair);
            SET_ATTRIB(new_x, new_attr_list);
        } else {
            let dim_pair = Rf_cons(new_dim, R_NilValue());
            SETTAG(dim_pair, sym_Dim());
            SET_ATTRIB(new_x, dim_pair);
        }
        SET_OBJECT(new_x, OBJECT(x));
        Rf_unprotect(2);
        new_x
    }
}

// ---------------------------------------------------------------------------
// GetRowNames -- extract row.names from dimnames
// ---------------------------------------------------------------------------

/// Extract row names from a dimnames list.
unsafe fn GetRowNames(dimnames: SEXP) -> SEXP {
    unsafe {
        if isNull(dimnames) {
            return R_NilValue();
        }
        if TYPEOF(dimnames) == SEXPTYPE::VECSXP {
            VECTOR_ELT(dimnames, 0)
        } else {
            CAR(dimnames)
        }
    }
}

// ---------------------------------------------------------------------------
// GetArrayDimnames -- get dimnames attribute
// ---------------------------------------------------------------------------

/// Get the dimnames attribute of x.
unsafe fn GetArrayDimnames(x: SEXP) -> SEXP {
    unsafe { getAttrib(x, sym_DimNames()) }
}

// ---------------------------------------------------------------------------
// installTrChar -- install a symbol from a CHARSXP
// ---------------------------------------------------------------------------

/// Install a symbol from a CHARSXP (translate character).
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

// ---------------------------------------------------------------------------
// translateChar -- get the UTF-8 string from a CHARSXP
// ---------------------------------------------------------------------------

unsafe fn translateChar(x: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateChar(x) }
}

// ---------------------------------------------------------------------------
// checkArity -- local stub (no-op for now)
// ---------------------------------------------------------------------------

/// Check that the number of arguments matches the function arity.
unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe { crate::mainutils::relop::checkArity(op, args) }
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
unsafe fn pstrmatch(target: SEXP, input: SEXP, slen: usize) -> pmatch {
    unsafe {
        if isNull(target) {
            return pmatch::NO_MATCH;
        }

        let st: *const c_char = match TYPEOF(target) {
            t if t == SEXPTYPE::SYMSXP => CHAR(PRINTNAME(target)),
            t if t == SEXPTYPE::CHARSXP => translateChar(target),
            _ => return pmatch::NO_MATCH,
        };

        let si = translateChar(input);
        if st.is_null() || si.is_null() {
            return pmatch::NO_MATCH;
        }

        let st_slice = std::ffi::CStr::from_ptr(st);
        let si_slice = std::ffi::CStr::from_ptr(si);

        let st_bytes = st_slice.to_bytes();
        let si_bytes = si_slice.to_bytes();

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
}

// ---------------------------------------------------------------------------
// VECTOR_ELT_FIX_NAMED -- ensure NAMEDMAX on extracted list elements
// ---------------------------------------------------------------------------

/// If RHS (container or element) has NAMED > 0, set NAMED = NAMEDMAX.
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
// ExtractSubset -- extract elements from x according to integer/real subscripts
// ---------------------------------------------------------------------------

/// Extract subset elements from `x` according to subscripts in `indx`.
///
/// This allocates the result and transfers elements from `x` to `result`
/// according to integer or real subscripts in `indx`.
#[allow(clippy::absurd_extreme_comparisons)]
pub unsafe fn ExtractSubset(x: SEXP, indx: SEXP, call: SEXP) -> SEXP {
    unsafe {
        if isNull(x) {
            return x;
        }

        // ALTREP fast path -- skip for now (no ALTREP in Rust port)
        let result: SEXP;
        let n = xlength(indx);
        let nx = xlength(x);
        let mode = TYPEOF(x);

        /* protect allocation in case _ELT operations need to allocate */
        result = Rf_protect(Rf_allocVector3(mode, n));

        if TYPEOF(indx) == SEXPTYPE::INTSXP {
            let pindx = INTEGER(indx);
            for i in 0..n {
                let ii = *pindx.add(i as usize);
                if ii > 0 && (ii as R_xlen_t) <= nx {
                    let ii_0 = (ii - 1) as usize;
                    match mode {
                        t if t == SEXPTYPE::LGLSXP => {
                            *LOGICAL(result).add(i as usize) = LOGICAL_ELT(x, (ii - 1) as c_int);
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            *INTEGER(result).add(i as usize) = INTEGER_ELT(x, (ii - 1) as c_int);
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            *REAL(result).add(i as usize) = REAL_ELT(x, (ii - 1) as c_int);
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            let c = COMPLEX_ELT(x, (ii - 1) as c_int);
                            *COMPLEX(result).add(i as usize) = c;
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            SET_STRING_ELT(result, i, STRING_ELT(x, (ii - 1) as R_xlen_t));
                        }
                        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                            SET_VECTOR_ELT(result, i, VECTOR_ELT_FIX_NAMED(x, ii_0 as R_xlen_t));
                        }
                        t if t == SEXPTYPE::RAWSXP => {
                            *RAW(result).add(i as usize) = RAW_ELT(x, (ii - 1) as c_int);
                        }
                        _ => {
                            let _ = ii_0; // suppress warning
                            errorcallNotSubsettable(x, call);
                        }
                    }
                } else {
                    // Out of bounds or NA
                    match mode {
                        t if t == SEXPTYPE::LGLSXP => {
                            *LOGICAL(result).add(i as usize) = NA_LOGICAL;
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            *INTEGER(result).add(i as usize) = NA_INTEGER;
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            *REAL(result).add(i as usize) = NA_REAL;
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            *COMPLEX(result).add(i as usize) = super::super::sexp::ffi::Rcomplex {
                                r: NA_REAL,
                                i: NA_REAL,
                            };
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            SET_STRING_ELT(result, i, R_NilValue());
                        }
                        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                            SET_VECTOR_ELT(result, i, R_NilValue());
                        }
                        t if t == SEXPTYPE::RAWSXP => {
                            *RAW(result).add(i as usize) = 0;
                        }
                        _ => {} // intentionally unhandled: unsupported SEXPTYPE for NA fill in subset
                    }
                }
            }
        } else {
            // REAL subscript
            let pindx = REAL(indx);
            for i in 0..n {
                let di = *pindx.add(i as usize);
                let ii = (di - 1.0) as R_xlen_t;
                if R_FINITE(di) && ii >= 0 && ii < nx {
                    match mode {
                        t if t == SEXPTYPE::LGLSXP => {
                            *LOGICAL(result).add(i as usize) = LOGICAL_ELT(x, ii as c_int);
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            *INTEGER(result).add(i as usize) = INTEGER_ELT(x, ii as c_int);
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            *REAL(result).add(i as usize) = REAL_ELT(x, ii as c_int);
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            let c = COMPLEX_ELT(x, ii as c_int);
                            *COMPLEX(result).add(i as usize) = c;
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            SET_STRING_ELT(result, i, STRING_ELT(x, ii as R_xlen_t));
                        }
                        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                            SET_VECTOR_ELT(result, i, VECTOR_ELT_FIX_NAMED(x, ii));
                        }
                        t if t == SEXPTYPE::RAWSXP => {
                            *RAW(result).add(i as usize) = RAW_ELT(x, ii as c_int);
                        }
                        _ => {
                            errorcallNotSubsettable(x, call);
                        }
                    }
                } else {
                    // Out of bounds or NA
                    match mode {
                        t if t == SEXPTYPE::LGLSXP => {
                            *LOGICAL(result).add(i as usize) = NA_LOGICAL;
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            *INTEGER(result).add(i as usize) = NA_INTEGER;
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            *REAL(result).add(i as usize) = NA_REAL;
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            *COMPLEX(result).add(i as usize) = super::super::sexp::ffi::Rcomplex {
                                r: NA_REAL,
                                i: NA_REAL,
                            };
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            SET_STRING_ELT(result, i, R_NilValue());
                        }
                        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                            SET_VECTOR_ELT(result, i, R_NilValue());
                        }
                        t if t == SEXPTYPE::RAWSXP => {
                            *RAW(result).add(i as usize) = 0;
                        }
                        _ => {} // intentionally unhandled: unsupported SEXPTYPE for NA fill in subset
                    }
                }
            }
        }

        Rf_unprotect(1); /* result */
        result
    }
}

// ---------------------------------------------------------------------------
// VectorSubset -- single-index subsetting (including 1D arrays)
// ---------------------------------------------------------------------------

/// Subset a vector by a single index. Handles special matrix subscripting
/// when the index has the same number of columns as the dimension of x.
unsafe fn VectorSubset(x: SEXP, s: SEXP, call: SEXP) -> SEXP {
    unsafe {
        if s == R_NilValue() || TYPEOF(s) == SEXPTYPE::SYMSXP {
            // Missing arg check
            let missing_sym = Rf_install(std::ffi::CString::new("").unwrap_or_default().as_ptr());
            if s == R_NilValue() {
                return Rf_protect(crate::mainutils::duplicate::duplicate(x));
            }
        }

        // If s is R_MissingArg, duplicate x
        // R_MissingArg has mark bit set; we check via a special approach
        // If s looks like a symbol with empty name, treat as missing.
        // The simpler approach: check if the symbol's printname is empty.
        let is_missing = if isSymbol(s) {
            let pn = PRINTNAME(s);
            if isNull(pn) {
                false
            } else {
                let c = CHAR(pn);
                !c.is_null() && *c == 0
            }
        } else {
            false
        };

        if is_missing {
            return Rf_protect(crate::mainutils::duplicate::duplicate(x));
        }

        /* Protect s */
        Rf_protect(s);

        /* Check for special matrix subscripting (skip for now -- no strmat2intmat/mat2indsub) */
        /* This optimization requires strmat2intmat and mat2indsub which are not yet ported */

        /* Convert to a vector of integer subscripts in the range 1:length(x). */
        let mut stretch: R_xlen_t = 1;
        let indx = Rf_protect(makeSubscript(x, s, &mut stretch, call));

        /* Allocate the result. */
        let mode = TYPEOF(x);
        let result = Rf_protect(ExtractSubset(x, indx, call));
        if mode == SEXPTYPE::VECSXP || mode == SEXPTYPE::EXPRSXP {
            /* we do not duplicate the values when extracting the subset,
            so to be conservative mark the result as NAMED = NAMEDMAX */
            ENSURE_NAMEDMAX(result);
        }

        if !isNull(result) {
            /* Handle names attribute */
            let mut attrib = getAttrib(x, sym_Names());
            let mut has_names = !isNull(attrib);

            /* Here we might have an array. Use row names if 1D */
            if !has_names && isArray(x) {
                let dimnames = getAttrib(x, sym_DimNames());
                if !isNull(dimnames) && length_int(dimnames) == 1 {
                    attrib = GetRowNames(dimnames);
                    has_names = !isNull(attrib);
                }
            }

            if has_names {
                Rf_protect(attrib);
                let nattrib = Rf_protect(ExtractSubset(attrib, indx, call));
                setAttrib(result, sym_Names(), nattrib);
                Rf_unprotect(2); /* attrib, nattrib */
            }

            /* Handle srcref attribute */
            let srcref = getAttrib(x, sym_Srcref());
            if !isNull(srcref) && TYPEOF(srcref) == SEXPTYPE::VECSXP {
                let nattrib = Rf_protect(ExtractSubset(srcref, indx, call));
                setAttrib(result, sym_Srcref(), nattrib);
                Rf_unprotect(1);
            }
        }

        Rf_unprotect(3); /* s, indx, result */
        result
    }
}

// ---------------------------------------------------------------------------
// MatrixSubset -- 2D subsetting
// ---------------------------------------------------------------------------

/// Subset a matrix by row and column indices.
#[allow(clippy::absurd_extreme_comparisons)]
unsafe fn MatrixSubset(x: SEXP, s: SEXP, call: SEXP, drop: c_int) -> SEXP {
    unsafe {
        let nr = nrows(x);
        let nc = ncols(x);

        /* s is protected on entry */
        let dim = Rf_protect(getAttrib(x, sym_Dim()));

        /* Convert row and column subscripts to integer form */
        let sr = Rf_protect(int_arraySubscript(0, CAR(s), dim, x, call));
        let sc = Rf_protect(int_arraySubscript(1, CADR(s), dim, x, call));
        let nrs = LENGTH(sr);
        let ncs = LENGTH(sc);

        /* Overflow check */
        if (nrs as R_xlen_t) * (ncs as R_xlen_t) > R_XLEN_T_MAX {
            errorcall(call, "dimensions would exceed maximum size of array");
        }

        let psr = INTEGER(sr);
        let psc = INTEGER(sc);
        let result = Rf_protect(Rf_allocVector3(
            TYPEOF(x),
            (nrs as R_xlen_t) * (ncs as R_xlen_t),
        ));

        let mut i: R_xlen_t;
        let mut j: R_xlen_t;
        let mut ii: R_xlen_t;
        let mut jj: R_xlen_t;
        let mut ij: R_xlen_t;
        let mut iijj: R_xlen_t;

        /* Matrix subset loop -- column-major order */
        for j in 0..(ncs as R_xlen_t) {
            jj = *psc.add(j as usize) as R_xlen_t;
            if jj != NA_INTEGER as R_xlen_t {
                if jj < 1 || jj > nr as R_xlen_t {
                    errorcallOutOfBounds(x, 0, jj, call);
                }
                jj -= 1;
            }
            for i_idx in 0..(nrs as R_xlen_t) {
                ii = *psr.add(i_idx as usize) as R_xlen_t;
                if ii != NA_INTEGER as R_xlen_t {
                    if ii < 1 || ii > nr as R_xlen_t {
                        errorcallOutOfBounds(x, 1, ii, call);
                    }
                    ii -= 1;
                }
                ij = i_idx + j * (nrs as R_xlen_t);
                if ii == NA_INTEGER as R_xlen_t || jj == NA_INTEGER as R_xlen_t {
                    /* NA code */
                    match TYPEOF(x) {
                        t if t == SEXPTYPE::LGLSXP => {
                            *LOGICAL(result).add(ij as usize) = NA_LOGICAL;
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            *INTEGER(result).add(ij as usize) = NA_INTEGER;
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            *REAL(result).add(ij as usize) = NA_REAL;
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            *COMPLEX(result).add(ij as usize) = super::super::sexp::ffi::Rcomplex {
                                r: NA_REAL,
                                i: NA_REAL,
                            };
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            SET_STRING_ELT(result, ij, R_NilValue());
                        }
                        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                            SET_VECTOR_ELT(result, ij, R_NilValue());
                        }
                        t if t == SEXPTYPE::RAWSXP => {
                            *RAW(result).add(ij as usize) = 0;
                        }
                        _ => {} // intentionally unhandled: unsupported SEXPTYPE for NA fill in subset
                    }
                } else {
                    iijj = ii + jj * (nr as R_xlen_t);
                    /* Standard code */
                    match TYPEOF(x) {
                        t if t == SEXPTYPE::LGLSXP => {
                            *LOGICAL(result).add(ij as usize) = LOGICAL_ELT(x, iijj as c_int);
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            *INTEGER(result).add(ij as usize) = INTEGER_ELT(x, iijj as c_int);
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            *REAL(result).add(ij as usize) = REAL_ELT(x, iijj as c_int);
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            *COMPLEX(result).add(ij as usize) = COMPLEX_ELT(x, iijj as c_int);
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            SET_STRING_ELT(result, ij, STRING_ELT(x, iijj as R_xlen_t));
                        }
                        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                            SET_VECTOR_ELT(result, ij, VECTOR_ELT_FIX_NAMED(x, iijj));
                        }
                        t if t == SEXPTYPE::RAWSXP => {
                            *RAW(result).add(ij as usize) = RAW_ELT(x, iijj as c_int);
                        }
                        _ => {
                            errorcall(call, "matrix subscripting not handled for this type");
                        }
                    }
                }
            }
        }

        /* Set dim attribute */
        if nrs >= 0 && ncs >= 0 {
            let attr = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP, 2));
            *INTEGER(attr).add(0) = nrs;
            *INTEGER(attr).add(1) = ncs;
            if !isNull(getAttrib(dim, sym_Names())) {
                setAttrib(attr, sym_Names(), getAttrib(dim, sym_Names()));
            }
            setAttrib(result, sym_Dim(), attr);
            Rf_unprotect(1);
        }

        /* Transfer dimnames */
        if nrs >= 0 && ncs >= 0 {
            let dimnames = getAttrib(x, sym_DimNames());
            let dimnamesnames = Rf_protect(getAttrib(dimnames, sym_Names()));
            if !isNull(dimnames) {
                let newdimnames = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP, 2));
                if TYPEOF(dimnames) == SEXPTYPE::VECSXP {
                    SET_VECTOR_ELT(
                        newdimnames,
                        0,
                        ExtractSubset(VECTOR_ELT(dimnames, 0), sr, call),
                    );
                    SET_VECTOR_ELT(
                        newdimnames,
                        1,
                        ExtractSubset(VECTOR_ELT(dimnames, 1), sc, call),
                    );
                } else {
                    SET_VECTOR_ELT(newdimnames, 0, ExtractSubset(CAR(dimnames), sr, call));
                    SET_VECTOR_ELT(newdimnames, 1, ExtractSubset(CADR(dimnames), sc, call));
                }
                setAttrib(newdimnames, sym_Names(), dimnamesnames);
                setAttrib(result, sym_DimNames(), newdimnames);
                Rf_unprotect(1); /* newdimnames */
            }
            Rf_unprotect(1); /* dimnamesnames */
        }

        if drop != 0 {
            DropDims(result);
        }

        Rf_unprotect(4); /* dim, sr, sc, result */
        result
    }
}

// ---------------------------------------------------------------------------
// findASubIndex -- compute linear index for array subscripting
// ---------------------------------------------------------------------------

/// Compute a linear index for array subscripting given per-dimension sub-indices.
unsafe fn findASubIndex(
    k: R_xlen_t,
    subs: *const *const c_int,
    indx: *const c_int,
    _pxdims: *const c_int,
    offset: *const R_xlen_t,
    _call: SEXP,
) -> R_xlen_t {
    unsafe {
        let mut ii: R_xlen_t = 0;
        for j in 0..(k as usize) {
            let jj = *(*subs.add(j)).add(*indx.add(j) as usize);
            if jj == NA_INTEGER {
                return NA_INTEGER as R_xlen_t;
            }
            ii += (jj as R_xlen_t - 1) * *offset.add(j);
        }
        ii
    }
}

// ---------------------------------------------------------------------------
// ArraySubset -- N-D subsetting
// ---------------------------------------------------------------------------

/// Subset an array (N dimensions) by per-dimension indices.
unsafe fn ArraySubset(x: SEXP, s: SEXP, call: SEXP, drop: c_int) -> SEXP {
    unsafe {
        let mode = TYPEOF(x);
        let xdims = Rf_protect(getAttrib(x, sym_Dim()));
        let k = length_int(xdims);
        let pxdims = INTEGER(xdims);

        /* Use Vec for temporary storage (equivalent to R_alloc in C) */
        let mut subs: Vec<*const c_int> = Vec::with_capacity(k as usize);
        let mut indx: Vec<c_int> = vec![0; k as usize];
        let mut bound: Vec<c_int> = Vec::with_capacity(k as usize);
        let mut offset_arr: Vec<R_xlen_t> = vec![0; k as usize];

        /* Construct subscripts and compute bounds */
        let mut n: R_xlen_t = 1;
        let mut r = s;
        for i in 0..(k as usize) {
            let sub = Rf_protect(int_arraySubscript(i as c_int, CAR(r), xdims, x, call));
            SETCAR(r, sub);
            bound.push(LENGTH(sub));
            subs.push(INTEGER(sub));
            n *= bound[i] as R_xlen_t;
            r = CDR(r);
        }

        /* Initialize index array */
        for i in 0..(k as usize) {
            indx[i] = 0;
        }

        /* Compute offsets (column-major) */
        offset_arr[0] = 1;
        for i in 1..(k as usize) {
            offset_arr[i] = offset_arr[i - 1] * (*pxdims.add(i - 1)) as R_xlen_t;
        }

        /* Range check on indices */
        for i in 0..(k as usize) {
            for j in 0..(bound[i] as usize) {
                let jj = *subs[i].add(j);
                if jj > *pxdims.add(i) || (jj < 1 && jj != NA_INTEGER) {
                    errorcallOutOfBounds(x, i as c_int, jj as R_xlen_t, call);
                }
            }
        }

        /* Transfer subset elements */
        let result = Rf_protect(Rf_allocVector3(mode, n));

        for i in 0..n {
            let ii = findASubIndex(
                k as R_xlen_t,
                subs.as_ptr(),
                indx.as_ptr(),
                pxdims,
                offset_arr.as_ptr(),
                call,
            );

            if ii != NA_INTEGER as R_xlen_t {
                match mode {
                    t if t == SEXPTYPE::LGLSXP => {
                        *LOGICAL(result).add(i as usize) = LOGICAL_ELT(x, ii as c_int);
                    }
                    t if t == SEXPTYPE::INTSXP => {
                        *INTEGER(result).add(i as usize) = INTEGER_ELT(x, ii as c_int);
                    }
                    t if t == SEXPTYPE::REALSXP => {
                        *REAL(result).add(i as usize) = REAL_ELT(x, ii as c_int);
                    }
                    t if t == SEXPTYPE::CPLXSXP => {
                        *COMPLEX(result).add(i as usize) = COMPLEX_ELT(x, ii as c_int);
                    }
                    t if t == SEXPTYPE::STRSXP => {
                        SET_STRING_ELT(result, i, STRING_ELT(x, ii as R_xlen_t));
                    }
                    t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                        SET_VECTOR_ELT(result, i, VECTOR_ELT_FIX_NAMED(x, ii));
                    }
                    t if t == SEXPTYPE::RAWSXP => {
                        *RAW(result).add(i as usize) = RAW_ELT(x, ii as c_int);
                    }
                    _ => {
                        errorcall(call, "array subscripting not handled for this type");
                    }
                }
            } else {
                match mode {
                    t if t == SEXPTYPE::LGLSXP => {
                        *LOGICAL(result).add(i as usize) = NA_LOGICAL;
                    }
                    t if t == SEXPTYPE::INTSXP => {
                        *INTEGER(result).add(i as usize) = NA_INTEGER;
                    }
                    t if t == SEXPTYPE::REALSXP => {
                        *REAL(result).add(i as usize) = NA_REAL;
                    }
                    t if t == SEXPTYPE::CPLXSXP => {
                        *COMPLEX(result).add(i as usize) = super::super::sexp::ffi::Rcomplex {
                            r: NA_REAL,
                            i: NA_REAL,
                        };
                    }
                    t if t == SEXPTYPE::STRSXP => {
                        SET_STRING_ELT(result, i, R_NilValue());
                    }
                    t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                        SET_VECTOR_ELT(result, i, R_NilValue());
                    }
                    t if t == SEXPTYPE::RAWSXP => {
                        *RAW(result).add(i as usize) = 0;
                    }
                    _ => {} // intentionally unhandled: unsupported SEXPTYPE for NA fill in subset
                }
            }

            /* Increment multi-dimensional index */
            if n > 1 {
                let mut j = 0;
                let kk = k as usize;
                loop {
                    indx[j] += 1;
                    if indx[j] < bound[j] {
                        break;
                    }
                    indx[j] = 0;
                    j = (j + 1) % kk;
                    if j == 0 {
                        break;
                    }
                }
            }
        }

        /* Set dim attribute */
        let new_dim = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP, k));
        for i in 0..(k as usize) {
            *INTEGER(new_dim).add(i) = bound[i];
        }
        if !isNull(getAttrib(xdims, sym_Names())) {
            setAttrib(new_dim, sym_Names(), getAttrib(xdims, sym_Names()));
        }
        setAttrib(result, sym_Dim(), new_dim);
        Rf_unprotect(1); /* new_dim */

        /* Transfer dimnames */
        let dimnames = getAttrib(x, sym_DimNames());
        let dimnamesnames = Rf_protect(getAttrib(dimnames, sym_Names()));
        if !isNull(dimnames) {
            let new_xdims = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP, k as R_xlen_t));
            let mut jj = 0;
            if TYPEOF(dimnames) == SEXPTYPE::VECSXP {
                let mut rr = s;
                for i in 0..(k as usize) {
                    if bound[i] > 0 {
                        SET_VECTOR_ELT(
                            new_xdims,
                            jj as R_xlen_t,
                            ExtractSubset(VECTOR_ELT(dimnames, i as R_xlen_t), CAR(rr), call),
                        );
                    } else {
                        SET_VECTOR_ELT(new_xdims, jj as R_xlen_t, R_NilValue());
                    }
                    jj += 1;
                    rr = CDR(rr);
                }
            } else {
                let mut p = dimnames;
                let mut q = new_xdims;
                let mut rr = s;
                for i in 0..(k as usize) {
                    SETCAR(q, ExtractSubset(CAR(p), CAR(rr), call));
                    p = CDR(p);
                    q = CDR(q);
                    rr = CDR(rr);
                }
            }
            setAttrib(new_xdims, sym_Names(), dimnamesnames);
            setAttrib(result, sym_DimNames(), new_xdims);
            Rf_unprotect(1); /* new_xdims */
        }

        if drop != 0 {
            DropDims(result);
        }

        Rf_unprotect(2 + k as i32); /* xdims, result, + k for int_arraySubscript results */
        result
    }
}

// ---------------------------------------------------------------------------
// scalarIndex -- fast path for scalar integer/real index
// ---------------------------------------------------------------------------

/// Fast-path extraction of a scalar integer or real index from SEXP.
/// Returns the 1-based index, or -1 for NA / non-scalar / attributed values.
unsafe fn scalarIndex(s: SEXP) -> R_xlen_t {
    unsafe {
        if isNull(s) {
            return -1;
        }
        if !isNull(ATTRIB(s)) {
            return -1;
        }

        let t = TYPEOF(s);
        if t == SEXPTYPE::INTSXP && IS_SCALAR(s, SEXPTYPE::INTSXP.into()) != 0 {
            let ival = SCALAR_IVAL(s);
            if ival != NA_INTEGER {
                ival as R_xlen_t
            } else {
                -1
            }
        } else if t == SEXPTYPE::REALSXP && IS_SCALAR(s, SEXPTYPE::REALSXP.into()) != 0 {
            let rval = SCALAR_DVAL(s);
            if R_FINITE(rval) { rval as R_xlen_t } else { -1 }
        } else {
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// ExtractArg -- find and remove a named argument from an argument list
// ---------------------------------------------------------------------------

/// Search for `arg_sym` in the argument list `args`. If found, remove it
/// from the list and return its value. Otherwise return R_NilValue.
unsafe fn ExtractArg(mut args: SEXP, arg_sym: SEXP) -> SEXP {
    unsafe {
        let mut prev_arg = args;
        let mut arg = args;

        while !isNull(arg) {
            if TAG(arg) == arg_sym {
                let val = CAR(arg);
                if arg == prev_arg {
                    /* found at head of args */
                    args = CDR(args);
                } else {
                    SETCDR(prev_arg, CDR(arg));
                }
                return val;
            }
            prev_arg = arg;
            arg = CDR(arg);
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// ExtractDropArg -- extract the `drop` argument from an argument list
// ---------------------------------------------------------------------------

/// Extract the `drop` argument (if present) from the argument list.
unsafe fn ExtractDropArg(el: SEXP, drop: *mut c_int) {
    unsafe {
        let val = asLogical(ExtractArg(el, sym_Drop()));
        if val == NA_LOGICAL {
            *drop = 1;
        } else {
            *drop = val;
        }
    }
}

// ---------------------------------------------------------------------------
// ExtractExactArg -- extract the `exact` argument from an argument list
// ---------------------------------------------------------------------------

/// Extract the `exact` argument. Returns 0 (not exact), 1 (exact), or -1 (NA).
unsafe fn ExtractExactArg(args: SEXP) -> c_int {
    unsafe {
        let argval = ExtractArg(args, sym_Exact());
        if isNull(argval) {
            return 1;
        } /* Default is true as from R 2.7.0 */
        let exact = asLogical(argval);
        if exact == NA_LOGICAL { -1 } else { exact }
    }
}

// ---------------------------------------------------------------------------
// R_DispatchOrEvalSP -- fast-path dispatch or eval for [ / [[ / $
// ---------------------------------------------------------------------------

/// Version of DispatchOrEval for `[`, `[[`, and `$` that speeds up simple cases.
/// Returns TRUE if dispatch succeeded (answer in *ans), FALSE if fall-through.
///
/// For now this always returns FALSE (no dispatch), which means we always
/// fall through to the default implementations. Full dispatch requires
/// eval(), DispatchOrEval(), etc.
unsafe fn R_DispatchOrEvalSP(
    call: SEXP,
    _op: SEXP,
    _generic: *const c_char,
    args: SEXP,
    rho: SEXP,
    ans: *mut SEXP,
) -> c_int {
    unsafe {
        /* If first arg is not an object, skip dispatch */
        if !isNull(args) && CAR(args) != R_NilValue() {
            /* Check if first arg is an object -- if not, just eval args and return FALSE */
            /* For the fallback path, we need to evaluate args manually */
            /* Since we don't have eval() yet, construct the evaluated list */
            let x = CAR(args);
            if !isObject(x) {
                /* Not an object, no dispatch needed. Build evaluated list. */
                /* For now, just pass args through since we don't have evalListKeepMissing */
                if !ans.is_null() {
                    *ans = args;
                }
                return 0; /* FALSE */
            }
        }
        /* Object path: would dispatch -- but we don't have DispatchOrEval yet */
        if !ans.is_null() {
            *ans = args;
        }
        0 /* FALSE -- fall through to default code */
    }
}

// ---------------------------------------------------------------------------
// do_subset -- the `[` subset operator
// ---------------------------------------------------------------------------

/// The `[` subset operator -- the most general form of subsetting.
pub unsafe fn do_subset(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();

        /* DispatchOrEval internal generic: [ */
        if R_DispatchOrEvalSP(
            call,
            op,
            b"[\0".as_ptr() as *const c_char,
            args,
            env,
            &mut ans,
        ) != 0
        {
            if NAMED(ans) != 0 {
                ENSURE_NAMEDMAX(ans);
            }
            return ans;
        }

        /* Method dispatch has failed, we now run the generic internal code. */
        do_subset_dflt(call, op, ans, env)
    }
}

// ---------------------------------------------------------------------------
// do_subset_dflt -- default method for `[`
// ---------------------------------------------------------------------------

/// Default method for `[`. Handles vector, matrix, and array subsetting.
pub unsafe fn do_subset_dflt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (op, rho);
        Rf_protect(args);

        let mut drop: c_int = 1;
        ExtractDropArg(args, &mut drop);

        let x = CAR(args);

        /* Handle NULL case */
        if isNull(x) {
            Rf_unprotect(1);
            return x;
        }

        let subs = CDR(args);
        let nsubs = length_int(subs);
        let xtype = TYPEOF(x);

        /* Coerce pair-based objects into generic vectors */
        let mut ax = x;
        if isVector(x) || isVectorList(x) {
            Rf_protect(ax);
        } else if isPairList(x) {
            let dim = getAttrib(x, sym_Dim());
            let ndim = length_int(dim);
            if ndim > 1 {
                ax = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP, xlength(x)));
                setAttrib(ax, sym_DimNames(), getAttrib(x, sym_DimNames()));
                setAttrib(ax, sym_Names(), getAttrib(x, sym_DimNames()));
            } else {
                ax = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP, xlength(x)));
                setAttrib(ax, sym_Names(), getAttrib(x, sym_Names()));
            }
            let mut px = x;
            let mut idx: R_xlen_t = 0;
            while !isNull(px) {
                SET_VECTOR_ELT(ax, idx, CAR(px));
                px = CDR(px);
                idx += 1;
            }
        } else {
            errorcallNotSubsettable(x, call);
        }

        /* The actual subsetting code */
        let mut ans: SEXP;
        if nsubs < 2 {
            let dim = getAttrib(x, sym_Dim());
            let ndim = length_int(dim);
            ans = Rf_protect(VectorSubset(
                ax,
                if nsubs == 1 { CAR(subs) } else { R_NilValue() },
                call,
            ));

            /* One-dimensional arrays should keep their dimension unless drop && len == 1 */
            if ndim == 1 {
                let len = length_int(ans);
                if drop == 0 || len > 1 {
                    let nm = Rf_protect(getAttrib(ans, sym_Names()));
                    let attr = Rf_protect(Rf_ScalarInteger(len));
                    if !isNull(getAttrib(dim, sym_Names())) {
                        setAttrib(attr, sym_Names(), getAttrib(dim, sym_Names()));
                    }
                    setAttrib(ans, sym_Dim(), attr);
                    let attrib = getAttrib(x, sym_DimNames());
                    if !isNull(attrib) {
                        let nattrib = Rf_protect(crate::mainutils::duplicate::duplicate(attrib));
                        SET_VECTOR_ELT(nattrib, 0, nm);
                        setAttrib(ans, sym_DimNames(), nattrib);
                        setAttrib(ans, sym_Names(), R_NilValue());
                        Rf_unprotect(1); /* nattrib */
                    }
                    Rf_unprotect(2); /* nm, attr */
                }
            }
        } else {
            if nsubs != length_int(getAttrib(x, sym_Dim())) {
                errorcall(call, "incorrect number of dimensions");
            }
            if nsubs == 2 {
                ans = Rf_protect(MatrixSubset(ax, subs, call, drop));
            } else {
                ans = Rf_protect(ArraySubset(ax, subs, call, drop));
            }
        }

        /* Convert back to LANGSXP if original was a language object */
        if xtype == SEXPTYPE::LANGSXP {
            ax = ans;
            ans = Rf_protect(allocLang(length_int(ax)));
            if length_int(ax) > 0 {
                let mut px = ans;
                let mut idx: c_int = 0;
                while !isNull(px) {
                    SETCAR(px, VECTOR_ELT(ax, idx as R_xlen_t));
                    px = CDR(px);
                    idx += 1;
                }
                setAttrib(ans, sym_Dim(), getAttrib(ax, sym_Dim()));
                setAttrib(ans, sym_DimNames(), getAttrib(ax, sym_DimNames()));
                setAttrib(ans, sym_Names(), getAttrib(ax, sym_Names()));
                RAISE_NAMED(ans, NAMED(ax));
            }
        } else {
            Rf_protect(ans);
        }

        /* Remove erroneous attributes */
        if !isNull(ATTRIB(ans)) {
            setAttrib(ans, sym_Tsp(), R_NilValue());
            setAttrib(ans, sym_Class(), R_NilValue());
        }

        Rf_unprotect(4); /* args, ax, ans, (and one more from the conditional) */
        ans
    }
}

// ---------------------------------------------------------------------------
// do_subset2 -- the `[[` subset operator
// ---------------------------------------------------------------------------

/// The `[[` subset operator. Designed to be fast for extracting single elements.
pub unsafe fn do_subset2(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();

        /* DispatchOrEval internal generic: [[ */
        if R_DispatchOrEvalSP(
            call,
            op,
            b"[[\0".as_ptr() as *const c_char,
            args,
            rho,
            &mut ans,
        ) != 0
        {
            if NAMED(ans) != 0 {
                ENSURE_NAMEDMAX(ans);
            }
            return ans;
        }

        /* Method dispatch has failed. We now run the generic internal code. */
        do_subset2_dflt(call, op, ans, rho)
    }
}

// ---------------------------------------------------------------------------
// do_subset2_dflt -- default method for `[[`
// ---------------------------------------------------------------------------

/// Default method for `[[`. Handles vector indexing, matrix/array indexing,
/// pair-list indexing, and environment subsetting.
pub unsafe fn do_subset2_dflt(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        Rf_protect(args);

        let mut drop: c_int = 1;
        ExtractDropArg(args, &mut drop);
        let exact = ExtractExactArg(args);
        let pok = if exact == -1 {
            exact
        } else {
            if exact == 0 { 1 } else { 0 }
        };

        let x = CAR(args);
        let subs = CDR(args);
        let nsubs = length_int(subs);

        if nsubs == 0 {
            errorcall(call, "no index specified");
        }

        /* Handle NULL case */
        if isNull(x) {
            Rf_unprotect(1);
            if !isNull(subs) && !isNull(CAR(subs)) && isSymbol(CAR(subs)) {
                let pn = PRINTNAME(CAR(subs));
                if !isNull(pn) {
                    let c = CHAR(pn);
                    if !c.is_null() && *c == 0 {
                        errorcallMissingSubs(x, call);
                    }
                }
            }
            return x;
        }

        let dims = getAttrib(x, sym_Dim());
        let ndims = length_int(dims);
        if nsubs > 1 && nsubs != ndims {
            errorcall(call, "incorrect number of subscripts");
        }

        Rf_protect(x);

        /* Environment subsetting */
        if isEnvironment(x) {
            if nsubs != 1 || !isString(CAR(subs)) || length_int(CAR(subs)) != 1 {
                errorcall(call, "wrong arguments for subsetting an environment");
            }
            let sym = installTrChar(STRING_ELT(CAR(subs), 0));
            let mut ans = R_findVarInFrame(x, sym);
            if isPromise(ans) {
                Rf_protect(ans);
                /* Force the promise -- in full R this would eval in rho */
                ans = CAR(ans); /* simplified: just get the value */
                Rf_unprotect(1);
            } else {
                ENSURE_NAMEDMAX(ans);
            }
            Rf_unprotect(2); /* args, x */
            if ans == R_UnboundValue() {
                return R_NilValue();
            }
            if NAMED(ans) != 0 {
                ENSURE_NAMEDMAX(ans);
            }
            return ans;
        }

        /* Check subsettable types */
        if !isVector(x) && !isPairList(x) && !isLanguage(x) {
            errorcallNotSubsettable(x, call);
        }

        let named_x = NAMED(x);

        if nsubs == 1 {
            /* Vector indexing */
            let thesub = CAR(subs);
            let len = length_int(thesub);

            if len > 1 {
                /* Multi-element index -- use vectorIndex to recursively subset */
                /* Simplified: just get the last element */
                let xnames = Rf_protect(getAttrib(x, sym_Names()));
                let offset = get1index(thesub, xnames, xlength(x), pok, (len - 1) as c_int, call);
                Rf_unprotect(1); /* xnames */
                if offset < 0 || offset >= xlength(x) {
                    if offset < 0
                        && (isVectorList(x) || isExpression(x) || isPairList(x) || isLanguage(x))
                    {
                        Rf_unprotect(2); /* args, x */
                        return R_NilValue();
                    } else {
                        errorcallOutOfBoundsSEXP(x, -1, thesub, call);
                    }
                }
                /* For multi-index on non-lists, get the final element */
                if isPairList(x) {
                    let ans = CAR(nthcdr(x, offset as c_int));
                    RAISE_NAMED(ans, named_x);
                    Rf_unprotect(2); /* args, x */
                    return ans;
                } else if isVectorList(x) {
                    let ans = VECTOR_ELT(x, offset);
                    RAISE_NAMED(ans, named_x);
                    Rf_unprotect(2); /* args, x */
                    return ans;
                } else {
                    /* atomic: return single-element vector */
                    let ans = Rf_protect(Rf_allocVector3(TYPEOF(x), 1));
                    match TYPEOF(x) {
                        t if t == SEXPTYPE::LGLSXP => {
                            *LOGICAL(ans).add(0) = LOGICAL_ELT(x, offset as c_int);
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            *INTEGER(ans).add(0) = INTEGER_ELT(x, offset as c_int);
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            *REAL(ans).add(0) = REAL_ELT(x, offset as c_int);
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            *COMPLEX(ans).add(0) = COMPLEX_ELT(x, offset as c_int);
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            SET_STRING_ELT(ans, 0, STRING_ELT(x, offset));
                        }
                        t if t == SEXPTYPE::RAWSXP => {
                            *RAW(ans).add(0) = RAW_ELT(x, offset as c_int);
                        }
                        _ => {} // intentionally unhandled: unsupported SEXPTYPE for scalar subset
                    }
                    Rf_unprotect(3); /* args, x, ans */
                    return ans;
                }
            }

            /* Single-element index */
            let xnames = Rf_protect(getAttrib(x, sym_Names()));
            let offset = get1index(thesub, xnames, xlength(x), pok, -1, call);
            Rf_unprotect(1); /* xnames */

            if offset < 0 || offset >= xlength(x) {
                if offset < 0
                    && (isVectorList(x) || isExpression(x) || isPairList(x) || isLanguage(x))
                {
                    Rf_unprotect(2); /* args, x */
                    return R_NilValue();
                } else {
                    errorcallOutOfBoundsSEXP(x, -1, thesub, call);
                }
            }

            /* Extract the element */
            if isPairList(x) {
                let ans = CAR(nthcdr(x, offset as c_int));
                RAISE_NAMED(ans, named_x);
                Rf_unprotect(2); /* args, x */
                return ans;
            } else if isVectorList(x) {
                let ans = VECTOR_ELT(x, offset);
                RAISE_NAMED(ans, named_x);
                Rf_unprotect(2); /* args, x */
                return ans;
            } else {
                /* Atomic vector: return scalar */
                let ans = Rf_protect(Rf_allocVector3(TYPEOF(x), 1));
                match TYPEOF(x) {
                    t if t == SEXPTYPE::LGLSXP => {
                        *LOGICAL(ans).add(0) = LOGICAL_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::INTSXP => {
                        *INTEGER(ans).add(0) = INTEGER_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::REALSXP => {
                        *REAL(ans).add(0) = REAL_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::CPLXSXP => {
                        *COMPLEX(ans).add(0) = COMPLEX_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::STRSXP => {
                        SET_STRING_ELT(ans, 0, STRING_ELT(x, offset));
                    }
                    t if t == SEXPTYPE::RAWSXP => {
                        *RAW(ans).add(0) = RAW_ELT(x, offset as c_int);
                    }
                    _ => {} // intentionally unhandled: unsupported SEXPTYPE for scalar subset
                }
                Rf_unprotect(3); /* args, x, ans */
                return ans;
            }
        } else {
            /* nsubs == ndims >= 2 : matrix|array indexing */
            let pdims = INTEGER(dims);
            let dimnames = getAttrib(x, sym_DimNames());
            let ndn = length_int(dimnames);

            let indx = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP, nsubs));
            let pindx = INTEGER(indx);

            let mut cur_subs = subs;
            for i in 0..(nsubs as usize) {
                let thesub = CAR(cur_subs);
                let dname = if (i as c_int) < ndn {
                    VECTOR_ELT(dimnames, i as R_xlen_t)
                } else {
                    R_NilValue()
                };
                let idx = get1index(thesub, dname, *pdims.add(i) as R_xlen_t, pok, -1, call);
                *pindx.add(i) = idx as c_int;
                cur_subs = CDR(cur_subs);
                if idx < 0 || idx >= (*pdims.add(i)) as R_xlen_t {
                    errorcallOutOfBoundsSEXP(x, i as c_int, thesub, call);
                }
            }

            /* Compute linear offset */
            let mut offset: R_xlen_t = 0;
            let mut i: usize = (nsubs - 1) as usize;
            loop {
                offset = (offset + *pindx.add(i) as R_xlen_t) * (*pdims.add(i - 1)) as R_xlen_t;
                if i == 1 {
                    break;
                }
                i -= 1;
            }
            offset += *pindx.add(0) as R_xlen_t;
            Rf_unprotect(1); /* indx */

            /* Extract the element */
            let ans: SEXP;
            if isPairList(x) {
                ans = CAR(nthcdr(x, offset as c_int));
                RAISE_NAMED(ans, named_x);
            } else if isVectorList(x) {
                ans = VECTOR_ELT(x, offset);
                RAISE_NAMED(ans, named_x);
            } else {
                ans = Rf_protect(Rf_allocVector3(TYPEOF(x), 1));
                match TYPEOF(x) {
                    t if t == SEXPTYPE::LGLSXP => {
                        *LOGICAL(ans).add(0) = LOGICAL_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::INTSXP => {
                        *INTEGER(ans).add(0) = INTEGER_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::REALSXP => {
                        *REAL(ans).add(0) = REAL_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::CPLXSXP => {
                        *COMPLEX(ans).add(0) = COMPLEX_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::STRSXP => {
                        SET_STRING_ELT(ans, 0, STRING_ELT(x, offset));
                    }
                    t if t == SEXPTYPE::RAWSXP => {
                        *RAW(ans).add(0) = RAW_ELT(x, offset as c_int);
                    }
                    _ => {} // intentionally unhandled: unsupported SEXPTYPE for scalar subset
                }
                Rf_unprotect(1); /* ans */
            }

            Rf_unprotect(2); /* args, x */
            ans
        }
    }
}

// ---------------------------------------------------------------------------
// dispatch_subset2 -- dispatch [[ on objects
// ---------------------------------------------------------------------------

/// Dispatch the `[[` operator on an object. If `x` is an object, uses
/// `do_subset2`; otherwise extracts the element directly.
pub unsafe fn dispatch_subset2(x: SEXP, i: R_xlen_t, call: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if isObject(x) {
            /* Would call do_subset2 with list2(x, ScalarReal(i+1)) */
            /* Simplified: just extract directly */
            let args = Rf_protect(list2(x, Rf_ScalarReal(i as c_double + 1.0)));
            let bracket_op = R_NilValue(); /* placeholder for R_Primitive("[[") */
            let _ = rho;
            let x_elt = do_subset2(call, bracket_op, args, rho);
            Rf_unprotect(1);
            x_elt
        } else {
            VECTOR_ELT(x, i)
        }
    }
}

// ---------------------------------------------------------------------------
// fixSubset3Args -- prepare arguments for `$`
// ---------------------------------------------------------------------------

/// Fix up arguments for the `$` operator. Translates the second argument
/// (a symbol or string) into a single-element character vector.
pub unsafe fn fixSubset3Args(call: SEXP, args: SEXP, env: SEXP, syminp: *mut SEXP) -> SEXP {
    unsafe {
        let input = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, 1));
        let x = Rf_eval(CAR(args), env);
        let mut nlist = CADR(args);

        /* Evaluate if promise */
        if isPromise(nlist) {
            nlist = CAR(nlist); /* simplified: just get the expression */
            let _ = env;
        }

        if isSymbol(nlist) {
            if !syminp.is_null() {
                *syminp = nlist;
            }
            SET_STRING_ELT(input, 0, PRINTNAME(nlist));
        } else if isString(nlist) {
            if length_int(nlist) != 1 {
                r_error("invalid subscript length");
            }
            SET_STRING_ELT(input, 0, STRING_ELT(nlist, 0));
        } else {
            let _ = call;
            r_error("invalid subscript type for $");
        }

        /* Replace the second argument with a string */
        let new_args = crate::mainutils::duplicate::shallow_duplicate(args);
        SETCAR(new_args, x);
        SETCAR(CDR(new_args), input);
        Rf_unprotect(1); /* input */
        new_args
    }
}

// ---------------------------------------------------------------------------
// do_subset3 -- the `$` subset operator
// ---------------------------------------------------------------------------

/// The `$` subset operator. Evaluates only the first argument; the second
/// is a symbol to be matched, not evaluated.
pub unsafe fn do_subset3(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();
        let _ = op;

        checkArity(op, args);
        let fixed_args = Rf_protect(fixSubset3Args(call, args, env, ptr::null_mut()));

        /* DispatchOrEval internal generic: $ */
        if R_DispatchOrEvalSP(
            call,
            op,
            b"$\0".as_ptr() as *const c_char,
            fixed_args,
            env,
            &mut ans,
        ) != 0
        {
            Rf_unprotect(1); /* fixed_args */
            if NAMED(ans) != 0 {
                ENSURE_NAMEDMAX(ans);
            }
            return ans;
        }

        ans = R_NilValue();
        Rf_protect(ans);
        if !isNull(fixed_args) && !isNull(CAR(fixed_args)) && !isNull(CADR(fixed_args)) {
            ans = R_subset3_dflt(CAR(fixed_args), STRING_ELT(CADR(fixed_args), 0), call);
        }
        Rf_unprotect(2); /* fixed_args, ans */
        ans
    }
}

// ---------------------------------------------------------------------------
// R_subset3_dflt -- default method for `$`
// ---------------------------------------------------------------------------

/// Default method for `$`. Performs partial matching on pair-list and
/// vector-list names, and also handles environment subsetting.
pub unsafe fn R_subset3_dflt(x: SEXP, input: SEXP, call: SEXP) -> SEXP {
    unsafe {
        Rf_protect(input);
        Rf_protect(x);

        /* Get the length of the input string for partial matching */
        let slen = {
            let c = translateChar(input);
            if c.is_null() {
                0
            } else {
                std::ffi::CStr::from_ptr(c).to_bytes().len()
            }
        };

        /* Pair-list / language / nil case */
        if isPairListOrNil(x) {
            let mut xmatch: SEXP = R_NilValue();
            let mut havematch: c_int = 0;
            let mut y = x;
            while !isNull(y) {
                match pstrmatch(TAG(y), input, slen) {
                    pmatch::EXACT_MATCH => {
                        let result = CAR(y);
                        RAISE_NAMED(result, NAMED(x));
                        Rf_unprotect(2); /* input, x */
                        return result;
                    }
                    pmatch::PARTIAL_MATCH => {
                        havematch += 1;
                        xmatch = y;
                    }
                    pmatch::NO_MATCH => {}
                }
                y = CDR(y);
            }
            if havematch == 1 {
                /* unique partial match */
                let result = CAR(xmatch);
                RAISE_NAMED(result, NAMED(x));
                Rf_unprotect(2); /* input, x */
                return result;
            }
            Rf_unprotect(2); /* input, x */
            return R_NilValue();
        }

        /* Vector list case */
        if isVectorList(x) {
            let nlist = getAttrib(x, sym_Names());
            let n = xlength(nlist);
            let mut imatch: R_xlen_t = -1;
            let mut havematch: c_int = 0;

            for i in 0..n {
                match pstrmatch(STRING_ELT(nlist, i), input, slen) {
                    pmatch::EXACT_MATCH => {
                        let result = VECTOR_ELT(x, i);
                        RAISE_NAMED(result, NAMED(x));
                        Rf_unprotect(2); /* input, x */
                        return result;
                    }
                    pmatch::PARTIAL_MATCH => {
                        havematch += 1;
                        if havematch == 1 {
                            /* For partial matches, mark NAMEDMAX to prevent aliasing */
                            let val = VECTOR_ELT(x, i);
                            ENSURE_NAMEDMAX(val);
                            SET_VECTOR_ELT(x, i, val);
                        }
                        imatch = i;
                    }
                    pmatch::NO_MATCH => {}
                }
            }

            if havematch == 1 {
                /* unique partial match */
                let result = VECTOR_ELT(x, imatch);
                RAISE_NAMED(result, NAMED(x));
                Rf_unprotect(2); /* input, x */
                return result;
            }
            Rf_unprotect(2); /* input, x */
            return R_NilValue();
        }

        /* Environment case */
        if isEnvironment(x) {
            let sym = installTrChar(input);
            let mut y = R_findVarInFrame(x, sym);
            if isPromise(y) {
                Rf_protect(y);
                y = CAR(y); /* simplified promise forcing */
                Rf_unprotect(1);
            }
            Rf_unprotect(2); /* input, x */
            if y != R_UnboundValue() {
                if NAMED(y) != 0 {
                    ENSURE_NAMEDMAX(y);
                } else {
                    RAISE_NAMED(y, NAMED(x));
                }
                return y;
            }
            return R_NilValue();
        }

        /* Atomic vector case */
        if isVectorAtomic(x) {
            let _ = call;
            r_error("$ operator is invalid for atomic vectors");
        }

        /* Default: not subsettable */
        errorcallNotSubsettable(x, call);
        R_NilValue() /* unreachable */
    }
}

// ---------------------------------------------------------------------------
// do_subassign -- the `[<-` assignment operator
// ---------------------------------------------------------------------------

/// The `[<-` assignment operator.
///
/// Dispatches to the appropriate method or falls through to default.
pub unsafe fn do_subassign(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();

        /* DispatchOrEval internal generic: [<- */
        if R_DispatchOrEvalSP(
            call,
            op,
            b"[<-\0".as_ptr() as *const c_char,
            args,
            env,
            &mut ans,
        ) != 0
        {
            if NAMED(ans) != 0 {
                ENSURE_NAMEDMAX(ans);
            }
            return ans;
        }

        /* Fall through to default -- delegated to subassign module */
        crate::mainutils::subassign::do_subassign_dflt(call, op, args, env)
    }
}

// ---------------------------------------------------------------------------
// do_subassign2 -- the `[[<-` assignment operator
// ---------------------------------------------------------------------------

/// The `[[<-` assignment operator.
pub unsafe fn do_subassign2(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();

        /* DispatchOrEval internal generic: [[<- */
        if R_DispatchOrEvalSP(
            call,
            op,
            b"[[<-\0".as_ptr() as *const c_char,
            args,
            env,
            &mut ans,
        ) != 0
        {
            if NAMED(ans) != 0 {
                ENSURE_NAMEDMAX(ans);
            }
            return ans;
        }

        /* Fall through to default -- delegated to subassign module */
        crate::mainutils::subassign::do_subassign2_dflt(call, op, args, env)
    }
}

// ---------------------------------------------------------------------------
// do_subassign3 -- the `$<-` assignment operator
// ---------------------------------------------------------------------------

/// The `$<-` assignment operator.
pub unsafe fn do_subassign3(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();

        /* DispatchOrEval internal generic: $<- */
        if R_DispatchOrEvalSP(
            call,
            op,
            b"$<-\0".as_ptr() as *const c_char,
            args,
            env,
            &mut ans,
        ) != 0
        {
            if NAMED(ans) != 0 {
                ENSURE_NAMEDMAX(ans);
            }
            return ans;
        }

        /* Fall through to default -- delegated to subassign module */
        /* For now, return the args unchanged */
        args
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scalar_index_null() {
        unsafe {
            assert_eq!(scalarIndex(ptr::null_mut()), -1);
        }
    }

    #[test]
    fn test_scalar_index_nil() {
        unsafe {
            assert_eq!(scalarIndex(R_NilValue()), -1);
        }
    }

    #[test]
    fn test_extract_exact_arg_nil() {
        unsafe {
            let result = ExtractExactArg(R_NilValue());
            assert_eq!(result, 1);
        }
    }

    #[test]
    fn test_r_dispatch_or_eval_sp_no_dispatch() {
        unsafe {
            let mut ans: SEXP = ptr::null_mut();
            let result = R_DispatchOrEvalSP(
                ptr::null_mut(),
                ptr::null_mut(),
                b"[\0".as_ptr() as *const c_char,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut ans,
            );
            assert_eq!(result, 0);
        }
    }

    #[test]
    fn test_extract_arg_empty_list() {
        unsafe {
            let result = ExtractArg(R_NilValue(), R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_extract_drop_arg_nil() {
        unsafe {
            let mut drop: c_int = 0;
            ExtractDropArg(R_NilValue(), &mut drop);
            assert_eq!(drop, 1); /* default when not found */
        }
    }

    #[test]
    fn test_as_logical_nil() {
        unsafe {
            assert_eq!(asLogical(R_NilValue()), NA_LOGICAL);
        }
    }

    #[test]
    fn test_nthcdr_null() {
        unsafe {
            let result = nthcdr(R_NilValue(), 5);
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_pstrmatch_null_target() {
        unsafe {
            let result = pstrmatch(R_NilValue(), R_NilValue(), 3);
            assert_eq!(result, pmatch::NO_MATCH);
        }
    }

    #[test]
    fn test_type_checking_helpers() {
        unsafe {
            /* NILSXP */
            assert!(isNull(R_NilValue()));
            assert!(!isVector(R_NilValue()));
            assert!(!isPairList(R_NilValue()));
            assert!(!isVectorList(R_NilValue()));
            assert!(!isEnvironment(R_NilValue()));
            assert!(!isSymbol(R_NilValue()));
        }
    }

    #[test]
    fn test_vector_subset_missing_arg() {
        unsafe {
            /* When x is a simple integer vector and s is missing, should duplicate */
            let x = Rf_allocVector(SEXPTYPE::INTSXP, 3);
            if !x.is_null() {
                for i in 0..3 {
                    *INTEGER(x).add(i) = ((i + 1) * 10) as c_int;
                }
                /* Missing arg: create a symbol with empty name */
                let missing = Rf_install(std::ffi::CString::new("").unwrap_or_default().as_ptr());
                let result = VectorSubset(x, missing, R_NilValue());
                /* Should return a duplicate (same length) */
                assert!(!result.is_null());
                assert_eq!(LENGTH(result), LENGTH(x));
            }
        }
    }

    #[test]
    fn test_extract_subset_null() {
        unsafe {
            let result = ExtractSubset(R_NilValue(), R_NilValue(), R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_find_a_sub_index() {
        unsafe {
            /* Simple 2D case: subs = [[1,2], [3,4]], indx = [0, 0], offset = [1, 3] */
            let sub0: Vec<c_int> = vec![1, 2];
            let sub1: Vec<c_int> = vec![3, 4];
            let subs: Vec<*const c_int> = vec![sub0.as_ptr(), sub1.as_ptr()];
            let indx: Vec<c_int> = vec![0, 0];
            let offset: Vec<R_xlen_t> = vec![1, 3];
            let result = findASubIndex(
                2,
                subs.as_ptr(),
                indx.as_ptr(),
                ptr::null(),
                offset.as_ptr(),
                R_NilValue(),
            );
            /* (1-1)*1 + (3-1)*3 = 0 + 6 = 6 */
            assert_eq!(result, 6);
        }
    }

    #[test]
    fn test_find_a_sub_index_na() {
        unsafe {
            let sub0: Vec<c_int> = vec![1, NA_INTEGER];
            let sub1: Vec<c_int> = vec![3, 4];
            let subs: Vec<*const c_int> = vec![sub0.as_ptr(), sub1.as_ptr()];
            let indx: Vec<c_int> = vec![1, 0]; /* index 1 into sub0 = NA_INTEGER */
            let offset: Vec<R_xlen_t> = vec![1, 3];
            let result = findASubIndex(
                2,
                subs.as_ptr(),
                indx.as_ptr(),
                ptr::null(),
                offset.as_ptr(),
                R_NilValue(),
            );
            assert_eq!(result, NA_INTEGER as R_xlen_t);
        }
    }

    #[test]
    fn test_r_finite() {
        assert!(R_FINITE(1.0));
        assert!(R_FINITE(-1.0));
        assert!(R_FINITE(0.0));
        assert!(!R_FINITE(f64::INFINITY));
        assert!(!R_FINITE(f64::NEG_INFINITY));
        assert!(!R_FINITE(f64::NAN));
    }
}
