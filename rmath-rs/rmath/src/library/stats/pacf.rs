/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1999-2022 The R Core Team
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/.
 *
 *  Ported from r-source/src/library/stats/src/pacf.c
 */

use std::ffi::c_void;
use std::os::raw::c_double;
use std::os::raw::c_int;
use std::ptr;

use crate::attrib_core::{R_DimSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, asReal, coerceVector};
use crate::main::errors::Rf_error;
use crate::mainutils::builtin::lengthgets;
use crate::mainutils::memory_main::{R_ExternalPtrAddr, R_ExternalPtrTag, R_MakeExternalPtr};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;
use crate::sexp::symbol::Rf_install;

use super::starma::{forkal, karma, starma, starma_struct};

unsafe fn allocMatrix(sexptype: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    let n = nrow * ncol;
    let s = Rf_allocVector(sexptype, n);
    let _s_guard = protect(s);
    let d = Rf_allocVector(SEXPTYPE::INTSXP, 2);
    let _d_guard = protect(d);
    *INTEGER(d) = nrow;
    *INTEGER(d).add(1) = ncol;
    setAttrib(s, R_DimSymbol(), d);
    s
}

// ---------------------------------------------------------------------------
// Helper: min / max
// ---------------------------------------------------------------------------

#[inline]
fn imax(a: c_int, b: c_int) -> c_int {
    if a > b { a } else { b }
}

#[inline]
fn imin(a: c_int, b: c_int) -> c_int {
    if a > b { b } else { a }
}

// ---------------------------------------------------------------------------
// partrans / invpartrans (local copies, same algorithm as arima.rs)
// ---------------------------------------------------------------------------

unsafe fn partrans_fn(p: c_int, raw: &[f64], new_: &mut [f64]) {
    if p > 100 {
        Rf_error(b"can only transform 100 pars in arima0\0".as_ptr() as *const libc::c_char);
        return;
    }
    let mut work = [0.0_f64; 100];

    // Step one: map (-Inf, Inf) to (-1, 1) via tanh
    for j in 0..(p as usize) {
        let val = raw[j].tanh();
        new_[j] = val;
        work[j] = val;
    }
    // Step two: Durbin-Levinson recursions
    for j in 1..(p as usize) {
        let a = new_[j];
        for k in 0..j {
            work[k] -= a * new_[j - k - 1];
        }
    }
}

unsafe fn invpartrans_fn(p: c_int, phi: &[f64], new_: &mut [f64]) {
    if p > 100 {
        Rf_error(b"can only transform 100 pars in arima0\0".as_ptr() as *const libc::c_char);
        return;
    }
    let mut work = [0.0_f64; 100];

    for j in 0..(p as usize) {
        let val = phi[j];
        new_[j] = val;
        work[j] = val;
    }
    // Run Durbin-Levinson backwards
    for j in (1..(p as usize)).rev() {
        let a = new_[j];
        for k in 0..j {
            work[k] = (new_[k] + a * new_[j - k - 1]) / (1.0 - a * a);
        }
    }
    for j in 0..(p as usize) {
        new_[j] = new_[j].atanh();
    }
}

unsafe fn dotrans_fn(
    mp: c_int,
    mq: c_int,
    msp: c_int,
    msq: c_int,
    m: c_int,
    trans: c_int,
    raw: &[f64],
    new_: &mut [f64],
) {
    let n = (mp + mq + msp + msq + m) as usize;
    for i in 0..n {
        new_[i] = raw[i];
    }
    if trans != 0 {
        let mut v: usize = 0;
        partrans_fn(mp, &raw[v..], &mut new_[v..]);
        v += mp as usize;
        partrans_fn(mq, &raw[v..], &mut new_[v..]);
        v += mq as usize;
        partrans_fn(msp, &raw[v..], &mut new_[v..]);
        v += msp as usize;
        partrans_fn(msq, &raw[v..], &mut new_[v..]);
    }
}

// ---------------------------------------------------------------------------
// uni_pacf — Durbin-Levinson algorithm for partial autocorrelations
// cor is the autocorrelations starting from 0 lag
// ---------------------------------------------------------------------------

unsafe fn uni_pacf(cor: *const c_double, p: *mut c_double, nlag: c_int) {
    let nlag = nlag as usize;
    let mut v = vec![0.0_f64; nlag];
    let mut w = vec![0.0_f64; nlag];

    let cor1 = *cor.add(1);
    *p.add(0) = cor1;
    w[0] = cor1;
    for ll in 1..nlag {
        let mut a = *cor.add(ll + 1);
        let mut b = 1.0;
        for i in 0..ll {
            a -= w[i] * *cor.add(ll - i);
            b -= w[i] * *cor.add(i + 1);
        }
    }
}

// ---------------------------------------------------------------------------
// pacf1 — public entry point for computing PACF from ACF
// ---------------------------------------------------------------------------

pub unsafe fn pacf1(acf: SEXP, lmax: SEXP) -> SEXP {
    let lagmax = asInteger(lmax);
    let acf = coerceVector(acf, SEXPTYPE::REALSXP.as_c_int());
    let _acf_guard = protect(acf);
    let ans = Rf_allocVector(SEXPTYPE::REALSXP, lagmax);
    let _ans_guard = protect(ans);
    uni_pacf(REAL(acf), REAL(ans), lagmax);

    let d = Rf_allocVector(SEXPTYPE::INTSXP, 3);
    let _d_guard = protect(d);
    *INTEGER(d) = lagmax;
    *INTEGER(d).add(1) = 1;
    *INTEGER(d).add(2) = 1;
    setAttrib(ans, R_DimSymbol(), d);

    ans
}

// ---------------------------------------------------------------------------
// Starma external pointer helpers
// ---------------------------------------------------------------------------

/// Tag symbol for Starma external pointer identification.
unsafe fn get_starma_tag() -> SEXP {
    if let Some(tag) = crate::sexp::instance::with_current_instance(|inst| {
        (!inst.stats_starma_tag.is_null()).then_some(inst.stats_starma_tag)
    })
    .flatten()
    {
        return tag;
    }

    let tag = Rf_install(b"STARMA_TAG\0".as_ptr() as *const libc::c_char);
    crate::sexp::instance::with_required_current_instance(|inst| {
        if inst.stats_starma_tag.is_null() {
            inst.stats_starma_tag = tag;
        }
        inst.stats_starma_tag
    })
}
/// Retrieve Starma struct from external pointer, or error.
unsafe fn get_starma(pG: SEXP) -> *mut starma_struct {
    if TYPEOF(pG) != SEXPTYPE::EXTPTRSXP || R_ExternalPtrTag(pG) != get_starma_tag() {
        Rf_error(b"bad Starma struct\0".as_ptr() as *const libc::c_char);
        return ptr::null_mut();
    }
    R_ExternalPtrAddr(pG) as *mut starma_struct
}

// ---------------------------------------------------------------------------
// setup_starma — allocate and initialise Starma struct
// ---------------------------------------------------------------------------

pub unsafe fn setup_starma(
    na: SEXP,
    x: SEXP,
    pn: SEXP,
    xreg: SEXP,
    pm: SEXP,
    dt: SEXP,
    ptrans: SEXP,
    sncond: SEXP,
) -> SEXP {
    use std::alloc::{Layout, alloc, handle_alloc_error};

    let rx = REAL(x);
    let rxreg = REAL(xreg);
    let na_int = INTEGER(na);

    let mp = *na_int.add(0);
    let mq = *na_int.add(1);
    let msp = *na_int.add(2);
    let msq = *na_int.add(3);
    let ns = *na_int.add(4);
    let n = asInteger(pn);
    let ncond = asInteger(sncond);
    let m = asInteger(pm);

    // Allocate starma_struct
    let g_layout = Layout::new::<starma_struct>();
    let g_ptr = alloc(g_layout) as *mut starma_struct;
    if g_ptr.is_null() {
        handle_alloc_error(g_layout);
    }

    (*g_ptr).mp = mp;
    (*g_ptr).mq = mq;
    (*g_ptr).msp = msp;
    (*g_ptr).msq = msq;
    (*g_ptr).ns = ns;
    (*g_ptr).n = n;
    (*g_ptr).ncond = ncond;
    (*g_ptr).m = m;

    let total_params = (mp + mq + msp + msq + m) as usize;
    let params_layout = Layout::array::<c_double>(total_params)
        .unwrap_or_else(|_| handle_alloc_error(Layout::new::<c_double>()));
    (*g_ptr).params = alloc(params_layout) as *mut c_double;

    let ip = ns * msp + mp;
    let iq = ns * msq + mq;
    let ir = imax(ip, iq + 1);
    let np = ir * (ir + 1) / 2;
    let nrbar = imax(1, np * (np - 1) / 2);

    (*g_ptr).p = ip;
    (*g_ptr).q = iq;
    (*g_ptr).r = ir;
    (*g_ptr).np = np;
    (*g_ptr).nrbar = nrbar;
    (*g_ptr).trans = asInteger(ptrans);
    (*g_ptr).delta = asReal(dt);

    // Allocate all internal arrays
    macro_rules! alloc_arr {
        ($field:ident, $count:expr) => {{
            let layout = Layout::array::<c_double>($count as usize)
                .unwrap_or_else(|_| handle_alloc_error(Layout::new::<c_double>()));
            let ptr = alloc(layout) as *mut c_double;
            if ptr.is_null() {
                handle_alloc_error(layout);
            }
        }};
    }

    alloc_arr!(a, ir);
    alloc_arr!(P, np);
    alloc_arr!(V, np);
    alloc_arr!(thetab, np);
    alloc_arr!(xnext, np);
    alloc_arr!(xrow, np);
    alloc_arr!(rbar, nrbar);
    alloc_arr!(w, n);
    alloc_arr!(wkeep, n);
    alloc_arr!(resid, n);
    alloc_arr!(phi, ir);
    alloc_arr!(theta, ir);
    alloc_arr!(reg, 1 + n * m); /* AIX can't calloc 0 items */

    for i in 0..(n as usize) {
        *(*g_ptr).w.add(i) = *rx.add(i);
        *(*g_ptr).wkeep.add(i) = *rx.add(i);
    }
    for i in 0..(n * m) as usize {
        *(*g_ptr).reg.add(i) = *rxreg.add(i);
    }

    let tag = get_starma_tag();
    let res = R_MakeExternalPtr(g_ptr as *mut c_void, tag, R_NilValue());
    res
}

// ---------------------------------------------------------------------------
// free_starma — deallocate Starma struct
// ---------------------------------------------------------------------------

pub unsafe fn free_starma(pG: SEXP) -> SEXP {
    use std::alloc::{Layout, dealloc};

    let G = get_starma(pG);
    if G.is_null() {
        return R_NilValue();
    }

    macro_rules! free_arr {
        ($field:ident) => {
            if !(*G).$field.is_null() {
                let layout = Layout::new::<c_double>();
                dealloc((*G).$field as *mut u8, layout);
            }
        };
    }

    free_arr!(params);
    free_arr!(a);
    free_arr!(P);
    free_arr!(V);
    free_arr!(thetab);
    free_arr!(xnext);
    free_arr!(xrow);
    free_arr!(rbar);
    free_arr!(w);
    free_arr!(wkeep);
    free_arr!(resid);
    free_arr!(phi);
    free_arr!(theta);
    free_arr!(reg);

    let g_layout = Layout::new::<starma_struct>();
    dealloc(G as *mut u8, g_layout);

    R_NilValue()
}

// ---------------------------------------------------------------------------
// Starma_method — set the method field
// ---------------------------------------------------------------------------

pub unsafe fn Starma_method(pG: SEXP, method: SEXP) -> SEXP {
    let G = get_starma(pG);
    if !G.is_null() {
        (*G).method = asInteger(method);
    }
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Dotrans — apply parameter transformation
// ---------------------------------------------------------------------------

pub unsafe fn Dotrans(pG: SEXP, x: SEXP) -> SEXP {
    let G = get_starma(pG);
    if G.is_null() {
        return R_NilValue();
    }
    let y = Rf_allocVector(SEXPTYPE::REALSXP, LENGTH(x) as c_int);
    let n = ((*G).mp + (*G).mq + (*G).msp + (*G).msq + (*G).m) as usize;
    let raw = std::slice::from_raw_parts(REAL(x), n);
    let new_ = std::slice::from_raw_parts_mut(REAL(y), n);
    dotrans_fn(
        (*G).mp,
        (*G).mq,
        (*G).msp,
        (*G).msq,
        (*G).m,
        (*G).trans,
        raw,
        new_,
    );
    y
}

// ---------------------------------------------------------------------------
// set_trans — set the trans flag
// ---------------------------------------------------------------------------

pub unsafe fn set_trans(pG: SEXP, ptrans: SEXP) -> SEXP {
    let G = get_starma(pG);
    if !G.is_null() {
        (*G).trans = asInteger(ptrans);
    }
    R_NilValue()
}

// ---------------------------------------------------------------------------
// arma0fa — compute ARMA log-likelihood
// ---------------------------------------------------------------------------

pub unsafe fn arma0fa(pG: SEXP, inparams: SEXP) -> SEXP {
    let G = get_starma(pG);
    if G.is_null() {
        return R_NilValue();
    }

    let mp = (*G).mp;
    let mq = (*G).mq;
    let msp = (*G).msp;
    let msq = (*G).msq;
    let ns = (*G).ns;
    let m = (*G).m;
    let n = (*G).n;
    let ncond = (*G).ncond;
    let method = (*G).method;
    let p = (*G).p;
    let q = (*G).q;

    // Transform parameters
    {
        let raw_params =
            std::slice::from_raw_parts(REAL(inparams), (mp + mq + msp + msq + m) as usize);
        let out_params =
            std::slice::from_raw_parts_mut((*G).params, (mp + mq + msp + msq + m) as usize);
        dotrans_fn(mp, mq, msp, msq, m, (*G).trans, raw_params, out_params);
    }

    // Expand seasonal ARMA models
    if ns > 0 {
        for i in 0..(mp as usize) {
            *(*G).phi.add(i) = *(*G).params.add(i);
        }
    }

    // Subtract regression effects
    let streg = (mp + mq + msp + msq) as usize;
    if m > 0 {
        for i in 0..(n as usize) {
            let mut tmp = *(*G).wkeep.add(i);
            for j in 0..(m as usize) {
                tmp -= *(*G).reg.add(i + n as usize * j) * *(*G).params.add(streg + j);
            }
        }
    }

    let ans: c_double;
    if method == 1 {
        // CSS method
        let pp = (mp + ns * msp) as usize;
        let qq = (mq + ns * msq) as usize;
        let mut nu: c_int = 0;
        let mut ssq: c_double = 0.0;
        for i in 0..(ncond as usize) {
            *(*G).resid.add(i) = 0.0;
        }
    }
    Rf_ScalarReal(ans)
}

// ---------------------------------------------------------------------------
// get_s2 — retrieve s2
// ---------------------------------------------------------------------------

pub unsafe fn get_s2(pG: SEXP) -> SEXP {
    let G = get_starma(pG);
    if G.is_null() {
        return Rf_ScalarReal(0.0);
    }
    Rf_ScalarReal((*G).s2)
}

// ---------------------------------------------------------------------------
// get_resid — retrieve residuals
// ---------------------------------------------------------------------------

pub unsafe fn get_resid(pG: SEXP) -> SEXP {
    let G = get_starma(pG);
    if G.is_null() {
        return R_NilValue();
    }
    let n = (*G).n as c_int;
    let res = Rf_allocVector(SEXPTYPE::REALSXP, n);
    let rres = REAL(res);
    for i in 0..(n as usize) {
        *rres.add(i) = *(*G).resid.add(i);
    }
    res
}

// ---------------------------------------------------------------------------
// arma0_kfore — ARIMA forecasting
// ---------------------------------------------------------------------------

pub unsafe fn arma0_kfore(pG: SEXP, pd: SEXP, psd: SEXP, nahead: SEXP) -> SEXP {
    let G = get_starma(pG);
    if G.is_null() {
        return R_NilValue();
    }

    let dd = asInteger(pd);
    let il = asInteger(nahead);
    let d_val = dd + (*G).ns * asInteger(psd);

    let res = Rf_allocVector(SEXPTYPE::VECSXP, 2);
    let _res_guard = protect(res);
    let x = Rf_allocVector(SEXPTYPE::REALSXP, il);
    let _x_guard = protect(x);
    let var = Rf_allocVector(SEXPTYPE::REALSXP, il);
    let _var_guard = protect(var);
    SET_VECTOR_ELT(res, 0, x);
    SET_VECTOR_ELT(res, 1, var);

    let mut del = vec![0.0_f64; (d_val + 1) as usize];
    let mut del2 = vec![0.0_f64; (d_val + 1) as usize];
    del[0] = 1.0;

    for _j in 0..dd {
        for i in 0..=(d_val as usize) {
            del2[i] = del[i];
        }
    }
    for _j in 0..asInteger(psd) {
        for i in 0..=(d_val as usize) {
            del2[i] = del[i];
        }
    }
    for i in 1..=(d_val as usize) {
        del[i] *= -1.0;
    }

    let mut ifault: c_int = 0;
    forkal(
        G as *mut c_void,
        d_val,
        il,
        del[1..].as_ptr() as *mut c_double,
        REAL(x),
        REAL(var),
        &mut ifault,
    );
    if ifault != 0 {
        Rf_error(b"forkal error\0".as_ptr() as *const libc::c_char);
        return R_NilValue();
    }

    res
}

// ---------------------------------------------------------------------------
// artoma — convert AR coefficients to MA coefficients
// ---------------------------------------------------------------------------

unsafe fn artoma(p: c_int, phi: *const c_double, psi: *mut c_double, npsi: c_int) {
    let p = p as usize;
    let npsi = npsi as usize;

    for i in 0..p {
        *psi.add(i) = *phi.add(i);
    }
    for i in p..npsi {
        *psi.add(i) = 0.0;
    }
    for i in 0..(npsi - p - 1) {
        for j in 0..p {
            *psi.add(i + j + 1) += *phi.add(j) * *psi.add(i);
        }
    }
}

// ---------------------------------------------------------------------------
// ar2ma — public entry for AR to MA conversion
// ---------------------------------------------------------------------------

pub unsafe fn ar2ma(ar: SEXP, npsi: SEXP) -> SEXP {
    let ar = coerceVector(ar, SEXPTYPE::REALSXP.as_c_int());
    let _ar_guard = protect(ar);
    let p = LENGTH(ar) as c_int;
    let ns = asInteger(npsi);
    let ns1 = ns + p + 1;
    let psi = Rf_allocVector(SEXPTYPE::REALSXP, ns1);
    let _psi_guard = protect(psi);
    artoma(p, REAL(ar), REAL(psi), ns1);
    let ans = lengthgets(psi, ns);
    ans
}

// ---------------------------------------------------------------------------
// Invtrans — inverse parameter transformation
// ---------------------------------------------------------------------------

pub unsafe fn Invtrans(pG: SEXP, x: SEXP) -> SEXP {
    let G = get_starma(pG);
    if G.is_null() {
        return R_NilValue();
    }

    let y = Rf_allocVector(SEXPTYPE::REALSXP, LENGTH(x) as c_int);
    let raw = REAL(x);
    let new_ = REAL(y);
    let mp = (*G).mp;
    let mq = (*G).mq;
    let msp = (*G).msp;
    let msq = (*G).msq;
    let m = (*G).m;

    let mut v: usize = 0;

    // mp
    if mp > 0 {
        let raw_slice = std::slice::from_raw_parts(raw.add(v), mp as usize);
        let new_slice = std::slice::from_raw_parts_mut(new_.add(v), mp as usize);
        invpartrans_fn(mp, raw_slice, new_slice);
    }
    v += mp as usize;

    // mq
    if mq > 0 {
        let raw_slice = std::slice::from_raw_parts(raw.add(v), mq as usize);
        let new_slice = std::slice::from_raw_parts_mut(new_.add(v), mq as usize);
        invpartrans_fn(mq, raw_slice, new_slice);
    }
    v += mq as usize;

    // msp
    if msp > 0 {
        let raw_slice = std::slice::from_raw_parts(raw.add(v), msp as usize);
        let new_slice = std::slice::from_raw_parts_mut(new_.add(v), msp as usize);
        invpartrans_fn(msp, raw_slice, new_slice);
    }
    v += msp as usize;

    // msq
    if msq > 0 {
        let raw_slice = std::slice::from_raw_parts(raw.add(v), msq as usize);
        let new_slice = std::slice::from_raw_parts_mut(new_.add(v), msq as usize);
        invpartrans_fn(msq, raw_slice, new_slice);
    }

    let n = (mp + mq + msp + msq) as usize;
    for i in n..(n + m as usize) {
        *new_.add(i) = *raw.add(i);
    }

    y
}

// ---------------------------------------------------------------------------
// Gradtrans — compute gradient of parameter transformation
// ---------------------------------------------------------------------------

pub unsafe fn Gradtrans(pG: SEXP, x: SEXP) -> SEXP {
    let G = get_starma(pG);
    if G.is_null() {
        return R_NilValue();
    }

    let mp = (*G).mp;
    let mq = (*G).mq;
    let msp = (*G).msp;
    let msq = (*G).msq;
    let m = (*G).m;
    let n = (mp + mq + msp + msq + m) as usize;

    let y = allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n as c_int, n as c_int);
    let raw = REAL(x);
    let a = REAL(y);
    let eps: c_double = 1e-3;

    // Initialise identity
    for i in 0..n {
        for j in 0..n {
            *a.add(i + j * n) = if i == j { 1.0 } else { 0.0 };
        }
    }

    let mut w1 = [0.0_f64; 100];
    let mut w2 = [0.0_f64; 100];
    let mut w3 = [0.0_f64; 100];

    // mp block
    if mp > 0 {
        for i in 0..(mp as usize) {
            w1[i] = *raw.add(i);
        }
    }

    // mq block
    if mq > 0 {
        let v = mp as usize;
        for i in 0..(mq as usize) {
            w1[i] = *raw.add(i + v);
        }
    }

    // msp block
    if msp > 0 {
        let v = (mp + mq) as usize;
        for i in 0..(msp as usize) {
            w1[i] = *raw.add(i + v);
        }
    }

    // msq block
    if msq > 0 {
        let v = (mp + mq + msp) as usize;
        for i in 0..(msq as usize) {
            w1[i] = *raw.add(i + v);
        }
    }

    y
}

// ---------------------------------------------------------------------------
// ARMAtoMA — convert ARMA to infinite MA representation
// ---------------------------------------------------------------------------

pub unsafe fn ARMAtoMA(ar: SEXP, ma: SEXP, lag_max: SEXP) -> SEXP {
    let m = asInteger(lag_max);
    if m <= 0 || m == NA_INTEGER {
        Rf_error(b"invalid value of lag.max\0".as_ptr() as *const libc::c_char);
        return R_NilValue();
    }

    let p = LENGTH(ar) as c_int;
    let q = LENGTH(ma) as c_int;
    let phi = REAL(ar);
    let theta = REAL(ma);

    let res = Rf_allocVector(SEXPTYPE::REALSXP, m);
    let _res_guard = protect(res);
    let psi = REAL(res);

    for i in 0..(m as usize) {
        let mut tmp = if i < q as usize { *theta.add(i) } else { 0.0 };
        for j in 0..imin(i as c_int + 1, p) as usize {
            tmp += *phi.add(j) * if i > j { *psi.add(i - j - 1) } else { 1.0 };
        }
        *psi.add(i) = tmp;
    }

    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn starma_tag_is_owned_by_active_session() {
        let mut left = RSession::new();
        let left_tag = unsafe { get_starma_tag() };
        let mut right = RSession::new();
        let right_tag = unsafe { get_starma_tag() };

        assert!(!left_tag.is_null());
        assert!(!right_tag.is_null());
        assert_ne!(left_tag, right_tag);

        let left_again = left
            .with_arena(|_| unsafe { get_starma_tag() })
            .expect("left session should be active");
        assert_eq!(left_tag, left_again);

        let right_again = right
            .with_arena(|_| unsafe { get_starma_tag() })
            .expect("right session should be active");
        assert_eq!(right_tag, right_again);
    }
}
