//! GNU `stats/src/arima.c` entry points used by `arima()`.

use crate::sexp::accessors::{
    CHAR, INTEGER, REAL, SET_VECTOR_ELT, STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::attrib_core::{R_NamesSymbol, getAttrib};
use crate::sexp::constructors::{Rf_ScalarReal, Rf_allocVector3};
use crate::sexp::ffi::{NA_REAL, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

fn partrans(p: usize, raw: &[f64], new: &mut [f64]) {
    if p > 100 {
        unsafe {
            crate::main::errors::Rf_error(
                b"can only transform 100 pars in arima0\0".as_ptr() as *const std::os::raw::c_char,
            );
        }
    }
    let mut work = [0.0f64; 100];
    for j in 0..p {
        work[j] = raw[j].tanh();
        new[j] = work[j];
    }
    for j in 1..p {
        let a = new[j];
        for k in 0..j {
            work[k] -= a * new[j - k - 1];
        }
        for k in 0..j {
            new[k] = work[k];
        }
    }
}

fn invpartrans(p: usize, phi: &[f64], new: &mut [f64]) {
    if p > 100 {
        unsafe {
            crate::main::errors::Rf_error(
                b"can only transform 100 pars in arima0\0".as_ptr() as *const std::os::raw::c_char,
            );
        }
    }
    let mut work = [0.0f64; 100];
    for j in 0..p {
        work[j] = phi[j];
        new[j] = phi[j];
    }
    for j in (1..p).rev() {
        let a = new[j];
        for k in 0..j {
            work[k] = (new[k] + a * new[j - k - 1]) / (1.0 - a * a);
        }
        for k in 0..j {
            new[k] = work[k];
        }
    }
    for j in 0..p {
        new[j] = new[j].atanh();
    }
}

unsafe fn list_elt(list: SEXP, name: &str) -> SEXP {
    unsafe {
        if TYPEOF(list) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names = getAttrib(list, R_NamesSymbol());
        if names.is_null() || names == R_NilValue() {
            return R_NilValue();
        }
        for i in 0..XLENGTH(list) {
            let nm = STRING_ELT(names, i);
            if nm.is_null() {
                continue;
            }
            if std::ffi::CStr::from_ptr(CHAR(nm)).to_bytes() == name.as_bytes() {
                return VECTOR_ELT(list, i);
            }
        }
        R_NilValue()
    }
}

pub unsafe extern "C-unwind" fn c_arima_trans_pars(sin: SEXP, sarma: SEXP, strans: SEXP) -> SEXP {
    unsafe {
        let arma = INTEGER(sarma);
        let trans = crate::mainutils::coerce::asLogical(strans) != 0;
        let mp = *arma as usize;
        let mq = *arma.add(1) as usize;
        let msp = *arma.add(2) as usize;
        let msq = *arma.add(3) as usize;
        let ns = *arma.add(4) as usize;
        let p = mp + ns * msp;
        let q = mq + ns * msq;
        let input = std::slice::from_raw_parts(REAL(sin), XLENGTH(sin) as usize);
        let mut params = input.to_vec();
        if trans {
            if mp > 0 {
                let mut mapped = params.clone();
                partrans(mp, input, &mut mapped);
                params[..mp].copy_from_slice(&mapped[..mp]);
            }
            let v = mp + mq;
            if msp > 0 {
                let mut mapped = params[v..].to_vec();
                partrans(msp, &input[v..], &mut mapped);
                params[v..v + msp].copy_from_slice(&mapped[..msp]);
            }
        }
        let res = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _g = protect(res);
        let sphi = Rf_allocVector3(SEXPTYPE::REALSXP, p as i64);
        let stheta = Rf_allocVector3(SEXPTYPE::REALSXP, q as i64);
        SET_VECTOR_ELT(res, 0, sphi);
        SET_VECTOR_ELT(res, 1, stheta);
        if p > 0 {
            let phi = REAL(sphi);
            for i in 0..p {
                *phi.add(i) = 0.0;
            }
            for i in 0..mp {
                *phi.add(i) = params[i];
            }
            if ns > 0 {
                for j in 0..msp {
                    let seasonal = params[j + mp + mq];
                    *phi.add((j + 1) * ns - 1) += seasonal;
                    for i in 0..mp {
                        *phi.add((j + 1) * ns + i) -= params[i] * seasonal;
                    }
                }
            }
        }
        if q > 0 {
            let theta = REAL(stheta);
            for i in 0..q {
                *theta.add(i) = 0.0;
            }
            for i in 0..mq {
                *theta.add(i) = params[i + mp];
            }
            if ns > 0 {
                for j in 0..msq {
                    let seasonal = params[j + mp + mq + msp];
                    *theta.add((j + 1) * ns - 1) += seasonal;
                    for i in 0..mq {
                        *theta.add((j + 1) * ns + i) += params[i + mp] * seasonal;
                    }
                }
            }
        }
        res
    }
}

pub unsafe extern "C-unwind" fn c_arima_undo_pars(sin: SEXP, sarma: SEXP) -> SEXP {
    unsafe {
        transform_ar_blocks(sin, sarma, true)
    }
}

pub unsafe extern "C-unwind" fn c_arima_invtrans(sin: SEXP, sarma: SEXP) -> SEXP {
    unsafe { transform_ar_blocks(sin, sarma, false) }
}

unsafe fn transform_ar_blocks(sin: SEXP, sarma: SEXP, forward: bool) -> SEXP {
    unsafe {
        let arma = INTEGER(sarma);
        let mp = *arma as usize;
        let mq = *arma.add(1) as usize;
        let msp = *arma.add(2) as usize;
        let n = XLENGTH(sin) as usize;
        let raw = std::slice::from_raw_parts(REAL(sin), n);
        let y = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let new = std::slice::from_raw_parts_mut(REAL(y), n);
        new.copy_from_slice(raw);
        if mp > 0 {
            if forward {
                partrans(mp, raw, new);
            } else {
                invpartrans(mp, raw, new);
            }
        }
        let v = mp + mq;
        if msp > 0 {
            if forward {
                partrans(msp, &raw[v..], &mut new[v..]);
            } else {
                invpartrans(msp, &raw[v..], &mut new[v..]);
            }
        }
        y
    }
}

pub unsafe extern "C-unwind" fn c_arima_gradtrans(sin: SEXP, sarma: SEXP) -> SEXP {
    unsafe {
        let arma = INTEGER(sarma);
        let mp = *arma as usize;
        let mq = *arma.add(1) as usize;
        let msp = *arma.add(2) as usize;
        let n = XLENGTH(sin) as usize;
        let raw = std::slice::from_raw_parts(REAL(sin), n);
        let y = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as i32, n as i32);
        let a = std::slice::from_raw_parts_mut(REAL(y), n * n);
        for j in 0..n {
            for i in 0..n {
                a[i + j * n] = if i == j { 1.0 } else { 0.0 };
            }
        }
        let eps = 1e-3;
        if mp > 0 {
            let mut w1 = raw[..mp].to_vec();
            let mut w2 = vec![0.0; mp];
            let mut w3 = vec![0.0; mp];
            partrans(mp, &w1, &mut w2);
            for i in 0..mp {
                w1[i] += eps;
                partrans(mp, &w1, &mut w3);
                for j in 0..mp {
                    a[i + j * n] = (w3[j] - w2[j]) / eps;
                }
                w1[i] -= eps;
            }
        }
        if msp > 0 {
            let v = mp + mq;
            let mut w1 = raw[v..v + msp].to_vec();
            let mut w2 = vec![0.0; msp];
            let mut w3 = vec![0.0; msp];
            partrans(msp, &w1, &mut w2);
            for i in 0..msp {
                w1[i] += eps;
                partrans(msp, &w1, &mut w3);
                for j in 0..msp {
                    a[i + v + (j + v) * n] = (w3[j] - w2[j]) / eps;
                }
                w1[i] -= eps;
            }
        }
        y
    }
}

pub unsafe extern "C-unwind" fn c_tsconv(a: SEXP, b: SEXP) -> SEXP {
    unsafe {
        let a = crate::main::coerce::coerceVector(a, SEXPTYPE::REALSXP.as_c_int());
        let b = crate::main::coerce::coerceVector(b, SEXPTYPE::REALSXP.as_c_int());
        let _a = protect(a);
        let _b = protect(b);
        let na = XLENGTH(a) as usize;
        let nb = XLENGTH(b) as usize;
        let nab = na + nb - 1;
        let ab = Rf_allocVector3(SEXPTYPE::REALSXP, nab as i64);
        if na > 0 && nb > 0 {
            let ra = REAL(a);
            let rb = REAL(b);
            let rab = REAL(ab);
            for i in 0..nab {
                *rab.add(i) = 0.0;
            }
            for i in 0..na {
                for j in 0..nb {
                    *rab.add(i + j) += *ra.add(i) * *rb.add(j);
                }
            }
        }
        ab
    }
}

pub unsafe extern "C-unwind" fn c_arima_css(
    sy: SEXP,
    sarma: SEXP,
    sphi: SEXP,
    stheta: SEXP,
    sncond: SEXP,
    give_resid: SEXP,
) -> SEXP {
    unsafe {
        let n = XLENGTH(sy) as usize;
        let y = REAL(sy);
        let arma = INTEGER(sarma);
        let p = XLENGTH(sphi) as usize;
        let q = XLENGTH(stheta) as usize;
        let ncond = crate::mainutils::coerce::asInteger(sncond) as usize;
        let phi = if p > 0 { REAL(sphi) } else { std::ptr::null() };
        let theta = if q > 0 { REAL(stheta) } else { std::ptr::null() };
        let mut w = vec![0.0; n];
        for l in 0..n {
            w[l] = *y.add(l);
        }
        for _ in 0..(*arma.add(5) as usize) {
            for l in (1..n).rev() {
                w[l] -= w[l - 1];
            }
        }
        let ns = *arma.add(4) as usize;
        for _ in 0..(*arma.add(6) as usize) {
            if ns == 0 {
                break;
            }
            for l in (ns..n).rev() {
                w[l] -= w[l - ns];
            }
        }
        let sresid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _g = protect(sresid);
        let resid = REAL(sresid);
        let use_resid = crate::mainutils::coerce::asLogical(give_resid) != 0;
        if use_resid {
            for l in 0..ncond.min(n) {
                *resid.add(l) = 0.0;
            }
        }
        let mut ssq = 0.0;
        let mut nu = 0usize;
        for l in ncond..n {
            let mut tmp = w[l];
            for j in 0..p {
                if l >= j + 1 {
                    tmp -= *phi.add(j) * w[l - j - 1];
                }
            }
            let ntheta = (l - ncond).min(q);
            for j in 0..ntheta {
                tmp -= *theta.add(j) * *resid.add(l - j - 1);
            }
            *resid.add(l) = tmp;
            if !tmp.is_nan() {
                nu += 1;
                ssq += tmp * tmp;
            }
        }
        let value = if nu == 0 { NA_REAL } else { ssq / nu as f64 };
        if use_resid {
            let res = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            SET_VECTOR_ELT(res, 0, Rf_ScalarReal(value));
            SET_VECTOR_ELT(res, 1, sresid);
            res
        } else {
            Rf_ScalarReal(value)
        }
    }
}

pub unsafe extern "C-unwind" fn c_arima_like(
    sy: SEXP,
    model: SEXP,
    sup: SEXP,
    give_resid: SEXP,
) -> SEXP {
    unsafe {
        let sphi = list_elt(model, "phi");
        let stheta = list_elt(model, "theta");
        let sdelta = list_elt(model, "Delta");
        let sa = list_elt(model, "a");
        let sp = list_elt(model, "P");
        let spn = list_elt(model, "Pn");
        let n = XLENGTH(sy) as usize;
        let rd = XLENGTH(sa) as usize;
        let p = XLENGTH(sphi) as usize;
        let q = XLENGTH(stheta) as usize;
        let d = XLENGTH(sdelta) as usize;
        let r = rd - d;
        let y = REAL(sy);
        let a = REAL(sa);
        let pstate = REAL(sp);
        let pnew = REAL(spn);
        let phi = if p > 0 { REAL(sphi) } else { std::ptr::null() };
        let theta = if q > 0 { REAL(stheta) } else { std::ptr::null() };
        let delta = if d > 0 { REAL(sdelta) } else { std::ptr::null() };
        let mut anew = vec![0.0; rd];
        let mut m = vec![0.0; rd];
        let mut mm = vec![0.0; rd * rd.max(1)];
        let use_resid = crate::mainutils::coerce::asLogical(give_resid) != 0;
        let sresid = if use_resid {
            Rf_allocVector3(SEXPTYPE::REALSXP, n as i64)
        } else {
            R_NilValue()
        };
        let _g = protect(sresid);
        let up = crate::mainutils::coerce::asInteger(sup);
        let mut sumlog = 0.0;
        let mut ssq = 0.0;
        let mut nu = 0i32;
        for l in 0..n {
            for i in 0..r {
                let mut tmp = if i < r - 1 { *a.add(i + 1) } else { 0.0 };
                if i < p {
                    tmp += *phi.add(i) * *a;
                }
                anew[i] = tmp;
            }
            if d > 0 {
                for i in (r + 1)..rd {
                    anew[i] = *a.add(i - 1);
                }
                let mut tmp = *a;
                for i in 0..d {
                    tmp += *delta.add(i) * *a.add(r + i);
                }
                anew[r] = tmp;
            }
            if l as i32 > up {
                if d == 0 {
                    for i in 0..r {
                        let vi = if i == 0 {
                            1.0
                        } else if i - 1 < q {
                            *theta.add(i - 1)
                        } else {
                            0.0
                        };
                        for j in 0..r {
                            let mut tmp = if j == 0 {
                                vi
                            } else if j - 1 < q {
                                vi * *theta.add(j - 1)
                            } else {
                                0.0
                            };
                            if i < p && j < p {
                                tmp += *phi.add(i) * *phi.add(j) * *pstate;
                            }
                            if i < r - 1 && j < r - 1 {
                                tmp += *pstate.add(i + 1 + r * (j + 1));
                            }
                            if i < p && j < r - 1 {
                                tmp += *phi.add(i) * *pstate.add(j + 1);
                            }
                            if j < p && i < r - 1 {
                                tmp += *phi.add(j) * *pstate.add(i + 1);
                            }
                            *pnew.add(i + r * j) = tmp;
                        }
                    }
                } else {
                    for i in 0..r {
                        for j in 0..rd {
                            let mut tmp = 0.0;
                            if i < p {
                                tmp += *phi.add(i) * *pstate.add(rd * j);
                            }
                            if i < r - 1 {
                                tmp += *pstate.add(i + 1 + rd * j);
                            }
                            mm[i + rd * j] = tmp;
                        }
                    }
                    for j in 0..rd {
                        let mut tmp = *pstate.add(rd * j);
                        for k in 0..d {
                            tmp += *delta.add(k) * *pstate.add(r + k + rd * j);
                        }
                        mm[r + rd * j] = tmp;
                    }
                    for i in 1..d {
                        for j in 0..rd {
                            mm[r + i + rd * j] = *pstate.add(r + i - 1 + rd * j);
                        }
                    }
                    for i in 0..r {
                        for j in 0..rd {
                            let mut tmp = 0.0;
                            if i < p {
                                tmp += *phi.add(i) * mm[j];
                            }
                            if i < r - 1 {
                                tmp += mm[rd * (i + 1) + j];
                            }
                            *pnew.add(j + rd * i) = tmp;
                        }
                    }
                    for j in 0..rd {
                        let mut tmp = mm[j];
                        for k in 0..d {
                            tmp += *delta.add(k) * mm[rd * (r + k) + j];
                        }
                        *pnew.add(rd * r + j) = tmp;
                    }
                    for i in 1..d {
                        for j in 0..rd {
                            *pnew.add(rd * (r + i) + j) = mm[rd * (r + i - 1) + j];
                        }
                    }
                    for i in 0..=q {
                        let vi = if i == 0 { 1.0 } else { *theta.add(i - 1) };
                        for j in 0..=q {
                            let vj = if j == 0 { 1.0 } else { *theta.add(j - 1) };
                            *pnew.add(i + rd * j) += vi * vj;
                        }
                    }
                }
            }
            if !(*y.add(l)).is_nan() {
                let mut resid = *y.add(l) - anew[0];
                for i in 0..d {
                    resid -= *delta.add(i) * anew[r + i];
                }
                for i in 0..rd {
                    let mut tmp = *pnew.add(i);
                    for j in 0..d {
                        tmp += *pnew.add(i + (r + j) * rd) * *delta.add(j);
                    }
                    m[i] = tmp;
                }
                let mut gain = m[0];
                for j in 0..d {
                    gain += *delta.add(j) * m[r + j];
                }
                if gain < 1e4 {
                    nu += 1;
                    ssq += resid * resid / gain;
                    sumlog += gain.ln();
                }
                if use_resid {
                    *REAL(sresid).add(l) = resid / gain.sqrt();
                }
                for i in 0..rd {
                    *a.add(i) = anew[i] + m[i] * resid / gain;
                }
                for i in 0..rd {
                    for j in 0..rd {
                        *pstate.add(i + j * rd) = *pnew.add(i + j * rd) - m[i] * m[j] / gain;
                    }
                }
            } else {
                for i in 0..rd {
                    *a.add(i) = anew[i];
                }
                let cells = rd * rd;
                for i in 0..cells {
                    *pstate.add(i) = *pnew.add(i);
                }
                if use_resid {
                    *REAL(sresid).add(l) = NA_REAL;
                }
            }
        }
        let nres = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        *REAL(nres) = ssq;
        *REAL(nres).add(1) = sumlog;
        *REAL(nres).add(2) = nu as f64;
        if use_resid {
            let res = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
            SET_VECTOR_ELT(res, 0, nres);
            SET_VECTOR_ELT(res, 1, sresid);
            res
        } else {
            nres
        }
    }
}

fn inclu2(
    np: usize,
    xnext: &mut [f64],
    xrow: &mut [f64],
    mut ynext: f64,
    d: &mut [f64],
    rbar: &mut [f64],
    thetab: &mut [f64],
) {
    xrow[..np].copy_from_slice(&xnext[..np]);
    let mut ithisr = 0usize;
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
            let xk = ynext;
            ynext = xk - xi * thetab[i];
            thetab[i] = cbar * thetab[i] + sbar * xk;
            if di == 0.0 {
                return;
            }
        } else {
            ithisr += np - i - 1;
        }
    }
}

pub unsafe extern "C-unwind" fn c_get_q0(sphi: SEXP, stheta: SEXP) -> SEXP {
    unsafe {
        let p = XLENGTH(sphi) as usize;
        let q = XLENGTH(stheta) as usize;
        let phi = if p > 0 { REAL(sphi) } else { std::ptr::null() };
        let theta = if q > 0 { REAL(stheta) } else { std::ptr::null() };
        let r = p.max(q + 1);
        if r > 350 {
            crate::main::errors::Rf_error(
                b"maximum supported lag is 350\0".as_ptr() as *const std::os::raw::c_char,
            );
        }
        let np = r * (r + 1) / 2;
        let nrbar = np * (np - 1) / 2;
        let mut v = vec![0.0; np];
        let mut ind = 0usize;
        for j in 0..r {
            let vj = if j == 0 {
                1.0
            } else if j - 1 < q {
                *theta.add(j - 1)
            } else {
                0.0
            };
            for i in j..r {
                let vi = if i == 0 {
                    1.0
                } else if i - 1 < q {
                    *theta.add(i - 1)
                } else {
                    0.0
                };
                v[ind] = vi * vj;
                ind += 1;
            }
        }
        let res = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            r as i32,
            r as i32,
        );
        let pmat = std::slice::from_raw_parts_mut(REAL(res), r * r);
        if r == 1 {
            pmat[0] = if p == 0 {
                1.0
            } else {
                1.0 / (1.0 - *phi * *phi)
            };
            return res;
        }
        if p > 0 {
            let mut xnext = vec![0.0; np];
            let mut xrow = vec![0.0; np];
            let mut rbar = vec![0.0; nrbar.max(1)];
            let mut thetab = vec![0.0; np];
            let mut ind = 0usize;
            let mut ind1: isize = -1;
            let npr = np - r;
            let npr1 = npr + 1;
            let mut indj = npr;
            let mut ind2 = npr as isize - 1;
            for j in 0..r {
                let phij = if j < p { *phi.add(j) } else { 0.0 };
                xnext[indj] = 0.0;
                indj += 1;
                let mut indi = npr1 + j;
                for i in j..r {
                    let ynext = v[ind];
                    ind += 1;
                    let phii = if i < p { *phi.add(i) } else { 0.0 };
                    if j != r - 1 {
                        xnext[indj] = -phii;
                        if i != r - 1 {
                            xnext[indi] -= phij;
                            ind1 += 1;
                            xnext[ind1 as usize] = -1.0;
                        }
                    }
                    xnext[npr] = -phii * phij;
                    ind2 += 1;
                    if ind2 >= np as isize {
                        ind2 = 0;
                    }
                    xnext[ind2 as usize] += 1.0;
                    inclu2(np, &mut xnext, &mut xrow, ynext, pmat, &mut rbar, &mut thetab);
                    xnext[ind2 as usize] = 0.0;
                    if i != r - 1 {
                        xnext[indi] = 0.0;
                        indi += 1;
                        xnext[ind1 as usize] = 0.0;
                    }
                }
            }
            let mut ithisr = nrbar as isize - 1;
            let mut im = np as isize - 1;
            for i in 0..np {
                let mut bi = thetab[im as usize];
                let mut jm = np as isize - 1;
                for _j in 0..i {
                    bi -= rbar[ithisr as usize] * pmat[jm as usize];
                    ithisr -= 1;
                    jm -= 1;
                }
                pmat[im as usize] = bi;
                im -= 1;
            }
            let mut indp = npr;
            for i in 0..r {
                xnext[i] = pmat[indp];
                indp += 1;
            }
            let mut indb = np as isize - 1;
            let mut ind1b = npr as isize - 1;
            for _i in 0..npr {
                pmat[indb as usize] = pmat[ind1b as usize];
                indb -= 1;
                ind1b -= 1;
            }
            for i in 0..r {
                pmat[i] = xnext[i];
            }
        } else {
            let mut indn = np;
            let mut indp = np;
            for i in 0..r {
                for j in 0..=i {
                    indp -= 1;
                    pmat[indp] = v[indp];
                    if j != 0 {
                        indn -= 1;
                        pmat[indp] += pmat[indn];
                    }
                }
            }
        }
        let mut indp = np;
        let mut i = r;
        while i > 1 {
            i -= 1;
            let mut j = r;
            while j >= i {
                j -= 1;
                indp -= 1;
                pmat[r * i + j] = pmat[indp];
            }
        }
        for i in 0..r.saturating_sub(1) {
            for j in (i + 1)..r {
                pmat[i + r * j] = pmat[j + r * i];
            }
        }
        res
    }
}

