#![allow(clippy::self_assignment, clippy::assigning_clones)]
#![allow(clippy::comparison_chain)]
#![allow(unused_variables, clippy::manual_memcpy, clippy::comparison_to_empty)]
#![allow(unused_assignments)]
use super::*;

pub(crate) fn chlhsn(nr: usize, n: usize, a: &mut [f64], epsm: f64, sx: &[f64], udiag: &mut [f64]) {
    // Scale hessian
    for j in 0..n {
        for i in j..n {
            a[i + j * nr] /= sx[i] * sx[j];
        }
    }

    let tol = sqrt(epsm);

    let mut diagmx = a[0];
    let mut diagmn = a[0];
    for i in 1..n {
        let tmp = a[i + i * nr];
        if diagmn > tmp {
            diagmn = tmp;
        }
        if diagmx < tmp {
            diagmx = tmp;
        }
    }
    let posmax = fmax2(diagmx, 0.0);

    if diagmn <= posmax * tol {
        let mut amu = tol * (posmax - diagmn) - diagmn;
        if amu == 0.0 {
            let mut offmax = 0.0;
            for i in 1..n {
                for j in 0..i {
                    let tmp = fabs(a[i + j * nr]);
                    if offmax < tmp {
                        offmax = tmp;
                    }
                }
            }
            if offmax == 0.0 {
                amu = 1.0;
            } else {
                amu = offmax * (tol + 1.0);
            }
        }
        for i in 0..n {
            a[i + i * nr] += amu;
        }
        diagmx += amu;
    }

    // Copy lower to upper, diagonal to udiag
    for i in 0..n {
        udiag[i] = a[i + i * nr];
        for j in 0..i {
            a[j + i * nr] = a[i + j * nr];
        }
    }
    let mut addmax = 0.0;
    choldc(nr, n, a, diagmx, tol, &mut addmax);

    if addmax > 0.0 {
        // Restore original a
        for i in 0..n {
            a[i + i * nr] = udiag[i];
            for j in 0..i {
                a[i + j * nr] = a[j + i * nr];
            }
        }

        let mut evmin = 0.0;
        let mut evmax = a[0];
        for i in 0..n {
            let mut offrow = 0.0;
            for j in 0..i {
                offrow += fabs(a[i + j * nr]);
            }
            for j in (i + 1)..n {
                offrow += fabs(a[j + i * nr]);
            }
            let tmp = a[i + i * nr] - offrow;
            if evmin > tmp {
                evmin = tmp;
            }
            let tmp = a[i + i * nr] + offrow;
            if evmax < tmp {
                evmax = tmp;
            }
        }
        let sdd = tol * (evmax - evmin) - evmin;
        let amu = fmin2(sdd, addmax);
        for i in 0..n {
            a[i + i * nr] += amu;
            udiag[i] = a[i + i * nr];
        }
        choldc(nr, n, a, 0.0, tol, &mut addmax);
    }

    // Unscale
    for j in 0..n {
        for i in j..n {
            a[i + j * nr] *= sx[i];
        }
        for i in 0..j {
            a[i + j * nr] *= sx[i] * sx[j];
        }
        udiag[j] *= sx[j] * sx[j];
    }
}

pub(crate) fn hsnint(nr: usize, n: usize, a: &mut [f64], sx: &[f64], method: i32) {
    for i in 0..n {
        a[i + i * nr] = if method == 3 { sx[i] * sx[i] } else { sx[i] };
        for j in 0..i {
            a[i + j * nr] = 0.0;
        }
    }
}

// =====================================================================
// Finite difference approximations (fstofd, fstocd, sndofd)
// =====================================================================

unsafe fn fstofd(
    nr: usize,
    m: usize,
    n: usize,
    xpls: &mut [f64],
    fcn: UncminFcn,
    state: *mut std::ffi::c_void,
    fpls: &[f64],
    a: &mut [f64],
    sx: &[f64],
    rnoise: f64,
    fhat: &mut [f64],
    icase: i32,
) {
    unsafe {
        for j in 0..n {
            let temp1 = fabs(xpls[j]);
            let temp2 = 1.0 / sx[j];
            let stepsz = sqrt(rnoise) * fmax2(temp1, temp2);
            let xtmpj = xpls[j];
            xpls[j] = xtmpj + stepsz;
            // C calls fcn (which is d1fcn cast when m == n) to fill fhat[0..m-1]
            fcn(
                n as std::os::raw::c_int,
                xpls.as_mut_ptr(),
                fhat.as_mut_ptr(),
                state,
            );
            xpls[j] = xtmpj;
            for i in 0..m {
                a[i + j * nr] = (fhat[i] - fpls[i]) / stepsz;
            }
        }
        if icase == 3 && n > 1 {
            for i in 1..m {
                for j in 0..i {
                    a[i + j * nr] = (a[i + j * nr] + a[j + i * nr]) / 2.0;
                }
            }
        }
    }
}

unsafe fn fstocd(
    n: usize,
    x: &mut [f64],
    fcn: UncminFcn,
    state: *mut std::ffi::c_void,
    sx: &[f64],
    rnoise: f64,
    g: &mut [f64],
) {
    unsafe {
        for i in 0..n {
            let xtempi = x[i];
            let temp1 = fabs(xtempi);
            let temp2 = 1.0 / sx[i];
            let stepi = pow(rnoise, 1.0 / 3.0) * fmax2(temp1, temp2);
            x[i] = xtempi + stepi;
            let mut fplus = 0.0;
            fcn(
                n as std::os::raw::c_int,
                x.as_mut_ptr(),
                &mut fplus as *mut f64,
                state,
            );
            x[i] = xtempi - stepi;
            let mut fminus = 0.0;
            fcn(
                n as std::os::raw::c_int,
                x.as_mut_ptr(),
                &mut fminus as *mut f64,
                state,
            );
            x[i] = xtempi;
            g[i] = (fplus - fminus) / (stepi * 2.0);
        }
    }
}

unsafe fn sndofd(
    nr: usize,
    n: usize,
    xpls: &mut [f64],
    fcn: UncminFcn,
    state: *mut std::ffi::c_void,
    fpls: f64,
    a: &mut [f64],
    sx: &[f64],
    rnoise: f64,
    stepsz: &mut [f64],
    anbr: &mut [f64],
) {
    unsafe {
        for i in 0..n {
            let xtmpi = xpls[i];
            stepsz[i] = pow(rnoise, 1.0 / 3.0) * fmax2(fabs(xtmpi), 1.0 / sx[i]);
            xpls[i] = xtmpi + stepsz[i];
            let mut fhat = 0.0;
            fcn(
                n as std::os::raw::c_int,
                xpls.as_mut_ptr(),
                &mut fhat as *mut f64,
                state,
            );
            anbr[i] = fhat;
            xpls[i] = xtmpi;
        }

        for i in 0..n {
            let xtmpi = xpls[i];
            xpls[i] = xtmpi + stepsz[i] * 2.0;
            let mut fhat = 0.0;
            fcn(
                n as std::os::raw::c_int,
                xpls.as_mut_ptr(),
                &mut fhat as *mut f64,
                state,
            );
            a[i + i * nr] = ((fpls - anbr[i]) + (fhat - anbr[i])) / (stepsz[i] * stepsz[i]);

            if i == 0 {
                xpls[i] = xtmpi;
                continue;
            }
            xpls[i] = xtmpi + stepsz[i];
            for j in 0..i {
                let xtmpj = xpls[j];
                xpls[j] = xtmpj + stepsz[j];
                let mut fhat = 0.0;
                fcn(
                    n as std::os::raw::c_int,
                    xpls.as_mut_ptr(),
                    &mut fhat as *mut f64,
                    state,
                );
                a[i + j * nr] = ((fpls - anbr[i]) + (fhat - anbr[j])) / (stepsz[i] * stepsz[j]);
                xpls[j] = xtmpj;
            }
            xpls[i] = xtmpi;
        }
    }
}

// =====================================================================
// Gradient/Hessian checking (grdchk, heschk)
// =====================================================================

unsafe fn grdchk(
    n: usize,
    x: &mut [f64],
    fcn: UncminFcn,
    state: *mut std::ffi::c_void,
    f: f64,
    g: &[f64],
    typsiz: &[f64],
    sx: &[f64],
    fscale: f64,
    rnf: f64,
    analtl: f64,
    wrk1: &mut [f64],
    msg: &mut i32,
) {
    unsafe {
        let mut wrk = 0.0;
        fstofd(
            1,
            1,
            n,
            x,
            fcn,
            state,
            std::slice::from_ref(&f),
            wrk1,
            sx,
            rnf,
            std::slice::from_mut(&mut wrk),
            1,
        );
        for i in 0..n {
            let gs = fmax2(fabs(f), fscale) / fmax2(fabs(x[i]), typsiz[i]);
            if fabs(g[i] - wrk1[i]) > fmax2(fabs(g[i]), gs) * analtl {
                *msg = -21;
                return;
            }
        }
    }
}

unsafe fn heschk(
    nr: usize,
    n: usize,
    x: &mut [f64],
    fcn: UncminFcn,
    d1fcn: UncminD1Fcn,
    d2fcn: UncminD2Fcn,
    state: *mut std::ffi::c_void,
    f: f64,
    g: &mut [f64],
    a: &mut [f64],
    typsiz: &[f64],
    sx: &[f64],
    rnf: f64,
    analtl: f64,
    iagflg: i32,
    udiag: &mut [f64],
    wrk1: &mut [f64],
    wrk2: &mut [f64],
    msg: &mut i32,
) {
    unsafe {
        if iagflg != 0 {
            fstofd(nr, n, n, x, d1fcn, state, g, a, sx, rnf, wrk1, 3);
        } else {
            sndofd(nr, n, x, fcn, state, f, a, sx, rnf, wrk1, wrk2);
        }

        for j in 0..n {
            udiag[j] = a[j + j * nr];
            for i in (j + 1)..n {
                a[j + i * nr] = a[i + j * nr];
            }
        }

        d2fcn(
            nr as std::os::raw::c_int,
            n as std::os::raw::c_int,
            x.as_mut_ptr(),
            a.as_mut_ptr(),
            state,
        );
        for j in 0..n {
            let hs = fmax2(fabs(g[j]), 1.0) / fmax2(fabs(x[j]), typsiz[j]);
            if fabs(a[j + j * nr] - udiag[j]) > fmax2(fabs(udiag[j]), hs) * analtl {
                *msg = -22;
                return;
            }
            for i in (j + 1)..n {
                let temp1 = a[i + j * nr];
                let temp2 = fabs(temp1 - a[j + i * nr]);
                if temp2 > fmax2(fabs(temp1), hs) * analtl {
                    *msg = -22;
                    return;
                }
            }
        }
    }
}

// =====================================================================
// Stopping criteria (opt_stop)
// =====================================================================

fn opt_stop(
    n: usize,
    xpls: &[f64],
    fpls: f64,
    gpls: &[f64],
    x: &[f64],
    itncnt: i32,
    icscmx: &mut i32,
    gradtl: f64,
    steptl: f64,
    sx: &[f64],
    fscale: f64,
    itnlim: i32,
    iretcd: i32,
    mxtake: bool,
) -> i32 {
    if iretcd == 1 {
        return 3;
    }

    let d = fmax2(fabs(fpls), fscale);
    let mut rgx = 0.0;
    for i in 0..n {
        let relgrd = fabs(gpls[i]) * fmax2(fabs(xpls[i]), 1.0 / sx[i]) / d;
        if rgx < relgrd {
            rgx = relgrd;
        }
    }

    if rgx <= gradtl {
        return 1;
    }
    if itncnt == 0 {
        return 0;
    }

    let mut rsx = 0.0;
    for i in 0..n {
        let relstp = fabs(xpls[i] - x[i]) / fmax2(fabs(xpls[i]), 1.0 / sx[i]);
        if rsx < relstp {
            rsx = relstp;
        }
    }
    if rsx <= steptl {
        return 2;
    }

    if itncnt >= itnlim {
        return 4;
    }

    if !mxtake {
        *icscmx = 0;
        return 0;
    } else {
        *icscmx += 1;
        if *icscmx < 5 {
            return 0;
        }
        return 5;
    }
}

// =====================================================================
// Input checking (optchk)
// =====================================================================

fn optchk(
    n: usize,
    x: &mut [f64],
    typsiz: &mut [f64],
    sx: &mut [f64],
    fscale: &mut f64,
    gradtl: f64,
    itnlim: &mut i32,
    ndigit: &mut i32,
    epsm: f64,
    dlt: &mut f64,
    method: &mut i32,
    iexp: &mut i32,
    iagflg: &mut i32,
    iahflg: &mut i32,
    stepmx: &mut f64,
    msg: &mut i32,
) {
    if *method < 1 || *method > 3 {
        *method = 1;
    }
    if *iagflg != 1 {
        *iagflg = 0;
    }
    if *iahflg != 1 {
        *iahflg = 0;
    }
    if *iexp != 0 {
        *iexp = 1;
    }
    if *msg / 2 % 2 == 1 && *iagflg == 0 {
        *msg = -6;
        return;
    }
    if *msg / 4 % 2 == 1 && *iahflg == 0 {
        *msg = -7;
        return;
    }

    if n == 0 {
        *msg = -1;
        return;
    }
    if n == 1 && *msg % 2 == 0 {
        *msg = -2;
        return;
    }

    for i in 0..n {
        if typsiz[i] == 0.0 {
            typsiz[i] = 1.0;
        } else if typsiz[i] < 0.0 {
            typsiz[i] = -typsiz[i];
        }
        sx[i] = 1.0 / typsiz[i];
    }

    if *stepmx <= 0.0 {
        let mut stpsiz = 0.0;
        for i in 0..n {
            stpsiz += x[i] * x[i] * sx[i] * sx[i];
        }
        *stepmx = 1000.0 * fmax2(sqrt(stpsiz), 1.0);
    }

    if *fscale == 0.0 {
        *fscale = 1.0;
    } else if *fscale < 0.0 {
        *fscale = -(*fscale);
    }

    if gradtl < 0.0 {
        *msg = -3;
        return;
    }
    if *itnlim <= 0 {
        *msg = -4;
        return;
    }

    if *ndigit == 0 {
        *msg = -5;
        return;
    } else if *ndigit < 0 {
        *ndigit = (-log10(epsm)) as i32;
    }

    if *dlt <= 0.0 {
        *dlt = -1.0;
    } else if *dlt > *stepmx {
        *dlt = *stepmx;
    }
}

// =====================================================================
// Print result
// =====================================================================

pub(crate) fn prt_result(
    _nr: usize,
    n: usize,
    x: &[f64],
    f: f64,
    g: &[f64],
    _a: &[f64],
    p: &[f64],
    itncnt: i32,
    iflg: i32,
) {
    crate::sexp::output::emit(&format!("iteration = {}\n", itncnt));
    if iflg != 0 {
        crate::sexp::output::emit("Step:\n");
        print_real_vector(p, n);
    }
    crate::sexp::output::emit("Parameter:\n");
    print_real_vector(x, n);
    crate::sexp::output::emit("Function Value\n");
    print_real_vector(std::slice::from_ref(&f), 1);
    crate::sexp::output::emit("Gradient:\n");
    print_real_vector(g, n);
    crate::sexp::output::emit("\n");
}

// =====================================================================
// End-of-optimization handler
// =====================================================================

fn optdrv_end(
    nr: usize,
    n: usize,
    xpls: &mut [f64],
    x: &[f64],
    gpls: &mut [f64],
    g: &[f64],
    fpls: &mut f64,
    f: f64,
    a: &[f64],
    p: &[f64],
    itncnt: i32,
    itrmcd: i32,
    msg: &mut i32,
) {
    if itrmcd == 3 {
        *fpls = f;
        for i in 0..n {
            xpls[i] = x[i];
            gpls[i] = g[i];
        }
    }
    if *msg / 8 % 2 == 0 {
        prt_result(nr, n, xpls, *fpls, gpls, a, p, itncnt, 0);
    }
    *msg = 0;
}

// =====================================================================
// Main optimizer driver (optdrv)
// =====================================================================

unsafe fn optdrv(
    nr: usize,
    n: usize,
    x: &mut [f64],
    fcn: UncminFcn,
    d1fcn: Option<UncminD1Fcn>,
    d2fcn: Option<UncminD2Fcn>,
    state: *mut std::ffi::c_void,
    typsiz: &mut [f64],
    fscale: f64,
    method: i32,
    iexp: i32,
    msg: &mut i32,
    ndigit: i32,
    itnlim: i32,
    iagflg: i32,
    iahflg: i32,
    dlt: f64,
    gradtl: f64,
    stepmx: f64,
    steptl: f64,
    xpls: &mut [f64],
    fpls: &mut f64,
    gpls: &mut [f64],
    itrmcd: &mut i32,
    a: &mut [f64],
    udiag: &mut [f64],
    g: &mut [f64],
    p: &mut [f64],
    sx: &mut [f64],
    wrk0: &mut [f64],
    wrk1: &mut [f64],
    wrk2: &mut [f64],
    wrk3: &mut [f64],
    itncnt: &mut i32,
) {
    unsafe {
        let epsm = f64::EPSILON;
        let mut method = method;
        let mut iagflg = iagflg;
        let mut iahflg = iahflg;
        let mut iexp = iexp;
        let mut dlt = dlt;
        let mut stepmx = stepmx;
        let mut fscale = fscale;
        let gradtl = gradtl;
        let mut ndigit = ndigit;
        let mut itnlim = itnlim;
        let mut msg_val = *msg;
        let mut mxtake = false;
        let mut noupdt = false;
        let mut icscmx = 0;
        let mut dltp = 0.0_f64;
        let mut phip0 = 0.0_f64;
        let mut phi = 0.0_f64;
        let mut amu = 0.0_f64;
        let mut dltsav = 0.0_f64;
        let mut amusav = 0.0_f64;
        let mut dlpsav = 0.0_f64;
        let mut phisav = 0.0_f64;
        let mut phpsav = 0.0_f64;

        *itncnt = 0;
        optchk(
            n,
            x,
            typsiz,
            sx,
            &mut fscale,
            gradtl,
            &mut itnlim,
            &mut ndigit,
            epsm,
            &mut dlt,
            &mut method,
            &mut iexp,
            &mut iagflg,
            &mut iahflg,
            &mut stepmx,
            &mut msg_val,
        );
        *msg = msg_val;
        if *msg < 0 {
            return;
        }

        for i in 0..n {
            p[i] = 0.0;
        }

        let mut rnf = pow(10.0, -(ndigit as f64));
        rnf = fmax2(rnf, epsm);
        let mut analtl = sqrt(rnf);
        analtl = fmax2(0.1, analtl);

        // Evaluate fcn(x)
        let mut f = 0.0_f64;
        fcn(
            n as std::os::raw::c_int,
            x.as_mut_ptr(),
            &mut f as *mut f64,
            state,
        );

        // Evaluate gradient
        if iagflg == 0 {
            let mut wrk = 0.0_f64;
            fstofd(
                1,
                1,
                n,
                x,
                fcn,
                state,
                std::slice::from_ref(&f),
                g,
                sx,
                rnf,
                std::slice::from_mut(&mut wrk),
                1,
            );
        } else {
            if let Some(d1) = d1fcn {
                d1(
                    n as std::os::raw::c_int,
                    x.as_mut_ptr(),
                    g.as_mut_ptr(),
                    state,
                );
            }
            if *msg / 2 % 2 == 0 {
                grdchk(
                    n, x, fcn, state, f, g, typsiz, sx, fscale, rnf, analtl, wrk1, msg,
                );
                if *msg < 0 {
                    return;
                }
            }
        }

        let iretcd = -1;
        *itrmcd = opt_stop(
            n,
            x,
            f,
            g,
            wrk1,
            *itncnt,
            &mut icscmx,
            gradtl,
            steptl,
            sx,
            fscale,
            itnlim,
            iretcd,
            false,
        );
        if *itrmcd != 0 {
            optdrv_end(nr, n, xpls, x, gpls, g, fpls, f, a, p, *itncnt, 3, msg);
            return;
        }

        if iexp != 0 {
            hsnint(nr, n, a, sx, method);
        } else {
            if iahflg == 0 {
                if iagflg != 0 {
                    if let Some(d1) = d1fcn {
                        fstofd(nr, n, n, x, d1, state, g, a, sx, rnf, wrk1, 3);
                    }
                } else {
                    sndofd(nr, n, x, fcn, state, f, a, sx, rnf, wrk1, wrk2);
                }
            } else {
                if *msg / 4 % 2 == 1 {
                    if let Some(d2) = d2fcn {
                        d2(
                            nr as std::os::raw::c_int,
                            n as std::os::raw::c_int,
                            x.as_mut_ptr(),
                            a.as_mut_ptr(),
                            state,
                        );
                    }
                } else {
                    if let (Some(d1), Some(d2)) = (d1fcn, d2fcn) {
                        heschk(
                            nr, n, x, fcn, d1, d2, state, f, g, a, typsiz, sx, rnf, analtl, iagflg,
                            udiag, wrk1, wrk2, msg,
                        );
                    }
                    if *msg < 0 {
                        return;
                    }
                }
            }
        }

        if *msg / 8 % 2 == 0 {
            prt_result(nr, n, x, f, g, a, p, *itncnt, 1);
        }

        // THE Iterations :
        'iters: loop {
            *itncnt += 1;

            // C control flow: `if (iexp && method != 3) goto L105;`
            // L103: chlhsn; L105: newton solve + global step. The central-
            // difference retry jumps back to L105 (method 1/2: no chlhsn,
            // keep the secant cholesky factor) or to L103 (method 3).
            let mut do_chlhsn = !(iexp != 0 && method != 3);
            'step: loop {
                if do_chlhsn {
                    chlhsn(nr, n, a, epsm, sx, udiag);
                }

                // L105: solve for newton step: ap = -g
                for i in 0..n {
                    wrk1[i] = -g[i];
                }
                lltslv(nr, n, a, p, wrk1);

                // save state for possible retry (methods 2/3 only)
                if iagflg == 0 && method != 1 {
                    dltsav = dlt;
                    if method != 2 {
                        /* i.e. method = 3 */
                        amusav = amu;
                        dlpsav = dltp;
                        phisav = phi;
                        phpsav = phip0;
                    }
                }

                let mut iretcd = 0;
                match method {
                    1 => {
                        lnsrch(
                            n,
                            x,
                            f,
                            g,
                            p,
                            xpls,
                            fpls,
                            fcn,
                            state,
                            &mut mxtake,
                            &mut iretcd,
                            stepmx,
                            steptl,
                            sx,
                        );
                    }
                    2 => {
                        dogdrv(
                            nr,
                            n,
                            x,
                            f,
                            g,
                            a,
                            p,
                            xpls,
                            fpls,
                            fcn,
                            state,
                            sx,
                            stepmx,
                            steptl,
                            &mut dlt,
                            &mut iretcd,
                            &mut mxtake,
                            wrk0,
                            wrk1,
                            wrk2,
                            wrk3,
                            *itncnt,
                        );
                    }
                    3 => {
                        hookdrv(
                            nr,
                            n,
                            x,
                            f,
                            g,
                            a,
                            udiag,
                            p,
                            xpls,
                            fpls,
                            fcn,
                            state,
                            sx,
                            stepmx,
                            steptl,
                            &mut dlt,
                            &mut iretcd,
                            &mut mxtake,
                            &mut amu,
                            &mut dltp,
                            &mut phi,
                            &mut phip0,
                            wrk0,
                            wrk1,
                            wrk2,
                            epsm,
                            *itncnt,
                        );
                    }
                    _ => {}
                }

                // if could not find satisfactory step and forward difference
                // gradient was used, retry using central difference gradient.
                if iretcd == 1 && iagflg == 0 {
                    iagflg = -1; // set iagflg for central differences
                    fstocd(n, x, fcn, state, sx, rnf, g);
                    if method == 1 {
                        do_chlhsn = false; /* goto L105 */
                        continue 'step;
                    }
                    dlt = dltsav;
                    if method == 2 {
                        do_chlhsn = false; /* goto L105 */
                        continue 'step;
                    }
                    /* else : method == 3 */
                    amu = amusav;
                    dltp = dlpsav;
                    phi = phisav;
                    phip0 = phpsav;
                    do_chlhsn = true; /* goto L103 */
                    continue 'step;
                }
                break 'step;
            }

            // Calculate step for output
            for i in 0..n {
                p[i] = xpls[i] - x[i];
            }

            // Calculate gradient at xpls
            match iagflg {
                -1 => {
                    fstocd(n, xpls, fcn, state, sx, rnf, gpls);
                }
                0 => {
                    let mut wrk = 0.0_f64;
                    fstofd(
                        1,
                        1,
                        n,
                        xpls,
                        fcn,
                        state,
                        std::slice::from_ref(fpls),
                        gpls,
                        sx,
                        rnf,
                        std::slice::from_mut(&mut wrk),
                        1,
                    );
                }
                _ => {
                    if let Some(d1) = d1fcn {
                        d1(
                            n as std::os::raw::c_int,
                            xpls.as_mut_ptr(),
                            gpls.as_mut_ptr(),
                            state,
                        );
                    }
                }
            }

            // Check stopping criteria
            *itrmcd = opt_stop(
                n,
                xpls,
                *fpls,
                gpls,
                x,
                *itncnt,
                &mut icscmx,
                gradtl,
                steptl,
                sx,
                fscale,
                itnlim,
                iretcd,
                mxtake,
            );
            if *itrmcd != 0 {
                break 'iters;
            }

            // Evaluate hessian at xpls
            if iexp != 0 {
                if method == 3 {
                    secunf(
                        nr,
                        n,
                        x,
                        g,
                        a,
                        udiag,
                        xpls,
                        gpls,
                        epsm,
                        *itncnt,
                        rnf,
                        iagflg,
                        &mut noupdt,
                        wrk1,
                        wrk2,
                        wrk3,
                    );
                } else {
                    secfac(
                        nr,
                        n,
                        x,
                        g,
                        a,
                        xpls,
                        gpls,
                        epsm,
                        *itncnt,
                        rnf,
                        iagflg,
                        &mut noupdt,
                        wrk0,
                        wrk1,
                        wrk2,
                        wrk3,
                    );
                }
            } else {
                if iahflg == 0 {
                    if iagflg != 0 {
                        if let Some(d1) = d1fcn {
                            fstofd(nr, n, n, xpls, d1, state, gpls, a, sx, rnf, wrk1, 3);
                        }
                    } else {
                        sndofd(nr, n, xpls, fcn, state, *fpls, a, sx, rnf, wrk1, wrk2);
                    }
                } else {
                    if let Some(d2) = d2fcn {
                        d2(
                            nr as std::os::raw::c_int,
                            n as std::os::raw::c_int,
                            xpls.as_mut_ptr(),
                            a.as_mut_ptr(),
                            state,
                        );
                    }
                }
            }

            if *msg / 16 % 2 == 1 {
                prt_result(nr, n, xpls, *fpls, gpls, a, p, *itncnt, 1);
            }

            // Update x, g, f
            f = *fpls;
            for i in 0..n {
                x[i] = xpls[i];
                g[i] = gpls[i];
            }
        }

        optdrv_end(
            nr, n, xpls, x, gpls, g, fpls, f, a, p, *itncnt, *itrmcd, msg,
        );
    }
}

// =====================================================================
// Public API: optif9
// =====================================================================

/// Complete interface to Dennis-Schnabel minimization package.
/// Called by R's nlm().
pub unsafe fn optif9(
    nr: std::os::raw::c_int,
    n: std::os::raw::c_int,
    x: *mut f64,
    fcn: UncminFcn,
    d1fcn: UncminD1Fcn,
    d2fcn: UncminD2Fcn,
    state: *mut std::ffi::c_void,
    typsiz: *mut f64,
    fscale: f64,
    method: std::os::raw::c_int,
    iexp: std::os::raw::c_int,
    msg: *mut std::os::raw::c_int,
    ndigit: std::os::raw::c_int,
    itnlim: std::os::raw::c_int,
    iagflg: std::os::raw::c_int,
    iahflg: std::os::raw::c_int,
    dlt: f64,
    gradtl: f64,
    stepmx: f64,
    steptl: f64,
    xpls: *mut f64,
    _fpls: *mut f64,
    _gpls: *mut f64,
    itrmcd: *mut std::os::raw::c_int,
    a: *mut f64,
    wrk: *mut f64,
    itncnt: *mut std::os::raw::c_int,
) {
    unsafe {
        let nr = nr as usize;
        let n = n as usize;
        let method = method as i32;
        let iexp = iexp as i32;
        let ndigit = ndigit as i32;
        let itnlim = itnlim as i32;
        let iagflg = iagflg as i32;
        let iahflg = iahflg as i32;

        // Create slices from raw pointers
        // wrk layout: wrk[0..nr] = udiag, wrk[nr..2*nr] = g, wrk[2*nr..3*nr] = p,
        //              wrk[3*nr..4*nr] = sx, wrk[4*nr..5*nr] = wrk0,
        //              wrk[5*nr..6*nr] = wrk1, wrk[6*nr..7*nr] = wrk2,
        //              wrk[7*nr..8*nr] = wrk3
        let wrk_slice = std::slice::from_raw_parts_mut(wrk, (nr * 8) as usize);

        let mut x = std::slice::from_raw_parts_mut(x, n);
        let mut xpls = std::slice::from_raw_parts_mut(xpls, n);
        let mut typsiz = std::slice::from_raw_parts_mut(typsiz, n);
        let mut a = std::slice::from_raw_parts_mut(a, nr * n);
        let mut fpls = 0.0_f64;
        let mut gpls = vec![0.0f64; n];
        let mut itrmcd_val = 0_i32;
        let mut itncnt_val = 0_i32;
        let mut msg_val = *msg;

        let (udiag, rest) = wrk_slice.split_at_mut(nr);
        let (g, rest) = rest.split_at_mut(nr);
        let (p, rest) = rest.split_at_mut(nr);
        let (sx, rest) = rest.split_at_mut(nr);
        let (wrk0, rest) = rest.split_at_mut(nr);
        let (wrk1, rest) = rest.split_at_mut(nr);
        let (wrk2, wrk3) = rest.split_at_mut(nr);

        optdrv(
            nr,
            n,
            &mut x,
            fcn,
            Some(d1fcn),
            Some(d2fcn),
            state,
            &mut typsiz,
            fscale,
            method,
            iexp,
            &mut msg_val,
            ndigit,
            itnlim,
            iagflg,
            iahflg,
            dlt,
            gradtl,
            stepmx,
            steptl,
            &mut xpls,
            &mut fpls,
            &mut gpls,
            &mut itrmcd_val,
            &mut a,
            udiag,
            g,
            p,
            sx,
            wrk0,
            wrk1,
            wrk2,
            wrk3,
            &mut itncnt_val,
        );

        *msg = msg_val;
        *itrmcd = itrmcd_val;
        *itncnt = itncnt_val;
        if !_fpls.is_null() {
            *_fpls = fpls;
        }
        if !_gpls.is_null() {
            for i in 0..n {
                *_gpls.add(i) = gpls[i];
            }
        }
    }
}
