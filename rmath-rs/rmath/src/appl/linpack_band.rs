#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]

/*
 * LINPACK banded Cholesky routines.
 * Ported from R's appl/dpbfa.f and appl/dpbsl.f
 *
 * Original LINPACK: Cleve Moler, University of New Mexico, Argonne National Lab.
 * Version dated 08/14/78.
 */

use std::os::raw::c_int;

// ---------------------------------------------------------------------------
// ddot: BLAS dot product
// ---------------------------------------------------------------------------

/// Compute dot product: sum(dx[i*incx] * dy[i*incy]) for i=0..n-1.
///
/// # Safety
/// `dx` and `dy` must be valid pointers for the accessed elements.
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

// ---------------------------------------------------------------------------
// daxpy: BLAS vector addition
// ---------------------------------------------------------------------------

/// Compute dy = dy + da * dx.
///
/// # Safety
/// `dx` and `dy` must be valid pointers for the accessed elements.
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

// ---------------------------------------------------------------------------
// dpbfa: Banded Cholesky factorization
// ---------------------------------------------------------------------------

/// Factor a symmetric positive definite matrix stored in band form.
///
/// On entry, abd contains the upper triangle in band storage. On return,
/// abd contains the upper triangular factor R such that A = R^T * R.
///
/// # Arguments (1-indexed Fortran, converted to 0-indexed Rust)
/// - `abd`: matrix in band storage, dimension (lda, n)
/// - `lda`: leading dimension of abd, lda >= m + 1
/// - `n`: order of the matrix
/// - `m`: number of diagonals above main diagonal
/// - `info`: 0 on success, k if leading minor of order k is not positive definite
///
/// # Safety
/// `abd` must be valid for lda*n elements. `info` must be a valid pointer.
pub unsafe fn dpbfa(abd: *mut f64, lda: c_int, n: c_int, m: c_int, info: *mut c_int) {
    let lda = lda as usize;
    let m = m as usize;
    let n = n as usize;

    let mut j: usize = 1; // 1-based
    while j <= n {
        *info = j as c_int;
        let mut s = 0.0f64;
        let mut ik = m + 1; // 1-based row in abd
        let mut jk = if j > m { j - m } else { 1 };
        let mu = if m + 2 > j { m + 2 - j } else { 1 }; // max(m+2-j, 1)

        if m >= mu {
            let mut k = mu;
            while k <= m {
                let t = *abd.add(k - 1 + (j - 1) * lda)
                    - ddot(
                        (k - mu) as c_int,
                        abd.add(ik - 1 + (jk - 1) * lda),
                        1,
                        abd.add(mu - 1 + (j - 1) * lda),
                        1,
                    );
                let t = t / *abd.add(m + (jk - 1) * lda);
                *abd.add(k - 1 + (j - 1) * lda) = t;
                s += t * t;
                ik -= 1;
                jk += 1;
                k += 1;
            }
        }

        s = *abd.add(m + (j - 1) * lda) - s;
        if s <= 0.0 {
            return; // info already set to j
        }

        *abd.add(m + (j - 1) * lda) = s.sqrt();
        j += 1;
    }
    *info = 0;
}

// ---------------------------------------------------------------------------
// dpbsl: Solve banded positive definite system
// ---------------------------------------------------------------------------

/// Solve A*x = b where A is factored by dpbfa.
///
/// # Arguments (1-indexed Fortran, converted to 0-indexed Rust)
/// - `abd`: factored matrix from dpbfa, dimension (lda, n)
/// - `lda`: leading dimension of abd
/// - `n`: order of the matrix
/// - `m`: number of diagonals above main diagonal
/// - `b`: right hand side vector, overwritten with solution
///
/// # Safety
/// `abd` must be valid for lda*n elements, `b` for n elements.
pub unsafe fn dpbsl(abd: *const f64, lda: c_int, n: c_int, m: c_int, b: *mut f64) {
    let lda = lda as usize;
    let m = m as usize;
    let n = n as usize;

    // Solve trans(R) * y = b
    let mut k: usize = 1;
    while k <= n {
        let lm = if k > m { m } else { k - 1 }; // min(k-1, m)
        let la = m + 1 - lm; // 1-based row in abd
        let lb = k - lm; // 1-based index in b
        let t = ddot(
            lm as c_int,
            abd.add(la - 1 + (k - 1) * lda),
            1,
            b.add(lb - 1),
            1,
        );
        *b.add(k - 1) = (*b.add(k - 1) - t) / *abd.add(m + (k - 1) * lda);
        k += 1;
    }

    // Solve R * x = y
    let mut kb: usize = 1;
    while kb <= n {
        let k = n + 1 - kb; // 1-based
        let lm = if k > m { m } else { k - 1 }; // min(k-1, m)
        let la = m + 1 - lm;
        let lb = k - lm;
        *b.add(k - 1) = *b.add(k - 1) / *abd.add(m + (k - 1) * lda);
        let t = -*b.add(k - 1);
        daxpy(
            lm as c_int,
            t,
            abd.add(la - 1 + (k - 1) * lda),
            1,
            b.add(lb - 1),
            1,
        );
        kb += 1;
    }
}
