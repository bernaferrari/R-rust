#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/seq.c -- sequence generation.
//!
//! Implements `:`, `seq.int()`, `seq_len()`, `seq_along()`, `rep()`,
//! `rep.int()`, `rep_len()`, and `sequence()`.

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
const R_XLEN_T_MAX_DBL: c_double = i64::MAX as c_double;

/// FLT_EPSILON for seq_colon rounding.
const FLT_EPSILON: c_double = 1.19209290e-07_f64;

/// FEPS tolerance for seq.int().
const FEPS: c_double = 1e-10;

/// INT_MAX value matching C.
const INT_MAX_C: c_double = i32::MAX as c_double;

/// INT_MIN value matching C.
const INT_MIN_C: c_double = i32::MIN as c_double;

/// DBL_EPSILON for seq.int().
const DBL_EPSILON_C: c_double = f64::EPSILON;

// ---------------------------------------------------------------------------
// SEXPTYPE integer values for use in match patterns
// ---------------------------------------------------------------------------

const LGLSXP_VAL: c_int = 10;
const INTSXP_VAL: c_int = 13;
const REALSXP_VAL: c_int = 14;
const CPLXSXP_VAL: c_int = 15;
const STRSXP_VAL: c_int = 16;
const VECSXP_VAL: c_int = 19;
const EXPRSXP_VAL: c_int = 20;
const RAWSXP_VAL: c_int = 24;
const LISTSXP_VAL: c_int = 2;
const LANGSXP_VAL: c_int = 6;
const DOTSXP_VAL: c_int = 17;
const NILSXP_VAL: c_int = 0;

// ---------------------------------------------------------------------------
// Local helpers and entry points
// (plain unsafe fn to avoid duplicate #[unsafe(no_mangle)] symbols)
// ---------------------------------------------------------------------------

unsafe fn DispatchOrEval(
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

unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe { crate::mainutils::relop::checkArity(op, args) }
}

unsafe fn check1arg(args: SEXP, call: SEXP, name: *const c_char) {
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
        if !supplied.is_empty() && supplied.len() < formal.len() && warn_partial_match_args() {
            let fsym = Rf_install_stub(name);
            let cond = crate::mainutils::errors::R_makePartialArgumentMatchWarningCondition(
                call, tag, fsym,
            );
            let _cond_guard = crate::sexp::protect::protect(cond);
            crate::mainutils::errors::R_signalWarningCondition(cond);
        }
    }
}

/// R_warn_partial_match_args — read options(warnPartialMatchArgs).
unsafe fn warn_partial_match_args() -> bool {
    unsafe {
        let s = crate::mainutils::options::GetOption1(Rf_install_stub(
            b"warnPartialMatchArgs\0".as_ptr() as *const c_char,
        ));
        !s.is_null() && s != R_NilValue() && asLogical(s) == 1
    }
}

unsafe fn errorcall(call: SEXP, format: *const c_char) {
    crate::mainutils::errors::errorcall(call, format);
}

fn errorcall_never(call: SEXP, msg: &str) -> ! {
    crate::mainutils::errors::errorcall_str(call, msg);
}

unsafe fn warningcall(call: SEXP, format: *const c_char) {
    unsafe { crate::mainutils::errors::warningcall(call, format) }
}

/// `xlength()` — like `XLENGTH()`, but walks pairlists (counting cells) and
/// treats non-vector nodes as length 1, matching R's `Rinlinedfuns.h`
/// `xlength()` used by `do_seq()` for `along.with`.
#[inline(always)]
unsafe fn xlength(s: SEXP) -> R_xlen_t {
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

unsafe fn error(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    crate::mainutils::errors::errorcall(std::ptr::null_mut(), c_msg.as_ptr() as *const c_char);
}

unsafe fn R_typeToChar(s: SEXP) -> *const c_char {
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

unsafe fn coerceVector(s: SEXP, t: c_int) -> SEXP {
    unsafe { crate::mainutils::coerce::coerceVector(s, t) }
}

unsafe fn UNIMPLEMENTED_TYPE(routine: *const c_char, s: SEXP) -> ! {
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

unsafe fn R_PreserveObject(x: SEXP) {
    unsafe { crate::sexp::protect::R_PreserveObject(x) }
}

/// Build a tagged pairlist of formal symbols for matchArgs_NR.
unsafe fn allocFormalsList5(a1: SEXP, a2: SEXP, a3: SEXP, a4: SEXP, a5: SEXP) -> SEXP {
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
unsafe fn allocFormalsList6(a1: SEXP, a2: SEXP, a3: SEXP, a4: SEXP, a5: SEXP, a6: SEXP) -> SEXP {
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

unsafe fn matchArgs_NR(formals: SEXP, args: SEXP, call: SEXP) -> SEXP {
    unsafe { crate::mainutils::match_mod::matchArgs_RC(formals, args, call) }
}
unsafe fn inherits(s: SEXP, what: *const c_char) -> c_int {
    unsafe { crate::mainutils::objects::inherits2(s, what) }
}

unsafe fn asReal(x: SEXP) -> c_double {
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

unsafe fn asInteger(x: SEXP) -> c_int {
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

unsafe fn asLogical(x: SEXP) -> c_int {
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

unsafe fn xlengthgets(x: SEXP, len: R_xlen_t) -> SEXP {
    unsafe { crate::mainutils::builtin::xlengthgets(x, len) }
}

unsafe fn R_compact_intrange(from: R_xlen_t, to: R_xlen_t) -> SEXP {
    unsafe {
        let n = (if from <= to { to - from } else { from - to } + 1) as c_int;
        let ans = Rf_allocVector(INTSXP_VAL, n);
        if !ans.is_null() && n > 0 {
            let data = INTEGER(ans);
            let step: c_int = if from <= to { 1 } else { -1 };
            let mut val = from as c_int;
            for i in 0..n as usize {
                *data.add(i) = val;
                val += step;
            }
        }
        ans
    }
}

unsafe fn shallow_duplicate(x: SEXP) -> SEXP {
    unsafe { crate::mainutils::duplicate::shallow_duplicate(x) }
}

unsafe fn lazy_duplicate(x: SEXP) -> SEXP {
    unsafe { crate::mainutils::duplicate::lazy_duplicate(x) }
}

unsafe fn Rf_duplicate(x: SEXP) -> SEXP {
    unsafe { crate::mainutils::duplicate::Rf_duplicate(x) }
}

unsafe fn Rf_shallow_duplicate(x: SEXP) -> SEXP {
    unsafe { crate::mainutils::duplicate::shallow_duplicate(x) }
}

unsafe fn setAttrib(x: SEXP, what: SEXP, val: SEXP) {
    unsafe { crate::eval::attrib_core::setAttrib(x, what, val) }
}

unsafe fn getAttrib(x: SEXP, what: SEXP) -> SEXP {
    unsafe { crate::eval::attrib_core::getAttrib(x, what) }
}

unsafe fn R_NamesSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_NamesSymbol() }
}

unsafe fn R_ClassSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_ClassSymbol() }
}

unsafe fn R_LevelsSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_LevelsSymbol() }
}

unsafe fn isObject(x: SEXP) -> c_int {
    unsafe { crate::eval::attrib_core::isObject(x) }
}

unsafe fn asBool2(x: SEXP, call: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asRbool(x, call) }
}

unsafe fn isVector(x: SEXP) -> c_int {
    unsafe { Rf_isVector(x) }
}

unsafe fn isInteger(x: SEXP) -> c_int {
    unsafe { Rf_isInteger(x) }
}

unsafe fn isReal(x: SEXP) -> c_int {
    unsafe { Rf_isReal(x) }
}

unsafe fn ScalarReal(x: c_double) -> SEXP {
    unsafe { Rf_ScalarReal(x) }
}

unsafe fn ScalarInteger(x: c_int) -> SEXP {
    unsafe { Rf_ScalarInteger(x) }
}

unsafe fn SET_S4_OBJECT(_x: SEXP) {}

unsafe fn IS_S4_OBJECT(_x: SEXP) -> c_int {
    0
}

unsafe fn Rf_install_stub(name: *const c_char) -> SEXP {
    unsafe { crate::sexp::symbol::Rf_install(name) }
}

unsafe fn R_DotsSymbol() -> SEXP {
    unsafe { crate::sexp::symbol::R_DotsSymbol() }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// fmax2 -- maximum of two doubles, propagating NaN.
#[inline]
fn fmax2(x: c_double, y: c_double) -> c_double {
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
const _S4_rep_keepClass: bool = true;

// ---------------------------------------------------------------------------
// cross_colon: cross product of two factors
// ---------------------------------------------------------------------------

unsafe fn cross_colon(call: SEXP, s: SEXP, t: SEXP) -> SEXP {
    unsafe {
        let ns = Rf_length(s);
        let nt = Rf_length(t);
        if ns != nt {
            errorcall(call, b"unequal factor lengths\0".as_ptr() as *const c_char);
        }
        let n = ns;
        let ls = getAttrib(s, R_LevelsSymbol());
        let lt = getAttrib(t, R_LevelsSymbol());
        let nls = LENGTH(ls);
        let nlt = LENGTH(lt);
        let a = Rf_allocVector(INTSXP_VAL, n);
        let rs = coerceVector(s, INTSXP_VAL);
        let rt = coerceVector(t, INTSXP_VAL);
        for i in 0..n as R_xlen_t {
            let vs = *INTEGER(rs).add(i as usize);
            let vt = *INTEGER(rt).add(i as usize);
            if vs == NA_INTEGER || vt == NA_INTEGER {
                *INTEGER(a).add(i as usize) = NA_INTEGER;
            } else {
                *INTEGER(a).add(i as usize) = vt + (vs - 1) * nlt;
            }
        }
        if Rf_isNull(ls) == 0 && Rf_isNull(lt) == 0 {
            let la = Rf_allocVector(STRSXP_VAL, (nls as R_xlen_t * nlt as R_xlen_t) as c_int);
            let mut k: R_xlen_t = 0;
            for i in 0..nls as R_xlen_t {
                let vi_ptr = translateChar(STRING_ELT(ls, i as R_xlen_t));
                let vi = std::ffi::CStr::from_ptr(vi_ptr).to_str().unwrap_or("");
                for j in 0..nlt as R_xlen_t {
                    let vj_ptr = translateChar(STRING_ELT(lt, j as R_xlen_t));
                    let vj = std::ffi::CStr::from_ptr(vj_ptr).to_str().unwrap_or("");
                    let label = format!("{}:{}\0", vi, vj);
                    let ch = Rf_mkChar(label.as_ptr() as *const c_char);
                    SET_STRING_ELT(la, k, ch);
                    k += 1;
                }
            }
            setAttrib(a, R_LevelsSymbol(), la);
        }
        let la = Rf_mkString(
            std::ffi::CString::new("factor")
                .unwrap_or_default()
                .as_ptr(),
        );
        setAttrib(a, R_ClassSymbol(), la);
        a
    }
}

// ---------------------------------------------------------------------------
// seq_colon: core `:` operator implementation
// ---------------------------------------------------------------------------

unsafe fn seq_colon(n1: c_double, n2: c_double, call: SEXP) -> SEXP {
    unsafe {
        let r = (n2 - n1).abs();
        if r >= R_XLEN_T_MAX_DBL {
            errorcall(
                call,
                b"result would be too long a vector\0".as_ptr() as *const c_char,
            );
        }

        // If both n1 and n2 are exact integers, use compact intrange.
        // R's colon produces a descending range when n1 > n2; the naive
        // (n2 - n1) as unsigned cast wraps, so pass both ends through and
        // let R_compact_intrange pick the direction.
        if n1 == n1 as i64 as c_double && n2 == n2 as i64 as c_double {
            return R_compact_intrange(n1 as i64 as R_xlen_t, n2 as i64 as R_xlen_t);
        }

        let n = (r + 1.0 + FLT_EPSILON) as R_xlen_t;

        let mut use_int = n1 <= INT_MAX_C && n1 == n1 as c_int as c_double;
        if use_int {
            if n1 <= INT_MIN_C {
                use_int = false;
            } else {
                let dn = n as c_double;
                let eff_to = if n1 <= n2 {
                    n1 + dn - 1.0
                } else {
                    n1 - (dn - 1.0)
                };
                if eff_to <= INT_MIN_C || eff_to > INT_MAX_C {
                    use_int = false;
                }
            }
        }

        if use_int {
            if n1 <= n2 {
                R_compact_intrange(n1 as R_xlen_t, (n1 + n as c_double - 1.0) as R_xlen_t)
            } else {
                R_compact_intrange(n1 as R_xlen_t, (n1 - n as c_double + 1.0) as R_xlen_t)
            }
        } else {
            let ans = Rf_allocVector3(REALSXP_VAL, n);
            let ra = REAL(ans);
            if n1 <= n2 {
                for i in 0..n {
                    *ra.add(i as usize) = n1 + i as c_double;
                }
            } else {
                for i in 0..n {
                    *ra.add(i as usize) = n1 - i as c_double;
                }
            }
            ans
        }
    }
}

// ---------------------------------------------------------------------------
// do_colon: `:` primitive
// ---------------------------------------------------------------------------

pub unsafe fn do_colon(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = rho;
        checkArity(op, args);
        let s1 = CAR(args);
        let s2 = CADR(args);

        if inherits(s1, b"factor\0".as_ptr() as *const c_char) != 0
            && inherits(s2, b"factor\0".as_ptr() as *const c_char) != 0
        {
            return cross_colon(call, s1, s2);
        }

        let n1 = LENGTH(s1) as c_double;
        let n2 = LENGTH(s2) as c_double;

        if n1 != 1.0 || n2 != 1.0 {
            if n1 == 0.0 || n2 == 0.0 {
                // C: errorcall(call, _("argument of length 0"));
                errorcall(call, b"argument of length 0\0".as_ptr() as *const c_char);
            }
            warningcall(
                call,
                b"numerical expression has length > 1\0".as_ptr() as *const c_char,
            );
        }

        let r_n1 = asReal(s1);
        let r_n2 = asReal(s2);
        if ISNAN(r_n1) || ISNAN(r_n2) {
            // C: errorcall(call, _("NA/NaN argument"));
            errorcall(call, b"NA/NaN argument\0".as_ptr() as *const c_char);
        }
        seq_colon(r_n1, r_n2, call)
    }
}

// ---------------------------------------------------------------------------
// rep2: rep.int(x, times) for a vector times
// ---------------------------------------------------------------------------

unsafe fn rep2(s: SEXP, ncopy: SEXP) -> SEXP {
    unsafe {
        let nc = XLENGTH(ncopy);
        let t: SEXP;

        // Coerce ncopy to appropriate type
        if TYPEOF(ncopy) != INTSXP_VAL {
            t = coerceVector(ncopy, REALSXP_VAL);
        } else {
            t = coerceVector(ncopy, INTSXP_VAL);
        }

        let mut sna: c_double = 0.0;
        if TYPEOF(t) == REALSXP_VAL {
            for i in 0..nc {
                let v = *REAL(t).add(i as usize);
                if ISNAN(v) || v <= -1.0 || v >= R_XLEN_T_MAX_DBL + 1.0 {
                    return ptr::null_mut();
                }
                sna += v as R_xlen_t as c_double;
            }
        } else {
            for i in 0..nc {
                let v = *INTEGER(t).add(i as usize);
                if v == NA_INTEGER || v < 0 {
                    return ptr::null_mut();
                }
                sna += v as c_double;
            }
        }
        if sna > R_XLEN_T_MAX_DBL {
            return ptr::null_mut();
        }
        let na = sna as R_xlen_t;

        let a = Rf_allocVector(TYPEOF(s), na as c_int);
        let mut n: R_xlen_t = 0;

        let stype = TYPEOF(s);
        if TYPEOF(t) == REALSXP_VAL {
            let it = REAL(t);
            match stype {
                LGLSXP_VAL => {
                    for i in 0..nc {
                        for _j in 0..*it.add(i as usize) as R_xlen_t {
                            *LOGICAL(a).add(n as usize) = *LOGICAL(s).add(i as usize);
                            n += 1;
                        }
                    }
                }
                INTSXP_VAL => {
                    for i in 0..nc {
                        let count = *it.add(i as usize) as R_xlen_t;
                        for _j in 0..count {
                            *INTEGER(a).add(n as usize) = *INTEGER(s).add(i as usize);
                            n += 1;
                        }
                    }
                }
                REALSXP_VAL => {
                    for i in 0..nc {
                        let count = *it.add(i as usize) as R_xlen_t;
                        for _j in 0..count {
                            *REAL(a).add(n as usize) = *REAL(s).add(i as usize);
                            n += 1;
                        }
                    }
                }
                CPLXSXP_VAL => {
                    for i in 0..nc {
                        let count = *it.add(i as usize) as R_xlen_t;
                        for _j in 0..count {
                            *COMPLEX(a).add(n as usize) = *COMPLEX(s).add(i as usize);
                            n += 1;
                        }
                    }
                }
                STRSXP_VAL => {
                    for i in 0..nc {
                        let count = *it.add(i as usize) as R_xlen_t;
                        for _j in 0..count {
                            SET_STRING_ELT(a, n, STRING_ELT(s, i));
                            n += 1;
                        }
                    }
                }
                VECSXP_VAL | EXPRSXP_VAL => {
                    for i in 0..nc {
                        let count = *it.add(i as usize) as R_xlen_t;
                        let elt = lazy_duplicate(VECTOR_ELT(s, i));
                        for _j in 0..count {
                            SET_VECTOR_ELT(a, n, elt);
                            n += 1;
                        }
                    }
                }
                RAWSXP_VAL => {
                    for i in 0..nc {
                        let count = *it.add(i as usize) as R_xlen_t;
                        for _j in 0..count {
                            *RAW(a).add(n as usize) = *RAW(s).add(i as usize);
                            n += 1;
                        }
                    }
                }
                _ => {
                    UNIMPLEMENTED_TYPE(b"rep2\0".as_ptr() as *const c_char, s);
                }
            }
        } else {
            let it = INTEGER(t);
            match stype {
                LGLSXP_VAL => {
                    for i in 0..nc {
                        for _j in 0..*it.add(i as usize) as R_xlen_t {
                            *LOGICAL(a).add(n as usize) = *LOGICAL(s).add(i as usize);
                            n += 1;
                        }
                    }
                }
                INTSXP_VAL => {
                    for i in 0..nc {
                        let count = *it.add(i as usize) as R_xlen_t;
                        for _j in 0..count {
                            *INTEGER(a).add(n as usize) = *INTEGER(s).add(i as usize);
                            n += 1;
                        }
                    }
                }
                REALSXP_VAL => {
                    for i in 0..nc {
                        let count = *it.add(i as usize) as R_xlen_t;
                        for _j in 0..count {
                            *REAL(a).add(n as usize) = *REAL(s).add(i as usize);
                            n += 1;
                        }
                    }
                }
                CPLXSXP_VAL => {
                    for i in 0..nc {
                        let count = *it.add(i as usize) as R_xlen_t;
                        for _j in 0..count {
                            *COMPLEX(a).add(n as usize) = *COMPLEX(s).add(i as usize);
                            n += 1;
                        }
                    }
                }
                STRSXP_VAL => {
                    for i in 0..nc {
                        let count = *it.add(i as usize) as R_xlen_t;
                        for _j in 0..count {
                            SET_STRING_ELT(a, n, STRING_ELT(s, i));
                            n += 1;
                        }
                    }
                }
                VECSXP_VAL | EXPRSXP_VAL => {
                    for i in 0..nc {
                        let count = *it.add(i as usize) as R_xlen_t;
                        let elt = lazy_duplicate(VECTOR_ELT(s, i));
                        for _j in 0..count {
                            SET_VECTOR_ELT(a, n, elt);
                            n += 1;
                        }
                    }
                }
                RAWSXP_VAL => {
                    for i in 0..nc {
                        let count = *it.add(i as usize) as R_xlen_t;
                        for _j in 0..count {
                            *RAW(a).add(n as usize) = *RAW(s).add(i as usize);
                            n += 1;
                        }
                    }
                }
                _ => {
                    UNIMPLEMENTED_TYPE(b"rep2\0".as_ptr() as *const c_char, s);
                }
            }
        }

        a
    }
}

// ---------------------------------------------------------------------------
// rep3: rep_len(x, len), also used for rep.int() with scalar times
// ---------------------------------------------------------------------------

unsafe fn rep3(s: SEXP, ns: R_xlen_t, na: R_xlen_t) -> SEXP {
    unsafe {
        let a = Rf_allocVector(TYPEOF(s), na as c_int);

        let stype = TYPEOF(s);
        match stype {
            LGLSXP_VAL => {
                let sa = LOGICAL(s);
                let aa = LOGICAL(a);
                mod_iterate1!(na, ns, i, j, {
                    *aa.add(i as usize) = *sa.add(j as usize);
                });
            }
            INTSXP_VAL => {
                let sa = INTEGER(s);
                let aa = INTEGER(a);
                mod_iterate1!(na, ns, i, j, {
                    *aa.add(i as usize) = *sa.add(j as usize);
                });
            }
            REALSXP_VAL => {
                let sa = REAL(s);
                let aa = REAL(a);
                mod_iterate1!(na, ns, i, j, {
                    *aa.add(i as usize) = *sa.add(j as usize);
                });
            }
            CPLXSXP_VAL => {
                let sa = COMPLEX(s);
                let aa = COMPLEX(a);
                mod_iterate1!(na, ns, i, j, {
                    *aa.add(i as usize) = *sa.add(j as usize);
                });
            }
            RAWSXP_VAL => {
                let sa = RAW(s);
                let aa = RAW(a);
                mod_iterate1!(na, ns, i, j, {
                    *aa.add(i as usize) = *sa.add(j as usize);
                });
            }
            STRSXP_VAL => {
                mod_iterate1!(na, ns, i, j, {
                    SET_STRING_ELT(a, i, STRING_ELT(s, j));
                });
            }
            VECSXP_VAL | EXPRSXP_VAL => {
                mod_iterate1!(na, ns, i, j, {
                    SET_VECTOR_ELT(a, i, lazy_duplicate(VECTOR_ELT(s, j)));
                });
            }
            _ => {
                UNIMPLEMENTED_TYPE(b"rep3\0".as_ptr() as *const c_char, s);
            }
        }

        a
    }
}

// ---------------------------------------------------------------------------
// rep4 macros (must be defined before rep4 function)
// ---------------------------------------------------------------------------

/// Macro for rep4 switch loop with REAL times.
macro_rules! rep4_switch_loop {
    ($a:expr, $x:expr, $itimes:expr, $lx:expr, $len:expr, $each:expr, $done:expr) => {
        let xtype = TYPEOF($x);
        let mut k: R_xlen_t = 0;
        let mut k2: R_xlen_t = 0;
        match xtype {
            LGLSXP_VAL => {
                for i in 0..$lx {
                    let mut sum: R_xlen_t = 0;
                    for _j in 0..$each {
                        sum += *$itimes.add(k as usize) as R_xlen_t;
                        k += 1;
                    }
                    for _k3 in 0..sum {
                        *LOGICAL($a).add(k2 as usize) = *LOGICAL($x).add(i as usize);
                        k2 += 1;
                        if k2 == $len {
                            $done = true;
                            break;
                        }
                    }
                    if $done {
                        break;
                    }
                }
            }
            INTSXP_VAL => {
                for i in 0..$lx {
                    let mut sum: R_xlen_t = 0;
                    for _j in 0..$each {
                        sum += *$itimes.add(k as usize) as R_xlen_t;
                        k += 1;
                    }
                    for _k3 in 0..sum {
                        *INTEGER($a).add(k2 as usize) = *INTEGER($x).add(i as usize);
                        k2 += 1;
                        if k2 == $len {
                            $done = true;
                            break;
                        }
                    }
                    if $done {
                        break;
                    }
                }
            }
            REALSXP_VAL => {
                for i in 0..$lx {
                    let mut sum: R_xlen_t = 0;
                    for _j in 0..$each {
                        sum += *$itimes.add(k as usize) as R_xlen_t;
                        k += 1;
                    }
                    for _k3 in 0..sum {
                        *REAL($a).add(k2 as usize) = *REAL($x).add(i as usize);
                        k2 += 1;
                        if k2 == $len {
                            $done = true;
                            break;
                        }
                    }
                    if $done {
                        break;
                    }
                }
            }
            CPLXSXP_VAL => {
                for i in 0..$lx {
                    let mut sum: R_xlen_t = 0;
                    for _j in 0..$each {
                        sum += *$itimes.add(k as usize) as R_xlen_t;
                        k += 1;
                    }
                    for _k3 in 0..sum {
                        *COMPLEX($a).add(k2 as usize) = *COMPLEX($x).add(i as usize);
                        k2 += 1;
                        if k2 == $len {
                            $done = true;
                            break;
                        }
                    }
                    if $done {
                        break;
                    }
                }
            }
            STRSXP_VAL => {
                for i in 0..$lx {
                    let mut sum: R_xlen_t = 0;
                    for _j in 0..$each {
                        sum += *$itimes.add(k as usize) as R_xlen_t;
                        k += 1;
                    }
                    for _k3 in 0..sum {
                        SET_STRING_ELT($a, k2, STRING_ELT($x, i));
                        k2 += 1;
                        if k2 == $len {
                            $done = true;
                            break;
                        }
                    }
                    if $done {
                        break;
                    }
                }
            }
            VECSXP_VAL | EXPRSXP_VAL => {
                for i in 0..$lx {
                    let mut sum: R_xlen_t = 0;
                    for _j in 0..$each {
                        sum += *$itimes.add(k as usize) as R_xlen_t;
                        k += 1;
                    }
                    let elt = lazy_duplicate(VECTOR_ELT($x, i));
                    for _k3 in 0..sum {
                        SET_VECTOR_ELT($a, k2, elt);
                        k2 += 1;
                        if k2 == $len {
                            $done = true;
                            break;
                        }
                    }
                    if $done {
                        break;
                    }
                }
            }
            RAWSXP_VAL => {
                for i in 0..$lx {
                    let mut sum: R_xlen_t = 0;
                    for _j in 0..$each {
                        sum += *$itimes.add(k as usize) as R_xlen_t;
                        k += 1;
                    }
                    for _k3 in 0..sum {
                        *RAW($a).add(k2 as usize) = *RAW($x).add(i as usize);
                        k2 += 1;
                        if k2 == $len {
                            $done = true;
                            break;
                        }
                    }
                    if $done {
                        break;
                    }
                }
            }
            _ => {
                UNIMPLEMENTED_TYPE(b"rep4\0".as_ptr() as *const c_char, $x);
            }
        }
    };
}

/// Macro for rep4 switch loop with INTEGER times.
macro_rules! rep4_switch_loop_int {
    ($a:expr, $x:expr, $itimes:expr, $lx:expr, $len:expr, $each:expr, $done:expr) => {
        let xtype = TYPEOF($x);
        let mut k: R_xlen_t = 0;
        let mut k2: R_xlen_t = 0;
        match xtype {
            LGLSXP_VAL => {
                for i in 0..$lx {
                    let mut sum: R_xlen_t = 0;
                    for _j in 0..$each {
                        sum += *$itimes.add(k as usize) as R_xlen_t;
                        k += 1;
                    }
                    for _k3 in 0..sum {
                        *LOGICAL($a).add(k2 as usize) = *LOGICAL($x).add(i as usize);
                        k2 += 1;
                        if k2 == $len {
                            $done = true;
                            break;
                        }
                    }
                    if $done {
                        break;
                    }
                }
            }
            INTSXP_VAL => {
                for i in 0..$lx {
                    let mut sum: R_xlen_t = 0;
                    for _j in 0..$each {
                        sum += *$itimes.add(k as usize) as R_xlen_t;
                        k += 1;
                    }
                    for _k3 in 0..sum {
                        *INTEGER($a).add(k2 as usize) = *INTEGER($x).add(i as usize);
                        k2 += 1;
                        if k2 == $len {
                            $done = true;
                            break;
                        }
                    }
                    if $done {
                        break;
                    }
                }
            }
            REALSXP_VAL => {
                for i in 0..$lx {
                    let mut sum: R_xlen_t = 0;
                    for _j in 0..$each {
                        sum += *$itimes.add(k as usize) as R_xlen_t;
                        k += 1;
                    }
                    for _k3 in 0..sum {
                        *REAL($a).add(k2 as usize) = *REAL($x).add(i as usize);
                        k2 += 1;
                        if k2 == $len {
                            $done = true;
                            break;
                        }
                    }
                    if $done {
                        break;
                    }
                }
            }
            CPLXSXP_VAL => {
                for i in 0..$lx {
                    let mut sum: R_xlen_t = 0;
                    for _j in 0..$each {
                        sum += *$itimes.add(k as usize) as R_xlen_t;
                        k += 1;
                    }
                    for _k3 in 0..sum {
                        *COMPLEX($a).add(k2 as usize) = *COMPLEX($x).add(i as usize);
                        k2 += 1;
                        if k2 == $len {
                            $done = true;
                            break;
                        }
                    }
                    if $done {
                        break;
                    }
                }
            }
            STRSXP_VAL => {
                for i in 0..$lx {
                    let mut sum: R_xlen_t = 0;
                    for _j in 0..$each {
                        sum += *$itimes.add(k as usize) as R_xlen_t;
                        k += 1;
                    }
                    for _k3 in 0..sum {
                        SET_STRING_ELT($a, k2, STRING_ELT($x, i));
                        k2 += 1;
                        if k2 == $len {
                            $done = true;
                            break;
                        }
                    }
                    if $done {
                        break;
                    }
                }
            }
            VECSXP_VAL | EXPRSXP_VAL => {
                for i in 0..$lx {
                    let mut sum: R_xlen_t = 0;
                    for _j in 0..$each {
                        sum += *$itimes.add(k as usize) as R_xlen_t;
                        k += 1;
                    }
                    let elt = lazy_duplicate(VECTOR_ELT($x, i));
                    for _k3 in 0..sum {
                        SET_VECTOR_ELT($a, k2, elt);
                        k2 += 1;
                        if k2 == $len {
                            $done = true;
                            break;
                        }
                    }
                    if $done {
                        break;
                    }
                }
            }
            RAWSXP_VAL => {
                for i in 0..$lx {
                    let mut sum: R_xlen_t = 0;
                    for _j in 0..$each {
                        sum += *$itimes.add(k as usize) as R_xlen_t;
                        k += 1;
                    }
                    for _k3 in 0..sum {
                        *RAW($a).add(k2 as usize) = *RAW($x).add(i as usize);
                        k2 += 1;
                        if k2 == $len {
                            $done = true;
                            break;
                        }
                    }
                    if $done {
                        break;
                    }
                }
            }
            _ => {
                UNIMPLEMENTED_TYPE(b"rep4\0".as_ptr() as *const c_char, $x);
            }
        }
    };
}

// ---------------------------------------------------------------------------
// rep4: rep() allowing for both times and each
// ---------------------------------------------------------------------------

unsafe fn rep4(x: SEXP, times: SEXP, len: R_xlen_t, each: R_xlen_t, nt: R_xlen_t) -> SEXP {
    unsafe {
        let lx = XLENGTH(x);

        // Fast path for common special case
        if each == 1 && nt == 1 {
            return rep3(x, lx, len);
        }

        let a = Rf_allocVector(TYPEOF(x), len as c_int);
        let mut done = false;

        if nt == 1 {
            // Simple case: single times value with each > 1
            let xtype = TYPEOF(x);
            match xtype {
                LGLSXP_VAL => {
                    for i in 0..len {
                        *LOGICAL(a).add(i as usize) = *LOGICAL(x).add((i / each % lx) as usize);
                    }
                }
                INTSXP_VAL => {
                    for i in 0..len {
                        *INTEGER(a).add(i as usize) = *INTEGER(x).add((i / each % lx) as usize);
                    }
                }
                REALSXP_VAL => {
                    for i in 0..len {
                        *REAL(a).add(i as usize) = *REAL(x).add((i / each % lx) as usize);
                    }
                }
                CPLXSXP_VAL => {
                    for i in 0..len {
                        *COMPLEX(a).add(i as usize) = *COMPLEX(x).add((i / each % lx) as usize);
                    }
                }
                STRSXP_VAL => {
                    for i in 0..len {
                        SET_STRING_ELT(a, i, STRING_ELT(x, i / each % lx));
                    }
                }
                VECSXP_VAL | EXPRSXP_VAL => {
                    for i in 0..len {
                        let elt = lazy_duplicate(VECTOR_ELT(x, i / each % lx));
                        SET_VECTOR_ELT(a, i, elt);
                    }
                }
                RAWSXP_VAL => {
                    for i in 0..len {
                        *RAW(a).add(i as usize) = *RAW(x).add((i / each % lx) as usize);
                    }
                }
                _ => {
                    UNIMPLEMENTED_TYPE(b"rep4\0".as_ptr() as *const c_char, x);
                }
            }
        } else if TYPEOF(times) == REALSXP_VAL {
            let itimes = REAL(times);
            rep4_switch_loop!(a, x, itimes, lx, len, each, done);
        } else {
            let itimes = INTEGER(times);
            rep4_switch_loop_int!(a, x, itimes, lx, len, each, done);
        }

        a
    }
}

// ---------------------------------------------------------------------------
// do_rep_int: .Internal(rep.int(x, times))
// ---------------------------------------------------------------------------

// datetime_seq: Date / POSIXct support for seq(), mirroring stock R's S3
// methods seq.Date() and seq.POSIXt() (src/library/base/R/dates.R / dateTime.R).
// Returns Some(result) when either endpoint carries a datetime class,
// None otherwise so the plain numeric path runs.
// ---------------------------------------------------------------------------

unsafe fn first_str_elt(x: SEXP) -> Option<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != STRSXP_VAL || LENGTH(x) == 0 {
            return None;
        }
        let s = STRING_ELT(x, 0);
        if s.is_null() || s == crate::sexp::globals::R_NaString() {
            return None;
        }
        Some(
            CStr::from_ptr(translateChar(s))
                .to_string_lossy()
                .into_owned(),
        )
    }
}

unsafe fn datetime_class_of(x: SEXP) -> Option<DatetimeKind> {
    unsafe {
        if crate::mainutils::essentials::sexp_has_class(x, "POSIXct") {
            Some(DatetimeKind::Posixct)
        } else if crate::mainutils::essentials::sexp_has_class(x, "Date") {
            Some(DatetimeKind::Date)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum DatetimeKind {
    Date,
    Posixct,
}

/// pmatch(x, table) for a single string: an exact match wins, otherwise a
/// unique prefix match; ambiguity or no match is NA (stock pmatch).
fn pmatch_one(x: &str, table: &[&str]) -> Option<usize> {
    if let Some(i) = table.iter().position(|t| *t == x) {
        return Some(i);
    }
    let mut matches = table.iter().enumerate().filter(|(_, t)| t.starts_with(x));
    match (matches.next(), matches.next()) {
        (Some((i, _)), None) => Some(i),
        _ => None,
    }
}

/// as.integer() on a character multiplier: parse as a double and truncate
/// toward zero (as.integer("1.5") == 1L).  Non-numeric or out-of-range
/// strings give NA with stock's coercion warning.
unsafe fn as_integer_multiplier(call: SEXP, s: &str) -> Option<i64> {
    unsafe {
        let warn = |msg: &str| {
            let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
            warningcall(call, c_msg.as_ptr());
        };
        let t = s.trim();
        if t.is_empty() {
            warn("NAs introduced by coercion");
            return None;
        }
        match t.parse::<c_double>() {
            Ok(v) if v.is_finite() && v >= INT_MIN_C && v <= INT_MAX_C => Some(v as i64),
            Ok(_) => {
                // Numeric but outside the integer range: stock's distinct
                // "coercion to integer range" warning.
                warn("NAs introduced by coercion to integer range");
                None
            }
            Err(_) => {
                warn("NAs introduced by coercion");
                None
            }
        }
    }
}

/// strsplit(s, " ", fixed=TRUE): split on every single space, dropping the
/// trailing empty strings R's strsplit discards.
fn split_by_spaces(s: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = s.split(' ').collect();
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    parts
}

/// Which POSIXlt field a calendar `by` steps (seq.POSIXt months/years/
/// DSTdays handling).
#[derive(Clone, Copy, PartialEq)]
enum CalendarField {
    Months,
    Years,
    Dstdays,
}

/// UTC POSIXlt-style fields of an epoch value.  The runtime models
/// Date/POSIXct in UTC, which is also what stock uses for Date endpoints
/// (as.POSIXlt.Date is UTC midnight).
fn posixlt_fields(secs: c_double) -> Option<(i64, i64, i64, c_double)> {
    if secs.to_bits() == NA_REAL.to_bits() || !secs.is_finite() {
        return None;
    }
    let frac = secs - secs.floor();
    let whole = secs.floor() as i64;
    let days = whole.div_euclid(86_400);
    let tod = whole.rem_euclid(86_400) as c_double + frac;
    let (y, m, d) = crate::mainutils::essentials::civil_from_days(days);
    Some((y, m - 1, d, tod))
}

/// mktime-style recomposition: month and day overflow normalizes by
/// rolling into later months (linear civil-day arithmetic, like mktime).
fn mktime_utc(year: i64, mon0: i64, mday: i64, tod: c_double) -> c_double {
    let y = year + mon0.div_euclid(12);
    let m = mon0.rem_euclid(12) + 1;
    crate::mainutils::essentials::days_from_civil(y, m, mday) as c_double * 86_400.0 + tod
}

/// seq.int(from, to, by) over exact integers: returns the number of values
/// (from, from+by, ... <= to for by > 0), applying stock's error checks.
unsafe fn calendar_count(call: SEXP, from: i64, to: i64, by: i64) -> i64 {
    unsafe {
        if by == 0 {
            if from == to {
                return 1;
            }
            errorcall(
                call,
                b"invalid '(to - from)/by'\0".as_ptr() as *const c_char,
            );
        }
        let del = to - from;
        if del != 0 && (del > 0) != (by > 0) {
            errorcall(
                call,
                b"wrong sign in 'by' argument\0".as_ptr() as *const c_char,
            );
        }
        del / by + 1
    }
}

unsafe fn attach_datetime_class(ans: SEXP, kind: DatetimeKind, tz_source: SEXP) -> SEXP {
    unsafe {
        match kind {
            DatetimeKind::Date => {
                crate::mainutils::essentials::set_single_class(ans, "Date");
            }
            DatetimeKind::Posixct => {
                let tz = crate::mainutils::essentials::posixct_tzone_string(tz_source);
                crate::mainutils::essentials::set_posixct_class(ans, &tz);
            }
        }
        ans
    }
}

/// Calendar stepping for by = "months"/"quarters"/"years"/"DSTdays"
/// (seq.POSIXt's POSIXlt arithmetic, which seq.Date delegates to).
unsafe fn calendar_seq(
    call: SEXP,
    field: CalendarField,
    mult: i64,
    vanchor: c_double,
    vother: c_double,
    miss_to: bool,
    miss_from: bool,
    lout: R_xlen_t,
) -> Vec<c_double> {
    unsafe {
        // Anchor fields (lres <- as.POSIXlt(if from given from else to)).
        let Some((year, mon0, mday, tod)) = posixlt_fields(vanchor) else {
            // NA anchor: the from+to modes filter everything out; the
            // length.out modes propagate NA fields (seq.int on NA).
            return if miss_to || miss_from {
                vec![c_double::NAN; lout.max(0) as usize]
            } else {
                Vec::new()
            };
        };

        // Value at integer step k from the anchor.
        let value_at = |k: i64| -> c_double {
            match field {
                CalendarField::Months | CalendarField::Years => {
                    let mon_step = if field == CalendarField::Months {
                        mult
                    } else {
                        12 * mult
                    };
                    let mon_abs = (year * 12 + mon0) + k * mon_step;
                    mktime_utc(0, mon_abs, mday, tod)
                }
                CalendarField::Dstdays => mktime_utc(year, mon0, mday + k * mult, tod),
            }
        };

        if miss_to || miss_from {
            // length.out mode: exactly lout values anchored at the given
            // endpoint (seq.int(to/from = <field>, by = by, length.out)).
            let n = lout.max(0) as i64;
            return (0..n)
                .map(|i| {
                    if miss_from {
                        value_at(i - (n - 1))
                    } else {
                        value_at(i)
                    }
                })
                .collect();
        }

        // from + to + by: seq.int(<field>, <target field>, by) then keep
        // values not past `to` (seq.POSIXt's res[res <= cto] filter, which
        // drops a final month whose day-overflow passes the endpoint).
        let mut values: Vec<c_double> = if field == CalendarField::Dstdays {
            // "We might have a short day, so need to over-estimate":
            // length.out = 2 + floor((cto - cfrom)/(by * 86400)).
            if mult == 0 {
                errorcall(
                    call,
                    b"invalid '(to - from)/by'\0".as_ptr() as *const c_char,
                );
            }
            let span = (vother - vanchor) / (mult as c_double * 86_400.0);
            let n_est = 2.0 + span.floor();
            let n_est = if n_est.is_finite() && n_est >= 0.0 {
                n_est as i64
            } else {
                0
            };
            (0..n_est).map(value_at).collect()
        } else {
            let Some((to_year, to_mon0, _, _)) = posixlt_fields(vother) else {
                return Vec::new();
            };
            let count = if field == CalendarField::Years {
                calendar_count(call, year, to_year, mult)
            } else {
                calendar_count(call, year * 12 + mon0, to_year * 12 + to_mon0, mult)
            };
            (0..count).map(value_at).collect()
        };
        if mult > 0 {
            values.retain(|v| *v <= vother);
        } else {
            values.retain(|v| *v >= vother);
        }
        values
    }
}

unsafe fn datetime_seq(
    call: SEXP,
    from: SEXP,
    to: SEXP,
    by: SEXP,
    lout: R_xlen_t,
    miss_from: bool,
    miss_to: bool,
) -> Option<SEXP> {
    unsafe {
        // Classify endpoints.  The leading endpoint's class wins when the
        // two differ, mirroring UseMethod dispatch on the first argument of
        // the seq.Date / seq.POSIXt S3 methods.
        let kind: DatetimeKind = {
            let kf = if miss_from {
                None
            } else {
                datetime_class_of(from)
            };
            let kt = if miss_to { None } else { datetime_class_of(to) };
            match (kf, kt) {
                (Some(k), _) | (_, Some(k)) => k,
                _ => return None,
            }
        };

        let have_lout = lout != NA_INTEGER as R_xlen_t;
        let by_given = by != R_MissingArg() && by != R_NilValue();

        // seq.POSIXt: "exactly three of 'to', 'from', 'by' and
        // 'length.out' / 'along.with' must be specified", then the class /
        // length-1 checks on the supplied endpoints ('to' first).
        if kind == DatetimeKind::Posixct {
            let missing_count =
                miss_from as u32 + miss_to as u32 + (!have_lout) as u32 + (!by_given) as u32;
            if missing_count != 1 {
                errorcall(
                    call,
                    b"exactly three of 'to', 'from', 'by' and 'length.out' / 'along.with' must be specified\0"
                        .as_ptr() as *const c_char,
                );
            }
            if !miss_to {
                if !crate::mainutils::essentials::sexp_has_class(to, "POSIXct") {
                    errorcall(
                        call,
                        b"'to' must be a \"POSIXt\" object\0".as_ptr() as *const c_char,
                    );
                }
                if LENGTH(to) != 1 {
                    errorcall(
                        call,
                        b"'to' must be of length 1\0".as_ptr() as *const c_char,
                    );
                }
            }
            if !miss_from {
                if !crate::mainutils::essentials::sexp_has_class(from, "POSIXct") {
                    errorcall(
                        call,
                        b"'from' must be a \"POSIXt\" object\0".as_ptr() as *const c_char,
                    );
                }
                if LENGTH(from) != 1 {
                    errorcall(
                        call,
                        b"'from' must be of length 1\0".as_ptr() as *const c_char,
                    );
                }
            }
        } else if !by_given && (miss_from || miss_to) && !have_lout {
            // seq.Date without 'by'.
            errorcall(
                call,
                b"without 'by', when one of 'to', 'from' is missing, 'length.out' / 'along.with' must be specified\0"
                    .as_ptr() as *const c_char,
            );
        }

        // 'by' handling -----------------------------------------------------
        // Linear step in native units (days for Date, seconds for POSIXct).
        let mut rby: c_double = 1.0;
        let mut calendar: Option<(CalendarField, i64)> = None;
        if by_given {
            if LENGTH(by) != 1 {
                errorcall(
                    call,
                    b"'by' must be of length 1\0".as_ptr() as *const c_char,
                );
            }
            if kind == DatetimeKind::Date {
                let missing_count = miss_from as u32 + miss_to as u32 + (!have_lout) as u32;
                if missing_count != 1 {
                    errorcall(
                        call,
                        b"given 'by', exactly two of 'to', 'from' and 'length.out' / 'along.with' must be specified\0"
                            .as_ptr() as *const c_char,
                    );
                }
            }
            if TYPEOF(by) == STRSXP_VAL {
                // strsplit(by, " ", fixed = TRUE); an NA string gives NA
                // fields, so pmatch returns NA ("invalid string for 'by'").
                let text = match first_str_elt(by) {
                    Some(t) => t,
                    None => errorcall_never(call, "invalid string for 'by'"),
                };
                let parts = split_by_spaces(&text);
                if parts.is_empty() || parts.len() > 2 {
                    errorcall(call, b"invalid 'by' string\0".as_ptr() as *const c_char);
                }
                let last = parts[parts.len() - 1];
                let table: &[&str] = if kind == DatetimeKind::Date {
                    &["days", "weeks", "months", "quarters", "years"]
                } else {
                    &[
                        "secs", "mins", "hours", "days", "weeks", "months", "years", "DSTdays",
                        "quarters",
                    ]
                };
                // pmatch: unique prefix or exact; ambiguous (e.g. "m" for
                // POSIXct: mins/months) is NA -> "invalid string for 'by'".
                let valid = match pmatch_one(last, table) {
                    Some(v) => v,
                    None => errorcall_never(call, "invalid string for 'by'"),
                };
                let mult: i64 = if parts.len() == 2 {
                    match as_integer_multiplier(call, parts[0]) {
                        Some(m) => m,
                        None => errorcall_never(call, "'by' is NA"),
                    }
                } else {
                    1
                };

                if kind == DatetimeKind::Date {
                    match valid {
                        0 => rby = mult as c_double,
                        1 => rby = 7.0 * mult as c_double,
                        2 => calendar = Some((CalendarField::Months, mult)),
                        3 => calendar = Some((CalendarField::Months, 3 * mult)),
                        _ => calendar = Some((CalendarField::Years, mult)),
                    }
                } else {
                    match valid {
                        0 => rby = mult as c_double,
                        1 => rby = 60.0 * mult as c_double,
                        2 => rby = 3600.0 * mult as c_double,
                        3 => rby = 86_400.0 * mult as c_double,
                        4 => rby = 7.0 * 86_400.0 * mult as c_double,
                        5 => calendar = Some((CalendarField::Months, mult)),
                        6 => calendar = Some((CalendarField::Years, mult)),
                        7 => calendar = Some((CalendarField::Dstdays, mult)),
                        _ => calendar = Some((CalendarField::Months, 3 * mult)),
                    }
                }
            } else if TYPEOF(by) == REALSXP_VAL || TYPEOF(by) == INTSXP_VAL {
                rby = asReal(by);
                if ISNAN(rby) {
                    errorcall(call, b"'by' is NA\0".as_ptr() as *const c_char);
                }
            } else {
                errorcall(call, b"invalid mode for 'by'\0".as_ptr() as *const c_char);
            }
        }

        // Endpoints as raw numbers (days for Date, seconds for POSIXct).
        let vfrom = if miss_from {
            c_double::NAN
        } else {
            asReal(from)
        };
        let vto = if miss_to { c_double::NAN } else { asReal(to) };

        let build = |first: c_double, step: c_double, n: usize| -> Vec<c_double> {
            (0..n).map(|i| first + i as c_double * step).collect()
        };

        let values: Vec<c_double> = if let Some((field, mult)) = calendar {
            // Calendar arithmetic runs in epoch seconds (seq.POSIXt's
            // POSIXlt path, which seq.Date delegates to at UTC midnight);
            // Date endpoints are day values.
            let (anchor, other) = if miss_from {
                (vto, c_double::NAN)
            } else {
                (vfrom, vto)
            };
            let (anchor, other) = if kind == DatetimeKind::Date {
                (anchor * 86_400.0, other * 86_400.0)
            } else {
                (anchor, other)
            };
            let secs = calendar_seq(call, field, mult, anchor, other, miss_to, miss_from, lout);
            if kind == DatetimeKind::Date {
                secs.into_iter().map(|s| s / 86_400.0).collect()
            } else {
                secs
            }
        } else if miss_to {
            // from + (by|length.out): step forward from `from`.
            build(vfrom, rby, lout.max(0) as usize)
        } else if miss_from {
            // to + (by|length.out): step backward from `to`.
            let n = lout.max(0) as usize;
            let start = vto - (n as c_double - 1.0) * rby;
            build(start, rby, n)
        } else if have_lout {
            // from + to + length.out (or no 'by'): linear interpolation.
            let n = lout.max(0) as usize;
            if n == 0 {
                Vec::new()
            } else if n == 1 {
                vec![vfrom]
            } else {
                let step = (vto - vfrom) / (n as c_double - 1.0);
                build(vfrom, step, n)
            }
        } else if by_given {
            // from + to + by: seq.int(from, to, by) semantics.
            let del = vto - vfrom;
            let n = del / rby;
            if !n.is_finite() {
                errorcall(
                    call,
                    b"invalid '(to - from)/by'\0".as_ptr() as *const c_char,
                );
            }
            if n > 100.0 * INT_MAX_C {
                errorcall(
                    call,
                    b"'by' argument is much too small\0".as_ptr() as *const c_char,
                );
            }
            if n < -FEPS {
                errorcall(
                    call,
                    b"wrong sign in 'by' argument\0".as_ptr() as *const c_char,
                );
            }
            let nn = (n + FEPS) as i64;
            build(vfrom, rby, (nn + 1) as usize)
        } else {
            // Date from:to without 'by' (seq.int(from, to) colon steps by
            // one day in either direction).
            let del = vto - vfrom;
            if del == 0.0 {
                vec![vfrom]
            } else {
                let step = if del > 0.0 { 1.0 } else { -1.0 };
                let n = del.abs() as usize + 1;
                build(vfrom, step, n)
            }
        };

        // Emit the result vector --------------------------------------------
        let ans = Rf_allocVector(REALSXP_VAL, values.len() as c_int);
        let ra = REAL(ans);
        for (i, v) in values.iter().enumerate() {
            *ra.add(i) = *v;
        }
        Some(attach_datetime_class(
            ans,
            kind,
            if miss_from { to } else { from },
        ))
    }
}

// ---------------------------------------------------------------------------

pub unsafe fn do_rep_int(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = rho;
        checkArity(op, args);
        let s = CAR(args);
        let ncopy = CADR(args);
        let mut a: SEXP = ptr::null_mut();

        // DispatchOrEval internal generic: rep.int
        if DispatchOrEval(
            call,
            op,
            b"rep.int\0".as_ptr() as *const c_char,
            args,
            rho,
            &mut a,
            0,
            0,
        ) != 0
        {
            return a;
        }

        // DispatchOrEval internal generic: rep
        if inherits(s, b"factor\0".as_ptr() as *const c_char) == 0
            && DispatchOrEval(
                call,
                op,
                b"rep\0".as_ptr() as *const c_char,
                args,
                rho,
                &mut a,
                0,
                0,
            ) != 0
        {
            return a;
        }

        if isVector(ncopy) == 0 {
            return ptr::null_mut();
        }

        if isVector(s) == 0 && s != R_NilValue() {
            return ptr::null_mut();
        }

        let nc = XLENGTH(ncopy);
        if nc == XLENGTH(s) {
            a = rep2(s, ncopy);
        } else {
            if nc != 1 {
                return ptr::null_mut();
            }

            let ns = XLENGTH(s);
            let mut nc_val: R_xlen_t = 0;
            if TYPEOF(ncopy) != INTSXP_VAL {
                let snc = asReal(ncopy);
                if !R_FINITE(snc) || snc <= -1.0 || (ns > 0 && snc >= R_XLEN_T_MAX_DBL + 1.0) {
                    return ptr::null_mut();
                }
                nc_val = if ns == 0 { 1 } else { snc as R_xlen_t };
            } else {
                nc_val = asInteger(ncopy) as R_xlen_t;
                if nc_val as c_int == NA_INTEGER || nc_val < 0 {
                    return ptr::null_mut();
                }
            }
            if nc_val as c_double * ns as c_double > R_XLEN_T_MAX_DBL {
                return ptr::null_mut();
            }
            a = rep3(s, ns, nc_val * ns);
        }

        // _S4_rep_keepClass
        if IS_S4_OBJECT(s) != 0 {
            setAttrib(a, R_ClassSymbol(), getAttrib(s, R_ClassSymbol()));
            SET_S4_OBJECT(a);
        }

        if inherits(s, b"factor\0".as_ptr() as *const c_char) != 0 {
            if inherits(s, b"ordered\0".as_ptr() as *const c_char) != 0 {
                let tmp = Rf_allocVector(STRSXP_VAL, 2);
                SET_STRING_ELT(tmp, 0, Rf_mkChar(b"ordered\0".as_ptr() as *const c_char));
                SET_STRING_ELT(tmp, 1, Rf_mkChar(b"factor\0".as_ptr() as *const c_char));
                setAttrib(a, R_ClassSymbol(), tmp);
            } else {
                let tmp = Rf_mkString(b"factor\0".as_ptr() as *const c_char);
                setAttrib(a, R_ClassSymbol(), tmp);
            }
            setAttrib(a, R_LevelsSymbol(), getAttrib(s, R_LevelsSymbol()));
        }

        a
    }
}

// ---------------------------------------------------------------------------
// do_rep_len: .Internal(rep_len(x, length.out))
// ---------------------------------------------------------------------------

pub unsafe fn do_rep_len(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = rho;
        checkArity(op, args);

        let mut a: SEXP = ptr::null_mut();

        // DispatchOrEval internal generic: rep_len
        if DispatchOrEval(
            call,
            op,
            b"rep_len\0".as_ptr() as *const c_char,
            args,
            rho,
            &mut a,
            0,
            0,
        ) != 0
        {
            return a;
        }

        let s = CAR(args);

        // For objects that aren't factors, try dispatching to rep()
        if isObject(s) != 0 && inherits(s, b"factor\0".as_ptr() as *const c_char) == 0 {
            let rep_call = Rf_shallow_duplicate(call);
            SETCAR(
                rep_call,
                Rf_install_stub(b"rep\0".as_ptr() as *const c_char),
            );
            SETTAG(
                CDDR(rep_call),
                Rf_install_stub(b"length.out\0".as_ptr() as *const c_char),
            );
            SETTAG(
                CDR(args),
                Rf_install_stub(b"length.out\0".as_ptr() as *const c_char),
            );
            if DispatchOrEval(
                rep_call,
                op,
                b"rep\0".as_ptr() as *const c_char,
                args,
                rho,
                &mut a,
                0,
                0,
            ) != 0
            {
                return a;
            }
        }

        if isVector(s) == 0 && s != R_NilValue() {
            return ptr::null_mut();
        }

        let len = CADR(args);
        if LENGTH(len) != 1 {
            return ptr::null_mut();
        }

        let na: R_xlen_t;
        if TYPEOF(len) != INTSXP_VAL {
            let sna = asReal(len);
            if ISNAN(sna) || sna <= -1.0 || sna >= R_XLEN_T_MAX_DBL + 1.0 {
                return ptr::null_mut();
            }
            na = sna as R_xlen_t;
        } else {
            na = asInteger(len) as R_xlen_t;
            if na as c_int == NA_INTEGER || na < 0 {
                return ptr::null_mut();
            }
        }

        if TYPEOF(s) == NILSXP_VAL && na > 0 {
            return ptr::null_mut();
        }

        let ns = XLENGTH(s);
        if ns == 0 {
            a = Rf_duplicate(s);
            if na > 0 {
                a = xlengthgets(a, na);
            }
            return a;
        }

        a = rep3(s, ns, na);

        // _S4_rep_keepClass
        if IS_S4_OBJECT(s) != 0 {
            setAttrib(a, R_ClassSymbol(), getAttrib(s, R_ClassSymbol()));
            SET_S4_OBJECT(a);
        }

        if inherits(s, b"factor\0".as_ptr() as *const c_char) != 0 {
            if inherits(s, b"ordered\0".as_ptr() as *const c_char) != 0 {
                let tmp = Rf_allocVector(STRSXP_VAL, 2);
                SET_STRING_ELT(tmp, 0, Rf_mkChar(b"ordered\0".as_ptr() as *const c_char));
                SET_STRING_ELT(tmp, 1, Rf_mkChar(b"factor\0".as_ptr() as *const c_char));
                setAttrib(a, R_ClassSymbol(), tmp);
            } else {
                let tmp = Rf_mkString(b"factor\0".as_ptr() as *const c_char);
                setAttrib(a, R_ClassSymbol(), tmp);
            }
            setAttrib(a, R_LevelsSymbol(), getAttrib(s, R_LevelsSymbol()));
        }

        a
    }
}

// ---------------------------------------------------------------------------
// do_rep: rep() primitive
// ---------------------------------------------------------------------------

pub unsafe fn do_rep(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = rho;
        let mut ans: SEXP = ptr::null_mut();
        let mut x: SEXP = ptr::null_mut();
        let mut times: SEXP = R_NilValue();
        let mut len: R_xlen_t = NA_INTEGER as R_xlen_t;
        let mut each: R_xlen_t = 1;
        let mut nt: R_xlen_t = 0;

        // DispatchOrEval internal generic: rep
        if DispatchOrEval(
            call,
            op,
            b"rep\0".as_ptr() as *const c_char,
            args,
            rho,
            &mut ans,
            0,
            0,
        ) != 0
        {
            return ans;
        }
        // After DispatchOrEval, args have been evaluated
        let args = ans;

        // Argument matching for rep(x, times, length.out, each, ...)
        let formals = allocFormalsList5(
            Rf_install_stub(b"x\0".as_ptr() as *const c_char),
            Rf_install_stub(b"times\0".as_ptr() as *const c_char),
            Rf_install_stub(b"length.out\0".as_ptr() as *const c_char),
            Rf_install_stub(b"each\0".as_ptr() as *const c_char),
            R_DotsSymbol(),
        );
        let args = matchArgs_NR(formals, args, call);

        x = CAR(args);

        if TYPEOF(x) == LISTSXP_VAL {
            errorcall(
                call,
                b"replication of pairlists is defunct\0".as_ptr() as *const c_char,
            );
        }

        let lx = XLENGTH(x);

        // Parse length.out
        let length_out_arg = CADDR(args);
        if TYPEOF(length_out_arg) != INTSXP_VAL {
            let slen = asReal(length_out_arg);
            if R_FINITE(slen) {
                if slen <= -1.0 || slen >= R_XLEN_T_MAX_DBL + 1.0 {
                    errorcall(
                        call,
                        b"invalid 'length.out' argument\0".as_ptr() as *const c_char,
                    );
                }
                len = slen as R_xlen_t;
            } else {
                len = NA_INTEGER as R_xlen_t;
            }
        } else {
            len = asInteger(length_out_arg) as R_xlen_t;
            if len != NA_INTEGER as R_xlen_t && len < 0 {
                errorcall(
                    call,
                    b"invalid 'length.out' argument\0".as_ptr() as *const c_char,
                );
            }
        }

        // Parse each
        let each_arg = CADDDR(args);
        if TYPEOF(each_arg) != INTSXP_VAL {
            let seach = asReal(each_arg);
            if R_FINITE(seach) {
                if seach <= -1.0 || (lx > 0 && seach >= R_XLEN_T_MAX_DBL + 1.0) {
                    errorcall(call, b"invalid 'each' argument\0".as_ptr() as *const c_char);
                }
                each = if lx == 0 {
                    NA_INTEGER as R_xlen_t
                } else {
                    seach as R_xlen_t
                };
            } else {
                each = NA_INTEGER as R_xlen_t;
            }
        } else {
            each = asInteger(each_arg) as R_xlen_t;
            if each != NA_INTEGER as R_xlen_t && each < 0 {
                errorcall(call, b"invalid 'each' argument\0".as_ptr() as *const c_char);
            }
        }
        if each == NA_INTEGER as R_xlen_t {
            each = 1;
        }

        // Handle zero-length x
        if lx == 0 {
            let a = Rf_duplicate(x);
            if len != NA_INTEGER as R_xlen_t && len > 0 && x != R_NilValue() {
                return xlengthgets(a, len);
            }
            return a;
        }

        if isVector(x) == 0 {
            errorcall(
                call,
                b"attempt to replicate an object of type 'not-a-vector'\0".as_ptr()
                    as *const c_char,
            );
        }

        // Determine final length using 'times' and 'each'
        if len != NA_INTEGER as R_xlen_t {
            nt = 1;
        } else {
            let mut sum: c_double = 0.0;
            let times_arg = CADR(args);
            if times_arg == R_MissingArg() {
                times = ScalarInteger(1);
            } else if TYPEOF(times_arg) != INTSXP_VAL {
                times = coerceVector(times_arg, REALSXP_VAL);
            } else {
                times = coerceVector(times_arg, INTSXP_VAL);
            }
            nt = XLENGTH(times);
            if nt == 1 {
                let it: R_xlen_t;
                if TYPEOF(times) == REALSXP_VAL {
                    let rt = *REAL(times);
                    if ISNAN(rt) || rt <= -1.0 || rt >= R_XLEN_T_MAX_DBL + 1.0 {
                        errorcall(
                            call,
                            b"invalid 'times' argument\0".as_ptr() as *const c_char,
                        );
                    }
                    it = rt as R_xlen_t;
                } else {
                    it = *INTEGER(times) as R_xlen_t;
                    if it as c_int == NA_INTEGER || it < 0 {
                        errorcall(
                            call,
                            b"invalid 'times' argument\0".as_ptr() as *const c_char,
                        );
                    }
                }
                if lx as c_double * it as c_double * each as c_double > R_XLEN_T_MAX_DBL {
                    errorcall(
                        call,
                        b"length(x) * 'times' * 'each' is too large\0".as_ptr() as *const c_char,
                    );
                }
                len = lx * it * each;
            } else {
                if nt as c_double != lx as c_double * each as c_double {
                    errorcall(
                        call,
                        b"invalid 'times' argument\0".as_ptr() as *const c_char,
                    );
                }
                if TYPEOF(times) == REALSXP_VAL {
                    for i in 0..nt {
                        let rt = *REAL(times).add(i as usize);
                        if ISNAN(rt) || rt <= -1.0 || rt >= R_XLEN_T_MAX_DBL + 1.0 {
                            errorcall(
                                call,
                                b"invalid 'times' argument\0".as_ptr() as *const c_char,
                            );
                        }
                        sum += rt as R_xlen_t as c_double;
                    }
                } else {
                    for i in 0..nt {
                        let it = *INTEGER(times).add(i as usize);
                        if it == NA_INTEGER || it < 0 {
                            errorcall(
                                call,
                                b"invalid 'times' argument\0".as_ptr() as *const c_char,
                            );
                        }
                        sum += it as c_double;
                    }
                }
                if sum > R_XLEN_T_MAX_DBL {
                    errorcall(
                        call,
                        b"invalid 'times' argument\0".as_ptr() as *const c_char,
                    );
                }
                len = sum as R_xlen_t;
            }
        }

        if len > 0 && each == 0 {
            errorcall(call, b"invalid 'each' argument\0".as_ptr() as *const c_char);
        }

        let xn = getAttrib(x, R_NamesSymbol());
        ans = rep4(x, times, len, each, nt);

        // Date / POSIXct class restoration: rep4 replicates the raw payload,
        // so re-attach the datetime class (and tzone) like stock R's
        // rep.Date / rep.POSIXct S3 methods do.
        if crate::mainutils::essentials::sexp_has_class(x, "POSIXct") {
            let tz = crate::mainutils::essentials::posixct_tzone_string(x);
            crate::mainutils::essentials::set_posixct_class(ans, &tz);
        } else if crate::mainutils::essentials::sexp_has_class(x, "Date") {
            crate::mainutils::essentials::set_single_class(ans, "Date");
        }

        if XLENGTH(xn) > 0 {
            setAttrib(ans, R_NamesSymbol(), rep4(xn, times, len, each, nt));
        }

        // _S4_rep_keepClass
        if IS_S4_OBJECT(x) != 0 {
            setAttrib(ans, R_ClassSymbol(), getAttrib(x, R_ClassSymbol()));
            SET_S4_OBJECT(ans);
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// do_seq: seq.int() primitive
// ---------------------------------------------------------------------------

pub unsafe fn do_seq(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = rho;
        let mut ans: SEXP = R_NilValue();
        let one_arg = Rf_length(args) == 1;
        // DispatchOrEval internal generic: seq
        if DispatchOrEval(
            call,
            op,
            b"seq\0".as_ptr() as *const c_char,
            args,
            rho,
            &mut ans,
            0,
            1,
        ) != 0
        {
            return ans;
        }

        // Argument matching for seq.int(from, to, by, length.out, along.with, ...)
        let formals = allocFormalsList6(
            Rf_install_stub(b"from\0".as_ptr() as *const c_char),
            Rf_install_stub(b"to\0".as_ptr() as *const c_char),
            Rf_install_stub(b"by\0".as_ptr() as *const c_char),
            Rf_install_stub(b"length.out\0".as_ptr() as *const c_char),
            Rf_install_stub(b"along.with\0".as_ptr() as *const c_char),
            R_DotsSymbol(),
        );
        let matched_args = matchArgs_NR(formals, args, call);

        let from = CAR(matched_args);
        let to = CADR(matched_args);
        let by = CADDR(matched_args);
        let len_arg = CADDDR(matched_args);
        // 5th formal (along.with): CDDDR lands on the 4th cell
        // (length.out), so take its CDR's CAR.
        let along = CADR(CDDDR(matched_args));

        let miss_from = from == R_MissingArg();
        let miss_to = to == R_MissingArg();

        // Single-argument form: seq(n) or seq(scalar).  R evaluates this as
        // `1:n` (do_colon on the evaluated first argument), so non-numeric
        // scalars coerce via asReal (NA/NaN -> error), length > 1 warns and
        // uses the length, and the result keeps integer type for integral n.
        if one_arg && !miss_from {
            if from == R_NilValue() {
                ans = Rf_allocVector(INTSXP_VAL, 0);
            } else if LENGTH(from) == 0 {
                errorcall(call, b"argument of length 0\0".as_ptr() as *const c_char);
            } else if LENGTH(from) > 1 {
                warningcall(
                    call,
                    b"numerical expression has length > 1\0".as_ptr() as *const c_char,
                );
                let n = asReal(from);
                if ISNAN(n) {
                    errorcall(call, b"NA/NaN argument\0".as_ptr() as *const c_char);
                }
                ans = seq_colon(1.0, n, call);
            } else {
                let rfrom = asReal(from);
                if ISNAN(rfrom) {
                    errorcall(call, b"NA/NaN argument\0".as_ptr() as *const c_char);
                }
                ans = seq_colon(1.0, rfrom, call);
            }
            return ans;
        }

        // along.with handling
        let mut lout: R_xlen_t = NA_INTEGER as R_xlen_t;
        if along != R_MissingArg() {
            lout = xlength(along);
            if one_arg {
                if lout > 0 {
                    ans = seq_colon(1.0, lout as c_double, call);
                } else {
                    ans = Rf_allocVector(INTSXP_VAL, 0);
                }
                return ans;
            }
        } else if len_arg != R_MissingArg() && len_arg != R_NilValue() {
            let mut rout = asReal(len_arg);
            if !R_FINITE(rout) {
                errorcall(
                    call,
                    b"'length.out' must be a finite number\0".as_ptr() as *const c_char,
                );
            }
            if ISNAN(rout) || rout <= -0.5 {
                errorcall(
                    call,
                    b"'length.out' must be a non-negative number\0".as_ptr() as *const c_char,
                );
            }
            rout = rout.ceil();
            if rout >= R_XLEN_T_MAX_DBL {
                errorcall(
                    call,
                    b"result would be too long a vector\0".as_ptr() as *const c_char,
                );
            }
            lout = rout as R_xlen_t;
        }

        // ------------------------------------------------------------------
        // Date / POSIXct sequences.  In stock R these are handled by the S3
        // methods seq.Date / seq.POSIXt, which unclass the operands, delegate
        // the arithmetic to seq.int and re-attach the class attribute.
        // This runtime implements datetime classes natively (no R-level
        // methods), so mirror that behaviour here.
        // ------------------------------------------------------------------
        if let Some(result) = unsafe { datetime_seq(call, from, to, by, lout, miss_from, miss_to) }
        {
            return result;
        }

        if lout == NA_INTEGER as R_xlen_t {
            // No length.out or along.with: use from, to, by
            let rfrom = if miss_from {
                1.0
            } else {
                if LENGTH(from) != 1 {
                    errorcall(
                        call,
                        b"'from' must be of length 1\0".as_ptr() as *const c_char,
                    );
                }
                let v = asReal(from);
                if !R_FINITE(v) {
                    errorcall(
                        call,
                        b"'from' must be a finite number\0".as_ptr() as *const c_char,
                    );
                }
                v
            };

            let rto = if miss_to {
                1.0
            } else {
                if LENGTH(to) != 1 {
                    errorcall(
                        call,
                        b"'to' must be of length 1\0".as_ptr() as *const c_char,
                    );
                }
                let v = asReal(to);
                if !R_FINITE(v) {
                    errorcall(
                        call,
                        b"'to' must be a finite number\0".as_ptr() as *const c_char,
                    );
                }
                v
            };

            if by == R_MissingArg() {
                ans = seq_colon(rfrom, rto, call);
            } else {
                // 'by' specified
                if LENGTH(by) != 1 {
                    errorcall(
                        call,
                        b"'by' must be of length 1\0".as_ptr() as *const c_char,
                    );
                }
                let del = rto - rfrom;
                if del == 0.0 && rto == 0.0 {
                    return to;
                }
                let rby = asReal(by);
                if (rby == 1.0 && del > 0.0) || (rby == -1.0 && del < 0.0) {
                    ans = seq_colon(rfrom, rto, call);
                    return ans;
                }
                let finite_del = R_FINITE(del);
                let n = if finite_del {
                    del / rby
                } else {
                    rto / rby - rfrom / rby
                };
                if !R_FINITE(n) {
                    if del == 0.0 && rby == 0.0 {
                        return if miss_from { ScalarReal(rfrom) } else { from };
                    } else {
                        errorcall(
                            call,
                            b"invalid '(to - from)/by'\0".as_ptr() as *const c_char,
                        );
                    }
                }
                if finite_del && del.abs() / fmax2(rto.abs(), rfrom.abs()) < 100.0 * DBL_EPSILON_C {
                    return if miss_from { ScalarReal(rfrom) } else { from };
                }
                if n > 100.0 * INT_MAX_C {
                    errorcall(
                        call,
                        b"'by' argument is much too small\0".as_ptr() as *const c_char,
                    );
                }
                if n < -FEPS {
                    errorcall(
                        call,
                        b"wrong sign in 'by' argument\0".as_ptr() as *const c_char,
                    );
                }

                if (!miss_from || TYPEOF(from) == INTSXP_VAL)
                    && (!miss_to || TYPEOF(to) == INTSXP_VAL)
                    && TYPEOF(by) == INTSXP_VAL
                {
                    let nn = n as R_xlen_t;
                    ans = Rf_allocVector(INTSXP_VAL, (nn + 1) as c_int);
                    let ia = INTEGER(ans);
                    let ifrom = if miss_from {
                        rfrom as c_int
                    } else {
                        asInteger(from)
                    };
                    let iby = asInteger(by);
                    for i in 0..=nn {
                        *ia.add(i as usize) = ifrom + (i as c_int) * iby;
                    }
                } else {
                    let nn = (n + FEPS) as R_xlen_t;
                    ans = Rf_allocVector(REALSXP_VAL, (nn + 1) as c_int);
                    let ra = REAL(ans);
                    if finite_del {
                        for i in 0..=nn {
                            *ra.add(i as usize) = rfrom + i as c_double * rby;
                        }
                    } else {
                        let rfrom_scaled = rfrom / 4.0;
                        let rby_scaled = rby / 4.0;
                        for i in 0..=nn {
                            *ra.add(i as usize) = (rfrom_scaled + i as c_double * rby_scaled) * 4.0;
                        }
                    }
                    // Fix last element if overshoot
                    if nn > 0 {
                        let last = *ra.add(nn as usize);
                        if (rby > 0.0 && last > rto) || (rby < 0.0 && last < rto) {
                            *ra.add(nn as usize) = rto;
                        }
                    }
                }
            }
        } else if lout == 0 {
            ans = Rf_allocVector(INTSXP_VAL, 0);
        } else if one_arg {
            ans = seq_colon(1.0, lout as c_double, call);
        } else if by == R_MissingArg() {
            // length.out specified, by missing
            let mut rfrom = asReal(from);
            let mut rto = asReal(to);
            let mut rby: c_double = 0.0;
            if miss_to {
                rto = rfrom + (lout as c_double) - 1.0;
            }
            if miss_from {
                rfrom = rto - (lout as c_double) + 1.0;
            }
            if !R_FINITE(rfrom) {
                errorcall(
                    call,
                    b"'from' must be a finite number\0".as_ptr() as *const c_char,
                );
            }
            if !R_FINITE(rto) {
                errorcall(
                    call,
                    b"'to' must be a finite number\0".as_ptr() as *const c_char,
                );
            }
            let mut finite_del = false;
            if lout > 2 {
                let nint = (lout - 1) as c_double;
                let del = rto - rfrom;
                if R_FINITE(del) {
                    finite_del = true;
                    rby = del / nint;
                } else {
                    rby = rto / nint - rfrom / nint;
                }
            }
            if rfrom <= INT_MAX_C
                && rfrom >= INT_MIN_C
                && rto <= INT_MAX_C
                && rto >= INT_MIN_C
                && rfrom == rfrom as c_int as c_double
                && (lout <= 1 || rto == rto as c_int as c_double)
                && (lout <= 2 || rby == rby as c_int as c_double)
            {
                ans = Rf_allocVector(INTSXP_VAL, lout as c_int);
                *INTEGER(ans) = rfrom as c_int;
                if lout > 1 {
                    *INTEGER(ans).add((lout - 1) as usize) = rto as c_int;
                }
                if lout > 2 {
                    for i in 1..lout - 1 {
                        *INTEGER(ans).add(i as usize) = (rfrom + i as c_double * rby) as c_int;
                    }
                }
            } else {
                ans = Rf_allocVector(REALSXP_VAL, lout as c_int);
                *REAL(ans) = rfrom;
                if lout > 1 {
                    *REAL(ans).add((lout - 1) as usize) = rto;
                }
                if lout > 2 {
                    if finite_del {
                        for i in 1..lout - 1 {
                            *REAL(ans).add(i as usize) = rfrom + i as c_double * rby;
                        }
                    } else {
                        let rfrom_s = rfrom / 4.0;
                        let rby_s = rby / 4.0;
                        for i in 1..lout - 1 {
                            *REAL(ans).add(i as usize) = (rfrom_s + i as c_double * rby_s) * 4.0;
                        }
                    }
                }
            }
        } else if miss_to {
            // length.out and by specified, to missing
            let mut rfrom = asReal(from);
            let rby = asReal(by);
            if miss_from {
                rfrom = 1.0;
            }
            if !R_FINITE(rfrom) {
                errorcall(
                    call,
                    b"'from' must be a finite number\0".as_ptr() as *const c_char,
                );
            }
            if !R_FINITE(rby) {
                errorcall(
                    call,
                    b"'by' must be a finite number\0".as_ptr() as *const c_char,
                );
            }
            let rto = rfrom + (lout - 1) as c_double * rby;
            if rfrom <= INT_MAX_C
                && rfrom >= INT_MIN_C
                && rto <= INT_MAX_C
                && rto >= INT_MIN_C
                && rby == rby as c_int as c_double
                && rfrom == rfrom as c_int as c_double
            {
                ans = Rf_allocVector(INTSXP_VAL, lout as c_int);
                for i in 0..lout {
                    *INTEGER(ans).add(i as usize) = (rfrom + i as c_double * rby) as c_int;
                }
            } else {
                ans = Rf_allocVector(REALSXP_VAL, lout as c_int);
                for i in 0..lout {
                    *REAL(ans).add(i as usize) = rfrom + i as c_double * rby;
                }
            }
        } else if miss_from {
            // length.out and by specified, from missing
            let rto = asReal(to);
            let rby = asReal(by);
            let rfrom = rto - (lout - 1) as c_double * rby;
            if !R_FINITE(rto) {
                errorcall(
                    call,
                    b"'to' must be a finite number\0".as_ptr() as *const c_char,
                );
            }
            if !R_FINITE(rby) {
                errorcall(
                    call,
                    b"'by' must be a finite number\0".as_ptr() as *const c_char,
                );
            }
            if rby == rby as c_int as c_double
                && rto == rto as c_int as c_double
                && rfrom <= INT_MAX_C
                && rfrom >= INT_MIN_C
                && rto <= INT_MAX_C
                && rto >= INT_MIN_C
            {
                ans = Rf_allocVector(INTSXP_VAL, lout as c_int);
                for i in 0..lout {
                    *INTEGER(ans).add(i as usize) =
                        (rto - (lout - 1 - i) as c_double * rby) as c_int;
                }
            } else {
                ans = Rf_allocVector(REALSXP_VAL, lout as c_int);
                for i in 0..lout {
                    *REAL(ans).add(i as usize) = rto - (lout - 1 - i) as c_double * rby;
                }
            }
        } else {
            // Too many arguments
            errorcall(call, b"too many arguments\0".as_ptr() as *const c_char);
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_seq_along: seq_along()
// ---------------------------------------------------------------------------

pub unsafe fn do_seq_along(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, rho);
        checkArity(op, args);
        check1arg(args, call, b"along.with\0".as_ptr() as *const c_char);

        let len = XLENGTH(CAR(args));
        if len == 0 {
            Rf_allocVector(INTSXP_VAL, 0)
        } else {
            R_compact_intrange(1, len)
        }
    }
}

// ---------------------------------------------------------------------------
// do_seq_len: seq_len()
// ---------------------------------------------------------------------------

pub unsafe fn do_seq_len(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = rho;
        checkArity(op, args);
        check1arg(args, call, b"length.out\0".as_ptr() as *const c_char);

        if LENGTH(CAR(args)) != 1 {
            warningcall(
                call,
                b"first element used of 'length.out' argument\0".as_ptr() as *const c_char,
            );
        }

        let dlen = asReal(CAR(args));
        if !R_FINITE(dlen) || dlen < 0.0 {
            errorcall(
                call,
                b"argument must be coercible to non-negative integer\0".as_ptr() as *const c_char,
            );
        }
        if dlen >= R_XLEN_T_MAX_DBL {
            errorcall(
                call,
                b"result would be too long a vector\0".as_ptr() as *const c_char,
            );
        }
        let len = dlen as R_xlen_t;

        if len == 0 {
            Rf_allocVector(INTSXP_VAL, 0)
        } else {
            R_compact_intrange(1, len)
        }
    }
}

// ---------------------------------------------------------------------------
// do_sequence: sequence()
// ---------------------------------------------------------------------------

pub unsafe fn do_sequence(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, rho);
        checkArity(op, args);

        // User-facing signature: sequence(nvec, from = 1L, by = 1L,
        // recycle = FALSE). Stock R wraps
        // .Internal(sequence(nvec, from, by, recycle)) with as.integer()
        // coercion of the numeric arguments; apply the same defaults and
        // coercion here rather than exposing the raw internal signature.
        // Match user args by tag/position so named calls like by= bind correctly.
        let formals = allocFormalsList5(
            Rf_install_stub(b"nvec\0".as_ptr() as *const c_char),
            Rf_install_stub(b"from\0".as_ptr() as *const c_char),
            Rf_install_stub(b"by\0".as_ptr() as *const c_char),
            Rf_install_stub(b"recycle\0".as_ptr() as *const c_char),
            R_DotsSymbol(),
        );
        let matched = matchArgs_NR(formals, args, call);

        let lengths_arg = CAR(matched);
        if lengths_arg.is_null() || lengths_arg == R_NilValue() || lengths_arg == R_MissingArg() {
            error("argument \"nvec\" is missing, with no default");
        }
        let lengths: SEXP = if TYPEOF(lengths_arg) != INTSXP_VAL {
            coerceVector(lengths_arg, INTSXP_VAL)
        } else {
            lengths_arg
        };
        let from_arg = CADR(matched);
        let from: SEXP =
            if from_arg.is_null() || from_arg == R_NilValue() || from_arg == R_MissingArg() {
                ScalarInteger(1)
            } else if TYPEOF(from_arg) != INTSXP_VAL {
                coerceVector(from_arg, INTSXP_VAL)
            } else {
                from_arg
            };
        let by_arg = CADDR(matched);
        let by: SEXP = if by_arg.is_null() || by_arg == R_NilValue() || by_arg == R_MissingArg() {
            ScalarInteger(1)
        } else if TYPEOF(by_arg) != INTSXP_VAL {
            coerceVector(by_arg, INTSXP_VAL)
        } else {
            by_arg
        };
        let recycle_1st_arg = CADDDR(matched);
        let recycle_1st = if recycle_1st_arg.is_null()
            || recycle_1st_arg == R_NilValue()
            || recycle_1st_arg == R_MissingArg()
        {
            false
        } else {
            asBool2(recycle_1st_arg, call) != 0
        };
        let lengths_len = XLENGTH(lengths);
        let from_len = XLENGTH(from);
        let by_len = XLENGTH(by);

        // sequence(integer(0)) is integer(0) regardless of the other args.
        if lengths_len == 0 {
            return Rf_allocVector(INTSXP_VAL, 0);
        }

        if !recycle_1st && lengths_len != 0 {
            if from_len == 0 {
                error("'from' has length 0, but not 'nvec'; 'recycle = TRUE' returns empty here");
            }
            if by_len == 0 {
                error("'by' has length 0, but not 'nvec'; 'recycle = TRUE' returns empty here");
            }
        } else {
            if from_len == 0 || by_len == 0 {
                return Rf_allocVector(INTSXP_VAL, 0);
            }
        }

        let mut max_len = std::cmp::max(std::cmp::max(lengths_len, from_len), by_len);

        // A shorter 'nvec' with recycle = FALSE only uses the first
        // lengths_len inputs; warn (if `recycle` was not supplied) that
        // future R's default 'recycle = TRUE' will recycle 'nvec' -- at most
        // once per R session.
        if !recycle_1st && lengths_len < max_len {
            // C's maybe_warn: the R-level wrapper passes 0L when `recycle`
            // is missing; the port sees R_MissingArg directly.
            let maybe_warn = recycle_1st_arg == R_MissingArg();
            static WARN_1ST: AtomicBool = AtomicBool::new(true);
            if maybe_warn && WARN_1ST.swap(false, Ordering::Relaxed) {
                let msg = format!(
                    "length(nvec) = {} < {} = max(length(from), length(by))",
                    lengths_len, max_len
                );
                let c_msg = std::ffi::CString::new(format!(
                    "{} -- future R's default 'recycle = TRUE' will recycle 'nvec'",
                    msg
                ))
                .unwrap_or_default();
                warningcall(R_NilValue(), c_msg.as_ptr());
            }
            max_len = lengths_len;
        }

        let lengths_elt = INTEGER(lengths);

        // Calculate total length
        let mut ans_len: R_xlen_t = 0;
        let mut i1: R_xlen_t = 0;
        for _i in 0..max_len {
            if recycle_1st && i1 >= lengths_len {
                i1 = 0;
            }
            let len_i = *lengths_elt.add(i1 as usize);
            if len_i == NA_INTEGER || len_i < 0 {
                error("'nvec' must be a vector of non-negative integers");
            }
            ans_len += len_i as R_xlen_t;
            i1 += 1;
        }

        let ans = Rf_allocVector(INTSXP_VAL, ans_len as c_int);
        let ans_elt = INTEGER(ans);
        let pfrom = INTEGER(from);
        let pby = INTEGER(by);

        let mut offset: R_xlen_t = 0;
        i1 = 0;
        let mut i2: R_xlen_t = 0;
        let mut i3: R_xlen_t = 0;
        for _i in 0..max_len {
            if recycle_1st && i1 >= lengths_len {
                i1 = 0;
            }
            if i2 >= from_len {
                i2 = 0;
            }
            if i3 >= by_len {
                i3 = 0;
            }
            let length_i = *lengths_elt.add(i1 as usize) as R_xlen_t;
            let from_val = *pfrom.add(i2 as usize);
            if length_i != 0 && from_val == NA_INTEGER {
                error("'from' contains NAs");
            }
            let by_val = *pby.add(i3 as usize);
            if length_i >= 2 && by_val == NA_INTEGER {
                error("'by' contains NAs");
            }
            let mut j = from_val;
            for _k in 0..length_i {
                *ans_elt.add(offset as usize) = j;
                j += by_val;
                offset += 1;
            }
            i1 += 1;
            i2 += 1;
            i3 += 1;
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::constructors::*;

    use super::*;

    /// Helper: create an integer vector with given values.
    unsafe fn make_int_vec(vals: &[c_int]) -> SEXP {
        unsafe {
            let v = Rf_allocVector(INTSXP_VAL, vals.len() as c_int);
            let data = INTEGER(v);
            for (i, &val) in vals.iter().enumerate() {
                *data.add(i) = val;
            }
            v
        }
    }

    /// Helper: create a real vector with given values.
    unsafe fn make_real_vec(vals: &[c_double]) -> SEXP {
        unsafe {
            let v = Rf_allocVector(REALSXP_VAL, vals.len() as c_int);
            let data = REAL(v);
            for (i, &val) in vals.iter().enumerate() {
                *data.add(i) = val;
            }
            v
        }
    }

    // -----------------------------------------------------------------------
    // seq_colon tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_seq_colon_simple_int_range() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let call = ptr::null_mut();
            let ans = seq_colon(1.0, 5.0, call);
            assert!(!ans.is_null());
            assert_eq!(TYPEOF(ans), INTSXP_VAL);
            assert_eq!(LENGTH(ans), 5);
            let data = INTEGER(ans);
            for i in 0..5 {
                assert_eq!(*data.add(i), (i + 1) as c_int);
            }
        }
    }

    #[test]
    fn test_seq_colon_descending() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let ans = seq_colon(5.0, 1.0, ptr::null_mut());
            assert!(!ans.is_null());
            assert_eq!(LENGTH(ans), 5);
            let data = INTEGER(ans);
            assert_eq!(*data.add(0), 5);
            assert_eq!(*data.add(4), 1);
        }
    }

    #[test]
    fn test_seq_colon_single_element() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let ans = seq_colon(3.0, 3.0, ptr::null_mut());
            assert!(!ans.is_null());
            assert_eq!(LENGTH(ans), 1);
            let data = INTEGER(ans);
            assert_eq!(*data.add(0), 3);
        }
    }

    #[test]
    fn test_seq_colon_real_range() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Non-integer values produce REALSXP
            let ans = seq_colon(1.5, 3.5, ptr::null_mut());
            assert!(!ans.is_null());
            assert_eq!(TYPEOF(ans), REALSXP_VAL);
            assert_eq!(LENGTH(ans), 3);
            let data = REAL(ans);
            assert!((*data.add(0) - 1.5).abs() < 1e-10);
            assert!((*data.add(1) - 2.5).abs() < 1e-10);
            assert!((*data.add(2) - 3.5).abs() < 1e-10);
        }
    }

    #[test]
    fn test_seq_colon_descending_real() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let ans = seq_colon(3.5, 1.5, ptr::null_mut());
            assert!(!ans.is_null());
            assert_eq!(TYPEOF(ans), REALSXP_VAL);
            assert_eq!(LENGTH(ans), 3);
            let data = REAL(ans);
            assert!((*data.add(0) - 3.5).abs() < 1e-10);
            assert!((*data.add(2) - 1.5).abs() < 1e-10);
        }
    }

    #[test]
    fn test_seq_colon_large_range_still_int() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Large range that fits in integer
            let ans = seq_colon(1.0, 100.0, ptr::null_mut());
            assert!(!ans.is_null());
            assert_eq!(LENGTH(ans), 100);
            let data = INTEGER(ans);
            assert_eq!(*data.add(0), 1);
            assert_eq!(*data.add(99), 100);
        }
    }

    // -----------------------------------------------------------------------
    // rep3 tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_rep3_basic_integer() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = make_int_vec(&[1, 2, 3]);
            let ans = rep3(s, 3, 9); // repeat 3-element vector 3 times
            assert!(!ans.is_null());
            assert_eq!(TYPEOF(ans), INTSXP_VAL);
            assert_eq!(LENGTH(ans), 9);
            let data = INTEGER(ans);
            assert_eq!(*data.add(0), 1);
            assert_eq!(*data.add(1), 2);
            assert_eq!(*data.add(2), 3);
            assert_eq!(*data.add(3), 1);
            assert_eq!(*data.add(4), 2);
            assert_eq!(*data.add(5), 3);
            assert_eq!(*data.add(6), 1);
            assert_eq!(*data.add(7), 2);
            assert_eq!(*data.add(8), 3);
        }
    }

    #[test]
    fn test_rep3_partial_cycle() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = make_int_vec(&[10, 20, 30]);
            let ans = rep3(s, 3, 5); // only 5 of the 6
            assert!(!ans.is_null());
            assert_eq!(LENGTH(ans), 5);
            let data = INTEGER(ans);
            assert_eq!(*data.add(0), 10);
            assert_eq!(*data.add(1), 20);
            assert_eq!(*data.add(2), 30);
            assert_eq!(*data.add(3), 10);
            assert_eq!(*data.add(4), 20);
        }
    }

    #[test]
    fn test_rep3_real_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = make_real_vec(&[1.5, 2.5]);
            let ans = rep3(s, 2, 4);
            assert!(!ans.is_null());
            assert_eq!(TYPEOF(ans), REALSXP_VAL);
            assert_eq!(LENGTH(ans), 4);
            let data = REAL(ans);
            assert!((*data.add(0) - 1.5).abs() < 1e-10);
            assert!((*data.add(1) - 2.5).abs() < 1e-10);
            assert!((*data.add(2) - 1.5).abs() < 1e-10);
            assert!((*data.add(3) - 2.5).abs() < 1e-10);
        }
    }

    #[test]
    fn test_rep3_zero_length_output() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = make_int_vec(&[1, 2, 3]);
            let ans = rep3(s, 3, 0);
            assert!(!ans.is_null());
            assert_eq!(LENGTH(ans), 0);
        }
    }

    #[test]
    fn test_rep3_unsupported_type_errors() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let extptr = crate::sexp::memory_ext::allocSExp(crate::sexp::ffi::SEXPTYPE::EXTPTRSXP);
            let err = std::panic::catch_unwind(|| {
                let _ = rep3(extptr, 1, 1);
            })
            .expect_err("unsupported rep3 type should raise an RError");
            let message = err
                .downcast_ref::<crate::sexp::context::RError>()
                .map(|err| err.message.as_str())
                .unwrap_or("");
            assert!(message.contains("rep3: unsupported SEXPTYPE"));
        }
    }

    // -----------------------------------------------------------------------
    // rep2 tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_rep2_vector_times() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = make_int_vec(&[1, 2, 3]);
            let ncopy = make_int_vec(&[2, 1, 3]);
            let ans = rep2(s, ncopy);
            assert!(!ans.is_null());
            assert_eq!(TYPEOF(ans), INTSXP_VAL);
            assert_eq!(LENGTH(ans), 6); // 2 + 1 + 3
            let data = INTEGER(ans);
            assert_eq!(*data.add(0), 1);
            assert_eq!(*data.add(1), 1);
            assert_eq!(*data.add(2), 2);
            assert_eq!(*data.add(3), 3);
            assert_eq!(*data.add(4), 3);
            assert_eq!(*data.add(5), 3);
        }
    }

    // -----------------------------------------------------------------------
    // do_seq_len tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_do_seq_len_simple() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let len_arg = make_int_vec(&[5]);
            let args = Rf_cons(len_arg, R_NilValue());
            let ans = do_seq_len(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!ans.is_null());
            assert_eq!(TYPEOF(ans), INTSXP_VAL);
            assert_eq!(LENGTH(ans), 5);
            let data = INTEGER(ans);
            assert_eq!(*data.add(0), 1);
            assert_eq!(*data.add(4), 5);
        }
    }

    #[test]
    fn test_do_seq_len_zero() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let len_arg = make_int_vec(&[0]);
            let args = Rf_cons(len_arg, R_NilValue());
            let ans = do_seq_len(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!ans.is_null());
            assert_eq!(LENGTH(ans), 0);
        }
    }

    #[test]
    fn test_do_seq_len_one() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let len_arg = make_int_vec(&[1]);
            let args = Rf_cons(len_arg, R_NilValue());
            let ans = do_seq_len(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!ans.is_null());
            assert_eq!(LENGTH(ans), 1);
            let data = INTEGER(ans);
            assert_eq!(*data.add(0), 1);
        }
    }

    // -----------------------------------------------------------------------
    // do_seq_along tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_do_seq_along() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_int_vec(&[10, 20, 30, 40]);
            let args = Rf_cons(x, R_NilValue());
            let ans = do_seq_along(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!ans.is_null());
            assert_eq!(TYPEOF(ans), INTSXP_VAL);
            assert_eq!(LENGTH(ans), 4);
            let data = INTEGER(ans);
            assert_eq!(*data.add(0), 1);
            assert_eq!(*data.add(3), 4);
        }
    }

    #[test]
    fn test_do_seq_along_empty() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = Rf_allocVector(INTSXP_VAL, 0);
            let args = Rf_cons(x, R_NilValue());
            let ans = do_seq_along(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!ans.is_null());
            assert_eq!(LENGTH(ans), 0);
        }
    }

    // -----------------------------------------------------------------------
    // do_sequence tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_do_sequence_basic() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let lengths = make_int_vec(&[3, 2]);
            let from = make_int_vec(&[1, 10]);
            let by = make_int_vec(&[1, 5]);
            let recycle = make_int_vec(&[1]);
            // args: (lengths, from, by, recycle)
            let a4 = Rf_cons(recycle, R_NilValue());
            let a3 = Rf_cons(by, a4);
            let a2 = Rf_cons(from, a3);
            let args = Rf_cons(lengths, a2);

            let ans = do_sequence(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!ans.is_null());
            assert_eq!(TYPEOF(ans), INTSXP_VAL);
            assert_eq!(LENGTH(ans), 5); // 3 + 2
            let data = INTEGER(ans);
            // First sequence: 1, 2, 3
            assert_eq!(*data.add(0), 1);
            assert_eq!(*data.add(1), 2);
            assert_eq!(*data.add(2), 3);
            // Second sequence: 10, 15
            assert_eq!(*data.add(3), 10);
            assert_eq!(*data.add(4), 15);
        }
    }

    #[test]
    fn test_do_sequence_empty() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // sequence() now exposes the user-facing signature
            // sequence(nvec, from = 1L, by = 1L, recycle = FALSE): the
            // defaults are supplied by the handler, so an empty nvec alone
            // must yield an empty result.
            let lengths = Rf_allocVector(INTSXP_VAL, 0);
            let args = Rf_cons(lengths, R_NilValue());

            let ans = do_sequence(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!ans.is_null());
            assert_eq!(LENGTH(ans), 0);
        }
    }

    // -----------------------------------------------------------------------
    // datetime seq tests (stock R 4.6.1 parity)
    // -----------------------------------------------------------------------

    #[test]
    fn test_pmatch_one_semantics() {
        let posixct_table = [
            "secs", "mins", "hours", "days", "weeks", "months", "years", "DSTdays", "quarters",
        ];
        let date_table = ["days", "weeks", "months", "quarters", "years"];
        // Ambiguous prefix is NA for POSIXct ("m" -> mins|months).
        assert_eq!(pmatch_one("m", &posixct_table), None);
        // But unique in the Date table ("m" -> months).
        assert_eq!(pmatch_one("m", &date_table), Some(2));
        assert_eq!(pmatch_one("month", &date_table), Some(2));
        assert_eq!(pmatch_one("DSTday", &posixct_table), Some(7));
        assert_eq!(pmatch_one("day", &posixct_table), Some(3));
        assert_eq!(pmatch_one("days", &posixct_table), Some(3));
        assert_eq!(pmatch_one("quarter", &posixct_table), Some(8));
        assert_eq!(pmatch_one("", &posixct_table), None);
        assert_eq!(pmatch_one("3", &posixct_table), None);
        assert_eq!(pmatch_one("secs", &date_table), None);
    }

    #[test]
    fn test_split_by_spaces_strsplit_semantics() {
        assert_eq!(split_by_spaces("3 months"), vec!["3", "months"]);
        assert_eq!(split_by_spaces("month"), vec!["month"]);
        assert_eq!(split_by_spaces(""), Vec::<&str>::new());
        // strsplit drops trailing empty strings only.
        assert_eq!(split_by_spaces("days "), vec!["days"]);
        assert_eq!(split_by_spaces(" days"), vec!["", "days"]);
        assert_eq!(split_by_spaces("1  days"), vec!["1", "", "days"]);
    }

    #[test]
    fn test_as_integer_multiplier_truncates_like_r() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(as_integer_multiplier(ptr::null_mut(), "3"), Some(3));
            // as.integer("1.5") == 1L: seq(..., by="1.5 days") steps a day.
            assert_eq!(as_integer_multiplier(ptr::null_mut(), "1.5"), Some(1));
            assert_eq!(as_integer_multiplier(ptr::null_mut(), "-2.9"), Some(-2));
            assert_eq!(as_integer_multiplier(ptr::null_mut(), "1e3"), Some(1000));
            assert_eq!(as_integer_multiplier(ptr::null_mut(), "abc"), None);
            assert_eq!(as_integer_multiplier(ptr::null_mut(), ""), None);
            // Out of integer range is NA too.
            assert_eq!(as_integer_multiplier(ptr::null_mut(), "1e10"), None);
        }
    }

    #[test]
    fn test_calendar_seq_months_matches_stock_dates() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let iso = |secs: c_double| {
                crate::mainutils::essentials::date_days_to_iso(secs / 86_400.0).unwrap()
            };
            // seq(as.Date('2020-01-31'), by = 'month', length.out = 3):
            // Feb 31 normalizes to Mar 2 (2020 is a leap year), then Mar 31.
            let anchor =
                crate::mainutils::essentials::days_from_civil(2020, 1, 31) as c_double * 86_400.0;
            let out = calendar_seq(
                ptr::null_mut(),
                CalendarField::Months,
                1,
                anchor,
                c_double::NAN,
                true,
                false,
                3,
            );
            let got: Vec<String> = out.iter().map(|s| iso(*s)).collect();
            assert_eq!(got, ["2020-01-31", "2020-03-02", "2020-03-31"]);

            // seq(as.Date('2020-02-29'), by = 'year', length.out = 3):
            // Feb 29 normalizes to Mar 1 in non-leap years.
            let anchor =
                crate::mainutils::essentials::days_from_civil(2020, 2, 29) as c_double * 86_400.0;
            let out = calendar_seq(
                ptr::null_mut(),
                CalendarField::Years,
                1,
                anchor,
                c_double::NAN,
                true,
                false,
                3,
            );
            let got: Vec<String> = out.iter().map(|s| iso(*s)).collect();
            assert_eq!(got, ["2020-02-29", "2021-03-01", "2022-03-01"]);
        }
    }

    #[test]
    fn test_calendar_seq_from_to_filters_endpoint() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let iso = |secs: c_double| {
                crate::mainutils::essentials::date_days_to_iso(secs / 86_400.0).unwrap()
            };
            // seq(as.Date('2020-06-30'), as.Date('2020-12-31'), by='month'):
            // day-30 stepping never hits Dec 31, so the endpoint is not
            // included (stock: 2020-06-30 .. 2020-12-30).
            let from =
                crate::mainutils::essentials::days_from_civil(2020, 6, 30) as c_double * 86_400.0;
            let to =
                crate::mainutils::essentials::days_from_civil(2020, 12, 31) as c_double * 86_400.0;
            let out = calendar_seq(
                ptr::null_mut(),
                CalendarField::Months,
                1,
                from,
                to,
                false,
                false,
                NA_INTEGER as R_xlen_t,
            );
            let got: Vec<String> = out.iter().map(|s| iso(*s)).collect();
            assert_eq!(
                got,
                [
                    "2020-06-30",
                    "2020-07-30",
                    "2020-08-30",
                    "2020-09-30",
                    "2020-10-30",
                    "2020-11-30",
                    "2020-12-30"
                ]
            );

            // to-anchored quarters keep the day-of-month:
            // seq(to = as.Date('2020-06-30'), by = 'quarter', length.out = 3)
            let to =
                crate::mainutils::essentials::days_from_civil(2020, 6, 30) as c_double * 86_400.0;
            let out = calendar_seq(
                ptr::null_mut(),
                CalendarField::Months,
                3,
                to,
                c_double::NAN,
                false,
                true,
                3,
            );
            let got: Vec<String> = out.iter().map(|s| iso(*s)).collect();
            assert_eq!(got, ["2019-12-30", "2020-03-30", "2020-06-30"]);

            // DSTdays over-estimate + filter:
            // seq(POSIXct 2020-01-01 .. 2020-01-05, by = '2 DSTdays')
            let from =
                crate::mainutils::essentials::days_from_civil(2020, 1, 1) as c_double * 86_400.0;
            let to =
                crate::mainutils::essentials::days_from_civil(2020, 1, 5) as c_double * 86_400.0;
            let out = calendar_seq(
                ptr::null_mut(),
                CalendarField::Dstdays,
                2,
                from,
                to,
                false,
                false,
                NA_INTEGER as R_xlen_t,
            );
            let got: Vec<i64> = out.iter().map(|s| (s / 86_400.0) as i64).collect();
            assert_eq!(
                got,
                [
                    crate::mainutils::essentials::days_from_civil(2020, 1, 1),
                    crate::mainutils::essentials::days_from_civil(2020, 1, 3),
                    crate::mainutils::essentials::days_from_civil(2020, 1, 5),
                ]
            );
        }
    }

    #[test]
    fn test_check1arg_partial_match_warning() {
        let mut session = crate::sexp::session::RSession::new();
        let _ = session.eval_script_with_output_capture("options(warnPartialMatchArgs = TRUE)");

        unsafe {
            // args cell: (l = 3L) checked against formal "length.out" —
            // "l" is a strict prefix, so a partial-argument-match warning
            // must be collected (default warn = 0).
            let args = Rf_cons(Rf_ScalarInteger(3), R_NilValue());
            let _args_guard = crate::sexp::protect::protect(args);
            SETTAG(args, Rf_install_stub(b"l\0".as_ptr() as *const c_char));
            check1arg(
                args,
                ptr::null_mut(),
                b"length.out\0".as_ptr() as *const c_char,
            );

            assert_eq!(crate::mainutils::errors::collect_warnings(), 1);
            let msg = crate::mainutils::errors::last_collected_warning_message();
            assert_eq!(msg.trim(), "partial argument match of 'l' to 'length.out'");

            // Full tag: no additional warning, no error.
            SETTAG(
                args,
                Rf_install_stub(b"length.out\0".as_ptr() as *const c_char),
            );
            check1arg(
                args,
                ptr::null_mut(),
                b"length.out\0".as_ptr() as *const c_char,
            );
            assert_eq!(crate::mainutils::errors::collect_warnings(), 1);

            // Non-matching tag errors (upstream: supplied argument name
            // '%s' does not match '%s').
            SETTAG(args, Rf_install_stub(b"bogus\0".as_ptr() as *const c_char));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                unsafe {
                    check1arg(
                        args,
                        ptr::null_mut(),
                        b"length.out\0".as_ptr() as *const c_char,
                    );
                }
            }));
            let err = result.unwrap_err();
            let payload = err
                .downcast_ref::<crate::sexp::context::RError>()
                .expect("RError payload");
            assert_eq!(
                payload.message.trim(),
                "supplied argument name 'bogus' does not match 'length.out'"
            );
        }
    }
}
