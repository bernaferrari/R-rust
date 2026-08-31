#![allow(unused_imports)]
use super::*;
use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::sexp::accessors::{
    CADDDR, CADDR, CADR, CAR, CDDDR, CDDR, CDR, CHAR, COMPLEX, INTEGER, LENGTH, LOGICAL, PRINTNAME,
    RAW, REAL, SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT,
    XLENGTH, translateChar,
};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector, Rf_allocVector3, Rf_isInteger, Rf_isNull,
    Rf_isReal, Rf_isVector, Rf_length, Rf_mkChar, Rf_mkString,
};
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_FINITE, R_xlen_t, SEXP};
use crate::sexp::globals::{R_MissingArg, R_NilValue};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R_XLEN_T_MAX as f64 (for overflow checks).
pub const R_XLEN_T_MAX_DBL: c_double = i64::MAX as c_double;

/// FLT_EPSILON for seq_colon rounding.
pub const FLT_EPSILON: c_double = 1.19209290e-07_f64;

/// FEPS tolerance for seq.int().
pub const FEPS: c_double = 1e-10;

/// INT_MAX value matching C.
pub const INT_MAX_C: c_double = i32::MAX as c_double;

/// INT_MIN value matching C.
pub const INT_MIN_C: c_double = i32::MIN as c_double;

/// DBL_EPSILON for seq.int().
pub const DBL_EPSILON_C: c_double = f64::EPSILON;

// ---------------------------------------------------------------------------
// SEXPTYPE integer values for use in match patterns
// ---------------------------------------------------------------------------

pub const LGLSXP_VAL: c_int = 10;
pub const INTSXP_VAL: c_int = 13;
pub const REALSXP_VAL: c_int = 14;
pub const CPLXSXP_VAL: c_int = 15;
pub const STRSXP_VAL: c_int = 16;
pub const VECSXP_VAL: c_int = 19;
pub const EXPRSXP_VAL: c_int = 20;
pub const RAWSXP_VAL: c_int = 24;
pub const LISTSXP_VAL: c_int = 2;
pub const LANGSXP_VAL: c_int = 6;
pub const DOTSXP_VAL: c_int = 17;
pub const NILSXP_VAL: c_int = 0;

// ---------------------------------------------------------------------------
// Local helpers and entry points
// (plain unsafe fn to avoid duplicate #[unsafe(no_mangle)] symbols)
// ---------------------------------------------------------------------------

pub unsafe fn DispatchOrEval(
    call: SEXP,
    op: SEXP,
    generic: *const c_char,
    args: SEXP,
    rho: SEXP,
    ans: *mut SEXP,
    narg: c_int,
    evalseq: c_int,
) -> c_int {
    unsafe {
        crate::eval::dispatch::DispatchOrEval(call, op, generic, args, rho, ans, narg, evalseq)
    }
}

pub unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe { crate::mainutils::relop::checkArity(op, args) }
}

pub unsafe fn check1arg(args: SEXP, call: SEXP, name: *const c_char) {
    unsafe {
        // Upstream util.c Rf_check1arg: a supplied tag must be a prefix of
        // the formal name; a strict prefix triggers the partial-match
        // warning when options(warnPartialMatchArgs=TRUE).
        let tag = TAG(args);
        if tag.is_null() || tag == R_NilValue() {
            return;
        }
        let tag_name = CHAR(PRINTNAME(tag));
        if tag_name.is_null() {
            return;
        }
        let supplied = CStr::from_ptr(tag_name).to_bytes();
        let formal = CStr::from_ptr(name).to_bytes();
        if supplied.len() > formal.len() || !formal.starts_with(supplied) {
            let msg = format!(
                "supplied argument name '{}' does not match '{}'\0",
                String::from_utf8_lossy(supplied),
                String::from_utf8_lossy(formal),
            );
            errorcall(call, msg.as_ptr() as *const c_char);
            return;
        }
        if !supplied.is_empty()
            && supplied.len() < formal.len()
            && crate::mainutils::options::logical_option_enabled(c"warnPartialMatchArgs")
        {
            let fsym = Rf_install_stub(name);
            let cond = crate::mainutils::errors::R_makePartialArgumentMatchWarningCondition(
                call, tag, fsym,
            );
            let _cond_guard = crate::sexp::protect::protect(cond);
            crate::mainutils::errors::R_signalWarningCondition(cond);
        }
    }
}

pub unsafe fn errorcall(call: SEXP, format: *const c_char) {
    crate::mainutils::errors::errorcall(call, format);
}

pub fn errorcall_never(call: SEXP, msg: &str) -> ! {
    crate::mainutils::errors::errorcall_str(call, msg);
}

pub unsafe fn warningcall(call: SEXP, format: *const c_char) {
    unsafe { crate::mainutils::errors::warningcall(call, format) }
}

/// `xlength()` — like `XLENGTH()`, but walks pairlists (counting cells) and
/// treats non-vector nodes as length 1, matching R's `Rinlinedfuns.h`
/// `xlength()` used by `do_seq()` for `along.with`.
#[inline(always)]
pub unsafe fn xlength(s: SEXP) -> R_xlen_t {
    unsafe {
        if s.is_null() || s == R_NilValue() {
            return 0;
        }
        let t = TYPEOF(s);
        if t == LISTSXP_VAL || t == LANGSXP_VAL || t == DOTSXP_VAL {
            let mut n: R_xlen_t = 0;
            let mut cur = s;
            while !cur.is_null() && cur != R_NilValue() {
                n += 1;
                cur = CDR(cur);
            }
            n
        } else if isVector(s) != 0 {
            XLENGTH(s)
        } else {
            1 // symbols, environments, ... (C xlength default)
        }
    }
}

pub unsafe fn error(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    crate::mainutils::errors::errorcall(std::ptr::null_mut(), c_msg.as_ptr() as *const c_char);
}

pub unsafe fn R_typeToChar(s: SEXP) -> *const c_char {
    // Return the static SEXPTYPE name for this SEXP (only used in errors).
    unsafe {
        if s.is_null() {
            return b"NULL\0".as_ptr() as *const c_char;
        }
        let t = TYPEOF(s);
        let name: &[u8] = match t {
            NILSXP_VAL => b"NULL\0",
            LGLSXP_VAL => b"logical\0",
            INTSXP_VAL => b"integer\0",
            REALSXP_VAL => b"double\0",
            CPLXSXP_VAL => b"complex\0",
            STRSXP_VAL => b"character\0",
            VECSXP_VAL => b"list\0",
            EXPRSXP_VAL => b"expression\0",
            RAWSXP_VAL => b"raw\0",
            LISTSXP_VAL => b"pairlist\0",
            _ => b"unknown\0",
        };
        name.as_ptr() as *const c_char
    }
}

pub unsafe fn coerceVector(s: SEXP, t: c_int) -> SEXP {
    unsafe { crate::mainutils::coerce::coerceVector(s, t) }
}

pub unsafe fn UNIMPLEMENTED_TYPE(routine: *const c_char, s: SEXP) -> ! {
    unsafe {
        let routine = if routine.is_null() {
            "seq"
        } else {
            CStr::from_ptr(routine).to_str().unwrap_or("seq")
        };
        let sexptype = if s.is_null() { -1 } else { TYPEOF(s) };
        std::panic::panic_any(crate::sexp::context::RError {
            message: format!("{routine}: unsupported SEXPTYPE {sexptype}"),
        });
    }
}

pub unsafe fn R_PreserveObject(x: SEXP) {
    unsafe { crate::sexp::protect::R_PreserveObject(x) }
}

/// Build a tagged pairlist of formal symbols for matchArgs_NR.
pub unsafe fn allocFormalsList5(a1: SEXP, a2: SEXP, a3: SEXP, a4: SEXP, a5: SEXP) -> SEXP {
    unsafe {
        let c5 = crate::sexp::constructors::Rf_cons(a5, crate::sexp::globals::R_NilValue());
        SETTAG(c5, a5);
        let c4 = crate::sexp::constructors::Rf_cons(a4, c5);
        SETTAG(c4, a4);
        let c3 = crate::sexp::constructors::Rf_cons(a3, c4);
        SETTAG(c3, a3);
        let c2 = crate::sexp::constructors::Rf_cons(a2, c3);
        SETTAG(c2, a2);
        let c1 = crate::sexp::constructors::Rf_cons(a1, c2);
        SETTAG(c1, a1);
        c1
    }
}

/// Build a tagged pairlist of formal symbols for matchArgs_NR.
#[allow(clippy::too_many_arguments)]
pub unsafe fn allocFormalsList6(
    a1: SEXP,
    a2: SEXP,
    a3: SEXP,
    a4: SEXP,
    a5: SEXP,
    a6: SEXP,
) -> SEXP {
    unsafe {
        let c6 = crate::sexp::constructors::Rf_cons(a6, crate::sexp::globals::R_NilValue());
        SETTAG(c6, a6);
        let c5 = crate::sexp::constructors::Rf_cons(a5, c6);
        SETTAG(c5, a5);
        let c4 = crate::sexp::constructors::Rf_cons(a4, c5);
        SETTAG(c4, a4);
        let c3 = crate::sexp::constructors::Rf_cons(a3, c4);
        SETTAG(c3, a3);
        let c2 = crate::sexp::constructors::Rf_cons(a2, c3);
        SETTAG(c2, a2);
        let c1 = crate::sexp::constructors::Rf_cons(a1, c2);
        SETTAG(c1, a1);
        c1
    }
}

pub unsafe fn matchArgs_NR(formals: SEXP, args: SEXP, call: SEXP) -> SEXP {
    unsafe { crate::mainutils::match_mod::matchArgs_RC(formals, args, call) }
}
pub unsafe fn inherits(s: SEXP, what: *const c_char) -> c_int {
    unsafe { crate::mainutils::objects::inherits2(s, what) }
}

pub unsafe fn asReal(x: SEXP) -> c_double {
    unsafe {
        if x.is_null() {
            return NA_REAL;
        }
        match TYPEOF(x) {
            REALSXP_VAL => {
                if REAL(x).is_null() {
                    NA_REAL
                } else {
                    *REAL(x)
                }
            }
            INTSXP_VAL => {
                if INTEGER(x).is_null() {
                    NA_REAL
                } else {
                    let v = *INTEGER(x);
                    if v == NA_INTEGER {
                        NA_REAL
                    } else {
                        v as c_double
                    }
                }
            }
            LGLSXP_VAL => {
                if LOGICAL(x).is_null() {
                    NA_REAL
                } else {
                    let v = *LOGICAL(x);
                    if v == NA_LOGICAL {
                        NA_REAL
                    } else {
                        v as c_double
                    }
                }
            }
            SYMSXP_VAL => {
                if x == R_MissingArg() {
                    NA_REAL
                } else {
                    0.0
                }
            }
        }
    }
}

pub unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_INTEGER;
        }
        match TYPEOF(x) {
            INTSXP_VAL => {
                if INTEGER(x).is_null() {
                    NA_INTEGER
                } else {
                    *INTEGER(x)
                }
            }
            REALSXP_VAL => {
                if REAL(x).is_null() {
                    NA_INTEGER
                } else {
                    let v = *REAL(x);
                    if ISNAN(v) || v > i32::MAX as c_double || v < i32::MIN as c_double {
                        NA_INTEGER
                    } else {
                        v as c_int
                    }
                }
            }
            LGLSXP_VAL => {
                if LOGICAL(x).is_null() {
                    NA_INTEGER
                } else {
                    *LOGICAL(x)
                }
            }
            SYMSXP_VAL => {
                if x == R_MissingArg() {
                    NA_INTEGER
                } else {
                    0
                }
            }
        }
    }
}

pub unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_LOGICAL;
        }
        match TYPEOF(x) {
            LGLSXP_VAL => {
                if LOGICAL(x).is_null() {
                    NA_LOGICAL
                } else {
                    *LOGICAL(x)
                }
            }
            INTSXP_VAL => {
                if INTEGER(x).is_null() {
                    NA_LOGICAL
                } else {
                    *INTEGER(x)
                }
            }
            _ => 0,
        }
    }
}

pub unsafe fn xlengthgets(x: SEXP, len: R_xlen_t) -> SEXP {
    unsafe { crate::mainutils::builtin::xlengthgets(x, len) }
}

pub unsafe fn shallow_duplicate(x: SEXP) -> SEXP {
    unsafe { crate::mainutils::duplicate::shallow_duplicate(x) }
}

pub unsafe fn lazy_duplicate(x: SEXP) -> SEXP {
    unsafe { crate::mainutils::duplicate::lazy_duplicate(x) }
}

pub unsafe fn Rf_duplicate(x: SEXP) -> SEXP {
    unsafe { crate::mainutils::duplicate::Rf_duplicate(x) }
}

pub unsafe fn Rf_shallow_duplicate(x: SEXP) -> SEXP {
    unsafe { crate::mainutils::duplicate::shallow_duplicate(x) }
}

pub unsafe fn setAttrib(x: SEXP, what: SEXP, val: SEXP) {
    unsafe { crate::eval::attrib_core::setAttrib(x, what, val) }
}

pub unsafe fn getAttrib(x: SEXP, what: SEXP) -> SEXP {
    unsafe { crate::eval::attrib_core::getAttrib(x, what) }
}

pub unsafe fn R_NamesSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_NamesSymbol() }
}

pub unsafe fn R_ClassSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_ClassSymbol() }
}

pub unsafe fn R_LevelsSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_LevelsSymbol() }
}

pub unsafe fn isObject(x: SEXP) -> c_int {
    unsafe { crate::eval::attrib_core::isObject(x) }
}

pub unsafe fn asBool2(x: SEXP, call: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asRbool(x, call) }
}

pub unsafe fn isVector(x: SEXP) -> c_int {
    unsafe { Rf_isVector(x) }
}

pub unsafe fn isInteger(x: SEXP) -> c_int {
    unsafe { Rf_isInteger(x) }
}

pub unsafe fn isReal(x: SEXP) -> c_int {
    unsafe { Rf_isReal(x) }
}

pub unsafe fn ScalarReal(x: c_double) -> SEXP {
    unsafe { Rf_ScalarReal(x) }
}

pub unsafe fn ScalarInteger(x: c_int) -> SEXP {
    unsafe { Rf_ScalarInteger(x) }
}

pub unsafe fn SET_S4_OBJECT(_x: SEXP) {}

pub unsafe fn IS_S4_OBJECT(_x: SEXP) -> c_int {
    0
}

pub unsafe fn Rf_install_stub(name: *const c_char) -> SEXP {
    unsafe { crate::sexp::symbol::Rf_install(name) }
}

pub unsafe fn R_DotsSymbol() -> SEXP {
    unsafe { crate::sexp::symbol::R_DotsSymbol() }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// fmax2 -- maximum of two doubles, propagating NaN.
#[inline]
pub fn fmax2(x: c_double, y: c_double) -> c_double {
    if x.is_nan() {
        x
    } else if y.is_nan() {
        y
    } else {
        x.max(y)
    }
}

// ---------------------------------------------------------------------------
// MOD_ITERATE1 macro replacement
// ---------------------------------------------------------------------------

/// Implement the MOD_ITERATE1 pattern:
/// Iterate i from 0..na, recycling j from 0..ns.
macro_rules! mod_iterate1 {
    ($na:expr, $ns:expr, $i:ident, $j:ident, $body:block) => {
        {
            let mut $i: R_xlen_t = 0;
            let mut $j: R_xlen_t = 0;
            while $i < $na {
                $body
                $j += 1;
                if $j >= $ns { $j = 0; }
                $i += 1;
            }
        }
    };
}

// ---------------------------------------------------------------------------
// _S4_rep_keepClass
// ---------------------------------------------------------------------------

/// When defined, rep(<S4>, .) keeps the class (e.g., for list-like).
pub const _S4_rep_keepClass: bool = true;

pub(crate) use mod_iterate1;
