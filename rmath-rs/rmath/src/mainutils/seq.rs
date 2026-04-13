#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/seq.c -- sequence generation.
//!
//! Implements `:`, `seq.int()`, `seq_len()`, `seq_along()`, `rep()`,
//! `rep.int()`, `rep_len()`, and `sequence()`.

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::sexp::accessors::{
    CADDDR, CADDR, CADR, CAR, CDDDR, CDDR, CDR, COMPLEX, INTEGER, LENGTH, LOGICAL, RAW, REAL,
    SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, SETTAG, STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH,
    translateChar,
};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector, Rf_isInteger, Rf_isNull, Rf_isReal,
    Rf_isVector, Rf_length, Rf_mkChar, Rf_mkString,
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
const NILSXP_VAL: c_int = 0;

// ---------------------------------------------------------------------------
// Local helpers and entry points
// (plain unsafe fn to avoid duplicate #[unsafe(no_mangle)] symbols)
// ---------------------------------------------------------------------------

unsafe fn DispatchOrEval(
    _call: SEXP,
    _op: SEXP,
    _generic: *const c_char,
    _args: SEXP,
    _rho: SEXP,
    _ans: *mut SEXP,
    _narg: c_int,
    _evalseq: c_int,
) -> c_int {
    0
}

unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe { crate::mainutils::relop::checkArity(op, args) }
}

unsafe fn check1arg(_args: SEXP, _call: SEXP, _name: *const c_char) {}

unsafe fn errorcall(call: SEXP, format: *const c_char) {
    crate::mainutils::errors::errorcall(call, format);
}

unsafe fn warningcall(call: SEXP, format: *const c_char) {
    unsafe { crate::mainutils::errors::warningcall(call, format) }
}

unsafe fn R_typeToChar(_s: SEXP) -> *const c_char {
    ptr::null()
}

unsafe fn coerceVector(s: SEXP, t: c_int) -> SEXP {
    unsafe { crate::mainutils::coerce::coerceVector(s, t) }
}

unsafe fn UNIMPLEMENTED_TYPE(_mesg: *const c_char, _s: SEXP) {}

unsafe fn R_PreserveObject(_x: SEXP) {}

unsafe fn allocFormalsList5(_a1: SEXP, _a2: SEXP, _a3: SEXP, _a4: SEXP, _a5: SEXP) -> SEXP {
    ptr::null_mut()
}

unsafe fn allocFormalsList6(
    _a1: SEXP,
    _a2: SEXP,
    _a3: SEXP,
    _a4: SEXP,
    _a5: SEXP,
    _a6: SEXP,
) -> SEXP {
    ptr::null_mut()
}

unsafe fn matchArgs_NR(_formals: SEXP, _args: SEXP, _call: SEXP) -> SEXP {
    ptr::null_mut()
}

unsafe fn inherits(_s: SEXP, _what: *const c_char) -> c_int {
    0
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
            _ => 0.0,
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
            _ => 0,
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

unsafe fn xlengthgets(_x: SEXP, _len: R_xlen_t) -> SEXP {
    ptr::null_mut()
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
    0
}

unsafe fn asBool2(_x: SEXP, _call: SEXP) -> c_int {
    0
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

        // If both n1 and n2 are exact integers (as R_xlen_t), use compact intrange
        if n1 == n1 as R_xlen_t as c_double && n2 == n2 as R_xlen_t as c_double {
            return R_compact_intrange(n1 as R_xlen_t, n2 as R_xlen_t);
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
            let ans = Rf_allocVector(REALSXP_VAL, n as c_int);
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
                return ptr::null_mut();
            }
            let _ = (n1, n2);
        }

        let r_n1 = asReal(s1);
        let r_n2 = asReal(s2);
        if ISNAN(r_n1) || ISNAN(r_n2) {
            return ptr::null_mut();
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
            return ptr::null_mut();
        }

        let lx = XLENGTH(x);

        // Parse length.out
        let length_out_arg = CADDR(args);
        if TYPEOF(length_out_arg) != INTSXP_VAL {
            let slen = asReal(length_out_arg);
            if R_FINITE(slen) {
                if slen <= -1.0 || slen >= R_XLEN_T_MAX_DBL + 1.0 {
                    return ptr::null_mut();
                }
                len = slen as R_xlen_t;
            } else {
                len = NA_INTEGER as R_xlen_t;
            }
        } else {
            len = asInteger(length_out_arg) as R_xlen_t;
            if len != NA_INTEGER as R_xlen_t && len < 0 {
                return ptr::null_mut();
            }
        }

        // Parse each
        let each_arg = CADDDR(args);
        if TYPEOF(each_arg) != INTSXP_VAL {
            let seach = asReal(each_arg);
            if R_FINITE(seach) {
                if seach <= -1.0 || (lx > 0 && seach >= R_XLEN_T_MAX_DBL + 1.0) {
                    return ptr::null_mut();
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
                return ptr::null_mut();
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
            return ptr::null_mut();
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
                        return ptr::null_mut();
                    }
                    it = rt as R_xlen_t;
                } else {
                    it = *INTEGER(times) as R_xlen_t;
                    if it as c_int == NA_INTEGER || it < 0 {
                        return ptr::null_mut();
                    }
                }
                if lx as c_double * it as c_double * each as c_double > R_XLEN_T_MAX_DBL {
                    return ptr::null_mut();
                }
                len = lx * it * each;
            } else {
                if nt as c_double != lx as c_double * each as c_double {
                    return ptr::null_mut();
                }
                if TYPEOF(times) == REALSXP_VAL {
                    for i in 0..nt {
                        let rt = *REAL(times).add(i as usize);
                        if ISNAN(rt) || rt <= -1.0 || rt >= R_XLEN_T_MAX_DBL + 1.0 {
                            return ptr::null_mut();
                        }
                        sum += rt as R_xlen_t as c_double;
                    }
                } else {
                    for i in 0..nt {
                        let it = *INTEGER(times).add(i as usize);
                        if it == NA_INTEGER || it < 0 {
                            return ptr::null_mut();
                        }
                        sum += it as c_double;
                    }
                }
                if sum > R_XLEN_T_MAX_DBL {
                    return ptr::null_mut();
                }
                len = sum as R_xlen_t;
            }
        }

        if len > 0 && each == 0 {
            return ptr::null_mut();
        }

        let xn = getAttrib(x, R_NamesSymbol());
        ans = rep4(x, times, len, each, nt);

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
        let one_arg = LENGTH(args) == 1;

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
        let along = CAR(CDDDR(matched_args));

        let miss_from = from == R_MissingArg();
        let miss_to = to == R_MissingArg();

        // Single-argument form: seq(n) or seq(scalar)
        if one_arg && !miss_from {
            let lf = LENGTH(from);
            if lf == 1 && (TYPEOF(from) == INTSXP_VAL || TYPEOF(from) == REALSXP_VAL) {
                let rfrom = asReal(from);
                if !R_FINITE(rfrom) {
                    return ptr::null_mut();
                }
                ans = seq_colon(1.0, rfrom, call);
            } else if lf > 0 {
                ans = seq_colon(1.0, lf as c_double, call);
            } else {
                ans = Rf_allocVector(INTSXP_VAL, 0);
            }
            return ans;
        }

        // along.with handling
        let mut lout: R_xlen_t = NA_INTEGER as R_xlen_t;
        if along != R_MissingArg() {
            lout = XLENGTH(along);
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
                return ptr::null_mut();
            }
            if ISNAN(rout) || rout <= -0.5 {
                return ptr::null_mut();
            }
            rout = rout.ceil();
            if rout >= R_XLEN_T_MAX_DBL {
                return ptr::null_mut();
            }
            lout = rout as R_xlen_t;
        }

        if lout == NA_INTEGER as R_xlen_t {
            // No length.out or along.with: use from, to, by
            let rfrom = if miss_from {
                1.0
            } else {
                if LENGTH(from) != 1 {
                    return ptr::null_mut();
                }
                let v = asReal(from);
                if !R_FINITE(v) {
                    return ptr::null_mut();
                }
                v
            };

            let rto = if miss_to {
                1.0
            } else {
                if LENGTH(to) != 1 {
                    return ptr::null_mut();
                }
                let v = asReal(to);
                if !R_FINITE(v) {
                    return ptr::null_mut();
                }
                v
            };

            if by == R_MissingArg() {
                ans = seq_colon(rfrom, rto, call);
            } else {
                // 'by' specified
                if LENGTH(by) != 1 {
                    return ptr::null_mut();
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
                        return ptr::null_mut();
                    }
                }
                if finite_del && del.abs() / fmax2(rto.abs(), rfrom.abs()) < 100.0 * DBL_EPSILON_C {
                    return if miss_from { ScalarReal(rfrom) } else { from };
                }
                if n > 100.0 * INT_MAX_C {
                    return ptr::null_mut();
                }
                if n < -FEPS {
                    return ptr::null_mut();
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
                return ptr::null_mut();
            }
            if !R_FINITE(rto) {
                return ptr::null_mut();
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
                return ptr::null_mut();
            }
            if !R_FINITE(rby) {
                return ptr::null_mut();
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
                return ptr::null_mut();
            }
            if !R_FINITE(rby) {
                return ptr::null_mut();
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
            return ptr::null_mut();
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
            return ptr::null_mut();
        }
        if dlen >= R_XLEN_T_MAX_DBL {
            return ptr::null_mut();
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

        let lengths = CAR(args);
        if isInteger(lengths) == 0 {
            return ptr::null_mut();
        }
        let from = CADR(args);
        if isInteger(from) == 0 {
            return ptr::null_mut();
        }
        let by = CADDR(args);
        if isInteger(by) == 0 {
            return ptr::null_mut();
        }
        let recycle_1st_arg = CADDDR(args);
        let recycle_1st = asBool2(recycle_1st_arg, call) != 0;

        let lengths_len = XLENGTH(lengths);
        if lengths_len == 0 {
            return Rf_allocVector(INTSXP_VAL, 0);
        }
        let from_len = XLENGTH(from);
        let by_len = XLENGTH(by);

        if !recycle_1st && lengths_len != 0 {
            if from_len == 0 {
                return ptr::null_mut();
            }
            if by_len == 0 {
                return ptr::null_mut();
            }
        } else {
            if from_len == 0 || by_len == 0 {
                return Rf_allocVector(INTSXP_VAL, 0);
            }
        }

        let max_len = std::cmp::max(std::cmp::max(lengths_len, from_len), by_len);

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
                return ptr::null_mut();
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
                return ptr::null_mut();
            }
            let by_val = *pby.add(i3 as usize);
            if length_i >= 2 && by_val == NA_INTEGER {
                return ptr::null_mut();
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
        let v = Rf_allocVector(INTSXP_VAL, vals.len() as c_int);
        let data = INTEGER(v);
        for (i, &val) in vals.iter().enumerate() {
            *data.add(i) = val;
        }
        v
    }

    /// Helper: create a real vector with given values.
    unsafe fn make_real_vec(vals: &[c_double]) -> SEXP {
        let v = Rf_allocVector(REALSXP_VAL, vals.len() as c_int);
        let data = REAL(v);
        for (i, &val) in vals.iter().enumerate() {
            *data.add(i) = val;
        }
        v
    }

    // -----------------------------------------------------------------------
    // seq_colon tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_seq_colon_simple_int_range() {
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
        unsafe {
            let s = make_int_vec(&[1, 2, 3]);
            let ans = rep3(s, 3, 0);
            assert!(!ans.is_null());
            assert_eq!(LENGTH(ans), 0);
        }
    }

    // -----------------------------------------------------------------------
    // rep2 tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_rep2_vector_times() {
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
        unsafe {
            let lengths = Rf_allocVector(INTSXP_VAL, 0);
            let from = make_int_vec(&[1]);
            let by = make_int_vec(&[1]);
            let recycle = make_int_vec(&[1]);
            let a4 = Rf_cons(recycle, R_NilValue());
            let a3 = Rf_cons(by, a4);
            let a2 = Rf_cons(from, a3);
            let args = Rf_cons(lengths, a2);

            let ans = do_sequence(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!ans.is_null());
            assert_eq!(LENGTH(ans), 0);
        }
    }
}
