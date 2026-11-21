#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]

/*
 * LINPACK QR routines.
 * Ported from R's appl/dqrdc2.f and appl/dqrsl.f
 *
 * dqrdc2: Modified QR decomposition with column pivoting
 * dqrsl:  Apply QR decomposition to solve least squares
 * dnrm2:  BLAS Euclidean norm
 *
 * Original LINPACK: G.W. Stewart, U. of Maryland, Argonne National Lab.
 * dqrdc2 modifications by Ross Ihaka (1995), BDR (1999, 2024).
 */

use std::os::raw::c_int;
use std::slice::{from_raw_parts, from_raw_parts_mut};

// ---------------------------------------------------------------------------
// BLAS helpers (local to this module)
// ---------------------------------------------------------------------------

/// Compute dy = dy + da * dx element-wise over the overlapping prefix.
fn daxpy(da: f64, dx: &[f64], dy: &mut [f64]) {
    if da == 0.0 {
        return;
    }
    // GNU builds the reference BLAS with default `-ffp-contract=fast`,
    // fusing `dy += da*dx` into an FMA; mirror that arithmetic so that
    // identical inputs (e.g. y equal to a model-matrix column) cancel
    // exactly, as LINPACK's least-squares cancellation requires.
    for (y, &x) in dy.iter_mut().zip(dx.iter()) {
        *y = da.mul_add(x, *y);
    }
}

/// Compute the dot product of the overlapping prefixes of `dx` and `dy`.
fn ddot(dx: &[f64], dy: &[f64]) -> f64 {
    let mut sum = 0.0f64;
    // FMA-contracted accumulation, matching the reference BLAS compiled
    // with `-ffp-contract=fast` (see daxpy note above).
    for (&x, &y) in dx.iter().zip(dy.iter()) {
        sum = x.mul_add(y, sum);
    }
    sum
}

/// Scale a vector: dx = dx * da.
fn dscal(da: f64, dx: &mut [f64]) {
    if da == 1.0 {
        return;
    }
    for x in dx.iter_mut() {
        *x *= da;
    }
}

/// Copy dy = dx over the overlapping prefix.
fn dcopy(dx: &[f64], dy: &mut [f64]) {
    for (y, &x) in dy.iter_mut().zip(dx.iter()) {
        *y = x;
    }
}

/// Euclidean norm of a vector: sqrt(sum(x[i]^2))
fn dnrm2(x: &[f64]) -> f64 {
    let mut ssq = 0.0f64;
    let mut scale = 0.0f64;
    for &xi in x {
        if xi != 0.0 {
            if scale < xi.abs() {
                ssq = 1.0 + ssq * (scale / xi).powi(2);
                scale = xi.abs();
            } else {
                ssq += (xi / scale).powi(2);
            }
        }
    }
    scale * ssq.sqrt()
}

// ---------------------------------------------------------------------------
// dqrdc2: QR decomposition with column pivoting (R modification)
// ---------------------------------------------------------------------------

/// Modified QR decomposition with limited column pivoting.
///
/// Uses Householder transformations to compute the QR factorization of an n by p
/// matrix x. Columns with near-zero norm are moved to the right-hand edge.
///
/// # Arguments (1-based Fortran indexing for arrays)
/// - `x`: n x p matrix, column-major. On return contains R in upper triangle.
/// - `ldx`: leading dimension of x (>= n)
/// - `n`: number of rows
/// - `p`: number of columns
/// - `tol`: tolerance for rank determination
/// - `k`: on return, rank of x
/// - `qraux`: auxiliary output, length p
/// - `jpvt`: pivot indices, length p
/// - `work`: work array, length p*2
///
/// # Safety
/// All pointers must be valid for the documented extents.
pub unsafe fn dqrdc2(
    x: *mut f64,
    ldx: c_int,
    n: c_int,
    p: c_int,
    tol: f64,
    k: *mut c_int,
    qraux: *mut f64,
    jpvt: *mut c_int,
    work: *mut f64,
) {
    unsafe {
        let (ldx, n, p) = (ldx as usize, n as usize, p as usize);
        // The Fortran entry documents x as (ldx, p) and the scratch vectors
        // as p (qraux, jpvt) and 2*p (work) elements.
        dqrdc2_impl(
            from_raw_parts_mut(x, ldx * p),
            ldx,
            n,
            p,
            tol,
            &mut *k,
            from_raw_parts_mut(qraux, p),
            from_raw_parts_mut(jpvt, p),
            from_raw_parts_mut(work, 2 * p),
        );
    }
}

pub(crate) fn dqrdc2_impl(
    x: &mut [f64],
    ldx: usize,
    n: usize,
    p: usize,
    tol: f64,
    k: &mut c_int,
    qraux: &mut [f64],
    jpvt: &mut [c_int],
    work: &mut [f64],
) {
    // Compute norms of columns of x
    if n > 0 {
        for j in 0..p {
            let norm = dnrm2(&x[j * ldx..j * ldx + n]);
            qraux[j] = norm;
            work[j] = norm;
            work[p + j] = norm;
            if work[p + j] == 0.0 {
                work[p + j] = 1.0;
            }
        }
    }

    // Householder reduction
    let lup = if n < p { n } else { p };
    let mut k_val = p + 1usize;

    let mut l: usize = 0;
    while l < lup {
        crate::eval::limits::poll_computation();
        // Cycle columns from l to p-1 to find one with non-negligible norm
        loop {
            crate::eval::limits::poll_computation();
            if l + 1 >= k_val || qraux[l] >= work[p + l] * tol {
                break;
            }
            // Shift columns left
            for i in 0..n {
                let t = x[i + l * ldx];
                for j in l..(p - 1) {
                    x[i + j * ldx] = x[i + (j + 1) * ldx];
                }
                x[i + (p - 1) * ldx] = t;
            }
            // Shift jpvt, qraux, work
            let tmp_jpvt = jpvt[l];
            let tmp_qraux = qraux[l];
            let tmp_work1 = work[l];
            let tmp_work2 = work[p + l];
            for j in l..(p - 1) {
                jpvt[j] = jpvt[j + 1];
                qraux[j] = qraux[j + 1];
                work[j] = work[j + 1];
                work[p + j] = work[p + j + 1];
            }
            jpvt[p - 1] = tmp_jpvt;
            qraux[p - 1] = tmp_qraux;
            work[p - 1] = tmp_work1;
            work[2 * p - 1] = tmp_work2;
            k_val -= 1;
        }

        if l + 1 != n {
            // Compute Householder transformation for column l
            let nrmxl = dnrm2(&x[l + l * ldx..l + l * ldx + (n - l)]);
            if nrmxl != 0.0 {
                let signed_norm = if x[l + l * ldx] != 0.0 {
                    nrmxl.copysign(x[l + l * ldx])
                } else {
                    nrmxl
                };
                dscal(
                    1.0 / signed_norm,
                    &mut x[l + l * ldx..l + l * ldx + (n - l)],
                );
                x[l + l * ldx] = 1.0 + x[l + l * ldx];

                // Apply transformation to remaining columns
                if p > l {
                    for j in (l + 1)..p {
                        let t = -ddot(
                            &x[l + l * ldx..l + l * ldx + (n - l)],
                            &x[l + j * ldx..l + j * ldx + (n - l)],
                        ) / x[l + l * ldx];
                        // The read column l precedes the written column j.
                        let (head, tail) = x.split_at_mut(l + j * ldx);
                        daxpy(
                            t,
                            &head[l + l * ldx..l + l * ldx + (n - l)],
                            &mut tail[..(n - l)],
                        );
                        if qraux[j] != 0.0 {
                            let tt = 1.0 - (x[l + j * ldx] / qraux[j]).powi(2);
                            let tt = tt.max(0.0);
                            // Re-compute norms if large reduction
                            if tt.abs() >= 1e-6 {
                                qraux[j] *= tt.sqrt();
                            } else {
                                qraux[j] =
                                    dnrm2(&x[l + 1 + j * ldx..l + 1 + j * ldx + (n - l - 1)]);
                                work[j] = qraux[j];
                            }
                        }
                    }
                }

                // Save transformation
                qraux[l] = x[l + l * ldx];
                x[l + l * ldx] = -signed_norm;
            }
        }
        l += 1;
    }

    let rank = if k_val - 1 < n { k_val - 1 } else { n };
    *k = rank as c_int;
}

// ---------------------------------------------------------------------------
// dqrsl: Apply QR to solve least squares
// ---------------------------------------------------------------------------

/// Apply QR decomposition to solve least squares problems.
///
/// # Arguments
/// - `x`: output from dqrdc, dimension (ldx, k)
/// - `ldx`: leading dimension
/// - `n`: number of rows
/// - `k`: number of columns of the decomposed matrix
/// - `qraux`: auxiliary output from dqrdc
/// - `y`: n-vector to be manipulated
/// - `qy`: output, Q*y (if job >= 10000)
/// - `qty`: output, trans(Q)*y
/// - `b`: output, least squares solution (length k)
/// - `rsd`: output, residuals (length n)
/// - `xb`: output, least squares approximation (length n)
/// - `job`: specifies what to compute:
///   - 10000s digit: compute qy
///   - 1000s digit: compute qty
///   - 100s digit: compute b
///   - 10s digit: compute rsd
///   - 1s digit: compute xb
/// - `info`: 0 on success, index of first zero diagonal if singular
///
/// # Safety
/// Inputs and selected outputs must be valid for the documented extents.
/// Unselected outputs are ignored and may be null. `qty` is selected whenever
/// any of the low four job digits is nonzero. Selected buffers must not
/// overlap, except that `y` and `qty` may be exactly the same buffer.
pub unsafe fn dqrsl(
    x: *mut f64,
    ldx: c_int,
    n: c_int,
    k: c_int,
    qraux: *const f64,
    y: *const f64,
    qy: *mut f64,
    qty: *mut f64,
    b: *mut f64,
    rsd: *mut f64,
    xb: *mut f64,
    job: c_int,
    info: *mut c_int,
) {
    unsafe {
        let (ldx, n, k) = (ldx as usize, n as usize, k as usize);
        let outputs = [
            job / 10000 != 0,
            job % 10000 != 0,
            job % 1000 / 100 != 0,
            job % 100 / 10 != 0,
            job % 10 != 0,
        ];
        let [cqy, cqty, cb, cr, cxb] = outputs;
        // In the in-place form, qty already contains y. Form just one
        // reference to that buffer.
        let y = (!(cqty && y == qty.cast_const())).then(|| from_raw_parts(y, n));
        // SAFETY: Only selected outputs have to be backed by valid storage.
        dqrsl_impl(
            from_raw_parts_mut(x, ldx * k),
            ldx,
            n,
            k,
            from_raw_parts(qraux, k),
            y,
            if cqy { from_raw_parts_mut(qy, n) } else { &mut [] },
            if cqty { from_raw_parts_mut(qty, n) } else { &mut [] },
            if cb { from_raw_parts_mut(b, k) } else { &mut [] },
            if cr { from_raw_parts_mut(rsd, n) } else { &mut [] },
            if cxb { from_raw_parts_mut(xb, n) } else { &mut [] },
            outputs,
            &mut *info,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn dqrsl_impl(
    x: &mut [f64],
    ldx: usize,
    n: usize,
    k: usize,
    qraux: &[f64],
    y: Option<&[f64]>,
    qy: &mut [f64],
    qty: &mut [f64],
    b: &mut [f64],
    rsd: &mut [f64],
    xb: &mut [f64],
    [cqy, cqty, cb, cr, cxb]: [bool; 5],
    info: &mut c_int,
) {
    *info = 0;

    let ju = if k < n - 1 { k } else { n - 1 };

    // Special action when n == 1
    if ju == 0 {
        let y0 = || y.map_or_else(|| qty[0], |y| y[0]);
        let y0 = if cqy || cqty { y0() } else { 0.0 };
        if cqy {
            qy[0] = y0;
        }
        if cqty {
            qty[0] = y0;
        }
        if cxb {
            xb[0] = y0;
        }
        if !cb {
            if cr {
                rsd[0] = 0.0;
            }
            return;
        }
        if x[0] != 0.0 {
            b[0] = y0 / x[0];
        } else {
            *info = 1;
        }
        if cr {
            rsd[0] = 0.0;
        }
        return;
    }

    // Set up to compute qy or qty
    if let Some(y) = y {
        if cqy {
            dcopy(&y[..n], &mut qy[..n]);
        }
        if cqty {
            dcopy(&y[..n], &mut qty[..n]);
        }
    } else if cqy {
        dcopy(&qty[..n], &mut qy[..n]);
    }

    // Compute qy
    if cqy {
        let mut jj: usize = 0;
        while jj < ju {
            let j = ju - 1 - jj;
            if qraux[j] == 0.0 {
                jj += 1;
                continue;
            }
            let temp = x[j + j * ldx];
            x[j + j * ldx] = qraux[j];
            let t =
                -ddot(&x[j + j * ldx..j + j * ldx + (n - j)], &qy[j..j + (n - j)]) / x[j + j * ldx];
            daxpy(
                t,
                &x[j + j * ldx..j + j * ldx + (n - j)],
                &mut qy[j..j + (n - j)],
            );
            x[j + j * ldx] = temp;
            jj += 1;
        }
    }

    // Compute trans(q)*y
    if cqty {
        for j in 0..ju {
            if qraux[j] == 0.0 {
                continue;
            }
            let temp = x[j + j * ldx];
            x[j + j * ldx] = qraux[j];
            let t = -ddot(&x[j + j * ldx..j + j * ldx + (n - j)], &qty[j..j + (n - j)])
                / x[j + j * ldx];
            daxpy(
                t,
                &x[j + j * ldx..j + j * ldx + (n - j)],
                &mut qty[j..j + (n - j)],
            );
            x[j + j * ldx] = temp;
        }
    }

    // Set up to compute b, rsd, or xb
    if cb {
        dcopy(&qty[..k], &mut b[..k]);
    }
    if cxb {
        dcopy(&qty[..k], &mut xb[..k]);
    }
    if cr && k < n {
        dcopy(&qty[k..n], &mut rsd[k..n]);
    }
    if cxb && k < n {
        for i in k..n {
            xb[i] = 0.0;
        }
    }
    if cr {
        for i in 0..k {
            rsd[i] = 0.0;
        }
    }

    // Compute b
    if cb {
        let mut jj: usize = 0;
        while jj < k {
            let j = k - 1 - jj;
            if x[j + j * ldx] != 0.0 {
                b[j] = b[j] / x[j + j * ldx];
                if j != 0 {
                    let t = -b[j];
                    daxpy(t, &x[j * ldx..j * ldx + j], &mut b[..j]);
                }
            } else {
                *info = j as c_int;
                break;
            }
            jj += 1;
        }
    }

    // Compute rsd or xb
    if cr || cxb {
        let mut jj: usize = 0;
        while jj < ju {
            let j = ju - 1 - jj;
            if qraux[j] == 0.0 {
                jj += 1;
                continue;
            }
            let temp = x[j + j * ldx];
            x[j + j * ldx] = qraux[j];
            if cr {
                let t = -ddot(&x[j + j * ldx..j + j * ldx + (n - j)], &rsd[j..j + (n - j)])
                    / x[j + j * ldx];
                daxpy(
                    t,
                    &x[j + j * ldx..j + j * ldx + (n - j)],
                    &mut rsd[j..j + (n - j)],
                );
            }
            if cxb {
                let t = -ddot(&x[j + j * ldx..j + j * ldx + (n - j)], &xb[j..j + (n - j)])
                    / x[j + j * ldx];
                daxpy(
                    t,
                    &x[j + j * ldx..j + j * ldx + (n - j)],
                    &mut xb[j..j + (n - j)],
                );
            }
            x[j + j * ldx] = temp;
            jj += 1;
        }
    }
}
