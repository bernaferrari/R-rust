#![allow(unused_imports)]
use super::helpers::mod_iterate1;
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
// rep2: rep.int(x, times) for a vector times
// ---------------------------------------------------------------------------

pub unsafe fn rep2(s: SEXP, ncopy: SEXP) -> SEXP {
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

pub unsafe fn rep3(s: SEXP, ns: R_xlen_t, na: R_xlen_t) -> SEXP {
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

pub unsafe fn rep4(x: SEXP, times: SEXP, len: R_xlen_t, each: R_xlen_t, nt: R_xlen_t) -> SEXP {
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
