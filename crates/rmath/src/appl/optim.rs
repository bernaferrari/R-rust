#![allow(unused_variables)]
#![allow(unused_assignments)]
// Ported from R's appl/optim.c
//
// BFGS variable-metric method (vmmin), Nelder-Mead (nmmin),
// and Conjugate Gradients (cgmin) function minimizers.
//
// Based on Pascal code in J.C. Nash, 'Compact Numerical Methods for
// Computers', 2nd edition, converted by p2c then re-crafted by B.D. Ripley.

use libm::*;

// =====================================================================
// Callback types
// =====================================================================

/// Objective function type: fn(n, x, ex) -> f64
pub type OptimFn =
    unsafe extern "C" fn(n: std::os::raw::c_int, x: *mut f64, ex: *mut std::ffi::c_void) -> f64;

/// Gradient function type: fn(n, x, g, ex)
pub type OptimGr = unsafe extern "C" fn(
    n: std::os::raw::c_int,
    x: *mut f64,
    g: *mut f64,
    ex: *mut std::ffi::c_void,
);

// =====================================================================
// Constants
// =====================================================================

const STEPREDN: f64 = 0.2;
const ACCTOL: f64 = 0.0001;
const RELTEST: f64 = 10.0;
const BIG: f64 = 1.0e+35;

// =====================================================================
// BFGS variable-metric method (vmmin)
// =====================================================================

/// BFGS variable-metric minimization.
///
/// Based on Pascal code in J.C. Nash, 'Compact Numerical Methods for
/// Computers', 2nd edition.
pub unsafe fn vmmin(
    n0: std::os::raw::c_int,
    b: *mut f64,
    fmin: *mut f64,
    fminfn: OptimFn,
    fmingr: OptimGr,
    maxit: std::os::raw::c_int,
    trace: std::os::raw::c_int,
    mask: *const std::os::raw::c_int,
    abstol: f64,
    reltol: f64,
    nreport: std::os::raw::c_int,
    ex: *mut std::ffi::c_void,
    fncount: *mut std::os::raw::c_int,
    grcount: *mut std::os::raw::c_int,
    fail: *mut std::os::raw::c_int,
) {
    unsafe {
        let n0 = n0 as i32;
        let maxit = maxit as i32;
        let trace = trace != 0;
        let nreport = nreport as i32;

        if maxit <= 0 {
            *fail = 0;
            *fmin = fminfn(n0 as std::os::raw::c_int, b, ex);
            *fncount = 0;
            *grcount = 0;
            return;
        }

        // Count active parameters (where mask[i] != 0)
        let mut l: Vec<usize> = Vec::with_capacity(n0 as usize);
        for i in 0..n0 as usize {
            if *mask.add(i) != 0 {
                l.push(i);
            }
        }
        let n = l.len() as i32;
        let n_usize = n as usize;

        let mut g = vec![0.0f64; n0 as usize];
        let mut t = vec![0.0f64; n_usize];
        let mut x = vec![0.0f64; n_usize];
        let mut c = vec![0.0f64; n_usize];

        // Lower triangular matrix B stored as Vec of rows
        let mut bmat: Vec<Vec<f64>> = Vec::with_capacity(n_usize);
        for i in 0..n_usize {
            bmat.push(vec![0.0f64; i + 1]);
        }

        let mut f = fminfn(n0 as std::os::raw::c_int, b, ex);
        if !f.is_finite() {
            eprintln!("initial value in 'vmmin' is not finite");
            *fmin = f;
            return;
        }
        if trace {
            eprintln!("initial  value {} ", f);
        }
        *fmin = f;
        let mut funcount: i32 = 1;
        let mut gradcount: i32 = 1;
        fmingr(n0 as std::os::raw::c_int, b, g.as_mut_ptr(), ex);
        let mut iter: i32 = 1;
        let mut ilast = gradcount;

        loop {
            if ilast == gradcount {
                for i in 0..n_usize {
                    for j in 0..i {
                        bmat[i][j] = 0.0;
                    }
                    bmat[i][i] = 1.0;
                }
            }
            for i in 0..n_usize {
                x[i] = *b.add(l[i]);
                c[i] = g[l[i]];
            }
            let mut gradproj = 0.0_f64;
            for i in 0..n_usize {
                let mut s = 0.0_f64;
                for j in 0..=i {
                    s -= bmat[i][j] * g[l[j]];
                }
                for j in (i + 1)..n_usize {
                    s -= bmat[j][i] * g[l[j]];
                }
                t[i] = s;
                gradproj += s * g[l[i]];
            }

            let mut count: i32 = 0;
            if gradproj < 0.0 {
                // search direction is downhill
                let mut steplength = 1.0_f64;
                let mut accpoint = false;
                loop {
                    count = 0;
                    for i in 0..n_usize {
                        *b.add(l[i]) = x[i] + steplength * t[i];
                        if RELTEST + x[i] == RELTEST + *b.add(l[i]) {
                            count += 1;
                        }
                    }
                    if count < n {
                        f = fminfn(n0 as std::os::raw::c_int, b, ex);
                        funcount += 1;
                        accpoint = f.is_finite() && f <= *fmin + gradproj * steplength * ACCTOL;
                        if !accpoint {
                            steplength *= STEPREDN;
                        }
                    }
                    if count == n || accpoint {
                        break;
                    }
                }

                let enough = f > abstol && fabs(f - *fmin) > reltol * (fabs(*fmin) + reltol);
                if !enough {
                    count = n;
                    *fmin = f;
                }
                if count < n {
                    // making progress
                    *fmin = f;
                    fmingr(n0 as std::os::raw::c_int, b, g.as_mut_ptr(), ex);
                    gradcount += 1;
                    iter += 1;
                    let mut d1 = 0.0_f64;
                    for i in 0..n_usize {
                        t[i] *= steplength;
                        c[i] = g[l[i]] - c[i];
                        d1 += t[i] * c[i];
                    }
                    if d1 > 0.0 {
                        let mut d2 = 0.0_f64;
                        for i in 0..n_usize {
                            let mut s = 0.0_f64;
                            for j in 0..=i {
                                s += bmat[i][j] * c[j];
                            }
                            for j in (i + 1)..n_usize {
                                s += bmat[j][i] * c[j];
                            }
                            x[i] = s;
                            d2 += s * c[i];
                        }
                        let d2 = 1.0 + d2 / d1;
                        for i in 0..n_usize {
                            for j in 0..=i {
                                bmat[i][j] += (d2 * t[i] * t[j] - x[i] * t[j] - t[i] * x[j]) / d1;
                            }
                        }
                    } else {
                        ilast = gradcount;
                    }
                } else {
                    // no progress
                    if ilast < gradcount {
                        count = 0;
                        ilast = gradcount;
                    }
                }
            } else {
                // uphill search
                count = 0;
                if ilast == gradcount {
                    count = n;
                } else {
                    ilast = gradcount;
                }
            }

            if trace && iter % nreport == 0 {
                eprintln!("iter{:4} value {}", iter, f);
            }
            if iter >= maxit {
                break;
            }
            if gradcount - ilast > 2 * n {
                ilast = gradcount;
            }

            // Check termination: count == n && ilast == gradcount
            let done = {
                let mut cc = 0;
                for i in 0..n_usize {
                    if RELTEST + *b.add(l[i]) == RELTEST + *b.add(l[i]) {
                        cc += 1;
                    }
                }
                cc == n && ilast == gradcount
            };
            if done {
                break;
            }
        }

        if trace {
            eprintln!("final  value {} ", *fmin);
            if iter < maxit {
                eprintln!("converged");
            } else {
                eprintln!("stopped after {} iterations", iter);
            }
        }
        *fail = if iter < maxit { 0 } else { 1 };
        *fncount = funcount;
        *grcount = gradcount;
    }
}

// =====================================================================
// Nelder-Mead (nmmin)
// =====================================================================

/// Nelder-Mead direct search function minimizer.
///
/// Based on Pascal code in J.C. Nash, 'Compact Numerical Methods for
/// Computers', 2nd edition.
pub unsafe fn nmmin(
    n: std::os::raw::c_int,
    bvec: *mut f64,
    x: *mut f64,
    fmin: *mut f64,
    fminfn: OptimFn,
    fail: *mut std::os::raw::c_int,
    abstol: f64,
    intol: f64,
    ex: *mut std::ffi::c_void,
    alpha: f64,
    bet: f64,
    gamm: f64,
    trace: std::os::raw::c_int,
    fncount: *mut std::os::raw::c_int,
    maxit: std::os::raw::c_int,
) {
    unsafe {
        let n = n as i32;
        let maxit = maxit as i32;
        let trace = trace != 0;
        let n_usize = n as usize;

        if maxit <= 0 {
            *fmin = fminfn(n as std::os::raw::c_int, bvec, ex);
            *fncount = 0;
            *fail = 0;
            return;
        }

        if trace {
            eprintln!("  Nelder-Mead direct search function minimizer");
        }

        let n1 = n + 1;
        let n1_usize = n1 as usize;
        let c = n + 2; // index for centroid (1-based)

        // P[n+1][n+1] matrix: first n rows are parameters, last row is function values
        let mut p: Vec<Vec<f64>> = vec![vec![0.0f64; n1_usize]; n1_usize];

        *fail = 0;
        let mut f = fminfn(n as std::os::raw::c_int, bvec, ex);
        if !f.is_finite() {
            eprintln!("function cannot be evaluated at initial parameters");
            *fail = 1;
            return;
        }

        if trace {
            eprintln!("function value for initial parameters = {}", f);
        }
        let mut funcount: i32 = 1;
        let convtol = intol * (fabs(f) + intol);
        if trace {
            eprintln!("  Scaled convergence tolerance is {}", convtol);
        }

        p[n1_usize - 1][0] = f;
        for i in 0..n_usize {
            p[i][0] = *bvec.add(i);
        }

        let mut l: i32 = 1; // index of best vertex (1-based)
        let mut size = 0.0_f64;

        // Determine step size
        let mut step = 0.0_f64;
        for i in 0..n_usize {
            let s = 0.1 * fabs(*bvec.add(i));
            if s > step {
                step = s;
            }
        }
        if step == 0.0 {
            step = 0.1;
        }
        if trace {
            eprintln!("Stepsize computed as {}", step);
        }

        for j in 2..=n1 {
            let j_usize = j as usize;
            for i in 0..n_usize {
                p[i][j_usize - 1] = *bvec.add(i);
            }
            let mut trystep = step;
            while p[j_usize - 2][j_usize - 1] == *bvec.add(j_usize - 2) {
                p[j_usize - 2][j_usize - 1] = *bvec.add(j_usize - 2) + trystep;
                trystep *= 10.0;
            }
            size += trystep;
        }

        let mut oldsize = size;
        let mut calcvert = true;

        loop {
            if calcvert {
                for j in 0..n1_usize {
                    if (j + 1) as i32 != l {
                        for i in 0..n_usize {
                            *bvec.add(i) = p[i][j];
                        }
                        f = fminfn(n as std::os::raw::c_int, bvec, ex);
                        if !f.is_finite() {
                            f = BIG;
                        }
                        funcount += 1;
                        p[n1_usize - 1][j] = f;
                    }
                }
                calcvert = false;
            }

            let mut vl = p[n1_usize - 1][(l - 1) as usize];
            let mut vh = vl;
            let mut h = l;

            for j in 1..=n1 {
                if j != l {
                    f = p[n1_usize - 1][(j - 1) as usize];
                    if f < vl {
                        l = j;
                        vl = f;
                    }
                    if f > vh {
                        h = j;
                        vh = f;
                    }
                }
            }

            if vh <= vl + convtol || vl <= abstol {
                break;
            }
            if funcount > maxit {
                break;
            }

            // Compute centroid
            for i in 0..n_usize {
                let mut temp = -p[i][(h - 1) as usize];
                for j in 0..n1_usize {
                    temp += p[i][j];
                }
                p[i][(c - 1) as usize] = temp / (n as f64);
            }

            // Reflection
            for i in 0..n_usize {
                *bvec.add(i) =
                    (1.0 + alpha) * p[i][(c - 1) as usize] - alpha * p[i][(h - 1) as usize];
            }
            f = fminfn(n as std::os::raw::c_int, bvec, ex);
            if !f.is_finite() {
                f = BIG;
            }
            funcount += 1;
            let vr = f;

            if vr < vl {
                // Extension
                p[n1_usize - 1][(c - 1) as usize] = f;
                for i in 0..n_usize {
                    let fval = gamm * *bvec.add(i) + (1.0 - gamm) * p[i][(c - 1) as usize];
                    p[i][(c - 1) as usize] = *bvec.add(i);
                    *bvec.add(i) = fval;
                }
                f = fminfn(n as std::os::raw::c_int, bvec, ex);
                if !f.is_finite() {
                    f = BIG;
                }
                funcount += 1;
                if f < vr {
                    for i in 0..n_usize {
                        p[i][(h - 1) as usize] = *bvec.add(i);
                    }
                    p[n1_usize - 1][(h - 1) as usize] = f;
                } else {
                    for i in 0..n_usize {
                        p[i][(h - 1) as usize] = p[i][(c - 1) as usize];
                    }
                    p[n1_usize - 1][(h - 1) as usize] = vr;
                }
            } else {
                // High reduction / low reduction
                if vr < vh {
                    for i in 0..n_usize {
                        p[i][(h - 1) as usize] = *bvec.add(i);
                    }
                    p[n1_usize - 1][(h - 1) as usize] = vr;
                }

                // Contraction
                for i in 0..n_usize {
                    *bvec.add(i) =
                        (1.0 - bet) * p[i][(h - 1) as usize] + bet * p[i][(c - 1) as usize];
                }
                f = fminfn(n as std::os::raw::c_int, bvec, ex);
                if !f.is_finite() {
                    f = BIG;
                }
                funcount += 1;

                if f < p[n1_usize - 1][(h - 1) as usize] {
                    for i in 0..n_usize {
                        p[i][(h - 1) as usize] = *bvec.add(i);
                    }
                    p[n1_usize - 1][(h - 1) as usize] = f;
                } else {
                    if vr >= vh {
                        // Shrink
                        calcvert = true;
                        size = 0.0;
                        for j in 0..n1_usize {
                            if (j + 1) as i32 != l {
                                for i in 0..n_usize {
                                    p[i][j] = bet * (p[i][j] - p[i][(l - 1) as usize])
                                        + p[i][(l - 1) as usize];
                                    size += fabs(p[i][j] - p[i][(l - 1) as usize]);
                                }
                            }
                        }
                        if size < oldsize {
                            oldsize = size;
                        } else {
                            if trace {
                                eprintln!("Polytope size measure not decreased in shrink");
                            }
                            *fail = 10;
                            break;
                        }
                    }
                }
            }
        }

        if trace {
            eprintln!("Exiting from Nelder Mead minimizer");
            eprintln!("    {} function evaluations used", funcount);
        }

        *fmin = p[n1_usize - 1][(l - 1) as usize];
        for i in 0..n_usize {
            *x.add(i) = p[i][(l - 1) as usize];
        }
        if funcount > maxit {
            *fail = 1;
        }
        *fncount = funcount;
    }
}

// =====================================================================
// Conjugate Gradients (cgmin)
// =====================================================================

/// Conjugate gradients function minimizer.
///
/// Supports three methods: Fletcher-Reeves (type=1), Polak-Ribiere (type=2),
/// Beale-Sorenson (type=3).
pub unsafe fn cgmin(
    n: std::os::raw::c_int,
    bvec: *mut f64,
    x: *mut f64,
    fmin: *mut f64,
    fminfn: OptimFn,
    fmingr: OptimGr,
    fail: *mut std::os::raw::c_int,
    abstol: f64,
    intol: f64,
    ex: *mut std::ffi::c_void,
    r#type: std::os::raw::c_int,
    trace: std::os::raw::c_int,
    fncount: *mut std::os::raw::c_int,
    grcount: *mut std::os::raw::c_int,
    maxit: std::os::raw::c_int,
) {
    unsafe {
        let n = n as i32;
        let maxit = maxit as i32;
        let trace = trace != 0;
        let typ = r#type as i32;
        let n_usize = n as usize;

        if maxit <= 0 {
            *fmin = fminfn(n as std::os::raw::c_int, bvec, ex);
            *fncount = 0;
            *grcount = 0;
            *fail = 0;
            return;
        }

        if trace {
            eprintln!("  Conjugate gradients function minimizer");
            match typ {
                1 => eprintln!("Method: Fletcher Reeves"),
                2 => eprintln!("Method: Polak Ribiere"),
                3 => eprintln!("Method: Beale Sorenson"),
                _ => {
                    eprintln!("unknown type in \"CG\" method of 'optim'");
                    *fail = 1;
                    return;
                }
            }
        }

        let mut c = vec![0.0f64; n_usize];
        let mut g = vec![0.0f64; n_usize];
        let mut t = vec![0.0f64; n_usize];

        let setstep = 1.7;
        *fail = 0;
        let cyclimit = n;
        let tol = intol * (n as f64) * sqrt(intol);

        if trace {
            eprintln!("tolerance used in gradient test={}", tol);
        }

        let mut f = fminfn(n as std::os::raw::c_int, bvec, ex);
        if !f.is_finite() {
            eprintln!("function cannot be evaluated at initial parameters");
            return;
        }

        *fmin = f;
        let mut funcount: i32 = 1;
        let mut gradcount: i32 = 0;

        loop {
            for i in 0..n_usize {
                t[i] = 0.0;
                c[i] = 0.0;
            }
            let mut cycle: i32 = 0;
            let mut oldstep = 1.0_f64;
            let mut count: i32 = 0;
            let mut g1 = 0.0_f64;
            let mut steplength = 1.0_f64;

            loop {
                cycle += 1;
                if trace {
                    eprintln!("{} {} {}", gradcount, funcount, *fmin);
                }
                gradcount += 1;
                if gradcount > maxit {
                    *fncount = funcount;
                    *grcount = gradcount;
                    *fail = 1;
                    return;
                }
                fmingr(n as std::os::raw::c_int, bvec, g.as_mut_ptr(), ex);

                g1 = 0.0_f64;
                let mut g2 = 0.0_f64;
                for i in 0..n_usize {
                    *x.add(i) = *bvec.add(i);
                    match typ {
                        1 => {
                            // Fletcher-Reeves
                            g1 += g[i] * g[i];
                            g2 += c[i] * c[i];
                        }
                        2 => {
                            // Polak-Ribiere
                            g1 += g[i] * (g[i] - c[i]);
                            g2 += c[i] * c[i];
                        }
                        3 => {
                            // Beale-Sorenson
                            g1 += g[i] * (g[i] - c[i]);
                            g2 += t[i] * (g[i] - c[i]);
                        }
                        _ => {}
                    }
                    c[i] = g[i];
                }

                if g1 > tol {
                    let g3 = if g2 > 0.0 { g1 / g2 } else { 1.0 };

                    let mut gradproj = 0.0_f64;
                    for i in 0..n_usize {
                        t[i] = t[i] * g3 - g[i];
                        gradproj += t[i] * g[i];
                    }

                    steplength = oldstep;
                    let mut accpoint = false;
                    loop {
                        count = 0;
                        for i in 0..n_usize {
                            *bvec.add(i) = *x.add(i) + steplength * t[i];
                            if RELTEST + *x.add(i) == RELTEST + *bvec.add(i) {
                                count += 1;
                            }
                        }
                        if count < n {
                            f = fminfn(n as std::os::raw::c_int, bvec, ex);
                            funcount += 1;
                            accpoint = f.is_finite() && f <= *fmin + gradproj * steplength * ACCTOL;
                            if !accpoint {
                                steplength *= STEPREDN;
                            }
                        }
                        if count == n || accpoint {
                            break;
                        }
                    }

                    if count < n {
                        let newstep = 2.0 * (f - *fmin - gradproj * steplength);
                        if newstep > 0.0 {
                            let ns = -(gradproj * steplength * steplength / newstep);
                            for i in 0..n_usize {
                                *bvec.add(i) = *x.add(i) + ns * t[i];
                            }
                            *fmin = f;
                            f = fminfn(n as std::os::raw::c_int, bvec, ex);
                            funcount += 1;
                            if f < *fmin {
                                *fmin = f;
                            } else {
                                for i in 0..n_usize {
                                    *bvec.add(i) = *x.add(i) + steplength * t[i];
                                }
                            }
                        }
                    }
                }
                oldstep = setstep * steplength;
                if oldstep > 1.0 {
                    oldstep = 1.0;
                }

                if count == n || g1 <= tol || cycle == cyclimit {
                    break;
                }
            }

            if cycle != 1 && !(count == n || g1 <= tol) && *fmin > abstol {
                continue;
            }
            break;
        }

        if trace {
            eprintln!("Exiting from conjugate gradients minimizer");
            eprintln!("    {} function evaluations used", funcount);
            eprintln!("    {} gradient evaluations used", gradcount);
        }
        *fncount = funcount;
        *grcount = gradcount;
    }
}
