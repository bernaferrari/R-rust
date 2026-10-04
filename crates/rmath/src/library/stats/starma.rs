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
 *  Ported from r-source/src/library/stats/src/starma.c
 */

#![forbid(unsafe_code)]

use core::ffi::{c_double, c_int};

use crate::sexp::ffi::{ISNAN, NA_REAL};

/// Canonical numerical state: dimensions and every buffer have one Rust owner.
/// Entry points validate dimensions before constructing this model; forecasts
/// work on a fallible owned copy and never mutate the fitted model.
#[derive(Debug, PartialEq)]
pub(super) struct StarmaModel {
    pub(super) p: c_int,
    pub(super) q: c_int,
    pub(super) r: c_int,
    pub(super) np: c_int,
    pub(super) nrbar: c_int,
    pub(super) n: c_int,
    pub(super) ncond: c_int,
    pub(super) m: c_int,
    pub(super) trans: c_int,
    pub(super) method: c_int,
    pub(super) nused: c_int,
    pub(super) mp: c_int,
    pub(super) mq: c_int,
    pub(super) msp: c_int,
    pub(super) msq: c_int,
    pub(super) ns: c_int,
    pub(super) delta: c_double,
    pub(super) s2: c_double,
    pub(super) params: Vec<f64>,
    pub(super) phi: Vec<f64>,
    pub(super) theta: Vec<f64>,
    pub(super) a: Vec<f64>,
    pub(super) P: Vec<f64>,
    pub(super) V: Vec<f64>,
    pub(super) thetab: Vec<f64>,
    pub(super) xnext: Vec<f64>,
    pub(super) xrow: Vec<f64>,
    pub(super) rbar: Vec<f64>,
    pub(super) w: Vec<f64>,
    pub(super) wkeep: Vec<f64>,
    pub(super) resid: Vec<f64>,
    pub(super) reg: Vec<f64>,
}

impl StarmaModel {
    /// Bound packed arithmetic and buffer sizes before allocating any workspace.
    pub(super) fn new(
        orders: [i32; 5],
        n: i32,
        m: i32,
        ncond: i32,
        trans: i32,
        delta: f64,
    ) -> Result<Self, ForecastError> {
        let [mp, mq, msp, msq, ns] = orders;
        if orders.into_iter().any(|order| order < 0) || n < 0 || m < 0 || ncond < 0 || ncond > n {
            return Err(ForecastError::Dimensions);
        }
        let size = |value: i64| i32::try_from(value).map_err(|_| ForecastError::PackedOverflow);
        let p = size(i64::from(mp) + i64::from(ns) * i64::from(msp))?;
        let q = size(i64::from(mq) + i64::from(ns) * i64::from(msq))?;
        let r = size(i64::from(p).max(i64::from(q) + 1))?;
        let np = size(i64::from(r) * (i64::from(r) + 1) / 2)?;
        let nrbar = size((i64::from(np) * (i64::from(np) - 1) / 2).max(1))?;
        let npar =
            size(i64::from(mp) + i64::from(mq) + i64::from(msp) + i64::from(msq) + i64::from(m))?;
        let reg_len = size(i64::from(n) * i64::from(m) + 1)?;
        let zeros = |length: i32| forecast_zeros(length.max(1) as usize);
        Ok(Self {
            p,
            q,
            r,
            np,
            nrbar,
            n,
            ncond,
            m,
            trans,
            delta,
            method: 0,
            nused: 0,
            mp,
            mq,
            msp,
            msq,
            ns,
            s2: 0.0,
            params: zeros(npar)?,
            phi: zeros(r)?,
            theta: zeros(r)?,
            a: zeros(r)?,
            P: zeros(np)?,
            V: zeros(np)?,
            thetab: zeros(np)?,
            xnext: zeros(np)?,
            xrow: zeros(np)?,
            rbar: zeros(nrbar)?,
            w: zeros(n)?,
            wkeep: zeros(n)?,
            resid: zeros(n)?,
            reg: zeros(reg_len)?,
        })
    }

    pub(super) fn try_clone(&self) -> Result<Self, ForecastError> {
        fn copy(values: &[f64]) -> Result<Vec<f64>, ForecastError> {
            let mut result = Vec::new();
            result
                .try_reserve_exact(values.len())
                .map_err(|_| ForecastError::Allocation)?;
            result.extend_from_slice(values);
            Ok(result)
        }
        Ok(Self {
            p: self.p,
            q: self.q,
            r: self.r,
            np: self.np,
            nrbar: self.nrbar,
            n: self.n,
            ncond: self.ncond,
            m: self.m,
            trans: self.trans,
            method: self.method,
            nused: self.nused,
            mp: self.mp,
            mq: self.mq,
            msp: self.msp,
            msq: self.msq,
            ns: self.ns,
            delta: self.delta,
            s2: self.s2,
            params: copy(&self.params)?,
            phi: copy(&self.phi)?,
            theta: copy(&self.theta)?,
            a: copy(&self.a)?,
            P: copy(&self.P)?,
            V: copy(&self.V)?,
            thetab: copy(&self.thetab)?,
            xnext: copy(&self.xnext)?,
            xrow: copy(&self.xrow)?,
            rbar: copy(&self.rbar)?,
            w: copy(&self.w)?,
            wkeep: copy(&self.wkeep)?,
            resid: copy(&self.resid)?,
            reg: copy(&self.reg)?,
        })
    }
}

/// Internal helper — update d, rbar, thetab by inclusion of xnext and ynext.
/// (AS154 subroutine inclu2)
fn inclu2(
    np: c_int,
    xnext: &[f64],
    xrow: &mut [f64],
    mut ynext: c_double,
    d: &mut [f64],
    rbar: &mut [f64],
    thetab: &mut [f64],
) {
    let mut cbar: c_double;
    let mut sbar: c_double;
    let mut di: c_double;
    let mut xi: c_double;
    let mut xk: c_double;
    let mut rbthis: c_double;
    let mut dpi: c_double;
    let mut ithisr: c_int = 0;

    for i in 0..np {
        xrow[i as usize] = xnext[i as usize];
    }

    let mut i: c_int = 0;
    while i < np {
        if xrow[i as usize] != 0.0 {
            xi = xrow[i as usize];
            di = d[i as usize];
            dpi = di + xi * xi;
            d[i as usize] = dpi;
            cbar = di / dpi;
            sbar = xi / dpi;
            let mut k: c_int = i + 1;
            while k < np {
                xk = xrow[k as usize];
                rbthis = rbar[ithisr as usize];
                xrow[k as usize] = xk - xi * rbthis;
                rbar[ithisr as usize] = cbar * rbthis + sbar * xk;
                ithisr += 1;
                k += 1;
            }
            xk = ynext;
            ynext = xk - xi * thetab[i as usize];
            thetab[i as usize] = cbar * thetab[i as usize] + sbar * xk;
            if di == 0.0 {
                return;
            }
        } else {
            ithisr += np - i - 1;
        }
        i += 1;
    }
}

/// Set initial values for the Kalman filter.
pub(super) fn starma(G: &mut StarmaModel, ifault: &mut c_int) {
    let p = G.p;
    let q = G.q;
    let r = G.r;
    let np = G.np;
    let nrbar = G.nrbar;
    let phi = &mut G.phi;
    let theta = &mut G.theta;
    let a = &mut G.a;
    let P = &mut G.P;
    let V = &mut G.V;
    let thetab = &mut G.thetab;
    let xnext = &mut G.xnext;
    let xrow = &mut G.xrow;
    let rbar = &mut G.rbar;

    /* Check if ar(1) */
    if !(q > 0 || p > 1) {
        V[0] = 1.0;
        a[0] = 0.0;
        P[0] = 1.0 / (1.0 - phi[0] * phi[0]);
        return;
    }

    /* Check for failure indication. */
    *ifault = 0;
    if p < 0 {
        *ifault = 1;
    }
    if q < 0 {
        *ifault += 2;
    }
    if p == 0 && q == 0 {
        *ifault = 4;
    }
    let mut k = q + 1;
    if k < p {
        k = p;
    }
    if r != k {
        *ifault = 5;
    }
    if i64::from(np) != i64::from(r) * (i64::from(r) + 1) / 2 {
        *ifault = 6;
    }
    if i64::from(nrbar) != i64::from(np) * (i64::from(np) - 1) / 2 {
        *ifault = 7;
    }
    if r == 1 {
        *ifault = 8;
    }
    if *ifault != 0 {
        return;
    }

    /* Now set a(0), V and phi. */
    let mut i: c_int;
    let mut j: c_int;
    for i in 1..r {
        a[i as usize] = 0.0;
        if i >= p {
            phi[i as usize] = 0.0;
        }
        V[i as usize] = 0.0;
        if i < q + 1 {
            V[i as usize] = theta[(i - 1) as usize];
        }
    }
    a[0] = 0.0;
    if p == 0 {
        phi[0] = 0.0;
    }
    V[0] = 1.0;
    let mut ind = r;
    for j in 1..r {
        let vj = V[j as usize];
        for i in j..r {
            V[ind as usize] = V[i as usize] * vj;
            ind += 1;
        }
    }

    /* Now find P(0). */
    if p > 0 {
        for i in 0..nrbar {
            rbar[i as usize] = 0.0;
        }
        for i in 0..np {
            P[i as usize] = 0.0;
            thetab[i as usize] = 0.0;
            xnext[i as usize] = 0.0;
        }
        ind = 0;
        let mut ind1: c_int = -1;
        let npr = np - r;
        let npr1 = npr + 1;
        let mut indj: c_int = npr;
        let mut ind2: c_int = npr - 1;
        for j in 0..r {
            let phij = phi[j as usize];
            xnext[indj as usize] = 0.0;
            indj += 1;
            let mut indi: c_int = npr1 + j;
            for i in j..r {
                let mut ynext = V[ind as usize];
                ind += 1;
                let phii = phi[i as usize];
                if j != r - 1 {
                    xnext[indj as usize] = -phii;
                    if i != r - 1 {
                        xnext[indi as usize] -= phij;
                        ind1 += 1;
                        xnext[ind1 as usize] = -1.0;
                    }
                }
                xnext[npr as usize] = -phii * phij;
                ind2 += 1;
                if ind2 >= np {
                    ind2 = 0;
                }
                xnext[ind2 as usize] += 1.0;
                inclu2(np, xnext, xrow, ynext, P, rbar, thetab);
                xnext[ind2 as usize] = 0.0;
                if i != r - 1 {
                    xnext[indi as usize] = 0.0;
                    indi += 1;
                    xnext[ind1 as usize] = 0.0;
                }
            }
        }

        let mut ithisr = nrbar - 1;
        let mut im = np - 1;
        for i in 0..np {
            let mut bi = thetab[im as usize];
            let mut jm = np - 1;
            for _j in 0..i as c_int {
                bi -= rbar[ithisr as usize] * P[jm as usize];
                ithisr -= 1;
                jm -= 1;
            }
            P[im as usize] = bi;
            im -= 1;
        }

        /* now re-order P. */
        ind = npr;
        for i in 0..r {
            xnext[i as usize] = P[ind as usize];
            ind += 1;
        }
        ind = np - 1;
        ind1 = npr - 1;
        for i in 0..npr {
            P[ind as usize] = P[ind1 as usize];
            ind -= 1;
            ind1 -= 1;
        }
        for i in 0..r {
            P[i as usize] = xnext[i as usize];
        }
    } else {
        /* P(0) is obtained by backsubstitution for a moving average process. */
        let mut indn = np;
        ind = np;
        for i in 0..r {
            for j in 0..=i {
                ind -= 1;
                P[ind as usize] = V[ind as usize];
                if j != 0 {
                    indn -= 1;
                    P[ind as usize] = P[ind as usize] + P[indn as usize];
                }
            }
        }
    }
}

/// Update Kalman filter by inclusion of data values w(1) to w(n).
pub(super) fn karma(
    G: &mut StarmaModel,
    sumlog: &mut f64,
    ssq: &mut f64,
    iupd: c_int,
    nit: &mut c_int,
) {
    let p = G.p;
    let q = G.q;
    let r = G.r;
    let n = G.n;
    let phi = &mut G.phi;
    let theta = &mut G.theta;
    let a = &mut G.a;
    let P = &mut G.P;
    let V = &mut G.V;
    let w = &mut G.w;
    let resid = &mut G.resid;
    let work = &mut G.xnext;

    if *nit == 0 {
        // A new pass. nused from the previous arma0fa call must not
        // be added again if this pass drops into the quick recursion.
        G.nused = 0;
        let mut nu: c_int = 0;
        for i in 0..n {
            /* prediction. */
            if iupd != 1 || i > 0 {
                /* here dt = ft - 1.0 */
                let dt_val = if r > 1 { P[r as usize] } else { 0.0 };
                if dt_val < G.delta {
                    /* jump to quick recursions */
                    G.nused = quick_recur(nit, ssq, w, phi, theta, resid, p, q, n, i as usize, nu);
                    return;
                }
                let a1 = a[0];
                for j in 0..r - 1 {
                    a[j as usize] = a[(j + 1) as usize];
                }
                a[(r - 1) as usize] = 0.0;
                for j in 0..p {
                    a[j as usize] += phi[j as usize] * a1;
                }
                if P[0] == 0.0 {
                    /* last obs was available */
                    let mut ind: c_int = -1;
                    let mut indn: c_int = r;
                    for j in 0..r {
                        for l in j..r {
                            ind += 1;
                            P[ind as usize] = V[ind as usize];
                            if l < r - 1 {
                                P[ind as usize] = P[ind as usize] + P[indn as usize];
                                indn += 1;
                            }
                        }
                    }
                } else {
                    for j in 0..r {
                        work[j as usize] = P[j as usize];
                    }
                    let mut ind: c_int = -1;
                    let mut indn: c_int = r;
                    let dt_p = P[0];
                    for j in 0..r {
                        let phij = phi[j as usize];
                        let phijdt = phij * dt_p;
                        for l in j..r {
                            ind += 1;
                            P[ind as usize] = V[ind as usize] + phi[l as usize] * phijdt;
                            if j < r - 1 {
                                P[ind as usize] =
                                    P[ind as usize] + work[(j + 1) as usize] * phi[l as usize];
                            }
                            if l < r - 1 {
                                P[ind as usize] = P[ind as usize]
                                    + work[(l + 1) as usize] * phij
                                    + P[indn as usize];
                                indn += 1;
                            }
                        }
                    }
                }
            }

            /* updating. */
            let ft = P[0];
            if !ISNAN(w[i as usize]) {
                let ut = w[i as usize] - a[0];
                if r > 1 {
                    let mut ind_p: c_int = r;
                    for j in 1..r {
                        let g_val = P[j as usize] / ft;
                        a[j as usize] += g_val * ut;
                        for l in j..r {
                            P[ind_p as usize] = P[ind_p as usize] - g_val * P[l as usize];
                            ind_p += 1;
                        }
                    }
                }
                a[0] = w[i as usize];
                resid[i as usize] = ut / ft.sqrt();
                *ssq += ut * ut / ft;
                *sumlog += ft.ln();
                nu += 1;
                for l in 0..r {
                    P[l as usize] = 0.0;
                }
            } else {
                resid[i as usize] = NA_REAL;
            }
        }
        *nit = n;
        G.nused = nu;
    } else {
        /* quick recursions: never used with missing values */
        G.nused = quick_recur(nit, ssq, w, phi, theta, resid, p, q, n, 0, G.nused);
    }
}

/// Quick recursions helper — extracted from karma's L610 label.
fn quick_recur(
    nit: &mut c_int,
    ssq: &mut f64,
    w: &[f64],
    phi: &[f64],
    theta: &[f64],
    resid: &mut [f64],
    p: c_int,
    q: c_int,
    n: c_int,
    start_i: usize,
    nu0: c_int,
) -> c_int {
    let mut nu: c_int = nu0;
    let mut et: c_double;
    let mut indw: c_int;

    *nit = start_i as c_int;
    for ii in start_i..n as usize {
        et = w[ii];
        indw = ii as c_int;
        for j in 0..p as usize {
            indw -= 1;
            if indw < 0 {
                break;
            }
            et -= phi[j] * w[indw as usize];
        }
        let qm = if ii < q as usize { ii } else { q as usize };
        for j in 0..qm {
            et -= theta[j] * resid[ii - j - 1];
        }
        resid[ii] = et;
        *ssq += et * et;
        nu += 1;
    }
    nu
}

/// Errors at the owned AS182 workspace boundary, before any native state changes.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ForecastError {
    Dimensions,
    History,
    PackedOverflow,
    Allocation,
    MissingHistory,
    Kernel(i32),
}

impl std::fmt::Display for ForecastError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dimensions => f.write_str("invalid starma forecast dimensions"),
            Self::History => f.write_str("starma differences require more observations"),
            Self::PackedOverflow => f.write_str("starma forecast dimensions are too large"),
            Self::Allocation => f.write_str("could not allocate starma forecast workspace"),
            Self::MissingHistory => f.write_str("missing value in starma forecast history"),
            Self::Kernel(code) => write!(f, "forkal error code {code}"),
        }
    }
}

/// Bound every remaining signed packed-index product before allocation.
pub(super) fn forecast_dimensions(r: i32, n: i32, d: i32) -> Result<(i32, i32), ForecastError> {
    if r < 1 || d < 0 || n < 1 {
        return Err(ForecastError::Dimensions);
    }
    if d >= n {
        return Err(ForecastError::History);
    }
    let rd = i64::from(r) + i64::from(d);
    let product = rd
        .checked_mul(rd + 1)
        .ok_or(ForecastError::PackedOverflow)?;
    if product > i64::from(i32::MAX) {
        return Err(ForecastError::PackedOverflow);
    }
    Ok((rd as i32, (product / 2) as i32))
}

fn forecast_zeros(length: usize) -> Result<Vec<f64>, ForecastError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(length)
        .map_err(|_| ForecastError::Allocation)?;
    values.resize(length, 0.0);
    Ok(values)
}

/// Finite sample prediction (AS182), using isolated initialized Rust workspaces.
/// The fitted buffers and observation count remain unchanged on
/// success, allocation failure, numerical error and unwind. The recurrence follows
/// https://raw.githubusercontent.com/r-devel/r-svn/master/src/library/stats/src/starma.c
pub(super) fn forkal(
    source: &StarmaModel,
    d: i32,
    il: i32,
    delta: &[f64],
) -> Result<(Vec<f64>, Vec<f64>), ForecastError> {
    if il < 1 {
        return Err(ForecastError::Kernel(11));
    }
    let (rd, rz) = forecast_dimensions(source.r, source.n, d)?;
    if delta.len() != d as usize
        || source.p < 0
        || source.q < 0
        || i64::from(source.r) != i64::from(source.p).max(i64::from(source.q) + 1)
        || i64::from(source.np) != i64::from(source.r) * (i64::from(source.r) + 1) / 2
        || source.nrbar < 1
        || (source.r > 1
            && i64::from(source.nrbar) != i64::from(source.np) * (i64::from(source.np) - 1) / 2)
    {
        return Err(ForecastError::Dimensions);
    }
    if source.p == 0 && source.q == 0 {
        return Err(ForecastError::Kernel(4));
    }
    let mut G = source.try_clone()?;
    G.a = forecast_zeros(rd as usize)?;
    G.P = forecast_zeros(rz as usize)?;
    let mut store = forecast_zeros(rd as usize)?;
    let mut y = forecast_zeros(il as usize)?;
    let mut amse = forecast_zeros(il as usize)?;
    let mut ifault = 0;
    let p = G.p;
    let r = G.r;
    let n = G.n;
    let np = G.np;

    let mut phii: c_double;
    let mut phij: c_double;
    let mut sigma2: c_double;
    let mut a1: c_double;
    let mut aa: c_double;
    let mut tmp: c_double;
    let mut k: c_int;
    let mut nu: c_int = 0;
    let mut k1: c_int;
    let i45: c_int;
    let mut jj: c_int;
    let mut kk: c_int;
    let mut lk: c_int;
    let mut ll: c_int;
    let nt: c_int;
    let mut kk1: c_int;
    let mut lk1: c_int;
    let mut ind: c_int;
    let jkl: c_int;
    let mut kkk: c_int;
    let mut ind1: c_int;
    let mut ind2: c_int;

    /* Find initial likelihood conditions. */
    if r == 1 {
        G.a[0] = 0.0;
        G.V[0] = 1.0;
        G.P[0] = 1.0 / (1.0 - G.phi[0] * G.phi[0]);
    } else {
        starma(&mut G, &mut ifault);
        if ifault != 0 {
            return Err(ForecastError::Kernel(ifault));
        }
    }

    // GNU R stats/src/starma.c stores G.w[n-j-2], including its one-observation
    // offset. forecast_dimensions validates the history this indexing needs.
    /* Calculate data transformations */
    nt = n - d;
    if d > 0 {
        for j in 0..d {
            store[j as usize] = G.w[(n - j - 2) as usize];
            if ISNAN(store[j as usize]) {
                return Err(ForecastError::MissingHistory);
            }
        }
        for i in 0..nt {
            aa = 0.0;
            for k in 0..d {
                aa -= delta[k as usize] * G.w[(d + i - k - 1) as usize];
            }
            G.w[i as usize] = G.w[(i + d) as usize] + aa;
        }
    }

    /* Evaluate likelihood to obtain final Kalman filter conditions */
    {
        let mut sumlog = 0.0_f64;
        let mut ssq_val = 0.0_f64;
        let mut nit_val: c_int = 0;
        G.n = nt;
        karma(&mut G, &mut sumlog, &mut ssq_val, 1, &mut nit_val);
    }

    let StarmaModel {
        phi,
        mut a,
        mut P,
        V,
        mut xrow,
        resid,
        ..
    } = G;

    /* Calculate m.l.e. of sigma squared */
    sigma2 = 0.0;
    for j in 0..nt {
        let tmp = resid[j as usize];
        if !ISNAN(tmp) {
            nu += 1;
            sigma2 += tmp * tmp;
        }
    }
    sigma2 /= nu as c_double;

    /* reset the initial a and P when differencing occurs */
    if d > 0 {
        for i in 0..np {
            xrow[i as usize] = P[i as usize];
        }
        for i in 0..rz {
            P[i as usize] = 0.0;
        }
        ind = 0;
        for j in 0..r {
            k = j * (rd + 1) - j * (j + 1) / 2;
            for i in j..r {
                P[k as usize] = xrow[ind as usize];
                ind += 1;
                k += 1;
            }
        }
        for j in 0..d {
            a[(r + j) as usize] = store[j as usize];
        }
    }

    i45 = 2 * rd + 1;
    jkl = r * (2 * d + r + 1) / 2;

    for l in 0..il {
        /* predict a */
        a1 = a[0];
        for i in 0..r - 1 {
            a[i as usize] = a[(i + 1) as usize];
        }
        a[(r - 1) as usize] = 0.0;
        for j in 0..p {
            a[j as usize] += phi[j as usize] * a1;
        }
        if d > 0 {
            for j in 0..d {
                a1 += delta[j as usize] * a[(r + j) as usize];
            }
            for i in (r + 1..rd).rev() {
                a[i as usize] = a[(i - 1) as usize];
            }
            a[r as usize] = a1;
        }

        /* predict P */
        if d > 0 {
            for i in 0..d {
                store[i as usize] = 0.0;
                for j in 0..d {
                    ll = if i > j { i } else { j };
                    k = if i < j { i } else { j };
                    jj = jkl + (ll - k) + k * (2 * d + 2 - k - 1) / 2;
                    store[i as usize] += delta[j as usize] * P[jj as usize];
                }
            }
            if d > 1 {
                for j in 0..d - 1 {
                    jj = d - j - 1;
                    lk = (jj - 1) * (2 * d + 2 - jj) / 2 + jkl;
                    lk1 = jj * (2 * d + 1 - jj) / 2 + jkl;
                    for i in 0..=j {
                        P[lk1 as usize] = P[lk as usize];
                        lk1 += 1;
                        lk += 1;
                    }
                }
                for j in 0..d - 1 {
                    P[(jkl + j + 1) as usize] = store[j as usize] + P[(r + j) as usize];
                }
            }
            P[jkl as usize] = P[0];
            for i in 0..d {
                P[jkl as usize] = P[jkl as usize]
                    + delta[i as usize] * (store[i as usize] + 2.0 * P[(r + i) as usize]);
            }
            for i in 0..d {
                store[i as usize] = P[(r + i) as usize];
            }
            for j in 0..r {
                kk1 = (j + 1) * (2 * rd - j - 2) / 2 + r;
                k1 = j * (2 * rd - j - 1) / 2 + r;
                for i in 0..d {
                    kk = kk1 + i;
                    k = k1 + i;
                    P[k as usize] = phi[j as usize] * store[i as usize];
                    if j < r - 1 {
                        P[k as usize] = P[k as usize] + P[kk as usize];
                    }
                }
            }

            for j in 0..r {
                store[j as usize] = 0.0;
                kkk = (j + 1) * (i45 - j - 1) / 2 - d;
                for i in 0..d {
                    store[j as usize] += delta[i as usize] * P[kkk as usize];
                    kkk += 1;
                }
            }
            for j in 0..r {
                k = (j + 1) * (rd + 1) - (j + 1) * (j + 2) / 2;
                for i in 0..d - 1 {
                    k -= 1;
                    P[k as usize] = P[(k - 1) as usize];
                }
            }
            for j in 0..r {
                k = j * (2 * rd - j - 1) / 2 + r;
                P[k as usize] = store[j as usize] + phi[j as usize] * P[0];
                if j < r - 1 {
                    P[k as usize] = P[k as usize] + P[(j + 1) as usize];
                }
            }
        }
        for i in 0..r {
            store[i as usize] = P[i as usize];
        }

        ind = 0;
        let dt_val = P[0];
        for j in 0..r {
            phij = phi[j as usize];
            let phijdt = phij * dt_val;
            ind2 = j * (2 * rd - j + 1) / 2 - 1;
            ind1 = (j + 1) * (i45 - j - 1) / 2 - 1;
            for i in j..r {
                ind2 += 1;
                phii = phi[i as usize];
                P[ind2 as usize] = V[ind as usize] + phii * phijdt;
                if j < r - 1 {
                    P[ind2 as usize] = P[ind2 as usize] + store[(j + 1) as usize] * phii;
                }
                if i < r - 1 {
                    ind1 += 1;
                    P[ind2 as usize] =
                        P[ind2 as usize] + store[(i + 1) as usize] * phij + P[ind1 as usize];
                }
                ind += 1;
            }
        }

        /* predict y */
        y[l as usize] = a[0];
        for j in 0..d {
            y[l as usize] += a[(r + j) as usize] * delta[j as usize];
        }

        /* calculate m.s.e. of y */
        let mut ams_val = P[0];
        if d > 0 {
            for j in 0..d {
                k = r * (i45 - r) / 2 + j * (2 * d + 1 - j) / 2;
                tmp = delta[j as usize];
                ams_val += 2.0 * tmp * P[(r + j) as usize] + P[k as usize] * tmp * tmp;
            }
            for j in 0..d - 1 {
                k = r * (i45 - r) / 2 + 1 + j * (2 * d + 1 - j) / 2;
                for i in j + 1..d {
                    ams_val += 2.0 * delta[i as usize] * delta[j as usize] * P[k as usize];
                    k += 1;
                }
            }
        }
        amse[l as usize] = ams_val * sigma2;
    }

    Ok((y, amse))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(orders: [i32; 5], observations: &[f64]) -> StarmaModel {
        let mut model = StarmaModel::new(orders, observations.len() as i32, 0, 0, 0, -1.0).unwrap();
        model.w[..observations.len()].copy_from_slice(observations);
        model.wkeep[..observations.len()].copy_from_slice(observations);
        model
    }

    #[test]
    fn owned_model_rejects_invalid_and_overflowing_dimensions_before_allocation() {
        assert!(matches!(
            StarmaModel::new([-1, 0, 0, 0, 0], 3, 0, 0, 0, 0.0),
            Err(ForecastError::Dimensions)
        ));
        assert!(matches!(
            StarmaModel::new([1, 0, 0, 0, 0], 3, 0, 4, 0, 0.0),
            Err(ForecastError::Dimensions)
        ));
        assert!(matches!(
            StarmaModel::new([1024, 0, 0, 0, 0], 3, 0, 0, 0, 0.0),
            Err(ForecastError::PackedOverflow)
        ));
        assert!(matches!(
            StarmaModel::new([1, 0, i32::MAX, 0, 2], 3, 0, 0, 0, 0.0),
            Err(ForecastError::PackedOverflow)
        ));
        assert!(matches!(
            StarmaModel::new([1, 0, 0, 0, 0], i32::MAX, 1, 0, 0, 0.0),
            Err(ForecastError::PackedOverflow)
        ));
    }

    #[test]
    fn owned_kernels_match_independent_ma1_and_ar2_gaussian_innovations() {
        // Cholesky innovations of the stationary Toeplitz covariance matrices:
        // MA(1): gamma=(5/4,1/2,0), AR(2): gamma=(48/25,32/25,28/25).
        // The AR covariance follows the Yule-Walker equations, independently
        // of AS154's packed-state initializer and inclusion regression.
        for (orders, phi, theta, residuals, ssq_expected, determinant) in [
            (
                [0, 1, 0, 0, 0],
                vec![],
                vec![0.5],
                [
                    1.0 / (5.0_f64 / 4.0).sqrt(),
                    (8.0 / 5.0) / (21.0_f64 / 20.0).sqrt(),
                    (47.0 / 21.0) / (85.0_f64 / 84.0).sqrt(),
                ],
                4.0 / 5.0 + 256.0 / 105.0 + 8836.0 / 1785.0,
                85.0_f64 / 64.0,
            ),
            (
                [2, 0, 0, 0, 0],
                vec![0.5, 0.25],
                vec![],
                [
                    1.0 / (48.0_f64 / 25.0).sqrt(),
                    (4.0 / 3.0) / (16.0_f64 / 15.0).sqrt(),
                    7.0 / 4.0,
                ],
                21.0 / 4.0,
                256.0_f64 / 125.0,
            ),
        ] {
            let mut model = model(orders, &[1.0, 2.0, 3.0]);
            model.phi[..phi.len()].copy_from_slice(&phi);
            model.theta[..theta.len()].copy_from_slice(&theta);
            let mut fault = 0;
            starma(&mut model, &mut fault);
            assert_eq!(fault, 0);
            let (mut logdet, mut ssq, mut iteration) = (0.0, 0.0, 0);
            karma(&mut model, &mut logdet, &mut ssq, 1, &mut iteration);
            for (actual, expected) in model.resid.iter().zip(residuals) {
                assert!(
                    (actual - expected).abs() < 1e-12,
                    "orders={orders:?}: {actual} vs {expected}"
                );
            }
            assert!((ssq - ssq_expected).abs() < 1e-12);
            assert!((logdet - determinant.ln()).abs() < 1e-12);
            assert_eq!(model.nused, 3);
        }
    }

    #[test]
    fn owned_filter_preserves_missing_observation_and_quick_recursion_counts() {
        let mut missing = model([1, 0, 0, 0, 0], &[1.0, NA_REAL, 3.0]);
        missing.phi[0] = 0.5;
        let mut fault = 0;
        starma(&mut missing, &mut fault);
        let (mut logdet, mut ssq, mut iteration) = (0.0, 0.0, 0);
        karma(&mut missing, &mut logdet, &mut ssq, 1, &mut iteration);
        // Missing x2 leaves prediction variance 1; x3 has variance 5/4
        // and conditional mean 1/4. Both available observations count.
        assert_eq!(missing.nused, 2);
        assert!(ISNAN(missing.resid[1]));
        assert!((missing.resid[0] - (3.0_f64 / 4.0).sqrt()).abs() < 1e-12);
        assert!((missing.resid[2] - (11.0 / 4.0) / (5.0_f64 / 4.0).sqrt()).abs() < 1e-12);
        assert!((ssq - 34.0 / 5.0).abs() < 1e-12);
        assert!((logdet - (5.0_f64 / 3.0).ln()).abs() < 1e-12);

        let mut quick = model([1, 0, 0, 0, 0], &[1.0, 2.0, 3.0]);
        quick.phi[0] = 0.5;
        quick.delta = 0.001;
        for _ in 0..2 {
            starma(&mut quick, &mut fault);
            let (mut logdet, mut ssq, mut iteration) = (0.0, 0.0, 0);
            karma(&mut quick, &mut logdet, &mut ssq, 1, &mut iteration);
            assert_eq!(iteration, 1);
            assert_eq!(quick.nused, 3);
            assert_eq!(&quick.resid[1..3], &[1.5, 2.0]);
            assert!((ssq - 7.0).abs() < 1e-12);
            assert!((logdet - (4.0_f64 / 3.0).ln()).abs() < 1e-12);
        }
    }
    #[test]
    fn owned_forecast_zero_horizon_rejects_before_workspace_or_state_changes() {
        let mut source = model([1, 0, 0, 0, 0], &[2.0, 4.0, 8.0]);
        source.phi[0] = 0.5;
        let original = source.try_clone().unwrap();
        assert_eq!(forkal(&source, 0, 0, &[]), Err(ForecastError::Kernel(11)));
        assert_eq!(source, original);
        let (means, variances) = forkal(&source, 0, 3, &[]).unwrap();
        assert_eq!(means, [4.0, 2.0, 1.0]);
        assert_eq!(variances, [16.0, 20.0, 21.0]);
        assert_eq!(source, original);
    }
}
