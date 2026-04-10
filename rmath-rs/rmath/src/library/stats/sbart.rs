
//! Cubic B-spline smoother (sbart).
//! Port of r-source/src/library/stats/src/sbart.c
//!
//! Originally translated by f2c from Fortran code by Finbarr O'Sullivan.
//! The C wrapper `sbart_` manages the control flow:
//! - When isetup == 1: uses precomputed SIGMA and X'WX matrices
//! - When isetup == 0 or 2: calls sgram, stxwx to compute SIGMA and X'WX
//! - When ispar >= 0: uses given spar value
//! - When ispar < 0: uses Brent's method to find optimal spar
//! - Calls sslvrg to compute the actual spline

use core::ffi::c_int;

// ----- CRIT macro equivalent -----
// "Correct" ./sslvrg.f (line 129):   crit = 3 + (dofoff-df)**2
#[inline(always)]
fn CRIT(fx: f64, icrit: c_int) -> f64 {
    if icrit == 3 { fx - 3.0 } else { fx }
}

// ----- BIG_f constant -----
const BIG_f: f64 = 1e100;

// ----- c_Gold: squared inverse of the golden ratio -----
// == (3. - sqrt(5.)) / 2.
const c_Gold: f64 = 0.381966011250105151795413165634;

// ----- SSPLINE_COMP inline -----
// Computes lspar from spar (or uses spar directly if spar_is_lambda),
// then calls sslvrg to evaluate the spline criterion.
#[inline(always)]
unsafe fn SSPLINE_COMP(
    spar: f64,
    spar_is_lambda: bool,
    ratio: f64,
    penalt: *mut f64,
    dofoff: *mut f64,
    xs: *mut f64,
    ys: *mut f64,
    ws: *mut f64,
    ssw: *mut f64,
    n: *mut c_int,
    knot: *mut f64,
    nk: *mut c_int,
    coef: *mut f64,
    sz: *mut f64,
    lev: *mut f64,
    crit: *mut f64,
    icrit: *mut c_int,
    lspar: *mut f64,
    xwy: *mut f64,
    hs0: *mut f64,
    hs1: *mut f64,
    hs2: *mut f64,
    hs3: *mut f64,
    sg0: *mut f64,
    sg1: *mut f64,
    sg2: *mut f64,
    sg3: *mut f64,
    abd: *mut f64,
    p1ip: *mut f64,
    p2ip: *mut f64,
    ld4: *mut c_int,
    ldnk: *mut c_int,
    ier: *mut c_int,
) {
    if spar_is_lambda {
        *lspar = spar;
    } else {
        *lspar = ratio * 16.0_f64.powf(spar * 6.0 - 2.0);
    }
    sslvrg(
        penalt, dofoff, xs, ys, ws, ssw, n, knot, nk, coef, sz, lev, crit, icrit, lspar, xwy, hs0,
        hs1, hs2, hs3, sg0, sg1, sg2, sg3, abd, p1ip, p2ip, ld4, ldnk, ier,
    );
}

// =========================================================================
// Ported Fortran routines (sgram, stxwx, sslvrg)
// =========================================================================

/// sgram - compute the Gram matrix SIGMA of B-spline second derivatives.
/// SIGMA[i,j] := Int B''(i,t) B''(j,t) dt
///
/// Output diagonals: sg0 (main), sg1 (super-1), sg2 (super-2), sg3 (super-3)
///
/// Ported from R's Fortran sgram.f.
/// All arrays use 1-based Fortran indexing internally.
unsafe fn sgram(
    sg0: *mut f64,
    sg1: *mut f64,
    sg2: *mut f64,
    sg3: *mut f64,
    knot: *mut f64,
    nk: *mut c_int,
) {
    let nk = *nk as usize;
    let lentb = nk + 4;
    let mut work = [0.0f64; 16];
    let mut vnikx = [0.0f64; 12]; // 4 x 3, column-major
    let mut yw1 = [0.0f64; 4];
    let mut yw2 = [0.0f64; 4];

    // Initialize sigma vectors
    for i in 0..nk {
        *sg0.add(i) = 0.0;
        *sg1.add(i) = 0.0;
        *sg2.add(i) = 0.0;
        *sg3.add(i) = 0.0;
    }

    let mut ileft: c_int = 1; // 1-based

    for i in 1..=nk {
        // Find interval using interv (0-indexed result, but Fortran uses 1-based)
        let mut mflag: c_int = 0;
        ileft = crate::appl::interv::findInterval(
            knot,
            (nk + 1) as c_int,
            *knot.add(i - 1),
            0,
            0,
            ileft,
            &mut mflag,
        );
        // findInterval returns 0-indexed; Fortran expects 1-based
        ileft += 1;

        // Left end second derivatives: bsplvd(knot, lentb, 4, tb(i), ileft, work, vnikx, 3)
        // vnikx(4,3) column-major: vnikx[(row-1) + (col-1)*4]
        super::bspline::bsplvd(
            knot,
            lentb as c_int,
            4,
            *knot.add(i - 1),
            ileft,
            work.as_mut_ptr(),
            vnikx.as_mut_ptr(),
            3,
        );
        for ii in 0..4 {
            yw1[ii] = vnikx[ii + 2 * 4]; // vnikx(ii, 3)
        }

        // Right end second derivatives
        super::bspline::bsplvd(
            knot,
            lentb as c_int,
            4,
            *knot.add(i),
            ileft,
            work.as_mut_ptr(),
            vnikx.as_mut_ptr(),
            3,
        );
        for ii in 0..4 {
            yw2[ii] = vnikx[ii + 2 * 4] - yw1[ii]; // slope * interval length
        }

        let wpt = *knot.add(i) - *knot.add(i - 1);
        let ileft_u = ileft as usize;

        if ileft_u >= 4 {
            for ii in 0..4 {
                let jj = ii;
                let idx = ileft_u - 4 + ii;
                if idx < nk {
                    let contrib = wpt
                        * (yw1[ii] * yw1[jj]
                            + (yw2[ii] * yw1[jj] + yw2[jj] * yw1[ii]) * 0.5
                            + yw2[ii] * yw2[jj] / 3.0);
                    *sg0.add(idx) += contrib;
                }
                if jj + 1 <= 3 {
                    let idx = ileft_u - 4 + ii + 1;
                    if idx < nk {
                        let contrib = wpt
                            * (yw1[ii] * yw1[jj + 1]
                                + (yw2[ii] * yw1[jj + 1] + yw2[jj + 1] * yw1[ii]) * 0.5
                                + yw2[ii] * yw2[jj + 1] / 3.0);
                        *sg1.add(idx) += contrib;
                    }
                }
                if jj + 2 <= 3 {
                    let idx = ileft_u - 4 + ii + 2;
                    if idx < nk {
                        let contrib = wpt
                            * (yw1[ii] * yw1[jj + 2]
                                + (yw2[ii] * yw1[jj + 2] + yw2[jj + 2] * yw1[ii]) * 0.5
                                + yw2[ii] * yw2[jj + 2] / 3.0);
                        *sg2.add(idx) += contrib;
                    }
                }
                if jj + 3 <= 3 {
                    let idx = ileft_u - 4 + ii + 3;
                    if idx < nk {
                        let contrib = wpt
                            * (yw1[ii] * yw1[jj + 3]
                                + (yw2[ii] * yw1[jj + 3] + yw2[jj + 3] * yw1[ii]) * 0.5
                                + yw2[ii] * yw2[jj + 3] / 3.0);
                        *sg3.add(idx) += contrib;
                    }
                }
            }
        } else if ileft_u == 3 {
            for ii in 0..3 {
                let jj = ii;
                let idx = ileft_u - 3 + ii;
                if idx < nk {
                    let contrib = wpt
                        * (yw1[ii] * yw1[jj]
                            + (yw2[ii] * yw1[jj] + yw2[jj] * yw1[ii]) * 0.5
                            + yw2[ii] * yw2[jj] / 3.0);
                    *sg0.add(idx) += contrib;
                }
                if jj + 1 <= 2 {
                    let idx = ileft_u - 3 + ii + 1;
                    if idx < nk {
                        let contrib = wpt
                            * (yw1[ii] * yw1[jj + 1]
                                + (yw2[ii] * yw1[jj + 1] + yw2[jj + 1] * yw1[ii]) * 0.5
                                + yw2[ii] * yw2[jj + 1] / 3.0);
                        *sg1.add(idx) += contrib;
                    }
                }
                if jj + 2 <= 2 {
                    let idx = ileft_u - 3 + ii + 2;
                    if idx < nk {
                        let contrib = wpt
                            * (yw1[ii] * yw1[jj + 2]
                                + (yw2[ii] * yw1[jj + 2] + yw2[jj + 2] * yw1[ii]) * 0.5
                                + yw2[ii] * yw2[jj + 2] / 3.0);
                        *sg2.add(idx) += contrib;
                    }
                }
            }
        } else if ileft_u == 2 {
            for ii in 0..2 {
                let jj = ii;
                let idx = ileft_u - 2 + ii;
                if idx < nk {
                    let contrib = wpt
                        * (yw1[ii] * yw1[jj]
                            + (yw2[ii] * yw1[jj] + yw2[jj] * yw1[ii]) * 0.5
                            + yw2[ii] * yw2[jj] / 3.0);
                    *sg0.add(idx) += contrib;
                }
                if jj + 1 <= 1 {
                    let idx = ileft_u - 2 + ii + 1;
                    if idx < nk {
                        let contrib = wpt
                            * (yw1[ii] * yw1[jj + 1]
                                + (yw2[ii] * yw1[jj + 1] + yw2[jj + 1] * yw1[ii]) * 0.5
                                + yw2[ii] * yw2[jj + 1] / 3.0);
                        *sg1.add(idx) += contrib;
                    }
                }
            }
        } else if ileft_u == 1 {
            let idx = 0;
            let contrib = wpt
                * (yw1[0] * yw1[0]
                    + (yw2[0] * yw1[0] + yw2[0] * yw1[0]) * 0.5
                    + yw2[0] * yw2[0] / 3.0);
            *sg0.add(idx) += contrib;
        }
    }
}

/// stxwx - compute X'WX and X'Wz matrices.
///
/// Outputs: xwy (X'Wy), hs0-hs3 (diagonals of X'WX)
///
/// Ported from R's Fortran stxwx.f.
/// All arrays use 1-based Fortran indexing internally.
unsafe fn stxwx(
    xs: *mut f64,
    ys: *mut f64,
    ws: *mut f64,
    n: *mut c_int,
    knot: *mut f64,
    nk: *mut c_int,
    xwy: *mut f64,
    hs0: *mut f64,
    hs1: *mut f64,
    hs2: *mut f64,
    hs3: *mut f64,
) {
    let n = *n as usize;
    let nk = *nk as usize;
    let lenxk = nk + 4;
    let mut work = [0.0f64; 16];
    let mut vnikx = [0.0f64; 4]; // 4 x 1
    let eps = 1e-9;

    // Initialize output vectors
    for i in 0..nk {
        *xwy.add(i) = 0.0;
        *hs0.add(i) = 0.0;
        *hs1.add(i) = 0.0;
        *hs2.add(i) = 0.0;
        *hs3.add(i) = 0.0;
    }

    let mut ileft: c_int = 1; // 1-based

    for i in 0..n {
        // Find interval
        let mut mflag: c_int = 0;
        ileft = crate::appl::interv::findInterval(
            knot,
            (nk + 1) as c_int,
            *xs.add(i),
            0,
            0,
            ileft,
            &mut mflag,
        );
        ileft += 1; // convert to 1-based

        if mflag == 1 {
            if *xs.add(i) <= *knot.add((ileft - 1) as usize) + eps {
                ileft -= 1;
            } else {
                return;
            }
        }

        // Evaluate B-splines at x(i)
        super::bspline::bsplvd(
            knot,
            lenxk as c_int,
            4,
            *xs.add(i),
            ileft,
            work.as_mut_ptr(),
            vnikx.as_mut_ptr(),
            1,
        );

        let j_base = ileft as usize - 4; // 0-based index into output arrays
        let w2 = *ws.add(i) * *ws.add(i);

        // Accumulate contributions from the 4 B-splines
        for b in 0..4 {
            let j = j_base + b; // 0-based column index
            if j >= nk {
                break;
            }
            let v = vnikx[b]; // vnikx(b+1, 1) = B-spline value
            *xwy.add(j) += w2 * *ys.add(i) * v;
            *hs0.add(j) += w2 * v * v;
            for bb in (b + 1)..4 {
                let jb = j_base + bb;
                if jb >= nk {
                    break;
                }
                let vb = vnikx[bb];
                let prod = w2 * v * vb;
                match bb - b {
                    1 => {
                        *hs1.add(j) += prod;
                    }
                    2 => {
                        *hs2.add(j) += prod;
                    }
                    3 => {
                        *hs3.add(j) += prod;
                    }
                    _ => {} // intentionally unhandled: derivative order not requested
                }
            }
        }
    }
}

/// sslvrg - solve the penalized least squares problem for the spline.
///
/// Solves [X'WX + lambda*SIGMA] coef = X'Wy  and computes
/// smoothed values, leverages, and cross-validation criterion.
///
/// Ported from R's Fortran sslvrg.f.
/// All arrays use 1-based Fortran indexing internally.
unsafe fn sslvrg(
    penalt: *mut f64,
    dofoff: *mut f64,
    xs: *mut f64,
    ys: *mut f64,
    ws: *mut f64,
    ssw: *mut f64,
    n: *mut c_int,
    knot: *mut f64,
    nk: *mut c_int,
    coef: *mut f64,
    sz: *mut f64,
    lev: *mut f64,
    crit: *mut f64,
    icrit: *mut c_int,
    lambda: *mut f64,
    xwy: *mut f64,
    hs0: *mut f64,
    hs1: *mut f64,
    hs2: *mut f64,
    hs3: *mut f64,
    sg0: *mut f64,
    sg1: *mut f64,
    sg2: *mut f64,
    sg3: *mut f64,
    abd: *mut f64,
    p1ip: *mut f64,
    p2ip: *mut f64,
    ld4: *mut c_int,
    ldnk: *mut c_int,
    info: *mut c_int,
) {
    let n = *n as usize;
    let nk = *nk as usize;
    let ld4 = *ld4 as usize;
    let ldnk = *ldnk as usize;
    let lenkno = nk + 4;
    let eps = 1e-11;
    let mut work = [0.0f64; 16];
    let mut vnikx = [0.0f64; 4]; // 4 x 1
    let mut ileft: c_int = 1;

    // Compute coefficients: coef = xwy, abd diagonal = hs + lambda*sg
    for i in 0..nk {
        *coef.add(i) = *xwy.add(i);
        // abd(4, j) = hs0(j) + lambda * sg0(j)  [1-based: row 4 = 0-based row 3]
        *abd.add(3 + i * ld4) = *hs0.add(i) + *lambda * *sg0.add(i);
    }
    for i in 0..(nk - 1) {
        // abd(3, j+1) = hs1(j) + lambda * sg1(j)  [0-based row 2]
        *abd.add(2 + (i + 1) * ld4) = *hs1.add(i) + *lambda * *sg1.add(i);
    }
    for i in 0..(nk - 2) {
        // abd(2, j+2) = hs2(j) + lambda * sg2(j)  [0-based row 1]
        *abd.add(1 + (i + 2) * ld4) = *hs2.add(i) + *lambda * *sg2.add(i);
    }
    for i in 0..(nk - 3) {
        // abd(1, j+3) = hs3(j) + lambda * sg3(j)  [0-based row 0]
        *abd.add(0 + (i + 3) * ld4) = *hs3.add(i) + *lambda * *sg3.add(i);
    }

    // Factorize banded matrix abd
    let mut info_val: c_int = 0;
    crate::appl::linpack_band::dpbfa(abd, ld4 as c_int, nk as c_int, 3, &mut info_val);
    if info_val != 0 {
        *info = info_val;
        return;
    }

    // Solve linear system
    crate::appl::linpack_band::dpbsl(abd, ld4 as c_int, nk as c_int, 3, coef);

    // Value of smooth at data points
    for i in 0..n {
        let xv = *xs.add(i);
        *sz.add(i) = super::bspline::bvalue(knot, coef, nk as c_int, 4, xv, 0);
    }

    // Compute criterion if requested
    if *icrit >= 1 {
        // Get leverages
        super::bspline::sinerp(abd, ld4 as c_int, nk as c_int, p1ip, p2ip, ldnk as c_int, 0);

        for i in 0..n {
            let mut xv = *xs.add(i);
            let mut mflag: c_int = 0;
            ileft = crate::appl::interv::findInterval(
                knot,
                (nk + 1) as c_int,
                xv,
                0,
                0,
                ileft,
                &mut mflag,
            );
            ileft += 1; // convert to 1-based

            if mflag == -1 {
                ileft = 4;
                xv = *knot.add(3) + eps;
            } else if mflag == 1 {
                ileft = nk as c_int;
                xv = *knot.add(nk) - eps;
            }

            let j = ileft as usize - 3; // 0-based

            super::bspline::bsplvd(
                knot,
                lenkno as c_int,
                4,
                xv,
                ileft,
                work.as_mut_ptr(),
                vnikx.as_mut_ptr(),
                1,
            );
            let b0 = vnikx[0];
            let b1 = vnikx[1];
            let b2 = vnikx[2];
            let b3 = vnikx[3];

            // p1ip uses column-major: p1ip(row-1, col-1) = p1ip[(row-1) + (col-1)*ld4]
            *lev.add(i) = (*p1ip.add(3 + j * ld4) * b0 * b0
                + 2.0 * *p1ip.add(2 + j * ld4) * b0 * b1
                + 2.0 * *p1ip.add(1 + j * ld4) * b0 * b2
                + 2.0 * *p1ip.add(0 + j * ld4) * b0 * b3
                + *p1ip.add(3 + (j + 1) * ld4) * b1 * b1
                + 2.0 * *p1ip.add(2 + (j + 1) * ld4) * b1 * b2
                + 2.0 * *p1ip.add(1 + (j + 1) * ld4) * b1 * b3
                + *p1ip.add(3 + (j + 2) * ld4) * b2 * b2
                + 2.0 * *p1ip.add(2 + (j + 2) * ld4) * b2 * b3
                + *p1ip.add(3 + (j + 3) * ld4) * b3 * b3)
                * *ws.add(i)
                * *ws.add(i);
        }

        // Evaluate criterion
        let mut df = 0.0;
        if *icrit == 1 {
            // Generalized CV
            let mut rss = *ssw;
            let mut sumw = 0.0;
            for i in 0..n {
                rss += ((*ys.add(i) - *sz.add(i)) * *ws.add(i)).powi(2);
                df += *lev.add(i);
                sumw += *ws.add(i) * *ws.add(i);
            }
            *crit = (rss / sumw) / ((1.0 - (*dofoff + *penalt * df) / sumw).powi(2));
        } else if *icrit == 2 {
            // Ordinary CV
            let mut c = 0.0;
            for i in 0..n {
                c += (((*ys.add(i) - *sz.add(i)) * *ws.add(i)) / (1.0 - *lev.add(i))).powi(2);
            }
            *crit = c / n as f64;
        } else {
            // df matching (icrit == 3) or df - dofoff (icrit == 4)
            for i in 0..n {
                df += *lev.add(i);
            }
            if *icrit == 3 {
                *crit = 3.0 + (*dofoff - df).powi(2);
            } else {
                *crit = df - *dofoff;
            }
        }
    }
}

// =========================================================================
// Main entry point
// =========================================================================

/// A Cubic B-spline Smoothing routine.
///
/// The algorithm minimises:
///
///   (1/n) * sum ws(i)^2 * (ys(i)-sz(i))^2 + lambda* int ( s"(x) )^2 dx
///
/// lambda is a function of the spar which is assumed to be between 0 and 1.
///
/// Port of `F77_SUB(sbart)` from R's `src/library/stats/src/sbart.c`.
///
/// # Safety
/// All pointer arguments must be valid and point to appropriately sized arrays.
pub unsafe fn sbart_(
    penalt: *mut f64,
    dofoff: *mut f64,
    xs: *mut f64,
    ys: *mut f64,
    ws: *mut f64,
    ssw: *mut f64,
    n: *mut c_int,
    knot: *mut f64,
    nk: *mut c_int,
    coef: *mut f64,
    sz: *mut f64,
    lev: *mut f64,
    crit: *mut f64,
    icrit: *mut c_int,
    spar: *mut f64,
    ispar: *mut c_int,
    iter: *mut c_int,
    lspar: *mut f64,
    uspar: *mut f64,
    tol: *mut f64,
    eps: *mut f64,
    Ratio: *mut f64,
    isetup: *mut c_int,
    xwy: *mut f64,
    hs0: *mut f64,
    hs1: *mut f64,
    hs2: *mut f64,
    hs3: *mut f64,
    sg0: *mut f64,
    sg1: *mut f64,
    sg2: *mut f64,
    sg3: *mut f64,
    abd: *mut f64,
    p1ip: *mut f64,
    p2ip: *mut f64,
    ld4: *mut c_int,
    ldnk: *mut c_int,
    ier: *mut c_int,
) {
    // Local variables
    let mut ratio: f64 = 1.0; // static in C; not needed in R
    let mut a: f64;
    let mut b: f64;
    let mut d: f64 = 0.0;
    let mut e: f64;
    let mut p: f64;
    let mut q: f64;
    let mut r: f64;
    let mut u: f64 = 0.0;
    let mut v: f64;
    let mut w: f64;
    let mut x: f64;
    let mut ax: f64;
    let mut fu: f64 = 0.0;
    let mut fv: f64;
    let mut fw: f64;
    let mut fx: f64;
    let mut bx: f64;
    let mut xm: f64;
    let mut tol1: f64;
    let mut tol2: f64;
    let mut i: c_int;
    let mut maxit: c_int;
    let mut Fparabol: bool = false;
    let tracing: bool;
    let mut spar_is_lambda: bool;

    // -----------------------------------------------------------------------
    // Trevor fixed this 4/19/88
    // Note: sbart, i.e. stxwx() and sslvrg() {mostly, not always!}, use
    // the square of the weights; the following rectifies that
    // -----------------------------------------------------------------------
    for i in 0..*n {
        if *ws.add(i as usize) > 0.0 {
            *ws.add(i as usize) = (*ws.add(i as usize)).sqrt();
        }
    }

    if *isetup < 0 {
        spar_is_lambda = true;
    } else if *isetup != 1 {
        // isetup == 0 or 2
        // SIGMA[i,j] := Int B''(i,t) B''(j,t) dt
        sgram(sg0, sg1, sg2, sg3, knot, nk);
        stxwx(xs, ys, ws, n, knot, nk, xwy, hs0, hs1, hs2, hs3);
        spar_is_lambda = *isetup == 2;
        if !spar_is_lambda {
            // Compute ratio := tr(X' W X) / tr(SIGMA)
            let mut t1: f64 = 0.0;
            let mut t2: f64 = 0.0;
            for i in 2..*nk - 3 {
                t1 += *hs0.add(i as usize);
                t2 += *sg0.add(i as usize);
            }
            ratio = t1 / t2;
        }
        *isetup = 1;
    } else {
        // isetup == 1 (already set up)
        spar_is_lambda = false;
    }

    // Compute estimate
    tracing = *ispar < 0;

    if *ispar == 1 {
        // Value of spar supplied
        SSPLINE_COMP(
            *spar,
            spar_is_lambda,
            ratio,
            penalt,
            dofoff,
            xs,
            ys,
            ws,
            ssw,
            n,
            knot,
            nk,
            coef,
            sz,
            lev,
            crit,
            icrit,
            lspar,
            xwy,
            hs0,
            hs1,
            hs2,
            hs3,
            sg0,
            sg1,
            sg2,
            sg3,
            abd,
            p1ip,
            p2ip,
            ld4,
            ldnk,
            ier,
        );
        *Ratio = ratio;
        return;
    }

    // ---- spar not supplied --> compute it using Brent's method ----
    ax = *lspar;
    bx = *uspar;

    // Use Forsythe, Malcom and Moler routine to MINIMIZE criterion.
    // Combination of golden section search and successive parabolic interpolation.
    // Based on Brent, "Algorithms for Minimization without Derivatives", 1973.

    // Initialization
    maxit = *iter;
    *iter = 0;
    a = ax;
    b = bx;
    v = a + c_Gold * (b - a);
    w = v;
    x = v;
    e = 0.0;
    SSPLINE_COMP(
        x,
        spar_is_lambda,
        ratio,
        penalt,
        dofoff,
        xs,
        ys,
        ws,
        ssw,
        n,
        knot,
        nk,
        coef,
        sz,
        lev,
        crit,
        icrit,
        lspar,
        xwy,
        hs0,
        hs1,
        hs2,
        hs3,
        sg0,
        sg1,
        sg2,
        sg3,
        abd,
        p1ip,
        p2ip,
        ld4,
        ldnk,
        ier,
    );
    fx = *crit;
    fv = fx;
    fw = fx;

    // Main loop (equivalent to C's while(*ier == 0) / L20:)
    while *ier == 0 {
        xm = (a + b) * 0.5;
        tol1 = *eps * x.abs() + *tol / 3.0;
        tol2 = tol1 * 2.0;
        *iter += 1;

        if tracing {
            if *iter == 1 {
                // Write header
                let crit_name = if *icrit == 1 {
                    "GCV"
                } else if *icrit == 2 {
                    "CV"
                } else if *icrit == 3 {
                    "(df0-df)^2"
                } else {
                    "?f?"
                };
                eprintln!(
                    "sbart (ratio = {:15.8e}) iterations; initial tol1 = {:12.6e} :\n\
                     {:>11} {:>14}  {:>9} {:>11}  Kind {:>11} {:>12}\n\
                     {}",
                    ratio,
                    tol1,
                    "spar",
                    crit_name,
                    "b - a",
                    "e",
                    "NEW lspar",
                    "crit",
                    " ---------------------------------------\
                      ----------------------------------------"
                );
            }
            eprintln!(
                "{:11.8} {:14.9} {:9.4e} {:11.5}",
                x,
                CRIT(fx, *icrit),
                b - a,
                e
            );
            Fparabol = false;
        }

        // Check the (somewhat peculiar) stopping criterion:
        // the RHS is negative as long as the interval [a,b] is not small
        if (x - xm).abs() <= tol2 - (b - a) * 0.5 || *iter > maxit {
            break; // goto L_End
        }

        // Is golden-section necessary?
        // This labeled block replaces the C goto pattern:
        //   - If golden section is needed, compute d, then break out to evaluate u
        //   - If parabolic fit is attempted but rejected, 'continue' retries golden section
        //   - If parabolic fit succeeds, compute d, then break out to evaluate u
        let golden_sect: bool = 'compute_d: {
            if e.abs() <= tol1 || fx >= BIG_f || fv >= BIG_f || fw >= BIG_f {
                // Golden section step
                if tracing {
                    eprint!(" GS{}", if Fparabol { "" } else { " --" });
                }
                if x >= xm {
                    e = a - x;
                } else {
                    e = b - x;
                }
                d = c_Gold * e;
                break 'compute_d true; // golden section used
            } else {
                // Try parabolic fit
                if tracing {
                    eprint!(" FP");
                    Fparabol = true;
                }

                r = (x - w) * (fx - fv);
                q = (x - v) * (fx - fw);
                p = (x - v) * q - (x - w) * r;
                q = (q - r) * 2.0;
                if q > 0.0 {
                    p = -p;
                }
                q = q.abs();
                r = e;
                e = d;

                // Is parabola acceptable? Otherwise do golden-section
                if p.abs() >= (0.5 * q * r).abs() || q == 0.0 {
                    // above line added by BDR; in FTN: COMMON above ensures q is NOT a register variable
                    if tracing {
                        eprint!(" GS{}", if Fparabol { "" } else { " --" });
                    }
                    if x >= xm {
                        e = a - x;
                    } else {
                        e = b - x;
                    }
                    d = c_Gold * e;
                    break 'compute_d true; // fell back to golden section
                }

                if p <= q * (a - x) || p >= q * (b - x) {
                    if tracing {
                        eprint!(" GS{}", if Fparabol { "" } else { " --" });
                    }
                    if x >= xm {
                        e = a - x;
                    } else {
                        e = b - x;
                    }
                    d = c_Gold * e;
                    break 'compute_d true; // fell back to golden section
                }

                // Parabolic interpolation step
                if tracing {
                    eprint!(" PI ");
                }
                d = p / q;
                if !d.is_finite() {
                    eprintln!(
                        " !FIN(d:=p/q): ier={}, (v,w, p,q)= {}, {}, {}, {}",
                        *ier, v, w, p, q
                    );
                }
                u = x + d;

                // f must not be evaluated too close to ax or bx
                if u - a < tol2 || b - u < tol2 {
                    d = (xm - x).copysign(tol1);
                }

                false // parabolic interpolation used
            }
        };

        // L50_label: compute u from d
        u = x + if d.abs() >= tol1 { d } else { d.copysign(tol1) };
        // tol1 check: f must not be evaluated too close to x

        SSPLINE_COMP(
            u,
            spar_is_lambda,
            ratio,
            penalt,
            dofoff,
            xs,
            ys,
            ws,
            ssw,
            n,
            knot,
            nk,
            coef,
            sz,
            lev,
            crit,
            icrit,
            lspar,
            xwy,
            hs0,
            hs1,
            hs2,
            hs3,
            sg0,
            sg1,
            sg2,
            sg3,
            abd,
            p1ip,
            p2ip,
            ld4,
            ldnk,
            ier,
        );
        fu = *crit;
        if tracing {
            eprintln!("{:11} {:12}", *lspar, CRIT(fu, *icrit));
        }
        if !fu.is_finite() {
            eprintln!("spar-finding: non-finite value {}; using BIG value", fu);
            fu = 2.0 * BIG_f;
        }

        // Update a, b, v, w, and x
        if fu <= fx {
            if u >= x {
                a = x;
            } else {
                b = x;
            }
            v = w;
            fv = fw;
            w = x;
            fw = fx;
            x = u;
            fx = fu;
        } else {
            if u < x {
                a = u;
            } else {
                b = u;
            }
            if fu <= fw || w == x {
                // L70:
                v = w;
                fv = fw;
                w = u;
                fw = fu;
            } else if fu <= fv || v == x || v == w {
                // L80:
                v = u;
                fv = fu;
            }
        }
    } // end main loop

    // L_End:
    if tracing {
        eprintln!("  >>> {:12} {:12}", *lspar, CRIT(fx, *icrit));
    }
    *Ratio = ratio;
    *spar = x;
    *crit = fx;
}
