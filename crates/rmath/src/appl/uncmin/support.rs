#![allow(clippy::self_assignment, clippy::assigning_clones)]
#![allow(clippy::comparison_chain)]
#![allow(unused_variables, clippy::manual_memcpy, clippy::comparison_to_empty)]
#![allow(unused_assignments)]
use super::*;

#[inline(always)]
pub(crate) fn pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}
#[inline(always)]
fn hypot(x: f64, y: f64) -> f64 {
    x.hypot(y)
}
#[inline(always)]
pub(crate) fn sqrt(x: f64) -> f64 {
    x.sqrt()
}
#[inline(always)]
pub(crate) fn fabs(x: f64) -> f64 {
    x.abs()
}
#[inline(always)]
pub(crate) fn log10(x: f64) -> f64 {
    x.log10()
}
// =====================================================================
// Callback types
// =====================================================================
/// Function type: fn(n, x, f, state) — evaluates function and stores result in f
pub type UncminFcn = unsafe extern "C" fn(
    n: std::os::raw::c_int,
    x: *mut f64,
    f: *mut f64,
    state: *mut std::ffi::c_void,
);

/// Gradient function type: fn(n, x, g, state)
pub type UncminD1Fcn = unsafe extern "C" fn(
    n: std::os::raw::c_int,
    x: *mut f64,
    g: *mut f64,
    state: *mut std::ffi::c_void,
);

/// Hessian function type: fn(nr, n, x, h, state)
pub type UncminD2Fcn = unsafe extern "C" fn(
    nr: std::os::raw::c_int,
    n: std::os::raw::c_int,
    x: *mut f64,
    h: *mut f64,
    state: *mut std::ffi::c_void,
);

// =====================================================================
// Inline BLAS replacements
// =====================================================================

#[inline(always)]
fn ddot(n: i32, dx: &[f64], incx: usize, dy: &[f64], incy: usize) -> f64 {
    // gfortran -O2 (Rblas) contracts each accumulate into an FMA; the
    // reference Fortran's 5-wide unroll still adds one product at a
    // time, so this matches it for any n.
    let mut s = 0.0_f64;
    if incx == 1 && incy == 1 {
        for i in 0..n as usize {
            s = dx[i].mul_add(dy[i], s);
        }
    } else {
        let mut ix = 0;
        let mut iy = 0;
        for _ in 0..n {
            s = dx[ix].mul_add(dy[iy], s);
            ix += incx;
            iy += incy;
        }
    }
    s
}

#[inline(always)]
fn dnrm2(n: i32, x: &[f64], incx: usize) -> f64 {
    // Blue's algorithm as in Rblas (blas2.f90): for mid-range values
    // (2^-511 <= |x| <= 2^486, the only case reachable here) it is a
    // sequential sum of squares; gfortran contracts the accumulate.
    let mut sum = 0.0_f64;
    if incx == 1 {
        for i in 0..n as usize {
            sum = x[i].mul_add(x[i], sum);
        }
    } else {
        let mut ix = 0;
        for _ in 0..n {
            sum = x[ix].mul_add(x[ix], sum);
            ix += incx;
        }
    }
    sqrt(sum)
}

#[inline(always)]
fn dscal(n: i32, da: f64, dx: &mut [f64], incx: usize) {
    if da == 1.0 {
        return;
    }
    if incx == 1 {
        for i in 0..n as usize {
            dx[i] *= da;
        }
    } else {
        let mut ix = 0;
        for _ in 0..n {
            dx[ix] *= da;
            ix += incx;
        }
    }
}

/// LINPACK dtrsl: solve triangular system.
/// a is stored column-major (Fortran style), so a[j*nr + i] = a[i][j].
/// job: 0 = solve L*x=b, 10 = solve L'*x=b
/// Returns info: 0 = success, k > 0 = zero pivot at position k
fn dtrsl(a: &[f64], nr: i32, n: i32, x: &mut [f64], job: i32) -> i32 {
    let mut info = 0;

    if job == 0 {
        // Solve L*x = b (daxpy-based; gfortran contracts dy + da*dx)
        for j in 0..n as usize {
            if a[j * nr as usize + j] == 0.0 {
                info = (j + 1) as i32;
                return info;
            }
            x[j] /= a[j * nr as usize + j];
            let tmp = x[j];
            for i in (j + 1)..n as usize {
                x[i] = (-tmp).mul_add(a[j * nr as usize + i], x[i]);
            }
        }
    } else {
        // Solve L'*x = b (ddot-based back substitution)
        for j in (0..n as usize).rev() {
            let mut tmp = x[j];
            for i in (j + 1)..n as usize {
                tmp -= a[j * nr as usize + i] * x[i];
            }
            if a[j * nr as usize + j] == 0.0 {
                info = (j + 1) as i32;
                return info;
            }
            x[j] = tmp / a[j * nr as usize + j];
        }
    }
    info
}

// =====================================================================
// Helper: print vector (R's printRealVector, as uncmin.c's prt_result uses)
// =====================================================================

pub(crate) fn print_real_vector(v: &[f64], n: usize) {
    unsafe {
        crate::mainutils::printvector::printRealVector(v.as_ptr(), n as i64, 1);
    }
}

// =====================================================================
// fdhess: numerical Hessian approximation
// =====================================================================

/// Calculates a numerical approximation to the upper triangular
/// portion of the second derivative matrix (the Hessian).
pub unsafe fn fdhess(
    n: std::os::raw::c_int,
    x: *mut f64,
    fval: f64,
    fun: UncminFcn,
    state: *mut std::ffi::c_void,
    h: *mut f64,
    nfd: std::os::raw::c_int,
    step: *mut f64,
    f: *mut f64,
    ndigit: std::os::raw::c_int,
    typx: *const f64,
) {
    unsafe {
        let n = n as usize;
        let nfd = nfd as usize;
        let eta = pow(10.0, -(ndigit as f64) / 3.0);

        let x = std::slice::from_raw_parts_mut(x, n);
        let step = std::slice::from_raw_parts_mut(step, n);
        let f = std::slice::from_raw_parts_mut(f, n);
        let h = std::slice::from_raw_parts_mut(h, n * nfd);
        let typx = std::slice::from_raw_parts(typx, n);

        for i in 0..n {
            step[i] = eta * fmax2(x[i], typx[i]);
            if typx[i] < 0.0 {
                step[i] = -step[i];
            }

            let tempi = x[i];
            x[i] += step[i];
            step[i] = x[i] - tempi;
            let mut fval_tmp = 0.0;
            fun(
                n as std::os::raw::c_int,
                x.as_mut_ptr(),
                &mut fval_tmp as *mut f64,
                state,
            );
            f[i] = fval_tmp;
            x[i] = tempi;
        }

        for i in 0..n {
            let tempi = x[i];
            x[i] += step[i] * 2.0;
            let mut fii = 0.0;
            fun(
                n as std::os::raw::c_int,
                x.as_mut_ptr(),
                &mut fii as *mut f64,
                state,
            );
            h[i + i * nfd] = (fval - f[i] + (fii - f[i])) / (step[i] * step[i]);
            x[i] = tempi + step[i];

            for j in (i + 1)..n {
                let tempj = x[j];
                x[j] += step[j];
                let mut fij = 0.0;
                fun(
                    n as std::os::raw::c_int,
                    x.as_mut_ptr(),
                    &mut fij as *mut f64,
                    state,
                );
                h[i + j * nfd] = (fval - f[i] + (fij - f[j])) / (step[i] * step[j]);
                x[j] = tempj;
            }
            x[i] = tempi;
        }
    }
}

// =====================================================================
// Internal matrix/vector operations
// =====================================================================

/// Compute y = L*x where L is lower triangular stored in a (column-major)
fn mvmltl(nr: usize, n: usize, a: &[f64], x: &[f64], y: &mut [f64]) {
    for i in 0..n {
        let mut sum = 0.0;
        for j in 0..=i {
            // clang -O2 contracts sum + a*x
            sum = a[i + j * nr].mul_add(x[j], sum);
        }
        y[i] = sum;
    }
}

/// Compute y = L^T * x where L is lower triangular stored in a (column-major)
fn mvmltu(nr: usize, n: usize, a: &[f64], x: &[f64], y: &mut [f64]) {
    for i in 0..n {
        let length = n - i;
        y[i] = ddot(length as i32, &a[i + i * nr..], 1, &x[i..], 1);
    }
}

/// Compute y = A*x where A is symmetric stored in lower triangular part
fn mvmlts(nr: usize, n: usize, a: &[f64], x: &[f64], y: &mut [f64]) {
    for i in 0..n {
        let mut sum = 0.0;
        for j in 0..=i {
            sum = a[i + j * nr].mul_add(x[j], sum);
        }
        for j in (i + 1)..n {
            sum = a[j + i * nr].mul_add(x[j], sum);
        }
        y[i] = sum;
    }
}

/// Solve Ax=b where A = L*L^T, only L is stored
pub(crate) fn lltslv(nr: usize, n: usize, a: &[f64], x: &mut [f64], b: &[f64]) {
    if x.as_ptr() != b.as_ptr() {
        x[..n].copy_from_slice(&b[..n]);
    }
    let mut info = dtrsl(a, nr as i32, n as i32, x, 0);
    info = dtrsl(a, nr as i32, n as i32, x, 10);
    let _ = info;
}

/// Cholesky decomposition with tolerance perturbation
pub(crate) fn choldc(nr: usize, n: usize, a: &mut [f64], diagmx: f64, tol: f64, addmax: &mut f64) {
    *addmax = 0.0;
    let aminl = sqrt(diagmx * tol);
    let amnlsq = aminl * aminl;

    for i in 0..n {
        // Compute off-diagonal elements
        for j in 0..i {
            let mut sum = 0.0;
            for k in 0..j {
                sum += a[i + k * nr] * a[j + k * nr];
            }
            a[i + j * nr] = (a[i + j * nr] - sum) / a[j + j * nr];
        }

        // Compute diagonal
        let mut sum = 0.0;
        for k in 0..i {
            sum += a[i + k * nr] * a[i + k * nr];
        }

        let tmp1 = a[i + i * nr] - sum;
        if tmp1 >= amnlsq {
            a[i + i * nr] = sqrt(tmp1);
        } else {
            let mut offmax = 0.0;
            for j in 0..i {
                let tmp2 = fabs(a[i + j * nr]);
                if offmax < tmp2 {
                    offmax = tmp2;
                }
            }
            if offmax <= amnlsq {
                offmax = amnlsq;
            }
            a[i + i * nr] = sqrt(offmax);
            let tmp2 = offmax - tmp1;
            if *addmax < tmp2 {
                *addmax = tmp2;
            }
        }
    }
}

/// Interchange rows i,i+1 of upper hessenberg matrix r, columns i..n
fn qraux1(nr: usize, n: usize, r: &mut [f64], i: usize) {
    let mut r1_idx = i + i * nr;
    let mut r2_idx = r1_idx + 1;
    let mut n_remaining = n - i;
    while n_remaining > 0 {
        let tmp = r[r1_idx];
        r[r1_idx] = r[r2_idx];
        r[r2_idx] = tmp;
        r1_idx += nr;
        r2_idx += nr;
        n_remaining -= 1;
    }
}

/// Pre-multiply r by Jacobi rotation j(i,i+1,a,b)
fn qraux2(nr: usize, n: usize, r: &mut [f64], i: usize, a: f64, b: f64) {
    let den = hypot(a, b);
    let c = a / den;
    let s = b / den;

    let mut r1_idx = i + i * nr;
    let mut r2_idx = r1_idx + 1;
    let mut n_remaining = n - i;
    while n_remaining > 0 {
        let y = r[r1_idx];
        let z = r[r2_idx];
        r[r1_idx] = c * y - s * z;
        r[r2_idx] = s * y + c * z;
        r1_idx += nr;
        r2_idx += nr;
        n_remaining -= 1;
    }
}

/// QR update: find Q*R* = R + u*v^T
fn qrupdt(nr: usize, n: usize, a: &mut [f64], u: &mut [f64], v: &[f64]) {
    // Determine last non-zero in u
    let mut k = n - 1;
    while k > 0 && u[k] == 0.0 {
        k -= 1;
    }

    // Jacobi rotations to get upper hessenberg form
    if k > 0 {
        let mut ii = k;
        while ii > 0 {
            let i = ii - 1;
            if u[i] == 0.0 {
                qraux1(nr, n, a, i);
                u[i] = u[ii];
            } else {
                qraux2(nr, n, a, i, u[i], -u[ii]);
                u[i] = hypot(u[i], u[ii]);
            }
            ii = i;
        }
    }

    // r <- r + u(1) * v^T
    for j in 0..n {
        a[j * nr] = u[0].mul_add(v[j], a[j * nr]);
    }

    // Jacobi rotations to restore upper triangular
    for i in 0..k {
        if a[i + i * nr] == 0.0 {
            qraux1(nr, n, a, i);
        } else {
            let t1 = a[i + i * nr];
            let t2 = -a[i + 1 + i * nr];
            qraux2(nr, n, a, i, t1, t2);
        }
    }
}

// =====================================================================
// Trust region update (tregup)
// =====================================================================

unsafe fn tregup(
    nr: usize,
    n: usize,
    x: &[f64],
    f: f64,
    g: &[f64],
    a: &[f64],
    fcn: UncminFcn,
    state: *mut std::ffi::c_void,
    sc: &[f64],
    sx: &[f64],
    nwtake: bool,
    stepmx: f64,
    steptl: f64,
    dlt: &mut f64,
    iretcd: &mut i32,
    xplsp: &mut [f64],
    fplsp: &mut f64,
    xpls: &mut [f64],
    fpls: &mut f64,
    mxtake: &mut bool,
    method: i32,
    udiag: &[f64],
) {
    unsafe {
        *mxtake = false;
        for i in 0..n {
            xpls[i] = x[i] + sc[i];
        }

        let mut fpls_val = 0.0;
        fcn(
            n as std::os::raw::c_int,
            xpls.as_mut_ptr(),
            &mut fpls_val as *mut f64,
            state,
        );
        *fpls = fpls_val;

        let dltf = *fpls - f;
        let slp = ddot(n as i32, g, 1, sc, 1);

        if *iretcd == 3 && (*fpls >= *fplsp || dltf > slp * 1e-4) {
            *iretcd = 0;
            for i in 0..n {
                xpls[i] = xplsp[i];
            }
            *fpls = *fplsp;
            *dlt *= 0.5;
        } else {
            if dltf > slp * 1e-4 {
                let mut rln = 0.0;
                for i in 0..n {
                    let temp1 = fabs(sc[i]) / fmax2(fabs(xpls[i]), 1.0 / sx[i]);
                    if rln < temp1 {
                        rln = temp1;
                    }
                }
                if rln < steptl {
                    *iretcd = 1;
                } else {
                    *iretcd = 2;
                    let dltmp = -slp * *dlt / ((dltf - slp) * 2.0);
                    if dltmp < *dlt * 0.1 {
                        *dlt *= 0.1;
                    } else {
                        *dlt = dltmp;
                    }
                }
            } else {
                let mut dltfp = 0.0;
                if method == 2 {
                    for i in 0..n {
                        let mut temp1 = 0.0;
                        for j in i..n {
                            temp1 += a[j + i * nr] * sc[j];
                        }
                        dltfp += temp1 * temp1;
                    }
                } else {
                    for i in 0..n {
                        dltfp += udiag[i] * sc[i] * sc[i];
                        let mut temp1 = 0.0;
                        for j in (i + 1)..n {
                            temp1 += a[i + j * nr] * sc[i] * sc[j];
                        }
                        dltfp += temp1 * 2.0;
                    }
                }
                dltfp = slp + dltfp / 2.0;
                if *iretcd != 2
                    && fabs(dltfp - dltf) <= fabs(dltf) * 0.1
                    && nwtake
                    && *dlt <= stepmx * 0.99
                {
                    *iretcd = 3;
                    for i in 0..n {
                        xplsp[i] = xpls[i];
                    }
                    *fplsp = *fpls;
                    let temp1 = *dlt * 2.0;
                    *dlt = fmin2(temp1, stepmx);
                } else {
                    *iretcd = 0;
                    if *dlt > stepmx * 0.99 {
                        *mxtake = true;
                    }
                    if dltf >= dltfp * 0.1 {
                        *dlt *= 0.5;
                    } else {
                        if dltf <= dltfp * 0.75 {
                            let temp1 = *dlt * 2.0;
                            *dlt = fmin2(temp1, stepmx);
                        }
                    }
                }
            }
        }
    }
}

// =====================================================================
// Line search (lnsrch)
// =====================================================================

pub(crate) unsafe fn lnsrch(
    n: usize,
    x: &[f64],
    f: f64,
    g: &[f64],
    p: &mut [f64],
    xpls: &mut [f64],
    fpls: &mut f64,
    fcn: UncminFcn,
    state: *mut std::ffi::c_void,
    mxtake: &mut bool,
    iretcd: &mut i32,
    stepmx: f64,
    steptl: f64,
    sx: &[f64],
) {
    unsafe {
        let mut firstback = true;
        let mut pfpls = 0.0_f64;
        let mut plmbda = 0.0_f64;

        let mut temp1 = 0.0;
        for i in 0..n {
            temp1 += sx[i] * sx[i] * p[i] * p[i];
        }
        let mut sln = sqrt(temp1);
        if sln > stepmx {
            let scl = stepmx / sln;
            dscal(n as i32, scl, p, 1);
            sln = stepmx;
        }
        let slp = ddot(n as i32, g, 1, p, 1);

        let mut rln = 0.0;
        for i in 0..n {
            temp1 = fabs(p[i]) / fmax2(fabs(x[i]), 1.0 / sx[i]);
            if rln < temp1 {
                rln = temp1;
            }
        }
        let rmnlmb = steptl / rln;
        let mut lambda = 1.0_f64;

        *mxtake = false;
        *iretcd = 2;
        loop {
            for i in 0..n {
                // GNU's clang -O2 build contracts this into a single
                // fused multiply-add; replicate it for bit-exact iterates
                // (visible whenever lambda is not a power of two).
                xpls[i] = lambda.mul_add(p[i], x[i]);
            }
            let mut fpls_val = 0.0;
            fcn(
                n as std::os::raw::c_int,
                xpls.as_mut_ptr(),
                &mut fpls_val as *mut f64,
                state,
            );
            *fpls = fpls_val;

            if *fpls <= f + slp * 1e-4 * lambda {
                *iretcd = 0;
                if lambda == 1.0 && sln > stepmx * 0.99 {
                    *mxtake = true;
                }
                return;
            }

            if lambda < rmnlmb {
                *iretcd = 1;
                return;
            } else {
                if *fpls >= f64::MAX {
                    lambda *= 0.1;
                    firstback = true;
                } else {
                    let mut tlmbda;
                    if firstback {
                        tlmbda = -lambda * slp / ((*fpls - f - slp) * 2.0);
                        firstback = false;
                    } else {
                        let t1 = *fpls - f - lambda * slp;
                        let t2 = pfpls - f - plmbda * slp;
                        let t3 = 1.0 / (lambda - plmbda);
                        let a3 = 3.0 * t3 * (t1 / (lambda * lambda) - t2 / (plmbda * plmbda));
                        let b = t3
                            * (t2 * lambda / (plmbda * plmbda) - t1 * plmbda / (lambda * lambda));
                        let disc = b * b - a3 * slp;
                        if disc > b * b {
                            tlmbda = (-b + if a3 < 0.0 { -sqrt(disc) } else { sqrt(disc) }) / a3;
                        } else {
                            tlmbda = (-b + if a3 < 0.0 { sqrt(disc) } else { -sqrt(disc) }) / a3;
                        }
                        if tlmbda > lambda * 0.5 {
                            tlmbda = lambda * 0.5;
                        }
                    }
                    plmbda = lambda;
                    pfpls = *fpls;
                    if tlmbda < lambda * 0.1 {
                        lambda *= 0.1;
                    } else {
                        lambda = tlmbda;
                    }
                }
            }
            if *iretcd <= 1 {
                break;
            }
        }
    }
}

// =====================================================================
// Double dogleg step (dog_1step, dogdrv)
// =====================================================================

fn dog_1step(
    nr: usize,
    n: usize,
    g: &[f64],
    a: &[f64],
    p: &[f64],
    sx: &[f64],
    rnwtln: f64,
    dlt: &mut f64,
    nwtake: &mut bool,
    fstdog: &mut bool,
    ssd: &mut [f64],
    v: &mut [f64],
    cln: &mut f64,
    eta: &mut f64,
    sc: &mut [f64],
    stepmx: f64,
) {
    *nwtake = rnwtln <= *dlt;

    if *nwtake {
        for i in 0..n {
            sc[i] = p[i];
        }
        *dlt = rnwtln;
        return;
    }

    if *fstdog {
        *fstdog = false;
        let mut alpha = 0.0;
        for i in 0..n {
            alpha += g[i] * g[i] / (sx[i] * sx[i]);
        }
        let mut bet = 0.0;
        for i in 0..n {
            let mut tmp = 0.0;
            for j in i..n {
                tmp += a[j + i * nr] * g[j] / (sx[j] * sx[j]);
            }
            bet += tmp * tmp;
        }
        for i in 0..n {
            ssd[i] = -(alpha / bet) * g[i] / sx[i];
        }
        *cln = alpha * sqrt(alpha) / bet;
        *eta = (0.8 * alpha * alpha / (-bet * ddot(n as i32, g, 1, p, 1))) + 0.2;
        for i in 0..n {
            v[i] = *eta * sx[i] * p[i] - ssd[i];
        }
        if *dlt == -1.0 {
            *dlt = fmin2(*cln, stepmx);
        }
    }

    if *eta * rnwtln <= *dlt {
        for i in 0..n {
            sc[i] = *dlt / rnwtln * p[i];
        }
    } else if *cln >= *dlt {
        for i in 0..n {
            sc[i] = *dlt / *cln * ssd[i] / sx[i];
        }
    } else {
        let dot1 = ddot(n as i32, v, 1, ssd, 1);
        let dot2 = ddot(n as i32, v, 1, v, 1);
        let alam = (-dot1 + sqrt(dot1 * dot1 - dot2 * (*cln * *cln - *dlt * *dlt))) / dot2;
        for i in 0..n {
            sc[i] = (ssd[i] + alam * v[i]) / sx[i];
        }
    }
}

pub(crate) unsafe fn dogdrv(
    nr: usize,
    n: usize,
    x: &[f64],
    f: f64,
    g: &[f64],
    a: &[f64],
    p: &[f64],
    xpls: &mut [f64],
    fpls: &mut f64,
    fcn: UncminFcn,
    state: *mut std::ffi::c_void,
    sx: &[f64],
    stepmx: f64,
    steptl: f64,
    dlt: &mut f64,
    iretcd: &mut i32,
    mxtake: &mut bool,
    sc: &mut [f64],
    wrk1: &mut [f64],
    wrk2: &mut [f64],
    wrk3: &mut [f64],
    _itncnt: i32,
) {
    unsafe {
        let mut tmp = 0.0;
        for i in 0..n {
            tmp += sx[i] * sx[i] * p[i] * p[i];
        }
        let rnwtln = sqrt(tmp);

        *iretcd = 4;
        let mut fstdog = true;
        let mut nwtake = false;
        let mut cln = 0.0;
        let mut eta = 0.0;
        let mut fplsp = 0.0;

        loop {
            dog_1step(
                nr,
                n,
                g,
                a,
                p,
                sx,
                rnwtln,
                dlt,
                &mut nwtake,
                &mut fstdog,
                wrk1,
                wrk2,
                &mut cln,
                &mut eta,
                sc,
                stepmx,
            );
            tregup(
                nr,
                n,
                x,
                f,
                g,
                a,
                fcn,
                state,
                sc,
                sx,
                nwtake,
                stepmx,
                steptl,
                dlt,
                iretcd,
                wrk3,
                &mut fplsp,
                xpls,
                fpls,
                mxtake,
                2,
                &[],
            );
        }
    }
}

// =====================================================================
// More-Hebdon step (hook_1step, hookdrv)
// =====================================================================

unsafe fn hook_1step(
    nr: usize,
    n: usize,
    g: &[f64],
    a: &mut [f64],
    udiag: &[f64],
    p: &[f64],
    sx: &[f64],
    rnwtln: f64,
    dlt: &mut f64,
    amu: &mut f64,
    dltp: f64,
    phi: &mut f64,
    phip0: &mut f64,
    fstime: &mut bool,
    sc: &mut [f64],
    nwtake: &mut bool,
    wrk0: &mut [f64],
    epsm: f64,
) {
    let hi = 1.5_f64;
    let alo = 0.75_f64;

    *nwtake = rnwtln <= hi * *dlt;
    if *nwtake {
        for i in 0..n {
            sc[i] = p[i];
        }
        *dlt = fmin2(*dlt, rnwtln);
        *amu = 0.0;
        return;
    }

    if *amu > 0.0 {
        *amu -= (*phi + dltp) * (dltp - *dlt + *phi) / (*dlt * *phip0);
    }

    *phi = rnwtln - *dlt;
    if *fstime {
        for i in 0..n {
            wrk0[i] = sx[i] * sx[i] * p[i];
        }
        let info = dtrsl(a, nr as i32, n as i32, wrk0, 0);
        let temp1 = dnrm2(n as i32, wrk0, 1);
        *phip0 = -(temp1 * temp1) / rnwtln;
        *fstime = false;
        let _ = info;
    }
    let phip = *phip0;
    let mut amulo = -(*phi) / phip;
    let mut amuup = 0.0;
    for i in 0..n {
        amuup += g[i] * g[i] / (sx[i] * sx[i]);
    }
    amuup = sqrt(amuup) / *dlt;

    loop {
        if *amu < amulo || *amu > amuup {
            *amu = fmax2(sqrt(amulo * amuup), amuup * 0.001);
        }

        // Copy (h,udiag) to L
        for i in 0..n {
            a[i + i * nr] = udiag[i] + *amu * sx[i] * sx[i];
            for j in 0..i {
                a[i + j * nr] = a[j + i * nr];
            }
        }

        // Factor H = L*L^T
        let temp1 = sqrt(epsm);
        let mut addmax = 0.0;
        choldc(nr, n, a, 0.0, temp1, &mut addmax);

        // Solve H*p = -g
        for i in 0..n {
            wrk0[i] = -g[i];
        }
        lltslv(nr, n, a, sc, wrk0);

        let mut stepln = 0.0;
        for i in 0..n {
            stepln += sx[i] * sx[i] * sc[i] * sc[i];
        }
        stepln = sqrt(stepln);
        *phi = stepln - *dlt;

        for i in 0..n {
            wrk0[i] = sx[i] * sx[i] * sc[i];
        }
        let info = dtrsl(a, nr as i32, n as i32, wrk0, 0);
        let temp1 = dnrm2(n as i32, wrk0, 1);
        let phip_val = -(temp1 * temp1) / stepln;
        let _ = info;

        if (alo * *dlt <= stepln && stepln <= hi * *dlt) || (amuup - amulo > 0.0) {
            break;
        } else {
            let temp1 = (*amu - *phi) / phip_val;
            amulo = fmax2(amulo, temp1);
            if *phi < 0.0 {
                amuup = fmin2(amuup, *amu);
            }
            *amu -= stepln * *phi / (*dlt * phip_val);
        }
    }
}

pub(crate) unsafe fn hookdrv(
    nr: usize,
    n: usize,
    x: &[f64],
    f: f64,
    g: &[f64],
    a: &mut [f64],
    udiag: &[f64],
    p: &[f64],
    xpls: &mut [f64],
    fpls: &mut f64,
    fcn: UncminFcn,
    state: *mut std::ffi::c_void,
    sx: &[f64],
    stepmx: f64,
    steptl: f64,
    dlt: &mut f64,
    iretcd: &mut i32,
    mxtake: &mut bool,
    amu: &mut f64,
    dltp: &mut f64,
    phi: &mut f64,
    phip0: &mut f64,
    sc: &mut [f64],
    xplsp: &mut [f64],
    wrk0: &mut [f64],
    epsm: f64,
    itncnt: i32,
) {
    unsafe {
        let mut tmp = 0.0;
        for i in 0..n {
            tmp += sx[i] * sx[i] * p[i] * p[i];
        }
        let rnwtln = sqrt(tmp);

        if itncnt == 1 {
            *amu = 0.0;
            if *dlt == -1.0 {
                let mut alpha = 0.0;
                for i in 0..n {
                    alpha += g[i] * g[i] / (sx[i] * sx[i]);
                }
                let mut bet = 0.0;
                for i in 0..n {
                    tmp = 0.0;
                    for j in i..n {
                        tmp += a[j + i * nr] * g[j] / (sx[j] * sx[j]);
                    }
                    bet += tmp * tmp;
                }
                *dlt = alpha * sqrt(alpha) / bet;
                if *dlt > stepmx {
                    *dlt = stepmx;
                }
            }
        }

        *iretcd = 4;
        let mut fstime = true;
        let mut fplsp = 0.0;
        let mut nwtake = false;

        loop {
            hook_1step(
                nr,
                n,
                g,
                a,
                udiag,
                p,
                sx,
                rnwtln,
                dlt,
                amu,
                *dltp,
                phi,
                phip0,
                &mut fstime,
                sc,
                &mut nwtake,
                wrk0,
                epsm,
            );
            *dltp = *dlt;
            tregup(
                nr, n, x, f, g, a, fcn, state, sc, sx, false, stepmx, steptl, dlt, iretcd, xplsp,
                &mut fplsp, xpls, fpls, mxtake, 3i32, udiag,
            );
        }
    }
}

// =====================================================================
// BFGS updates (secunf, secfac)
// =====================================================================

pub(crate) fn secunf(
    nr: usize,
    n: usize,
    x: &[f64],
    g: &[f64],
    a: &mut [f64],
    udiag: &[f64],
    xpls: &[f64],
    gpls: &[f64],
    epsm: f64,
    itncnt: i32,
    rnf: f64,
    iagflg: i32,
    noupdt: &mut bool,
    s: &mut [f64],
    y: &mut [f64],
    t: &mut [f64],
) {
    // Copy hessian to lower triangular
    for i in 0..n {
        a[i + i * nr] = udiag[i];
        for j in 0..i {
            a[i + j * nr] = a[j + i * nr];
        }
    }

    *noupdt = itncnt == 1;

    for i in 0..n {
        s[i] = xpls[i] - x[i];
        y[i] = gpls[i] - g[i];
    }
    let den1 = ddot(n as i32, s, 1, y, 1);
    let snorm2 = dnrm2(n as i32, s, 1);
    let ynrm2 = dnrm2(n as i32, y, 1);
    if den1 < sqrt(epsm) * snorm2 * ynrm2 {
        return;
    }

    mvmlts(nr, n, a, s, t);
    let mut den2 = ddot(n as i32, s, 1, t, 1);
    if *noupdt {
        let gam = den1 / den2;
        den2 *= gam;
        for j in 0..n {
            t[j] *= gam;
            for i in j..n {
                a[i + j * nr] *= gam;
            }
        }
        *noupdt = false;
    }

    let mut skpupd = true;
    for i in 0..n {
        let mut tol = rnf * fmax2(fabs(g[i]), fabs(gpls[i]));
        if iagflg == 0 {
            tol /= sqrt(rnf);
        }
        if fabs(y[i] - t[i]) >= tol {
            skpupd = false;
            break;
        }
    }
    if skpupd {
        return;
    }

    for j in 0..n {
        for i in j..n {
            a[i + j * nr] += y[i] * y[j] / den1 - t[i] * t[j] / den2;
        }
    }
}

pub(crate) fn secfac(
    nr: usize,
    n: usize,
    x: &[f64],
    g: &[f64],
    a: &mut [f64],
    xpls: &[f64],
    gpls: &[f64],
    epsm: f64,
    itncnt: i32,
    rnf: f64,
    iagflg: i32,
    noupdt: &mut bool,
    s: &mut [f64],
    y: &mut [f64],
    u: &mut [f64],
    w: &mut [f64],
) {
    *noupdt = itncnt == 1;

    for i in 0..n {
        s[i] = xpls[i] - x[i];
        y[i] = gpls[i] - g[i];
    }
    let den1 = ddot(n as i32, s, 1, y, 1);
    let snorm2 = dnrm2(n as i32, s, 1);
    let ynrm2 = dnrm2(n as i32, y, 1);
    if den1 < sqrt(epsm) * snorm2 * ynrm2 {
        return;
    }

    mvmltu(nr, n, a, s, u);
    let mut den2 = ddot(n as i32, u, 1, u, 1);

    let mut alp = sqrt(den1 / den2);
    if *noupdt {
        for j in 0..n {
            u[j] *= alp;
            for i in j..n {
                a[i + j * nr] *= alp;
            }
        }
        *noupdt = false;
        den2 = den1;
        alp = 1.0;
    }

    // w = L*L^T * s = H*s
    mvmltl(nr, n, a, u, w);
    let reltol = if iagflg == 0 { sqrt(rnf) } else { rnf };

    let mut skpupd = true;
    for i in 0..n {
        skpupd = fabs(y[i] - w[i]) < reltol * fmax2(fabs(g[i]), fabs(gpls[i]));
        if !skpupd {
            break;
        }
    }
    if skpupd {
        return;
    }

    // w = y - alp*L*L^T*s
    for i in 0..n {
        w[i] = y[i] - alp * w[i];
    }
    alp /= den1;
    for i in 0..n {
        u[i] *= alp;
    }

    // Copy L to upper triangular, zero L
    for i in 1..n {
        for j in 0..i {
            a[j + i * nr] = a[i + j * nr];
            a[i + j * nr] = 0.0;
        }
    }

    // QR update
    qrupdt(nr, n, a, u, w);

    // Copy back to lower triangular
    for i in 1..n {
        for j in 0..i {
            a[i + j * nr] = a[j + i * nr];
        }
    }
}

// =====================================================================
// Hessian handling (chlhsn, hsnint)
// =====================================================================
