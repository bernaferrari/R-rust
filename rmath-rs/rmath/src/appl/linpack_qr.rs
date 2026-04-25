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

// ---------------------------------------------------------------------------
// BLAS helpers (local to this module)
// ---------------------------------------------------------------------------

unsafe fn daxpy(n: c_int, da: f64, dx: *const f64, incx: c_int, dy: *mut f64, incy: c_int) {
    if n <= 0 || da == 0.0 {
        return;
    }
    let mut ix: isize = 0;
    let mut iy: isize = 0;
    for _ in 0..n {
        *dy.offset(iy) += da * *dx.offset(ix);
        ix += incx as isize;
        iy += incy as isize;
    }
}

unsafe fn ddot(n: c_int, dx: *const f64, incx: c_int, dy: *const f64, incy: c_int) -> f64 {
    let mut sum = 0.0f64;
    if n <= 0 {
        return sum;
    }
    let mut ix: isize = 0;
    let mut iy: isize = 0;
    for _ in 0..n {
        sum += *dx.offset(ix) * *dy.offset(iy);
        ix += incx as isize;
        iy += incy as isize;
    }
    sum
}

unsafe fn dscal(n: c_int, da: f64, dx: *mut f64, incx: c_int) {
    if n <= 0 || da == 1.0 {
        return;
    }
    let mut ix: isize = 0;
    for _ in 0..n {
        *dx.offset(ix) *= da;
        ix += incx as isize;
    }
}

unsafe fn dcopy(n: c_int, dx: *const f64, incx: c_int, dy: *mut f64, incy: c_int) {
    if n <= 0 {
        return;
    }
    let mut ix: isize = 0;
    let mut iy: isize = 0;
    for _ in 0..n {
        *dy.offset(iy) = *dx.offset(ix);
        ix += incx as isize;
        iy += incy as isize;
    }
}

/// Euclidean norm of a vector: sqrt(sum(x[i]^2))
unsafe fn dnrm2(n: c_int, x: *const f64, incx: c_int) -> f64 {
    if n <= 0 {
        return 0.0;
    }
    let mut ix: isize = 0;
    let mut ssq = 0.0f64;
    let mut scale = 0.0f64;
    for _ in 0..n {
        let xi = *x.offset(ix);
        if xi != 0.0 {
            if scale < xi.abs() {
                ssq = 1.0 + ssq * (scale / xi).powi(2);
                scale = xi.abs();
            } else {
                ssq += (xi / scale).powi(2);
            }
        }
        ix += incx as isize;
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
/// All pointers must be valid.
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
    let ldx = ldx as usize;
    let n = n as usize;
    let p = p as usize;

    // Compute norms of columns of x
    if n > 0 {
        for j in 0..p {
            let norm = dnrm2(n as c_int, x.add(j * ldx), 1);
            *qraux.add(j) = norm;
            *work.add(j) = norm;
            *work.add(p + j) = norm;
            if *work.add(p + j) == 0.0 {
                *work.add(p + j) = 1.0;
            }
        }
    }

    // Householder reduction
    let lup = if n < p { n } else { p };
    let mut k_val = p + 1usize;

    let mut l: usize = 0;
    while l < lup {
        // Cycle columns from l to p-1 to find one with non-negligible norm
        loop {
            if l >= k_val || *qraux.add(l) >= *work.add(p + l) * tol {
                break;
            }
            // Shift columns left
            for i in 0..n {
                let t = *x.add(i + l * ldx);
                for j in l..(p - 1) {
                    *x.add(i + j * ldx) = *x.add(i + (j + 1) * ldx);
                }
                *x.add(i + (p - 1) * ldx) = t;
            }
            // Shift jpvt, qraux, work
            let tmp_jpvt = *jpvt.add(l);
            let tmp_qraux = *qraux.add(l);
            let tmp_work1 = *work.add(l);
            let tmp_work2 = *work.add(p + l);
            for j in l..(p - 1) {
                *jpvt.add(j) = *jpvt.add(j + 1);
                *qraux.add(j) = *qraux.add(j + 1);
                *work.add(j) = *work.add(j + 1);
                *work.add(p + j) = *work.add(p + j + 1);
            }
            *jpvt.add(p - 1) = tmp_jpvt;
            *qraux.add(p - 1) = tmp_qraux;
            *work.add(p - 1) = tmp_work1;
            *work.add(2 * p - 1) = tmp_work2;
            k_val -= 1;
        }

        if l != n {
            // Compute Householder transformation for column l
            let nrmxl = dnrm2((n - l) as c_int, x.add(l + l * ldx), 1);
            if nrmxl != 0.0 {
                let sign = if *x.add(l + l * ldx) != 0.0 {
                    nrmxl.copysign(*x.add(l + l * ldx))
                } else {
                    nrmxl
                };
                dscal((n - l) as c_int, 1.0 / nrmxl, x.add(l + l * ldx), 1);
                *x.add(l + l * ldx) = 1.0 + *x.add(l + l * ldx);

                // Apply transformation to remaining columns
                if p > l {
                    for j in (l + 1)..p {
                        let t = -ddot(
                            (n - l) as c_int,
                            x.add(l + l * ldx),
                            1,
                            x.add(l + j * ldx),
                            1,
                        ) / *x.add(l + l * ldx);
                        daxpy(
                            (n - l) as c_int,
                            t,
                            x.add(l + l * ldx),
                            1,
                            x.add(l + j * ldx),
                            1,
                        );
                        if *qraux.add(j) != 0.0 {
                            let tt = 1.0 - (*x.add(l + j * ldx) / *qraux.add(j)).powi(2);
                            let tt = tt.max(0.0);
                            // Re-compute norms if large reduction
                            if tt.abs() >= 1e-6 {
                                *qraux.add(j) *= tt.sqrt();
                            } else {
                                *qraux.add(j) =
                                    dnrm2((n - l - 1) as c_int, x.add(l + 1 + j * ldx), 1);
                                *work.add(j) = *qraux.add(j);
                            }
                        }
                    }
                }

                // Save transformation
                *qraux.add(l) = *x.add(l + l * ldx);
                *x.add(l + l * ldx) = -nrmxl;
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
/// All pointers must be valid.
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
    let ldx = ldx as usize;
    let n = n as usize;
    let k = k as usize;

    *info = 0;

    let cqy = job / 10000 != 0;
    let cqty = job % 10000 != 0;
    let cb = job % 1000 / 100 != 0;
    let cr = job % 100 / 10 != 0;
    let cxb = job % 10 != 0;
    let ju = if k < n - 1 { k } else { n - 1 };

    // Special action when n == 1
    if ju == 0 {
        if cqy {
            *qy = *y;
        }
        if cqty {
            *qty = *y;
        }
        if cxb {
            *xb = *y;
        }
        if !cb {
            if cr {
                *rsd = 0.0;
            }
            return;
        }
        if *x != 0.0 {
            *b = *y / *x;
        } else {
            *info = 1;
        }
        if cr {
            *rsd = 0.0;
        }
        return;
    }

    // Set up to compute qy or qty
    if cqy {
        dcopy(n as c_int, y, 1, qy, 1);
    }
    if cqty {
        dcopy(n as c_int, y, 1, qty, 1);
    }

    // Compute qy
    if cqy {
        let mut jj: usize = 0;
        while jj < ju {
            let j = ju - jj;
            if *qraux.add(j) == 0.0 {
                continue;
            }
            let temp = *x.add(j + j * ldx);
            *x.add(j + j * ldx) = *qraux.add(j);
            let t =
                -ddot((n - j) as c_int, x.add(j + j * ldx), 1, qy.add(j), 1) / *x.add(j + j * ldx);
            daxpy((n - j) as c_int, t, x.add(j + j * ldx), 1, qy.add(j), 1);
            *x.add(j + j * ldx) = temp;
            jj += 1;
        }
    }

    // Compute trans(q)*y
    if cqty {
        for j in 0..ju {
            if *qraux.add(j) == 0.0 {
                continue;
            }
            let temp = *x.add(j + j * ldx);
            *x.add(j + j * ldx) = *qraux.add(j);
            let t =
                -ddot((n - j) as c_int, x.add(j + j * ldx), 1, qty.add(j), 1) / *x.add(j + j * ldx);
            daxpy((n - j) as c_int, t, x.add(j + j * ldx), 1, qty.add(j), 1);
            *x.add(j + j * ldx) = temp;
        }
    }

    // Set up to compute b, rsd, or xb
    if cb {
        dcopy(k as c_int, qty, 1, b, 1);
    }
    let kp1 = k + 1;
    if cxb {
        dcopy(k as c_int, qty, 1, xb, 1);
    }
    if cr && k < n {
        dcopy((n - k) as c_int, qty.add(kp1), 1, rsd.add(kp1), 1);
    }
    if cxb && kp1 <= n {
        for i in kp1..n {
            *xb.add(i) = 0.0;
        }
    }
    if cr {
        for i in 0..k {
            *rsd.add(i) = 0.0;
        }
    }

    // Compute b
    if cb {
        let mut jj: usize = 0;
        while jj < k {
            let j = k - 1 - jj;
            if *x.add(j + j * ldx) != 0.0 {
                *b.add(j) = *b.add(j) / *x.add(j + j * ldx);
                if j != 0 {
                    let t = -*b.add(j);
                    daxpy(j as c_int, t, x.add(j * ldx), 1, b, 1);
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
            let j = ju - jj;
            if *qraux.add(j) == 0.0 {
                continue;
            }
            let temp = *x.add(j + j * ldx);
            *x.add(j + j * ldx) = *qraux.add(j);
            if cr {
                let t = -ddot((n - j) as c_int, x.add(j + j * ldx), 1, rsd.add(j), 1)
                    / *x.add(j + j * ldx);
                daxpy((n - j) as c_int, t, x.add(j + j * ldx), 1, rsd.add(j), 1);
            }
            if cxb {
                let t = -ddot((n - j) as c_int, x.add(j + j * ldx), 1, xb.add(j), 1)
                    / *x.add(j + j * ldx);
                daxpy((n - j) as c_int, t, x.add(j + j * ldx), 1, xb.add(j), 1);
            }
            *x.add(j + j * ldx) = temp;
            jj += 1;
        }
    }
}
