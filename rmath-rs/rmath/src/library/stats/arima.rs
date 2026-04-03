#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

use std::os::raw::c_int;
use std::ptr;

use crate::main::coerce::asInteger;
use crate::main::coerce::asLogical;
use crate::main::coerce::asReal;
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::memory_ext::*;
use crate::sexp::protect::Rf_protect;
use crate::sexp::protect::Rf_unprotect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Helper: getListElement
// ---------------------------------------------------------------------------

unsafe fn getListElement(list: SEXP, str: &str) -> SEXP {
    if TYPEOF(list) != SEXPTYPE::VECSXP.0 {
        return R_NilValue();
    }
    let names = getAttrib(list, R_NamesSymbol());
    if names == R_NilValue() {
        return R_NilValue();
    }
    let len = Rf_length(list) as isize;
    let target = Rf_install(str);
    for i in 0..len {
        if STRING_ELT(names, i as R_xlen_t) != ptr::null_mut()
            && STRING_ELT(names, i as R_xlen_t) == target
        {
            return VECTOR_ELT(list, i as R_xlen_t);
        }
    }
    // Fallback: compare by name
    let cstr = std::ffi::CString::new(str).unwrap_or_default();
    for i in 0..len {
        let el = STRING_ELT(names, i as R_xlen_t);
        if !el.is_null() {
            let s = std::ffi::CStr::from_ptr(CHAR(el) as *const _);
            if s.to_bytes() == cstr.as_bytes() {
                return VECTOR_ELT(list, i as R_xlen_t);
            }
        }
    }
    R_NilValue()
}

// ---------------------------------------------------------------------------
// KalmanLike
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn KalmanLike(
    sy: SEXP,
    mod_: SEXP,
    sUP: SEXP,
    op: SEXP,
    update: SEXP,
) -> SEXP {
    let lop = asLogical(op);
    let mod_ = Rf_protect(duplicate(mod_));

    let sZ = getListElement(mod_, "Z");
    let sa = getListElement(mod_, "a");
    let sP = getListElement(mod_, "P");
    let sT = getListElement(mod_, "T");
    let sV = getListElement(mod_, "V");
    let sh = getListElement(mod_, "h");
    let sPn = getListElement(mod_, "Pn");

    if TYPEOF(sy) != SEXPTYPE::REALSXP.0
        || TYPEOF(sZ) != SEXPTYPE::REALSXP.0
        || TYPEOF(sa) != SEXPTYPE::REALSXP.0
        || TYPEOF(sP) != SEXPTYPE::REALSXP.0
        || TYPEOF(sPn) != SEXPTYPE::REALSXP.0
        || TYPEOF(sT) != SEXPTYPE::REALSXP.0
        || TYPEOF(sV) != SEXPTYPE::REALSXP.0
    {
        Rf_error(b"invalid argument type\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let n = Rf_length(sy) as isize;
    let p = Rf_length(sa) as isize;
    let y = REAL(sy);
    let Z = REAL(sZ);
    let T = REAL(sT);
    let V = REAL(sV);
    let P = REAL(sP);
    let a = REAL(sa);
    let Pnew = REAL(sPn);
    let h = asReal(sh);

    let mut anew = Vec::with_capacity(p);
    let mut M = Vec::with_capacity(p);
    let mut mm = Vec::with_capacity(p * p);
    unsafe {
        anew.set_len(p);
        M.set_len(p);
        mm.set_len(p * p);
    }

    let sup = asInteger(sUP);

    let ans: SEXP;
    let resid: SEXP;
    let states: SEXP;
    if lop != 0 {
        ans = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, 3));
        let r = Rf_allocVector3(SEXPTYPE::REALSXP.0, n as R_xlen_t);
        let st = Rf_allocVector3(SEXPTYPE::REALSXP.0, (n * p) as R_xlen_t);
        SET_VECTOR_ELT(ans, 1, r);
        SET_VECTOR_ELT(ans, 2, st);
        let nm = Rf_allocVector3(SEXPTYPE::STRSXP.0, 3);
        SET_STRING_ELT(nm, 0, Rf_mkChar("values"));
        SET_STRING_ELT(nm, 1, Rf_mkChar("resid"));
        SET_STRING_ELT(nm, 2, Rf_mkChar("states"));
        setAttrib(ans, R_NamesSymbol(), nm);
        Rf_unprotect(1);
        resid = r;
        states = st;
    } else {
        ans = R_NilValue();
        resid = ptr::null_mut();
        states = ptr::null_mut();
    }

    let mut sumlog = 0.0_f64;
    let mut ssq = 0.0_f64;
    let mut nu: isize = 0;

    for l in 0..n {
        // anew = T %*% a
        for i in 0..p {
            let mut tmp = 0.0_f64;
            for k in 0..p {
                tmp += T[i + p * k] * a[k];
            }
            anew[i] = tmp;
        }
        if l > sup {
            // mm = T %*% P %*% T'
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = 0.0_f64;
                    for k in 0..p {
                        tmp += T[i + p * k] * P[k + p * j];
                    }
                    mm[i + p * j] = tmp;
                }
            }
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = V[i + p * j];
                    for k in 0..p {
                        tmp += mm[i + p * k] * T[j + p * k];
                    }
                    Pnew[i + p * j] = tmp;
                }
            }
        }
        if !ISNAN(y[l]) {
            nu += 1;
            let mut resid0 = y[l];
            for i in 0..p {
                resid0 -= Z[i] * anew[i];
            }
            let mut gain = h;
            for i in 0..p {
                let mut tmp = 0.0_f64;
                for j in 0..p {
                    tmp += Pnew[i + j * p] * Z[j];
                }
                M[i] = tmp;
                gain += Z[i] * M[i];
            }
            ssq += resid0 * resid0 / gain;
            if lop != 0 {
                REAL(resid)[l] = resid0 / gain.sqrt();
            }
            sumlog += gain.ln();
            for i in 0..p {
                a[i] = anew[i] + M[i] * resid0 / gain;
            }
            for i in 0..p {
                for j in 0..p {
                    P[i + j * p] = Pnew[i + j * p] - M[i] * M[j] / gain;
                }
            }
        } else {
            for i in 0..p {
                a[i] = anew[i];
            }
            for i in 0..p * p {
                P[i] = Pnew[i];
            }
            if lop != 0 {
                REAL(resid)[l] = NA_REAL;
            }
        }
        if lop != 0 {
            let rs = REAL(states);
            for j in 0..p {
                rs[l + n * j] = a[j];
            }
        }
    }

    let res = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, 2));
    REAL(res)[0] = ssq / nu as f64;
    REAL(res)[1] = sumlog / nu as f64;

    if lop != 0 {
        SET_VECTOR_ELT(ans, 0, res);
        if asLogical(update) != 0 {
            setAttrib(ans, Rf_install("mod"), mod_);
        }
        Rf_unprotect(3);
        ans
    } else {
        if asLogical(update) != 0 {
            setAttrib(res, Rf_install("mod"), mod_);
        }
        Rf_unprotect(2);
        res
    }
}

// ---------------------------------------------------------------------------
// KalmanSmooth
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn KalmanSmooth(sy: SEXP, mod_: SEXP, sUP: SEXP) -> SEXP {
    let sZ = getListElement(mod_, "Z");
    let sa = getListElement(mod_, "a");
    let sP = getListElement(mod_, "P");
    let sT = getListElement(mod_, "T");
    let sV = getListElement(mod_, "V");
    let sh = getListElement(mod_, "h");
    let sPn = getListElement(mod_, "Pn");

    if TYPEOF(sy) != SEXPTYPE::REALSXP.0
        || TYPEOF(sZ) != SEXPTYPE::REALSXP.0
        || TYPEOF(sa) != SEXPTYPE::REALSXP.0
        || TYPEOF(sP) != SEXPTYPE::REALSXP.0
        || TYPEOF(sT) != SEXPTYPE::REALSXP.0
        || TYPEOF(sV) != SEXPTYPE::REALSXP.0
    {
        Rf_error(b"invalid argument type\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let n = Rf_length(sy) as isize;
    let p = Rf_length(sa) as isize;
    let y = REAL(sy);
    let Z = REAL(sZ);
    let T = REAL(sT);
    let V = REAL(sV);
    let h = asReal(sh);

    let ssa = Rf_protect(duplicate(sa));
    let a = REAL(ssa);
    let ssP = Rf_protect(duplicate(sP));
    let P = REAL(ssP);
    let ssPn = Rf_protect(duplicate(sPn));
    let Pnew = REAL(ssPn);

    let res = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, 2));
    let nm = Rf_allocVector3(SEXPTYPE::STRSXP.0, 2);
    SET_STRING_ELT(nm, 0, Rf_mkChar("smooth"));
    SET_STRING_ELT(nm, 1, Rf_mkChar("var"));
    setAttrib(res, R_NamesSymbol(), nm);
    Rf_unprotect(1);

    let states = Rf_allocVector3(SEXPTYPE::REALSXP.0, (n * p) as R_xlen_t);
    SET_VECTOR_ELT(res, 0, states);
    let at = REAL(states);

    let sN = Rf_allocVector3(SEXPTYPE::REALSXP.0, (n * p * p) as R_xlen_t);
    SET_VECTOR_ELT(res, 1, sN);
    let Nt = REAL(sN);

    let sup = asInteger(sUP);

    let mut anew = Vec::with_capacity(p);
    let mut M_arr = Vec::with_capacity(p);
    let mut mm = Vec::with_capacity(p * p);
    unsafe {
        anew.set_len(p);
        M_arr.set_len(p);
        mm.set_len(p * p);
    }

    let mut Pt = Vec::with_capacity(n * p * p);
    let mut gains = Vec::with_capacity(n);
    let mut resids = Vec::with_capacity(n);
    let mut Mt = Vec::with_capacity(n * p);
    let mut L = Vec::with_capacity(p * p);
    unsafe {
        Pt.set_len(n * p * p);
        gains.set_len(n);
        resids.set_len(n);
        Mt.set_len(n * p);
        L.set_len(p * p);
    }

    for l in 0..n {
        for i in 0..p {
            let mut tmp = 0.0_f64;
            for k in 0..p {
                tmp += T[i + p * k] * a[k];
            }
            anew[i] = tmp;
        }
        if l > sup {
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = 0.0_f64;
                    for k in 0..p {
                        tmp += T[i + p * k] * P[k + p * j];
                    }
                    mm[i + p * j] = tmp;
                }
            }
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = V[i + p * j];
                    for k in 0..p {
                        tmp += mm[i + p * k] * T[j + p * k];
                    }
                    Pnew[i + p * j] = tmp;
                }
            }
        }
        for i in 0..p {
            at[l + n * i] = anew[i];
        }
        for i in 0..p * p {
            Pt[l + n * i] = Pnew[i];
        }
        if !ISNAN(y[l]) {
            let mut resid0 = y[l];
            for i in 0..p {
                resid0 -= Z[i] * anew[i];
            }
            let mut gain = h;
            for i in 0..p {
                let mut tmp = 0.0_f64;
                for j in 0..p {
                    tmp += Pnew[i + j * p] * Z[j];
                }
                Mt[l + n * i] = M_arr[i] = tmp;
                gain += Z[i] * M_arr[i];
            }
            gains[l] = gain;
            resids[l] = resid0;
            for i in 0..p {
                a[i] = anew[i] + M_arr[i] * resid0 / gain;
            }
            for i in 0..p {
                for j in 0..p {
                    P[i + j * p] = Pnew[i + j * p] - M_arr[i] * M_arr[j] / gain;
                }
            }
        } else {
            for i in 0..p {
                a[i] = anew[i];
                Mt[l + n * i] = 0.0;
            }
            for i in 0..p * p {
                P[i] = Pnew[i];
            }
            gains[l] = NA_REAL;
            resids[l] = NA_REAL;
        }
    }

    // Backward pass
    let mut rt = Vec::with_capacity(n * p);
    unsafe {
        rt.set_len(n * p);
    }

    for l in (0..n).rev() {
        let gn: f64;
        if !ISNAN(gains[l]) {
            gn = 1.0 / gains[l];
            for i in 0..p {
                rt[l + n * i] = Z[i] * resids[l] * gn;
            }
        } else {
            for i in 0..p {
                rt[l + n * i] = 0.0;
            }
            gn = 0.0;
        }

        // N_{t-1} initialization
        for i in 0..p {
            for j in 0..p {
                Nt[l + n * i + n * p * j] = Z[i] * Z[j] * gn;
            }
        }

        if l < n - 1 {
            // compute r_{t-1}
            for i in 0..p {
                for j in 0..p {
                    mm[i + p * j] = if i == j { 1.0 } else { 0.0 } - Mt[l + n * i] * Z[j] * gn;
                }
            }
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = 0.0_f64;
                    for k in 0..p {
                        tmp += T[i + p * k] * mm[k + p * j];
                    }
                    L[i + p * j] = tmp;
                }
            }
            for i in 0..p {
                let mut tmp = 0.0_f64;
                for j in 0..p {
                    tmp += L[j + p * i] * rt[l + 1 + n * j];
                }
                rt[l + n * i] += tmp;
            }
            // compute N_{t-1}
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = 0.0_f64;
                    for k in 0..p {
                        tmp += L[k + p * i] * Nt[l + 1 + n * k + n * p * j];
                    }
                    mm[i + p * j] = tmp;
                }
            }
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = 0.0_f64;
                    for k in 0..p {
                        tmp += mm[i + p * k] * L[k + p * j];
                    }
                    Nt[l + n * i + n * p * j] += tmp;
                }
            }
        }

        for i in 0..p {
            let mut tmp = 0.0_f64;
            for j in 0..p {
                tmp += Pt[l + n * i + n * p * j] * rt[l + n * j];
            }
            at[l + n * i] += tmp;
        }
    }

    // Variance computation
    for l in 0..n {
        for i in 0..p {
            for j in 0..p {
                let mut tmp = 0.0_f64;
                for k in 0..p {
                    tmp += Pt[l + n * i + n * p * k] * Nt[l + n * k + n * p * j];
                }
                mm[i + p * j] = tmp;
            }
        }
        for i in 0..p {
            for j in 0..p {
                let mut tmp = Pt[l + n * i + n * p * j];
                for k in 0..p {
                    tmp -= mm[i + p * k] * Pt[l + n * k + n * p * j];
                }
                Nt[l + n * i + n * p * j] = tmp;
            }
        }
    }

    Rf_unprotect(4);
    res
}

// ---------------------------------------------------------------------------
// KalmanFore
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn KalmanFore(nahead: SEXP, mod_: SEXP, update: SEXP) -> SEXP {
    let mod_ = Rf_protect(duplicate(mod_));
    let sZ = getListElement(mod_, "Z");
    let sa = getListElement(mod_, "a");
    let sP = getListElement(mod_, "P");
    let sT = getListElement(mod_, "T");
    let sV = getListElement(mod_, "V");
    let sh = getListElement(mod_, "h");

    if TYPEOF(sZ) != SEXPTYPE::REALSXP.0
        || TYPEOF(sa) != SEXPTYPE::REALSXP.0
        || TYPEOF(sP) != SEXPTYPE::REALSXP.0
        || TYPEOF(sT) != SEXPTYPE::REALSXP.0
        || TYPEOF(sV) != SEXPTYPE::REALSXP.0
    {
        Rf_error(b"invalid argument type\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let n = asInteger(nahead) as isize;
    let p = Rf_length(sa) as isize;
    let Z = REAL(sZ);
    let a = REAL(sa);
    let P = REAL(sP);
    let T = REAL(sT);
    let V = REAL(sV);
    let h = asReal(sh);

    let mut anew = Vec::with_capacity(p);
    let mut Pnew = Vec::with_capacity(p * p);
    let mut mm = Vec::with_capacity(p * p);
    unsafe {
        anew.set_len(p);
        Pnew.set_len(p * p);
        mm.set_len(p * p);
    }

    let res = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, 2));
    let forecasts = Rf_allocVector3(SEXPTYPE::REALSXP.0, n as R_xlen_t);
    let se = Rf_allocVector3(SEXPTYPE::REALSXP.0, n as R_xlen_t);
    SET_VECTOR_ELT(res, 0, forecasts);
    SET_VECTOR_ELT(res, 1, se);
    let nm = Rf_allocVector3(SEXPTYPE::STRSXP.0, 2);
    SET_STRING_ELT(nm, 0, Rf_mkChar("pred"));
    SET_STRING_ELT(nm, 1, Rf_mkChar("var"));
    setAttrib(res, R_NamesSymbol(), nm);
    Rf_unprotect(1);

    for l in 0..n {
        let mut fc = 0.0_f64;
        for i in 0..p {
            let mut tmp = 0.0_f64;
            for k in 0..p {
                tmp += T[i + p * k] * a[k];
            }
            anew[i] = tmp;
            fc += tmp * Z[i];
        }
        for i in 0..p {
            a[i] = anew[i];
        }
        REAL(forecasts)[l] = fc;

        // Pnew = T P T' + V
        for i in 0..p {
            for j in 0..p {
                let mut tmp = 0.0_f64;
                for k in 0..p {
                    tmp += T[i + p * k] * P[k + p * j];
                }
                mm[i + p * j] = tmp;
            }
        }
        for i in 0..p {
            for j in 0..p {
                let mut tmp = V[i + p * j];
                for k in 0..p {
                    tmp += mm[i + p * k] * T[j + p * k];
                }
                Pnew[i + p * j] = tmp;
            }
        }
        let mut tmp = h;
        for i in 0..p {
            for j in 0..p {
                P[i + j * p] = Pnew[i + j * p];
                tmp += Z[i] * Z[j] * P[i + j * p];
            }
        }
        REAL(se)[l] = tmp;
    }

    if asLogical(update) != 0 {
        setAttrib(res, Rf_install("mod"), mod_);
    }
    Rf_unprotect(2);
    res
}

// ---------------------------------------------------------------------------
// partrans / invpartrans
// ---------------------------------------------------------------------------

unsafe fn partrans(p: isize, raw: &[f64], new_: &mut [f64]) {
    if p > 100 {
        Rf_error(b"can only transform 100 pars in arima0\0".as_ptr() as *const _);
        return;
    }
    let mut work = [0.0_f64; 100];

    // Step one: map (-Inf, Inf) to (-1, 1) via tanh
    for j in 0..p {
        work[j] = new_[j] = raw[j].tanh();
    }
    // Step two: Durbin-Levinson recursions
    for j in 1..p {
        let a = new_[j];
        for k in 0..j {
            work[k] -= a * new_[j - k - 1];
        }
        for k in 0..j {
            new_[k] = work[k];
        }
    }
}

unsafe fn invpartrans(p: isize, phi: &[f64], new_: &mut [f64]) {
    if p > 100 {
        Rf_error(b"can only transform 100 pars in arima0\0".as_ptr() as *const _);
        return;
    }
    let mut work = [0.0_f64; 100];

    for j in 0..p {
        work[j] = new_[j] = phi[j];
    }
    // Run Durbin-Levinson backwards
    for j in (1..p).rev() {
        let a = new_[j];
        for k in 0..j {
            work[k] = (new_[k] + a * new_[j - k - 1]) / (1.0 - a * a);
        }
        for k in 0..j {
            new_[k] = work[k];
        }
    }
    for j in 0..p {
        new_[j] = (1.0 - new_[j]) / (1.0 + new_[j]).ln(); // atanh
    }
}

// ---------------------------------------------------------------------------
// ARIMA_undoPars
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ARIMA_undoPars(sin: SEXP, sarma: SEXP) -> SEXP {
    let arma = INTEGER(sarma);
    let mp = *arma.add(0) as isize;
    let mq = *arma.add(1) as isize;
    let msp = *arma.add(2) as isize;
    let n = Rf_length(sin) as isize;
    let in_ = REAL(sin);

    let res = Rf_allocVector3(SEXPTYPE::REALSXP.0, n as R_xlen_t);
    let params = REAL(res);

    for i in 0..n {
        params[i] = in_[i];
    }
    if mp > 0 {
        let mut new_arr = [0.0_f64; 100];
        partrans(
            mp,
            std::slice::from_raw_parts(in_, mp as usize),
            &mut new_arr,
        );
        for i in 0..mp {
            params[i] = new_arr[i];
        }
    }
    let v = mp + mq;
    if msp > 0 {
        let mut new_arr = [0.0_f64; 100];
        partrans(
            msp,
            std::slice::from_raw_parts(in_.add(v), msp as usize),
            &mut new_arr,
        );
        for i in 0..msp {
            params[v + i] = new_arr[i];
        }
    }
    res
}

// ---------------------------------------------------------------------------
// ARIMA_transPars
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ARIMA_transPars(sin: SEXP, sarma: SEXP, strans: SEXP) -> SEXP {
    let arma = INTEGER(sarma);
    let trans = asLogical(strans);
    let mp = *arma.add(0) as isize;
    let mq = *arma.add(1) as isize;
    let msp = *arma.add(2) as isize;
    let msq = *arma.add(3) as isize;
    let ns = *arma.add(4) as isize;
    let p = mp + ns * msp;
    let q = mq + ns * msq;
    let in_ = REAL(sin);

    let res = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, 2));
    let sPhi = Rf_allocVector3(SEXPTYPE::REALSXP.0, p as R_xlen_t);
    let sTheta = Rf_allocVector3(SEXPTYPE::REALSXP.0, q as R_xlen_t);
    SET_VECTOR_ELT(res, 0, sPhi);
    SET_VECTOR_ELT(res, 1, sTheta);
    let phi = REAL(sPhi);
    let theta = REAL(sTheta);

    let mut params_raw: Vec<f64> = Vec::new();
    let params: *const f64;

    if trans != 0 {
        let nn = mp + mq + msp + msq;
        params_raw.resize(nn, 0.0);
        for i in 0..nn {
            params_raw[i] = in_[i];
        }
        if mp > 0 {
            let mut new_arr = [0.0_f64; 100];
            partrans(mp, &params_raw[..mp], &mut new_arr);
            for i in 0..mp {
                params_raw[i] = new_arr[i];
            }
        }
        let v = mp + mq;
        if msp > 0 {
            let mut new_arr = [0.0_f64; 100];
            partrans(msp, &params_raw[v..v + msp], &mut new_arr);
            for i in 0..msp {
                params_raw[v + i] = new_arr[i];
            }
        }
        params = params_raw.as_ptr();
    } else {
        params = in_;
    }

    if ns > 0 {
        // expand out seasonal ARMA models
        for i in 0..mp {
            phi[i] = *params.add(i);
        }
        for i in 0..mq {
            theta[i] = *params.add(i + mp);
        }
        for i in mp..p {
            phi[i] = 0.0;
        }
        for i in mq..q {
            theta[i] = 0.0;
        }
        for j in 0..msp {
            phi[(j + 1) * ns - 1] += *params.add(j + mp + mq);
            for i in 0..mp {
                phi[(j + 1) * ns + i] -= *params.add(i) * *params.add(j + mp + mq);
            }
        }
        for j in 0..msq {
            theta[(j + 1) * ns - 1] += *params.add(j + mp + mq + msp);
            for i in 0..mq {
                theta[(j + 1) * ns + i] += *params.add(i + mp) * *params.add(j + mp + mq + msp);
            }
        }
    } else {
        for i in 0..mp {
            phi[i] = *params.add(i);
        }
        for i in 0..mq {
            theta[i] = *params.add(i + mp);
        }
    }

    Rf_unprotect(1);
    res
}

// ---------------------------------------------------------------------------
// ARIMA_Invtrans
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ARIMA_Invtrans(in_: SEXP, sarma: SEXP) -> SEXP {
    let arma = INTEGER(sarma);
    let mp = *arma.add(0) as isize;
    let mq = *arma.add(1) as isize;
    let msp = *arma.add(2) as isize;
    let n = Rf_length(in_) as isize;
    let raw = REAL(in_);

    let y = Rf_allocVector3(SEXPTYPE::REALSXP.0, n as R_xlen_t);
    let new_ = REAL(y);

    for i in 0..n {
        new_[i] = raw[i];
    }
    if mp > 0 {
        let mut new_arr = [0.0_f64; 100];
        invpartrans(
            mp,
            std::slice::from_raw_parts(raw, mp as usize),
            &mut new_arr,
        );
        for i in 0..mp {
            new_[i] = new_arr[i];
        }
    }
    let v = mp + mq;
    if msp > 0 {
        let mut new_arr = [0.0_f64; 100];
        invpartrans(
            msp,
            std::slice::from_raw_parts(raw.add(v), msp as usize),
            &mut new_arr,
        );
        for i in 0..msp {
            new_[v + i] = new_arr[i];
        }
    }
    y
}

// ---------------------------------------------------------------------------
// ARIMA_Gradtrans
// ---------------------------------------------------------------------------

const ARIMA_EPS: f64 = 1e-3;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ARIMA_Gradtrans(in_: SEXP, sarma: SEXP) -> SEXP {
    let arma = INTEGER(sarma);
    let mp = *arma.add(0) as isize;
    let mq = *arma.add(1) as isize;
    let msp = *arma.add(2) as isize;
    let n = Rf_length(in_) as isize;
    let raw = REAL(in_);

    let y = Rf_allocVector3(SEXPTYPE::REALSXP.0, (n * n) as R_xlen_t);
    let A = REAL(y);

    for i in 0..n {
        for j in 0..n {
            A[i + j * n] = if i == j { 1.0 } else { 0.0 };
        }
    }

    let mut w1 = [0.0_f64; 100];
    let mut w2 = [0.0_f64; 100];
    let mut w3 = [0.0_f64; 100];

    if mp > 0 {
        for i in 0..mp {
            w1[i] = raw[i];
        }
        partrans(mp, &w1[..mp], &mut w2);
        for i in 0..mp {
            w1[i] += ARIMA_EPS;
            partrans(mp, &w1[..mp], &mut w3);
            for j in 0..mp {
                A[i + j * n] = (w3[j] - w2[j]) / ARIMA_EPS;
            }
            w1[i] -= ARIMA_EPS;
        }
    }
    if msp > 0 {
        let v = mp + mq;
        for i in 0..msp {
            w1[i] = raw[i + v];
        }
        partrans(msp, &w1[..msp], &mut w2);
        for i in 0..msp {
            w1[i] += ARIMA_EPS;
            partrans(msp, &w1[..msp], &mut w3);
            for j in 0..msp {
                A[i + v + (j + v) * n] = (w3[j] - w2[j]) / ARIMA_EPS;
            }
            w1[i] -= ARIMA_EPS;
        }
    }
    y
}

// ---------------------------------------------------------------------------
// ARIMA_Like
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ARIMA_Like(sy: SEXP, mod_: SEXP, sUP: SEXP, giveResid: SEXP) -> SEXP {
    let sPhi = getListElement(mod_, "phi");
    let sTheta = getListElement(mod_, "theta");
    let sDelta = getListElement(mod_, "Delta");
    let sa = getListElement(mod_, "a");
    let sP = getListElement(mod_, "P");
    let sPn = getListElement(mod_, "Pn");

    if TYPEOF(sPhi) != SEXPTYPE::REALSXP.0
        || TYPEOF(sTheta) != SEXPTYPE::REALSXP.0
        || TYPEOF(sDelta) != SEXPTYPE::REALSXP.0
        || TYPEOF(sa) != SEXPTYPE::REALSXP.0
        || TYPEOF(sP) != SEXPTYPE::REALSXP.0
        || TYPEOF(sPn) != SEXPTYPE::REALSXP.0
    {
        Rf_error(b"invalid argument type\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let n = Rf_length(sy) as isize;
    let rd = Rf_length(sa) as isize;
    let p = Rf_length(sPhi) as isize;
    let q = Rf_length(sTheta) as isize;
    let d = Rf_length(sDelta) as isize;
    let r = rd - d;
    let y = REAL(sy);
    let a = REAL(sa);
    let P = REAL(sP);
    let Pnew = REAL(sPn);
    let phi = REAL(sPhi);
    let theta = REAL(sTheta);
    let delta = REAL(sDelta);

    let sup = asInteger(sUP);
    let use_resid = asBool(giveResid) != 0;

    let mut sumlog = 0.0_f64;
    let mut ssq = 0.0_f64;
    let mut nu: isize = 0;

    let mut anew = Vec::with_capacity(rd);
    let mut M_arr = Vec::with_capacity(rd);
    let mut mm: Vec<f64> = Vec::new();
    anew.resize(rd, 0.0);
    M_arr.resize(rd, 0.0);
    if d > 0 {
        mm.resize(rd * rd, 0.0);
    }

    let sResid: SEXP;
    let rs_resid: *mut f64;
    if use_resid {
        sResid = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, n as R_xlen_t));
        rs_resid = REAL(sResid);
    } else {
        sResid = ptr::null_mut();
        rs_resid = ptr::null_mut();
    }

    for l in 0..n {
        // State prediction
        for i in 0..r {
            let mut tmp = if i < r - 1 { a[i + 1] } else { 0.0 };
            if i < p {
                tmp += phi[i] * a[0];
            }
            anew[i] = tmp;
        }
        if d > 0 {
            for i in (r + 1)..rd {
                anew[i] = a[i - 1];
            }
            let mut tmp = a[0];
            for i in 0..d {
                tmp += delta[i] * a[r + i];
            }
            anew[r] = tmp;
        }

        // Covariance prediction
        if l > sup {
            if d == 0 {
                for i in 0..r {
                    for j in 0..r {
                        let mut tmp = 0.0_f64;
                        if j == 0 {
                            tmp = 1.0;
                        } else if j - 1 < q {
                            tmp = theta[j - 1];
                        }
                        if j == 0 {
                            tmp = tmp;
                        } else if j - 1 < q {
                            tmp = tmp * theta[j - 1];
                        }
                        if i < p && j < p {
                            tmp += phi[i] * phi[j] * P[0];
                        }
                        if i < r - 1 && j < r - 1 {
                            tmp += P[i + 1 + r * (j + 1)];
                        }
                        if i < p && j < r - 1 {
                            tmp += phi[i] * P[j + 1];
                        }
                        if j < p && i < r - 1 {
                            tmp += phi[j] * P[i + 1];
                        }
                        Pnew[i + r * j] = tmp;
                    }
                }
            } else {
                // mm = TP
                for i in 0..r {
                    for j in 0..rd {
                        let mut tmp = 0.0_f64;
                        if i < p {
                            tmp += phi[i] * P[rd * j];
                        }
                        if i < r - 1 {
                            tmp += P[i + 1 + rd * j];
                        }
                        mm[i + rd * j] = tmp;
                    }
                }
                for j in 0..rd {
                    let mut tmp = P[rd * j];
                    for k in 0..d {
                        tmp += delta[k] * P[r + k + rd * j];
                    }
                    mm[r + rd * j] = tmp;
                }
                for i in 1..d {
                    for j in 0..rd {
                        mm[r + i + rd * j] = P[r + i - 1 + rd * j];
                    }
                }
                // Pnew = mmT'
                for i in 0..r {
                    for j in 0..rd {
                        let mut tmp = 0.0_f64;
                        if i < p {
                            tmp += phi[i] * mm[j];
                        }
                        if i < r - 1 {
                            tmp += mm[rd * (i + 1) + j];
                        }
                        Pnew[j + rd * i] = tmp;
                    }
                }
                for j in 0..rd {
                    let mut tmp = mm[j];
                    for k in 0..d {
                        tmp += delta[k] * mm[rd * (r + k) + j];
                    }
                    Pnew[rd * r + j] = tmp;
                }
                for i in 1..d {
                    for j in 0..rd {
                        Pnew[rd * (r + i) + j] = mm[rd * (r + i - 1) + j];
                    }
                }
                // Pnew += (1 theta) %o% (1 theta)
                for i in 0..=q {
                    let vi = if i == 0 { 1.0 } else { theta[i - 1] };
                    for j in 0..=q {
                        let vj = if j == 0 { 1.0 } else { theta[j - 1] };
                        Pnew[i + rd * j] += vi * vj;
                    }
                }
            }
        }

        if !ISNAN(y[l]) {
            let mut resid = y[l] - anew[0];
            for i in 0..d {
                resid -= delta[i] * anew[r + i];
            }
            for i in 0..rd {
                let mut tmp = Pnew[i];
                for j in 0..d {
                    tmp += Pnew[i + (r + j) * rd] * delta[j];
                }
                M_arr[i] = tmp;
            }
            let mut gain = M_arr[0];
            for j in 0..d {
                gain += delta[j] * M_arr[r + j];
            }
            if gain < 1e4 {
                nu += 1;
                ssq += resid * resid / gain;
                sumlog += gain.ln();
            }
            if use_resid {
                rs_resid[l] = resid / gain.sqrt();
            }
            for i in 0..rd {
                a[i] = anew[i] + M_arr[i] * resid / gain;
            }
            for i in 0..rd {
                for j in 0..rd {
                    P[i + j * rd] = Pnew[i + j * rd] - M_arr[i] * M_arr[j] / gain;
                }
            }
        } else {
            for i in 0..rd {
                a[i] = anew[i];
            }
            for i in 0..rd * rd {
                P[i] = Pnew[i];
            }
            if use_resid {
                rs_resid[l] = NA_REAL;
            }
        }
    }

    if use_resid {
        let res = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, 3));
        let nres = Rf_allocVector3(SEXPTYPE::REALSXP.0, 3);
        REAL(nres)[0] = ssq;
        REAL(nres)[1] = sumlog;
        REAL(nres)[2] = nu as f64;
        SET_VECTOR_ELT(res, 0, nres);
        SET_VECTOR_ELT(res, 1, sResid);
        Rf_unprotect(2);
        res
    } else {
        let nres = Rf_allocVector3(SEXPTYPE::REALSXP.0, 3);
        REAL(nres)[0] = ssq;
        REAL(nres)[1] = sumlog;
        REAL(nres)[2] = nu as f64;
        nres
    }
}

// ---------------------------------------------------------------------------
// ARIMA_CSS
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ARIMA_CSS(
    sy: SEXP,
    sarma: SEXP,
    sPhi: SEXP,
    sTheta: SEXP,
    sncond: SEXP,
    giveResid: SEXP,
) -> SEXP {
    let y = REAL(sy);
    let phi = REAL(sPhi);
    let theta = REAL(sTheta);
    let n = Rf_length(sy) as isize;
    let p = Rf_length(sPhi) as isize;
    let q = Rf_length(sTheta) as isize;
    let ncond = asInteger(sncond) as isize;
    let arma = INTEGER(sarma);
    let use_resid = asBool(giveResid) != 0;
    let ns = arma[4] as isize;

    let mut w = vec![0.0_f64; n];
    for l in 0..n {
        w[l] = y[l];
    }
    for _i in 0..arma[5] {
        for l in (1..n).rev() {
            w[l] -= w[l - 1];
        }
    }
    for _i in 0..arma[6] {
        for l in (ns..n).rev() {
            w[l] -= w[l - ns];
        }
    }

    let sResid = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, n as R_xlen_t));
    let resid = REAL(sResid);
    if use_resid {
        for l in 0..ncond {
            resid[l] = 0.0;
        }
    }

    let mut ssq = 0.0_f64;
    let mut nu: isize = 0;

    for l in ncond..n {
        let mut tmp = w[l];
        for j in 0..p {
            tmp -= phi[j] * w[l - j - 1];
        }
        let q_lim = std::cmp::min(l - ncond, q);
        for j in 0..q_lim {
            tmp -= theta[j] * resid[l - j - 1];
        }
        resid[l] = tmp;
        if !ISNAN(tmp) {
            nu += 1;
            ssq += tmp * tmp;
        }
    }

    if use_resid {
        let res = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP.0, 2));
        let val = Rf_ScalarReal(ssq / nu as f64);
        SET_VECTOR_ELT(res, 0, val);
        SET_VECTOR_ELT(res, 1, sResid);
        Rf_unprotect(2);
        res
    } else {
        Rf_unprotect(1);
        Rf_ScalarReal(ssq / nu as f64)
    }
}

// ---------------------------------------------------------------------------
// TSconv
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn TSconv(a: SEXP, b: SEXP) -> SEXP {
    let a = Rf_protect(coerceVector(a, SEXPTYPE::REALSXP.0));
    let b = Rf_protect(coerceVector(b, SEXPTYPE::REALSXP.0));
    let na = Rf_length(a) as isize;
    let nb = Rf_length(b) as isize;
    let nab = na + nb - 1;

    let ab = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, nab as R_xlen_t));
    let ra = REAL(a);
    let rb = REAL(b);
    let rab = REAL(ab);

    for i in 0..nab {
        rab[i] = 0.0;
    }
    for i in 0..na {
        for j in 0..nb {
            rab[i + j] += ra[i] * rb[j];
        }
    }

    Rf_unprotect(3);
    ab
}

// ---------------------------------------------------------------------------
// inclu2 (from AS154)
// ---------------------------------------------------------------------------

unsafe fn inclu2(
    np: usize,
    xnext: &mut [f64],
    xrow: &mut [f64],
    ynext: &mut f64,
    d: &mut [f64],
    rbar: &mut [f64],
    thetab: &mut [f64],
) {
    for i in 0..np {
        xrow[i] = xnext[i];
    }

    let mut ithisr: usize = 0;
    for i in 0..np {
        if xrow[i] != 0.0 {
            let xi = xrow[i];
            let di = d[i];
            let dpi = di + xi * xi;
            d[i] = dpi;
            let cbar = di / dpi;
            let sbar = xi / dpi;
            for k in (i + 1)..np {
                let xk = xrow[k];
                let rbthis = rbar[ithisr];
                xrow[k] = xk - xi * rbthis;
                rbar[ithisr] = cbar * rbthis + sbar * xk;
                ithisr += 1;
            }
            let xk = *ynext;
            *ynext = xk - xi * thetab[i];
            thetab[i] = cbar * thetab[i] + sbar * xk;
            if di == 0.0 {
                return;
            }
        } else {
            ithisr += np - i - 1;
        }
    }
}

// ---------------------------------------------------------------------------
// getQ0bis
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getQ0bis(sPhi: SEXP, sTheta: SEXP, _sTol: SEXP) -> SEXP {
    let p = Rf_length(sPhi) as isize;
    let q = Rf_length(sTheta) as isize;
    let phi = REAL(sPhi);
    let theta = REAL(sTheta);

    let r = if p > q + 1 { p } else { q + 1 };

    let res = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, (r * r) as R_xlen_t));
    let P = REAL(res);

    // Clean P
    for i in 0..(r * r) {
        P[i] = 0.0;
    }

    let mut ttheta = vec![0.0_f64; (q + 1) as usize];
    ttheta[0] = 1.0;
    for i in 1..=(q as usize) {
        ttheta[i] = theta[i - 1];
    }

    if p > 0 {
        let r2 = if p + q > p + 1 { p + q } else { p + 1 };
        let mut gam = vec![0.0_f64; (r2 * r2) as usize];
        let mut g = vec![0.0_f64; r2 as usize];

        let mut tphi = vec![0.0_f64; (p + 1) as usize];
        tphi[0] = 1.0;
        for i in 1..=(p as usize) {
            tphi[i] = -phi[i - 1];
        }

        // C1[E]
        for j in 0..r2 {
            for i in j..r2 {
                if (i - j) < (p + 1) {
                    gam[j * r2 + i] += tphi[(i - j) as usize];
                }
            }
        }
        // C2[E]
        for i in 0..r2 {
            for j in 1..r2 {
                if (i + j) < (p + 1) {
                    gam[j * r2 + i] += tphi[(i + j) as usize];
                }
            }
        }

        // g = (1 0 0 ... 0)
        g[0] = 1.0;
        for i in 1..r2 {
            g[i] = 0.0;
        }

        // rU = solve(Gam, g)
        let sgam = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, (r2 * r2) as R_xlen_t));
        for i in 0..(r2 * r2) {
            REAL(sgam)[i] = gam[i];
        }
        // reshape to matrix
        setAttrib(
            sgam,
            R_DimSymbol(),
            Rf_lang3(Rf_install("c"), Rf_ScalarInteger(r2), Rf_ScalarInteger(r2)),
        );

        let sg = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, r2 as R_xlen_t));
        for i in 0..r2 {
            REAL(sg)[i] = g[i];
        }

        let callS = Rf_protect(Rf_lang4(Rf_install("solve.default"), sgam, sg, _sTol));
        let su = Rf_protect(crate::eval::eval::Rf_eval(callS, R_BaseEnv()));
        let u = REAL(su);

        // Q0 += A1 A SU A^T A1^T
        for i in 0..r {
            for j in i..r {
                for k in 0..p {
                    if i + k < p {
                        for L in k..(q + 1) as usize {
                            for m in 0..p {
                                if j + m < p {
                                    for n_ in m..(q + 1) as usize {
                                        P[r * i + j] += phi[i + k]
                                            * phi[j + m]
                                            * ttheta[L - k]
                                            * ttheta[n_ - m]
                                            * u[(if L > n_ { L - n_ } else { n_ - L }).abs()];
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Rf_unprotect(4);

        // Compute correlation between X and Z
        let mut rrz = vec![0.0_f64; q as usize];
        if q > 0 {
            for i in 0..q {
                rrz[i] = ttheta[i as usize];
                for j in if i > p - 1 { i - (p - 1) } else { 0 }..i {
                    rrz[i] -= rrz[j] * tphi[(i - j) as usize];
                }
            }
        }

        // Q0 += A1 SXZ A2^T + transpose
        for i in 0..r {
            for j in i..r {
                for k in 0..p {
                    if i + k < p {
                        for L in (k + 1)..=(q) as usize {
                            if j + (L as isize) < q + 1 {
                                P[r * i + j] += phi[i + k]
                                    * ttheta[j + (L as isize)]
                                    * rrz[(L as isize) - k - 1];
                            }
                        }
                    }
                }
                for k in 0..p {
                    if j + k < p {
                        for L in (k + 1)..=(q) as usize {
                            if i + (L as isize) < q + 1 {
                                P[r * i + j] += phi[j + k]
                                    * ttheta[i + (L as isize)]
                                    * rrz[(L as isize) - k - 1];
                            }
                        }
                    }
                }
            }
        }
    }

    // Q0 += A2 A2^T
    for i in 0..r {
        for j in i..r {
            for k in 0..(q + 1) as usize {
                if j + (k as isize) < q + 1 {
                    P[r * i + j] += ttheta[i + (k as isize)] * ttheta[j + (k as isize)];
                }
            }
        }
    }

    // Symmetrize
    for i in 0..r {
        for j in (i + 1)..r {
            P[r * j + i] = P[r * i + j];
        }
    }

    Rf_unprotect(1);
    res
}

// ---------------------------------------------------------------------------
// getQ0
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getQ0(sPhi: SEXP, sTheta: SEXP) -> SEXP {
    let p = Rf_length(sPhi) as isize;
    let q = Rf_length(sTheta) as isize;
    let phi = REAL(sPhi);
    let theta = REAL(sTheta);

    let r = if p > q + 1 { p } else { q + 1 };

    if r > 350 {
        Rf_error(b"maximum supported lag is 350\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let np = (r * (r + 1)) / 2;
    let nrbar = np * (np - 1) / 2;

    let mut xnext = vec![0.0_f64; np];
    let mut xrow = vec![0.0_f64; np];
    let mut rbar = vec![0.0_f64; nrbar];
    let mut thetab = vec![0.0_f64; np];
    let mut V = vec![0.0_f64; np];

    for j in 0..r {
        let vj = if j == 0 {
            1.0
        } else if j - 1 < q {
            theta[j - 1]
        } else {
            0.0
        };
        for i in j..r {
            let vi = if i == 0 {
                1.0
            } else if i - 1 < q {
                theta[i - 1]
            } else {
                0.0
            };
            V[j + i * r - j * (j + 1) / 2 + j] = vi * vj;
        }
    }

    let res = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, (r * r) as R_xlen_t));
    let P = REAL(res);

    if r == 1 {
        if p == 0 {
            P[0] = 1.0;
        } else {
            P[0] = 1.0 / (1.0 - phi[0] * phi[0]);
        }
        Rf_unprotect(1);
        return res;
    }

    if p > 0 {
        for i in 0..nrbar {
            rbar[i] = 0.0;
        }
        for i in 0..np {
            P[i] = 0.0;
            thetab[i] = 0.0;
            xnext[i] = 0.0;
        }

        let mut ind: isize = 0;
        let mut ind1: isize = -1;
        let npr = np - r;
        let npr1 = npr + 1;
        let mut indj: isize = npr;
        let mut ind2: isize = npr - 1;

        for j in 0..r {
            let phij = if j < p { phi[j] } else { 0.0 };
            xnext[indj] = 0.0;
            indj += 1;
            let mut indi: isize = npr1 + j;
            for i in j..r {
                let mut ynext = V[ind as usize];
                let phii = if i < p { phi[i] } else { 0.0 };
                if j != r - 1 {
                    xnext[indj] = -phii;
                    if i != r - 1 {
                        xnext[indi] -= phij;
                        ind1 += 1;
                        xnext[ind1] = -1.0;
                    }
                }
                xnext[npr] = -phii * phij;
                ind2 += 1;
                if ind2 >= np as isize {
                    ind2 = 0;
                }
                xnext[ind2] += 1.0;
                inclu2(
                    np,
                    &mut xnext,
                    &mut xrow,
                    &mut ynext,
                    &mut P,
                    &mut rbar,
                    &mut thetab,
                );
                xnext[ind2] = 0.0;
                if i != r - 1 {
                    xnext[indi] = 0.0;
                    indi += 1;
                    xnext[ind1] = 0.0;
                }
            }
        }

        let mut ithisr: isize = nrbar as isize - 1;
        let mut im: isize = np as isize - 1;
        for i in 0..np {
            let mut bi = thetab[im as usize];
            let mut jm: isize = np as isize - 1;
            for _j in 0..i {
                bi -= rbar[ithisr as usize] * P[jm as usize];
                ithisr -= 1;
                jm -= 1;
            }
            P[im as usize] = bi;
            im -= 1;
        }

        // Re-order P
        ind = npr;
        for i in 0..r {
            xnext[i] = P[ind as usize];
            ind += 1;
        }
        ind = np as isize - 1;
        ind1 = npr - 1;
        for i in 0..npr {
            P[ind as usize] = P[ind1 as usize];
            ind -= 1;
            ind1 -= 1;
        }
        for i in 0..r {
            P[i] = xnext[i];
        }
    } else {
        let mut indn: isize = np as isize;
        let mut ind: isize = np as isize;
        for i in 0..r {
            for j in 0..=i {
                ind -= 1;
                P[ind as usize] = V[ind as usize];
                if j != 0 {
                    indn -= 1;
                    P[ind as usize] += P[indn as usize];
                }
            }
        }
    }

    // Unpack to full matrix
    let mut ind: isize = np as isize;
    for i in (1..r).rev() {
        for j in (i..r).rev() {
            ind -= 1;
            P[r * i + j] = P[ind as usize];
        }
    }
    for i in 0..r - 1 {
        for j in (i + 1)..r {
            P[i + r * j] = P[j + r * i];
        }
    }

    Rf_unprotect(1);
    res
}
