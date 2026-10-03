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

use core::ffi::{c_double, c_int, c_void};

use crate::sexp::ffi::{ISNAN, NA_REAL};

/// Mirror of the C `starma_struct` from ts.h.
/// Layout must match the C struct exactly for FFI compatibility.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct starma_struct {
    pub p: c_int,
    pub q: c_int,
    pub r: c_int,
    pub np: c_int,
    pub nrbar: c_int,
    pub n: c_int,
    pub ncond: c_int,
    pub m: c_int,
    pub trans: c_int,
    pub method: c_int,
    pub nused: c_int,
    pub mp: c_int,
    pub mq: c_int,
    pub msp: c_int,
    pub msq: c_int,
    pub ns: c_int,
    pub delta: c_double,
    pub s2: c_double,
    pub params: *mut c_double,
    pub phi: *mut c_double,
    pub theta: *mut c_double,
    pub a: *mut c_double,
    pub P: *mut c_double,
    pub V: *mut c_double,
    pub thetab: *mut c_double,
    pub xnext: *mut c_double,
    pub xrow: *mut c_double,
    pub rbar: *mut c_double,
    pub w: *mut c_double,
    pub wkeep: *mut c_double,
    pub resid: *mut c_double,
    pub reg: *mut c_double,
}

/// Internal helper — update d, rbar, thetab by inclusion of xnext and ynext.
/// (AS154 subroutine inclu2)
unsafe fn inclu2(
    np: c_int,
    xnext: *mut c_double,
    xrow: *mut c_double,
    mut ynext: c_double,
    d: *mut c_double,
    rbar: *mut c_double,
    thetab: *mut c_double,
) {
    unsafe {
        let mut cbar: c_double;
        let mut sbar: c_double;
        let mut di: c_double;
        let mut xi: c_double;
        let mut xk: c_double;
        let mut rbthis: c_double;
        let mut dpi: c_double;
        let mut ithisr: c_int = 0;

        for i in 0..np {
            *xrow.add(i as usize) = *xnext.add(i as usize);
        }

        let mut i: c_int = 0;
        while i < np {
            if *xrow.add(i as usize) != 0.0 {
                xi = *xrow.add(i as usize);
                di = *d.add(i as usize);
                dpi = di + xi * xi;
                *d.add(i as usize) = dpi;
                cbar = di / dpi;
                sbar = xi / dpi;
                let mut k: c_int = i + 1;
                while k < np {
                    xk = *xrow.add(k as usize);
                    rbthis = *rbar.add(ithisr as usize);
                    *xrow.add(k as usize) = xk - xi * rbthis;
                    *rbar.add(ithisr as usize) = cbar * rbthis + sbar * xk;
                    ithisr += 1;
                    k += 1;
                }
                xk = ynext;
                ynext = xk - xi * *thetab.add(i as usize);
                *thetab.add(i as usize) = cbar * *thetab.add(i as usize) + sbar * xk;
                if di == 0.0 {
                    return;
                }
            } else {
                ithisr += np - i - 1;
            }
            i += 1;
        }
    }
}

/// Set initial values for the Kalman filter.
pub unsafe fn starma(g: *mut c_void, ifault: *mut c_int) {
    unsafe {
        let G = &mut *(g as *mut starma_struct);
        let p = G.p;
        let q = G.q;
        let r = G.r;
        let np = G.np;
        let nrbar = G.nrbar;
        let phi = G.phi;
        let theta = G.theta;
        let a = G.a;
        let P = G.P;
        let V = G.V;
        let thetab = G.thetab;
        let xnext = G.xnext;
        let xrow = G.xrow;
        let rbar = G.rbar;

        /* Check if ar(1) */
        if !(q > 0 || p > 1) {
            *V.add(0) = 1.0;
            *a.add(0) = 0.0;
            *P.add(0) = 1.0 / (1.0 - *phi.add(0) * *phi.add(0));
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
            *a.add(i as usize) = 0.0;
            if i >= p {
                *phi.add(i as usize) = 0.0;
            }
            *V.add(i as usize) = 0.0;
            if i < q + 1 {
                *V.add(i as usize) = *theta.add((i - 1) as usize);
            }
        }
        *a.add(0) = 0.0;
        if p == 0 {
            *phi.add(0) = 0.0;
        }
        *V.add(0) = 1.0;
        let mut ind = r;
        for j in 1..r {
            let vj = *V.add(j as usize);
            for i in j..r {
                *V.add(ind as usize) = *V.add(i as usize) * vj;
                ind += 1;
            }
        }

        /* Now find P(0). */
        if p > 0 {
            for i in 0..nrbar {
                *rbar.add(i as usize) = 0.0;
            }
            for i in 0..np {
                *P.add(i as usize) = 0.0;
                *thetab.add(i as usize) = 0.0;
                *xnext.add(i as usize) = 0.0;
            }
            ind = 0;
            let mut ind1: c_int = -1;
            let npr = np - r;
            let npr1 = npr + 1;
            let mut indj: c_int = npr;
            let mut ind2: c_int = npr - 1;
            for j in 0..r {
                let phij = *phi.add(j as usize);
                *xnext.add(indj as usize) = 0.0;
                indj += 1;
                let mut indi: c_int = npr1 + j;
                for i in j..r {
                    let mut ynext = *V.add(ind as usize);
                    ind += 1;
                    let phii = *phi.add(i as usize);
                    if j != r - 1 {
                        *xnext.add(indj as usize) = -phii;
                        if i != r - 1 {
                            *xnext.add(indi as usize) -= phij;
                            ind1 += 1;
                            *xnext.add(ind1 as usize) = -1.0;
                        }
                    }
                    *xnext.add(npr as usize) = -phii * phij;
                    ind2 += 1;
                    if ind2 >= np {
                        ind2 = 0;
                    }
                    *xnext.add(ind2 as usize) += 1.0;
                    inclu2(np, xnext, xrow, ynext, P, rbar, thetab);
                    *xnext.add(ind2 as usize) = 0.0;
                    if i != r - 1 {
                        *xnext.add(indi as usize) = 0.0;
                        indi += 1;
                        *xnext.add(ind1 as usize) = 0.0;
                    }
                }
            }

            let mut ithisr = nrbar - 1;
            let mut im = np - 1;
            for i in 0..np {
                let mut bi = *thetab.add(im as usize);
                let mut jm = np - 1;
                for _j in 0..i as c_int {
                    bi -= *rbar.add(ithisr as usize) * *P.add(jm as usize);
                    ithisr -= 1;
                    jm -= 1;
                }
                *P.add(im as usize) = bi;
                im -= 1;
            }

            /* now re-order P. */
            ind = npr;
            for i in 0..r {
                *xnext.add(i as usize) = *P.add(ind as usize);
                ind += 1;
            }
            ind = np - 1;
            ind1 = npr - 1;
            for i in 0..npr {
                *P.add(ind as usize) = *P.add(ind1 as usize);
                ind -= 1;
                ind1 -= 1;
            }
            for i in 0..r {
                *P.add(i as usize) = *xnext.add(i as usize);
            }
        } else {
            /* P(0) is obtained by backsubstitution for a moving average process. */
            let mut indn = np;
            ind = np;
            for i in 0..r {
                for j in 0..=i {
                    ind -= 1;
                    *P.add(ind as usize) = *V.add(ind as usize);
                    if j != 0 {
                        indn -= 1;
                        *P.add(ind as usize) += *P.add(indn as usize);
                    }
                }
            }
        }
    }
}

/// Update Kalman filter by inclusion of data values w(1) to w(n).
pub unsafe fn karma(g: *mut c_void, sumlog: *mut f64, ssq: *mut f64, iupd: c_int, nit: *mut c_int) {
    unsafe {
        let G = &mut *(g as *mut starma_struct);
        let p = G.p;
        let q = G.q;
        let r = G.r;
        let n = G.n;
        let phi = G.phi;
        let theta = G.theta;
        let a = G.a;
        let P = G.P;
        let V = G.V;
        let w = G.w;
        let resid = G.resid;
        let work = G.xnext;

        if *nit == 0 {
            // A new pass. nused from the previous arma0fa call must not
            // be added again if this pass drops into the quick recursion.
            G.nused = 0;
            let mut nu: c_int = 0;
            for i in 0..n {
                /* prediction. */
                if iupd != 1 || i > 0 {
                    /* here dt = ft - 1.0 */
                    let dt_val = if r > 1 { *P.add(r as usize) } else { 0.0 };
                    if dt_val < G.delta {
                        /* jump to quick recursions */
                        quick_recur(G, nit, ssq, w, phi, theta, resid, p, q, n, i as usize, nu);
                        return;
                    }
                    let a1 = *a.add(0);
                    for j in 0..r - 1 {
                        *a.add(j as usize) = *a.add((j + 1) as usize);
                    }
                    *a.add((r - 1) as usize) = 0.0;
                    for j in 0..p {
                        *a.add(j as usize) += *phi.add(j as usize) * a1;
                    }
                    if *P.add(0) == 0.0 {
                        /* last obs was available */
                        let mut ind: c_int = -1;
                        let mut indn: c_int = r;
                        for j in 0..r {
                            for l in j..r {
                                ind += 1;
                                *P.add(ind as usize) = *V.add(ind as usize);
                                if l < r - 1 {
                                    *P.add(ind as usize) += *P.add(indn as usize);
                                    indn += 1;
                                }
                            }
                        }
                    } else {
                        for j in 0..r {
                            *work.add(j as usize) = *P.add(j as usize);
                        }
                        let mut ind: c_int = -1;
                        let mut indn: c_int = r;
                        let dt_p = *P.add(0);
                        for j in 0..r {
                            let phij = *phi.add(j as usize);
                            let phijdt = phij * dt_p;
                            for l in j..r {
                                ind += 1;
                                *P.add(ind as usize) =
                                    *V.add(ind as usize) + *phi.add(l as usize) * phijdt;
                                if j < r - 1 {
                                    *P.add(ind as usize) +=
                                        *work.add((j + 1) as usize) * *phi.add(l as usize);
                                }
                                if l < r - 1 {
                                    *P.add(ind as usize) +=
                                        *work.add((l + 1) as usize) * phij + *P.add(indn as usize);
                                    indn += 1;
                                }
                            }
                        }
                    }
                }

                /* updating. */
                let ft = *P.add(0);
                if !ISNAN(*w.add(i as usize)) {
                    let ut = *w.add(i as usize) - *a.add(0);
                    if r > 1 {
                        let mut ind_p: c_int = r;
                        for j in 1..r {
                            let g_val = *P.add(j as usize) / ft;
                            *a.add(j as usize) += g_val * ut;
                            for l in j..r {
                                *P.add(ind_p as usize) -= g_val * *P.add(l as usize);
                                ind_p += 1;
                            }
                        }
                    }
                    *a.add(0) = *w.add(i as usize);
                    *resid.add(i as usize) = ut / ft.sqrt();
                    *ssq += ut * ut / ft;
                    *sumlog += ft.ln();
                    nu += 1;
                    for l in 0..r {
                        *P.add(l as usize) = 0.0;
                    }
                } else {
                    *resid.add(i as usize) = NA_REAL;
                }
            }
            *nit = n;
            G.nused = nu;
        } else {
            /* quick recursions: never used with missing values */
            quick_recur(G, nit, ssq, w, phi, theta, resid, p, q, n, 0, G.nused);
        }
    }
}

/// Quick recursions helper — extracted from karma's L610 label.
unsafe fn quick_recur(
    G: &mut starma_struct,
    nit: *mut c_int,
    ssq: *mut f64,
    w: *mut c_double,
    phi: *mut c_double,
    theta: *mut c_double,
    resid: *mut c_double,
    p: c_int,
    q: c_int,
    n: c_int,
    start_i: usize,
    nu0: c_int,
) {
    unsafe {
        let mut nu: c_int = nu0;
        let mut et: c_double;
        let mut indw: c_int;

        *nit = start_i as c_int;
        for ii in start_i..n as usize {
            et = *w.add(ii);
            indw = ii as c_int;
            for j in 0..p as usize {
                indw -= 1;
                if indw < 0 {
                    break;
                }
                et -= *phi.add(j) * *w.add(indw as usize);
            }
            let qm = if ii < q as usize { ii } else { q as usize };
            for j in 0..qm {
                et -= *theta.add(j) * *resid.add(ii - j - 1);
            }
            *resid.add(ii) = et;
            *ssq += et * et;
            nu += 1;
        }
        G.nused = nu;
    }
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
    if r < 1 || d < 0 || n < 1 { return Err(ForecastError::Dimensions); }
    if d >= n { return Err(ForecastError::History); }
    let rd = i64::from(r) + i64::from(d);
    let product = rd.checked_mul(rd + 1).ok_or(ForecastError::PackedOverflow)?;
    if product > i64::from(i32::MAX) { return Err(ForecastError::PackedOverflow); }
    Ok((rd as i32, (product / 2) as i32))
}

fn forecast_zeros(length: usize) -> Result<Vec<f64>, ForecastError> {
    let mut values = Vec::new();
    values.try_reserve_exact(length).map_err(|_| ForecastError::Allocation)?;
    values.resize(length, 0.0);
    Ok(values)
}

/// Finite sample prediction (AS182), using isolated initialized Rust workspaces.
/// The fitted buffers, observation count and native pointers remain unchanged on
/// success, allocation failure, numerical error and unwind. The recurrence follows
/// https://raw.githubusercontent.com/r-devel/r-svn/master/src/library/stats/src/starma.c
pub(super) fn forkal(
    source: &starma_struct,
    source_buffers: &[Vec<f64>; 14],
    d: i32,
    il: i32,
    delta: &[f64],
) -> Result<(Vec<f64>, Vec<f64>), ForecastError> {
    let (rd, rz) = forecast_dimensions(source.r, source.n, d)?;
    if il < 0 || delta.len() != d as usize || source.p < 0 || source.q < 0
        || i64::from(source.r) != i64::from(source.p).max(i64::from(source.q) + 1)
        || i64::from(source.np) != i64::from(source.r) * (i64::from(source.r) + 1) / 2
        || source.nrbar < 1
        || (source.r > 1 && i64::from(source.nrbar) != i64::from(source.np) * (i64::from(source.np) - 1) / 2)
    { return Err(ForecastError::Dimensions); }
    if source.p == 0 && source.q == 0 { return Err(ForecastError::Kernel(4)); }
    let required = [0, source.r, source.r, source.r, source.np, source.np,
        source.np, source.np, source.np, source.nrbar, source.n, source.n, source.n, 0];
    if source_buffers.iter().zip(required).any(|(buffer, length)| buffer.len() < length as usize) {
        return Err(ForecastError::Dimensions);
    }
    let mut workspaces: [Vec<f64>; 14] = std::array::from_fn(|_| Vec::new());
    for (copy, original) in workspaces.iter_mut().zip(source_buffers) {
        copy.try_reserve_exact(original.len()).map_err(|_| ForecastError::Allocation)?;
        copy.extend_from_slice(original);
    }
    workspaces[3] = forecast_zeros(rd as usize)?;
    workspaces[4] = forecast_zeros(rz as usize)?;
    let [mut params, mut phi, mut theta, mut a, mut P, mut V, mut thetab,
        mut xnext, mut xrow, mut rbar, mut w, mut wkeep, mut resid, mut reg] = workspaces;
    let mut G = *source;
    macro_rules! bind_workspaces {
        () => {{
            G.params = params.as_mut_ptr(); G.phi = phi.as_mut_ptr(); G.theta = theta.as_mut_ptr();
            G.a = a.as_mut_ptr(); G.P = P.as_mut_ptr(); G.V = V.as_mut_ptr();
            G.thetab = thetab.as_mut_ptr(); G.xnext = xnext.as_mut_ptr(); G.xrow = xrow.as_mut_ptr();
            G.rbar = rbar.as_mut_ptr(); G.w = w.as_mut_ptr(); G.wkeep = wkeep.as_mut_ptr();
            G.resid = resid.as_mut_ptr(); G.reg = reg.as_mut_ptr();
        }};
    }
    let mut store = forecast_zeros(rd as usize)?;
    let mut y = forecast_zeros(il as usize)?;
    let mut amse = forecast_zeros(il as usize)?;
    if il == 0 { return Ok((y, amse)); }
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
        a[0] = 0.0;
        V[0] = 1.0;
        P[0] = 1.0 / (1.0 - phi[0] * phi[0]);
    } else {
        bind_workspaces!();
        unsafe { starma((&mut G as *mut starma_struct).cast::<c_void>(), &mut ifault); }
        if ifault != 0 { return Err(ForecastError::Kernel(ifault)); }
    }

    // GNU R stats/src/starma.c stores w[n-j-2], including its one-observation
    // offset. forecast_dimensions validates the history this indexing needs.
    /* Calculate data transformations */
    nt = n - d;
    if d > 0 {
        for j in 0..d {
            store[j as usize] = w[(n - j - 2) as usize];
            if ISNAN(store[j as usize]) {
                return Err(ForecastError::MissingHistory);
            }
        }
        for i in 0..nt {
            aa = 0.0;
            for k in 0..d {
                aa -= delta[k as usize] * w[(d + i - k - 1) as usize];
            }
            w[i as usize] = w[(i + d) as usize] + aa;
        }
    }

    /* Evaluate likelihood to obtain final Kalman filter conditions */
    {
        let mut sumlog = 0.0_f64;
        let mut ssq_val = 0.0_f64;
        let mut nit_val: c_int = 0;
        G.n = nt;
        bind_workspaces!();
        unsafe { karma((&mut G as *mut starma_struct).cast::<c_void>(), &mut sumlog, &mut ssq_val, 1, &mut nit_val); }
    }

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
                    P[(jkl + j + 1) as usize] =
                        store[j as usize] + P[(r + j) as usize];
                }
            }
            P[jkl as usize] = P[0];
            for i in 0..d {
                P[jkl as usize] += delta[i as usize]
                    * (store[i as usize] + 2.0 * P[(r + i) as usize]);
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
                        P[k as usize] += P[kk as usize];
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
                    P[k as usize] += P[(j + 1) as usize];
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
                    P[ind2 as usize] += store[(j + 1) as usize] * phii;
                }
                if i < r - 1 {
                    ind1 += 1;
                    P[ind2 as usize] +=
                        store[(i + 1) as usize] * phij + P[ind1 as usize];
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
                ams_val +=
                    2.0 * tmp * P[(r + j) as usize] + P[k as usize] * tmp * tmp;
            }
            for j in 0..d - 1 {
                k = r * (i45 - r) / 2 + 1 + j * (2 * d + 1 - j) / 2;
                for i in j + 1..d {
                    ams_val += 2.0
                        * delta[i as usize]
                        * delta[j as usize]
                        * P[k as usize];
                    k += 1;
                }
            }
        }
        amse[l as usize] = ams_val * sigma2;
    }

    Ok((y, amse))
}
