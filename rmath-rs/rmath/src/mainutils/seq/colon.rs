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
// cross_colon: cross product of two factors
// ---------------------------------------------------------------------------

pub unsafe fn cross_colon(call: SEXP, s: SEXP, t: SEXP) -> SEXP {
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
        let la = Rf_mkString(c"factor".as_ptr());
        setAttrib(a, R_ClassSymbol(), la);
        a
    }
}

// ---------------------------------------------------------------------------
// seq_colon: core `:` operator implementation
// ---------------------------------------------------------------------------

pub unsafe fn seq_colon(n1: c_double, n2: c_double, call: SEXP) -> SEXP {
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
