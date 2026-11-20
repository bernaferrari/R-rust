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

/// Compute dot product: sum(dx[i] * dy[i]) for i=0..n.
///
/// `dx` and `dy` are the strided elements already gathered by the caller;
/// the LINPACK call sites all use unit stride.
fn ddot(dx: &[f64], dy: &[f64]) -> f64 {
    let mut sum = 0.0f64;
    for (&x, &y) in dx.iter().zip(dy.iter()) {
        sum += x * y;
    }
    sum
}

// ---------------------------------------------------------------------------
// daxpy: BLAS vector addition
// ---------------------------------------------------------------------------

/// Compute dy = dy + da * dx element-wise.
fn daxpy(da: f64, dx: &[f64], dy: &mut [f64]) {
    if da == 0.0 {
        return;
    }
    for (y, &x) in dy.iter_mut().zip(dx.iter()) {
        *y += da * x;
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
///
/// # Returns
/// 0 on success, k if the leading minor of order k is not positive definite.
pub fn dpbfa(abd: &mut [f64], lda: c_int, n: c_int, m: c_int) -> c_int {
    let lda = lda as usize;
    let m = m as usize;
    let n = n as usize;

    let mut j: usize = 1; // 1-based
    while j <= n {
        let mut s = 0.0f64;
        let mut ik = m + 1; // 1-based row in abd
        let mut jk = if j > m { j - m } else { 1 };
        let mu = if m + 2 > j { m + 2 - j } else { 1 }; // max(m+2-j, 1)

        if m >= mu {
            let mut k = mu;
            while k <= m {
                let t = abd[k - 1 + (j - 1) * lda]
                    - ddot(
                        &abd[ik - 1 + (jk - 1) * lda..ik - 1 + (jk - 1) * lda + (k - mu)],
                        &abd[mu - 1 + (j - 1) * lda..mu - 1 + (j - 1) * lda + (k - mu)],
                    );
                let t = t / abd[m + (jk - 1) * lda];
                abd[k - 1 + (j - 1) * lda] = t;
                s += t * t;
                ik -= 1;
                jk += 1;
                k += 1;
            }
        }

        s = abd[m + (j - 1) * lda] - s;
        if s <= 0.0 {
            return j as c_int; // leading minor of order j is not positive definite
        }

        abd[m + (j - 1) * lda] = s.sqrt();
        j += 1;
    }
    0
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
pub fn dpbsl(abd: &[f64], lda: c_int, n: c_int, m: c_int, b: &mut [f64]) {
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
            &abd[la - 1 + (k - 1) * lda..la - 1 + (k - 1) * lda + lm],
            &b[lb - 1..lb - 1 + lm],
        );
        b[k - 1] = (b[k - 1] - t) / abd[m + (k - 1) * lda];
        k += 1;
    }

    // Solve R * x = y
    let mut kb: usize = 1;
    while kb <= n {
        let k = n + 1 - kb; // 1-based
        let lm = if k > m { m } else { k - 1 }; // min(k-1, m)
        let la = m + 1 - lm;
        let lb = k - lm;
        b[k - 1] = b[k - 1] / abd[m + (k - 1) * lda];
        let t = -b[k - 1];
        daxpy(
            t,
            &abd[la - 1 + (k - 1) * lda..la - 1 + (k - 1) * lda + lm],
            &mut b[lb - 1..lb - 1 + lm],
        );
        kb += 1;
    }
}
