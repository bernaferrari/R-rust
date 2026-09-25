/*
 * Pure Rust LAPACK backend using faer 0.24.
 *
 * Provides the same function signatures as the Fortran FFI in lapack.rs,
 * dispatching to faer-rs for linear algebra computations.
 * Enabled via the `rust-backend` feature flag.
 */

use crate::modules::lapack::lapack::Rcomplex;
use crate::sexp::instance::with_current_instance;
use crate::sexp::memory::{TransientReservation, with_arena_in};
use faer::linalg::solvers::{DenseSolveCore, Solve};
use faer::{Mat, MatRef, Side, c64};
// ============================================================
// Helper functions
// ============================================================

/// Reserve the temporary buffers used by a numerical kernel when an R
/// session is active. Standalone low-level LAPACK tests intentionally run
/// without an ambient session and therefore retain the historical unlimited
/// behavior.
fn reserve_native_workspace(bytes: usize) -> Result<Option<TransientReservation>, ()> {
    match with_current_instance(|instance| {
        with_arena_in(instance, |arena| arena.try_reserve_transient(bytes))
    }) {
        None => Ok(None),
        Some(Some(reservation)) => Ok(Some(reservation)),
        Some(None) => Err(()),
    }
}

/// Read a column-major matrix (with optional lda stride) into a faer Mat.
unsafe fn read_mat_f64(ptr: *const f64, m: usize, n: usize, lda: usize) -> Mat<f64> {
    unsafe {
        let mut mat = Mat::zeros(m, n);
        for j in 0..n {
            for i in 0..m {
                mat[(i, j)] = *ptr.add(i + j * lda);
            }
        }
        mat
    }
}

/// Write real matrix data to column-major pointer (MatRef).
unsafe fn write_matref_f64(mat: MatRef<'_, f64>, ptr: *mut f64, m: usize, n: usize, lda: usize) {
    unsafe {
        for j in 0..n {
            for i in 0..m {
                *ptr.add(i + j * lda) = mat[(i, j)];
            }
        }
    }
}

/// Write real matrix (owned) to column-major pointer.
unsafe fn write_owned_f64(mat: &Mat<f64>, ptr: *mut f64, m: usize, n: usize, lda: usize) {
    unsafe {
        for j in 0..n {
            for i in 0..m {
                *ptr.add(i + j * lda) = mat[(i, j)];
            }
        }
    }
}

/// Write complex matrix data to column-major Rcomplex pointer (MatRef).
unsafe fn write_matref_c64(
    mat: MatRef<'_, c64>,
    ptr: *mut Rcomplex,
    m: usize,
    n: usize,
    lda: usize,
) {
    unsafe {
        for j in 0..n {
            for i in 0..m {
                let c = mat[(i, j)];
                *ptr.add(i + j * lda) = Rcomplex { r: c.re, i: c.im };
            }
        }
    }
}

/// Write complex matrix (owned) to column-major Rcomplex pointer.
unsafe fn write_owned_c64(mat: &Mat<c64>, ptr: *mut Rcomplex, m: usize, n: usize, lda: usize) {
    unsafe {
        for j in 0..n {
            for i in 0..m {
                let c = mat[(i, j)];
                *ptr.add(i + j * lda) = Rcomplex { r: c.re, i: c.im };
            }
        }
    }
}

/// Read a column-major complex matrix into a faer Mat<c64>.
unsafe fn read_mat_c64(ptr: *const Rcomplex, m: usize, n: usize, lda: usize) -> Mat<c64> {
    unsafe {
        let mut mat = Mat::zeros(m, n);
        for j in 0..n {
            for i in 0..m {
                let rc = *ptr.add(i + j * lda);
                mat[(i, j)] = c64::new(rc.r, rc.i);
            }
        }
        mat
    }
}

/// Extract the backward permutation from a PermRef as a Vec<usize>.
fn get_bwd_perm(p: faer::perm::PermRef<'_, usize>) -> Vec<usize> {
    let (_fwd, bwd) = p.arrays();
    bwd.iter().copied().collect()
}

/// Convert a faer permutation (backward map: permuted → original) to LAPACK's 1-based ipiv.
fn perm_bwd_to_ipiv(bwd: &[usize], n: usize) -> Vec<i32> {
    let mut current: Vec<usize> = (0..n).collect();
    let mut inv: Vec<usize> = (0..n).collect();
    let mut ipiv = vec![0i32; n];
    for k in 0..n {
        let target = bwd[k];
        let pos = inv[target];
        if pos != k {
            let other = current[k];
            current[k] = target;
            current[pos] = other;
            inv[target] = k;
            inv[other] = pos;
        }
        ipiv[k] = (pos + 1) as i32;
    }
    ipiv
}

// ============================================================
// Real LAPACK routines (d-prefixed)
// ============================================================

/// DLANGE — matrix norm (Frobenius / 1-norm / inf-norm / max element).
pub unsafe fn dlange_(
    norm: *const u8,
    m: *const core::ffi::c_int,
    n: *const core::ffi::c_int,
    a: *const f64,
    lda: *const core::ffi::c_int,
    _work: *mut f64,
) -> f64 {
    unsafe {
        let m = *m as usize;
        let n = *n as usize;
        let lda = *lda as usize;
        let norm_byte = *norm;

        match norm_byte {
            b'M' | b'm' => {
                // Max absolute value
                let mut max_val: f64 = 0.0;
                for j in 0..n {
                    for i in 0..m {
                        let v = (*a.add(i + j * lda)).abs();
                        if v.is_nan() || v > max_val {
                            max_val = v;
                        }
                    }
                }
                max_val
            }
            b'O' | b'o' | b'1' => {
                // 1-norm: max column sum of absolute values
                let mut result: f64 = 0.0;
                for j in 0..n {
                    let mut col_sum: f64 = 0.0;
                    for i in 0..m {
                        col_sum += (*a.add(i + j * lda)).abs();
                    }
                    result = result.max(col_sum);
                }
                result
            }
            b'I' | b'i' => {
                // Infinity-norm: max row sum of absolute values
                let mut row_sums = vec![0.0f64; m];
                for j in 0..n {
                    for i in 0..m {
                        row_sums[i] += (*a.add(i + j * lda)).abs();
                    }
                }
                row_sums.into_iter().fold(0.0f64, f64::max)
            }
            b'F' | b'f' | b'E' | b'e' => {
                // Frobenius norm, accumulated with DLANGE-style scaling
                // (scale/ssq) so that very large or very small entries neither
                // overflow nor underflow the running sum of squares.
                let mut scale: f64 = 0.0;
                let mut ssq: f64 = 1.0;
                for j in 0..n {
                    for i in 0..m {
                        let v = (*a.add(i + j * lda)).abs();
                        if v != 0.0 {
                            if scale < v {
                                ssq = 1.0 + ssq * (scale / v) * (scale / v);
                                scale = v;
                            } else {
                                ssq += (v / scale) * (v / scale);
                            }
                        }
                    }
                }
                scale * ssq.sqrt()
            }
            _ => 0.0,
        }
    }
}

/// DGETRF — LU factorization with partial pivoting.
pub unsafe fn dgetrf_(
    m: *const core::ffi::c_int,
    n: *const core::ffi::c_int,
    a: *mut f64,
    lda: *const core::ffi::c_int,
    ipiv: *mut core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let m = *m as usize;
        let n = *n as usize;
        let lda = *lda as usize;

        if m == 0 || n == 0 {
            *info = 0;
            return;
        }

        let mat = read_mat_f64(a, m, n, lda);
        let lu = mat.partial_piv_lu();
        let l = lu.L();
        let u = lu.U();
        let p = lu.P();

        // Write combined L\U matrix back to a
        let k = m.min(n);
        // Write the combined L\U factor back to a, LAPACK-style:
        //   strict lower triangle: L (M x k), k = min(M, N)
        //   diagonal and strict upper: U (k x N)
        // This handles M != N: in the tall case (M > N) rows i >= N below column
        // j < N still carry L entries; in the wide case (N > M) columns j >= M
        // carry only U entries.
        let u_ncols = u.ncols();
        for j in 0..n {
            for i in 0..m {
                let val = if i > j {
                    // Strict lower triangle: from L
                    if j < k { l[(i, j)] } else { 0.0 }
                } else if i < k && j < u_ncols {
                    // Upper triangle (including diagonal): from U
                    u[(i, j)]
                } else {
                    0.0
                };
                *a.add(i + j * lda) = val;
            }
        }

        // Write pivot array. Decompose over the full row permutation, then
        // write only min(M, N) entries: LAPACK DGETRF's IPIV has dimension
        // min(M, N), and its swap sequence only covers the column steps —
        // permutation entries beyond that are identity no-ops.
        let bwd = get_bwd_perm(p);
        let pivots = perm_bwd_to_ipiv(&bwd, m.min(bwd.len()));
        for (i, &p) in pivots.iter().take(m.min(n)).enumerate() {
            *ipiv.add(i) = p;
        }
        let mut info_val = 0;
        for i in 0..k {
            if u[(i, i)].abs() == 0.0 {
                info_val = (i + 1) as core::ffi::c_int;
                break;
            }
        }
        *info = info_val;
    }
}

/// DGESV — solve Ax = B via LU.
pub unsafe fn dgesv_(
    n: *const core::ffi::c_int,
    nrhs: *const core::ffi::c_int,
    a: *mut f64,
    lda: *const core::ffi::c_int,
    ipiv: *mut core::ffi::c_int,
    b: *mut f64,
    ldb: *const core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let n = *n as usize;
        let nrhs = *nrhs as usize;
        let lda_val = *lda as usize;
        let ldb_val = *ldb as usize;

        if n == 0 {
            *info = 0;
            return;
        }

        for k in 0..n {
            let mut piv = k;
            let mut max = (*a.add(k + k * lda_val)).abs();
            for i in (k + 1)..n {
                let v = (*a.add(i + k * lda_val)).abs();
                if v > max {
                    max = v;
                    piv = i;
                }
            }
            *ipiv.add(k) = (piv + 1) as core::ffi::c_int;
            if piv != k {
                for j in 0..n {
                    let pa = a.add(k + j * lda_val);
                    let pb = a.add(piv + j * lda_val);
                    let tmp = *pa;
                    *pa = *pb;
                    *pb = tmp;
                }
            }
            let diag = *a.add(k + k * lda_val);
            if diag == 0.0 {
                *info = (k + 1) as core::ffi::c_int;
                return;
            }
            for i in (k + 1)..n {
                let lik = *a.add(i + k * lda_val) / diag;
                *a.add(i + k * lda_val) = lik;
                for j in (k + 1)..n {
                    let u = *a.add(k + j * lda_val);
                    *a.add(i + j * lda_val) -= lik * u;
                }
            }
        }
        for rhs in 0..nrhs {
            let col = b.add(rhs * ldb_val);
            for k in 0..n {
                let piv = (*ipiv.add(k) as usize) - 1;
                if piv != k {
                    let tmp = *col.add(k);
                    *col.add(k) = *col.add(piv);
                    *col.add(piv) = tmp;
                }
            }
            for i in 0..n {
                let mut s = *col.add(i);
                for k in 0..i {
                    s -= *a.add(i + k * lda_val) * *col.add(k);
                }
                *col.add(i) = s;
            }
            for i in (0..n).rev() {
                let mut s = *col.add(i);
                for k in (i + 1)..n {
                    s -= *a.add(i + k * lda_val) * *col.add(k);
                }
                *col.add(i) = s / *a.add(i + i * lda_val);
            }
        }
        *info = 0;
    }
}

/// DPOTRF — Cholesky factorization.
pub unsafe fn dpotrf_(
    uplo: *const u8,
    n: *const core::ffi::c_int,
    a: *mut f64,
    lda: *const core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let n = *n as usize;
        let lda = *lda as usize;
        let uplo_byte = *uplo;

        if n == 0 {
            *info = 0;
            return;
        }
        // Stock DPOTF2 flags a non-positive OR NaN pivot: report the first
        // NaN diagonal element as the failing minor (1-based column).
        if let Some(i) = (0..n).find(|&i| (*a.add(i + i * lda)).is_nan()) {
            *info = (i + 1) as core::ffi::c_int;
            return;
        }

        let mat = read_mat_f64(a, n, n, lda);
        let side = if uplo_byte == b'U' || uplo_byte == b'u' {
            Side::Upper
        } else {
            Side::Lower
        };

        match mat.llt(side) {
            Ok(llt) => {
                let l = llt.L();
                match uplo_byte {
                    b'U' | b'u' => {
                        // Write L^T (upper triangular) to upper triangle
                        for j in 0..n {
                            for i in 0..=j {
                                *a.add(i + j * lda) = l[(j, i)];
                            }
                            // Zero lower triangle
                            for i in (j + 1)..n {
                                *a.add(i + j * lda) = 0.0;
                            }
                        }
                    }
                    _ => {
                        // Write L (lower triangular) to lower triangle
                        for j in 0..n {
                            for i in j..n {
                                *a.add(i + j * lda) = l[(i, j)];
                            }
                            // Zero upper triangle
                            for i in 0..j {
                                *a.add(i + j * lda) = 0.0;
                            }
                        }
                    }
                }
                *info = 0;
            }
            Err(_) => {
                // The matrix is not positive definite. Reproduce the factorization
                // manually to identify the exact 1-based failing minor, as stock
                // DPOTRF does: info = i when the leading principal minor of order i
                // is not positive definite (i.e. the computed diagonal would be <= 0).
                let upper = matches!(uplo_byte, b'U' | b'u');
                let mut l = Mat::zeros(n, n);
                let mut failed = 0usize;
                'outer: for i in 0..n {
                    for j in 0..=i {
                        let mut s = 0.0;
                        for kk in 0..j {
                            s += l[(i, kk)] * l[(j, kk)];
                        }
                        let aij = if upper { mat[(j, i)] } else { mat[(i, j)] };
                        if i == j {
                            let d = aij - s;
                            if !(d > 0.0) {
                                failed = i + 1;
                                break 'outer;
                            }
                            l[(i, i)] = d.sqrt();
                        } else {
                            l[(i, j)] = (aij - s) / l[(j, j)];
                        }
                    }
                }
                *info = failed as core::ffi::c_int;
            }
        }
    }
}

/// DPOTRI — inverse from a Cholesky factor via DTRTRI + DLAUUM.
///
/// Stock LAPACK never reconstructs `A = UᵀU` / `LLᵀ`. Reconstruction
/// squares a factor near `1e155` out of binary64, while `U⁻¹U⁻ᵀ` stays
/// representable near `1e-310`. Only the `uplo` triangle is written.
pub unsafe fn dpotri_(
    uplo: *const u8,
    n: *const core::ffi::c_int,
    a: *mut f64,
    lda: *const core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let n = *n as usize;
        let lda = *lda as usize;
        let upper = matches!(*uplo, b'U' | b'u');

        if n == 0 {
            *info = 0;
            return;
        }

        for i in 0..n {
            if *a.add(i + i * lda) == 0.0 {
                *info = (i + 1) as core::ffi::c_int;
                return;
            }
        }

        let mut factor = vec![0.0f64; n * n];
        if upper {
            for j in 0..n {
                for i in 0..=j {
                    factor[i + j * n] = *a.add(i + j * lda);
                }
            }
            let inv = invert_upper_tri(&factor, n);
            let prod = multiply_upper_by_transpose(&inv, n);
            for j in 0..n {
                for i in 0..=j {
                    *a.add(i + j * lda) = prod[i + j * n];
                }
            }
        } else {
            for j in 0..n {
                for i in j..n {
                    factor[i + j * n] = *a.add(i + j * lda);
                }
            }
            let inv = invert_lower_tri(&factor, n);
            let prod = multiply_lower_transpose_by_self(&inv, n);
            for j in 0..n {
                for i in j..n {
                    *a.add(i + j * lda) = prod[i + j * n];
                }
            }
        }
        *info = 0;
    }
}

/// DTRTRI (non-unit upper): return `U⁻¹` in the upper triangle.
fn invert_upper_tri(u: &[f64], n: usize) -> Vec<f64> {
    let mut inv = vec![0.0f64; n * n];
    for j in 0..n {
        inv[j + j * n] = 1.0 / u[j + j * n];
        for i in (0..j).rev() {
            let mut s = 0.0;
            for k in (i + 1)..=j {
                s += u[i + k * n] * inv[k + j * n];
            }
            inv[i + j * n] = -s / u[i + i * n];
        }
    }
    inv
}

/// DTRTRI (non-unit lower): return `L⁻¹` in the lower triangle.
fn invert_lower_tri(l: &[f64], n: usize) -> Vec<f64> {
    let mut inv = vec![0.0f64; n * n];
    for j in 0..n {
        inv[j + j * n] = 1.0 / l[j + j * n];
        for i in (j + 1)..n {
            let mut s = 0.0;
            for k in j..i {
                s += l[i + k * n] * inv[k + j * n];
            }
            inv[i + j * n] = -s / l[i + i * n];
        }
    }
    inv
}

/// DLAUUM upper: `U Uᵀ` in the upper triangle.
fn multiply_upper_by_transpose(inv: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![0.0f64; n * n];
    for j in 0..n {
        for i in 0..=j {
            let mut s = 0.0;
            for k in j..n {
                s += inv[i + k * n] * inv[j + k * n];
            }
            out[i + j * n] = s;
        }
    }
    out
}

/// DLAUUM lower: `Lᵀ L` in the lower triangle.
fn multiply_lower_transpose_by_self(inv: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![0.0f64; n * n];
    for j in 0..n {
        for i in j..n {
            let mut s = 0.0;
            for k in i..n {
                s += inv[k + i * n] * inv[k + j * n];
            }
            out[i + j * n] = s;
        }
    }
    out
}



/// DPSTRF — pivoted Cholesky factorization.
pub unsafe fn dpstrf_(
    uplo: *const u8,
    n: *const core::ffi::c_int,
    a: *mut f64,
    lda: *const core::ffi::c_int,
    piv: *mut core::ffi::c_int,
    rank: *mut core::ffi::c_int,
    tol: *const f64,
    _work: *mut f64,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        *info = 0;
        let upper = matches!(*uplo, b'U' | b'u');
        if !upper && !matches!(*uplo, b'L' | b'l') {
            *info = -1;
            return;
        }
        if *n < 0 {
            *info = -2;
            return;
        }
        if *lda < (*n).max(1) {
            *info = -4;
            return;
        }
        let n = *n as usize;
        let lda = *lda as usize;
        *rank = 0;
        if n == 0 {
            return;
        }
        // Read only the selected triangle. The other triangle may be poison.
        let original = Mat::<f64>::from_fn(n, n, |i, j| {
            let (row, col) = if upper {
                (i.min(j), i.max(j))
            } else {
                (i.max(j), i.min(j))
            };
            *a.add(row + col * lda)
        });
        let mut order: Vec<usize> = (0..n).collect();
        let mut factor = Mat::<f64>::zeros(n, n);
        let max_diagonal = (0..n).map(|i| original[(i, i)]).fold(0.0, f64::max);
        let threshold = if *tol < 0.0 {
            n as f64 * f64::EPSILON * max_diagonal
        } else {
            *tol
        };
        for k in 0..n {
            let residual = |i: usize| {
                original[(order[i], order[i])]
                    - (0..k).map(|j| factor[(i, j)] * factor[(i, j)]).sum::<f64>()
            };
            let mut pivot = k;
            let mut diagonal = residual(k);
            for i in k + 1..n {
                let candidate = residual(i);
                if candidate > diagonal {
                    diagonal = candidate;
                    pivot = i;
                }
            }
            order.swap(k, pivot);
            for j in 0..k {
                let saved = factor[(k, j)];
                factor[(k, j)] = factor[(pivot, j)];
                factor[(pivot, j)] = saved;
            }
            if diagonal <= threshold || !diagonal.is_finite() {
                factor[(k, k)] = diagonal;
                *info = 1;
                break;
            }
            factor[(k, k)] = diagonal.sqrt();
            for i in k + 1..n {
                let dot = (0..k).map(|j| factor[(i, j)] * factor[(k, j)]).sum::<f64>();
                factor[(i, k)] = (original[(order[i], order[k])] - dot) / factor[(k, k)];
            }
            *rank += 1;
        }
        for i in 0..n {
            *piv.add(i) = (order[i] + 1) as core::ffi::c_int;
        }
        for j in 0..n {
            for i in j..n {
                if upper {
                    *a.add(j + i * lda) = factor[(i, j)];
                } else {
                    *a.add(i + j * lda) = factor[(i, j)];
                }
            }
        }
    }
}

/// DGESDD — SVD.
pub unsafe fn dgesdd_(
    jobz: *const u8,
    m: *const core::ffi::c_int,
    n: *const core::ffi::c_int,
    a: *mut f64,
    lda: *const core::ffi::c_int,
    s: *mut f64,
    u: *mut f64,
    ldu: *const core::ffi::c_int,
    vt: *mut f64,
    ldvt: *const core::ffi::c_int,
    work: *mut f64,
    lwork: *const core::ffi::c_int,
    _iwork: *mut core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let m = *m as usize;
        let n = *n as usize;
        let lda = *lda as usize;
        let ldu_val = *ldu as usize;
        let ldvt_val = *ldvt as usize;
        let jobz_byte = *jobz;
        let lwork_val = *lwork;

        if m == 0 || n == 0 {
            *info = 0;
            return;
        }

        // Workspace query
        if lwork_val == -1 {
            let min_mn = m.min(n);
            let max_mn = m.max(n);
            *work = (3_usize * min_mn * min_mn
                + max_mn.max(4_usize * min_mn * min_mn + 4_usize * min_mn))
                as f64;
            *info = 0;
            return;
        }

        let mat = read_mat_f64(a, m, n, lda);
        let min_mn = m.min(n);

        let svals = match mat.singular_values() {
            Ok(v) => v,
            Err(_) => {
                *info = 1;
                return;
            }
        };

        // Write singular values
        for i in 0..min_mn {
            *s.add(i) = svals[i];
        }

        match jobz_byte {
            b'N' | b'n' => {
                // Only singular values
            }
            _ => {
                // Compute full or thin SVD
                let svd = match mat.svd() {
                    Ok(s) => s,
                    Err(_) => {
                        *info = 1;
                        return;
                    }
                };
                let u_mat = svd.U();
                let v_mat = svd.V();

                match jobz_byte {
                    b'A' | b'a' => {
                        // Full U (m×m), Full VT (n×n)
                        write_matref_f64(u_mat, u, m, m, ldu_val);
                        let vt_mat = v_mat.transpose();
                        write_matref_f64(vt_mat, vt, n, n, ldvt_val);
                    }
                    b'S' | b's' | _ => {
                        // Thin: U (m×min), VT (min×n)
                        let mut u_thin = Mat::zeros(m, min_mn);
                        for j in 0..min_mn {
                            for i in 0..m {
                                u_thin[(i, j)] = u_mat[(i, j)];
                            }
                        }
                        write_owned_f64(&u_thin, u, m, min_mn, ldu_val);

                        let mut vt_thin = Mat::zeros(min_mn, n);
                        let v_t = v_mat.transpose();
                        for j in 0..n {
                            for i in 0..min_mn {
                                vt_thin[(i, j)] = v_t[(i, j)];
                            }
                        }
                        write_owned_f64(&vt_thin, vt, min_mn, n, ldvt_val);
                    }
                }
            }
        }
        *info = 0;
    }
}

/// DSYEVR — symmetric eigenvalue decomposition with range selection.
pub unsafe fn dsyevr_(
    jobz: *const u8,
    range: *const u8,
    uplo: *const u8,
    n: *const core::ffi::c_int,
    a: *mut f64,
    lda: *const core::ffi::c_int,
    vl: *const f64,
    vu: *const f64,
    il: *const core::ffi::c_int,
    iu: *const core::ffi::c_int,
    _abstol: *const f64,
    m: *mut core::ffi::c_int,
    w: *mut f64,
    z: *mut f64,
    ldz: *const core::ffi::c_int,
    isuppz: *mut core::ffi::c_int,
    work: *mut f64,
    lwork: *const core::ffi::c_int,
    _iwork: *mut core::ffi::c_int,
    liwork: *const core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let ldz_val = *ldz as usize;
        let jobz_byte = *jobz;
        let range_byte = *range;
        let uplo_byte = *uplo;
        let lwork_val = *lwork;
        let liwork_val = *liwork;

        if n_val == 0 {
            *m = 0;
            *info = 0;
            return;
        }

        // Workspace query
        if lwork_val == -1 || liwork_val == -1 {
            if lwork_val == -1 {
                *work = (26 * n_val) as f64;
            }
            if liwork_val == -1 {
                *_iwork = (10 * n_val) as core::ffi::c_int;
            }
            *info = 0;
            return;
        }

        let side = if uplo_byte == b'U' || uplo_byte == b'u' {
            Side::Upper
        } else {
            Side::Lower
        };

        let mat = read_mat_f64(a, n_val, n_val, lda_val);

        // Compute all eigenvalues
        let all_evals = match mat.self_adjoint_eigenvalues(side) {
            Ok(v) => v,
            Err(_) => {
                *info = 1;
                return;
            }
        };

        // Filter by range
        let selected: Vec<usize> = match range_byte {
            b'A' | b'a' => (0..n_val).collect(),
            b'V' | b'v' => {
                let vl_val = *vl;
                let vu_val = *vu;
                (0..n_val)
                    .filter(|&i| all_evals[i] >= vl_val && all_evals[i] <= vu_val)
                    .collect()
            }
            b'I' | b'i' => {
                let il_val = *il as usize;
                let iu_val = *iu as usize;
                ((il_val - 1)..iu_val.min(n_val)).collect()
            }
            _ => (0..n_val).collect(),
        };

        *m = selected.len() as core::ffi::c_int;

        // Write eigenvalues
        for (idx, &i) in selected.iter().enumerate() {
            *w.add(idx) = all_evals[i];
        }

        // Compute eigenvectors if requested
        if jobz_byte == b'V' || jobz_byte == b'v' {
            let eigen = match mat.self_adjoint_eigen(side) {
                Ok(e) => e,
                Err(_) => {
                    *info = 1;
                    return;
                }
            };
            let evecs = eigen.U();

            for (idx, &i) in selected.iter().enumerate() {
                for j in 0..n_val {
                    *z.add(j + idx * ldz_val) = evecs[(j, i)];
                }
                // isuppz: estimate support (conservative: full range)
                *isuppz.add(2 * idx) = 1;
                *isuppz.add(2 * idx + 1) = n_val as core::ffi::c_int;
            }
        }
        *info = 0;
    }
}

/// DGEEV — general eigenvalue decomposition.
pub unsafe fn dgeev_(
    jobvl: *const u8,
    jobvr: *const u8,
    n: *const core::ffi::c_int,
    a: *mut f64,
    lda: *const core::ffi::c_int,
    wr: *mut f64,
    wi: *mut f64,
    vl: *mut f64,
    ldvl: *const core::ffi::c_int,
    vr: *mut f64,
    ldvr: *const core::ffi::c_int,
    work: *mut f64,
    lwork: *const core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let ldvl_val = *ldvl as usize;
        let ldvr_val = *ldvr as usize;
        let jobvl_byte = *jobvl;
        let jobvr_byte = *jobvr;
        let lwork_val = *lwork;

        if n_val == 0 {
            *info = 0;
            return;
        }

        // Workspace query
        if lwork_val == -1 {
            *work = (4 * n_val * n_val + 2 * n_val) as f64;
            *info = 0;
            return;
        }

        let mat = read_mat_f64(a, n_val, n_val, lda_val);

        // Compute complex eigenvalues and eigenvectors
        let eigen = match mat.eigen() {
            Ok(e) => e,
            Err(_) => {
                *info = 1;
                return;
            }
        };

        let evals_c64: Vec<c64> = match mat.eigenvalues() {
            Ok(v) => v,
            Err(_) => {
                *info = 1;
                return;
            }
        };
        let evecs_c64 = eigen.U();

        // Sort eigenvalues: by real part, then by imaginary part (positive first)
        let mut indices: Vec<usize> = (0..n_val).collect();
        indices.sort_by(|&a, &b| {
            let ea = evals_c64[a];
            let eb = evals_c64[b];
            ea.re.partial_cmp(&eb.re).unwrap().then_with(|| {
                // Positive imaginary first, then negative
                eb.im.partial_cmp(&ea.im).unwrap()
            })
        });

        // Write eigenvalues in LAPACK format (wr, wi)
        let mut j = 0;
        while j < n_val {
            let idx = indices[j];
            let ev = evals_c64[idx];
            if ev.im.abs() < 1e-15 * ev.re.abs().max(1.0) {
                // Real eigenvalue
                *wr.add(j) = ev.re;
                *wi.add(j) = 0.0;
                j += 1;
            } else {
                // Complex conjugate pair
                let idx2 = if j + 1 < n_val { indices[j + 1] } else { idx };
                let ev2 = evals_c64[idx2];

                // Make sure first has positive imaginary part
                if ev.im > 0.0 {
                    *wr.add(j) = ev.re;
                    *wi.add(j) = ev.im;
                    *wr.add(j + 1) = ev2.re;
                    *wi.add(j + 1) = -ev.im;
                } else {
                    *wr.add(j) = ev.re;
                    *wi.add(j) = -ev.im;
                    *wr.add(j + 1) = ev2.re;
                    *wi.add(j + 1) = ev.im;
                }
                j += 2;
            }
        }

        // Write right eigenvectors if requested
        if jobvr_byte == b'V' || jobvr_byte == b'v' {
            // Convert complex eigenvectors to LAPACK real format
            let mut j = 0;
            while j < n_val {
                let idx = indices[j];
                let ev = evals_c64[idx];

                if ev.im.abs() < 1e-15 * ev.re.abs().max(1.0) {
                    // Real eigenvector
                    for i in 0..n_val {
                        *vr.add(i + j * ldvr_val) = evecs_c64[(i, idx)].re;
                    }
                    j += 1;
                } else {
                    // Complex pair: real part in col j, imag part in col j+1
                    let imag_sign = if ev.im > 0.0 { 1.0 } else { -1.0 };
                    for i in 0..n_val {
                        let c = evecs_c64[(i, idx)];
                        *vr.add(i + j * ldvr_val) = c.re;
                        *vr.add(i + (j + 1) * ldvr_val) = c.im * imag_sign;
                    }
                    j += 2;
                }
            }
        }

        // Left eigenvectors not typically requested (jobvl='N')
        if jobvl_byte == b'V' || jobvl_byte == b'v' {
            // Zero out for now (R doesn't typically request these)
            for j in 0..n_val {
                for i in 0..n_val {
                    *vl.add(i + j * ldvl_val) = 0.0;
                }
            }
        }
        *info = 0;
    }
}

/// DGEQP3 — QR factorization with column pivoting.
pub unsafe fn dgeqp3_(
    m: *const core::ffi::c_int,
    n: *const core::ffi::c_int,
    a: *mut f64,
    lda: *const core::ffi::c_int,
    jpvt: *mut core::ffi::c_int,
    tau: *mut f64,
    work: *mut f64,
    lwork: *const core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        *info = 0;
        if *m < 0 {
            *info = -1;
            return;
        }
        if *n < 0 {
            *info = -2;
            return;
        }
        if *lda < (*m).max(1) {
            *info = -4;
            return;
        }
        let lwork_val = *lwork;
        if lwork_val < -1 {
            *info = -8;
            return;
        }
        let m_val = *m as usize;
        let n_val = *n as usize;
        let lda_val = *lda as usize;

        let minimum_work = 3i64 * i64::from(*n) + 1;
        if lwork_val != -1 && i64::from(lwork_val) < minimum_work {
            *info = -8;
            return;
        }

        // Workspace query
        if lwork_val == -1 {
            let Some(matrix_elems) = m_val.checked_mul(n_val) else {
                *info = -1;
                return;
            };
            let Some(query) = (3usize)
                .checked_mul(n_val)
                .and_then(|value| value.checked_add(1))
                .map(|value| value.max(matrix_elems))
            else {
                *info = -2;
                return;
            };
            *work = query as f64;
            *info = 0;
            return;
        }

        if m_val == 0 || n_val == 0 {
            *work = 1.0;
            return;
        }
        let Some(matrix_elems) = m_val.checked_mul(n_val) else {
            *info = -1;
            return;
        };
        let Some(workspace_elems) = matrix_elems
            .checked_add(n_val)
            .and_then(|value| value.checked_add(m_val))
        else {
            *info = -1;
            return;
        };
        let Some(workspace_bytes) = workspace_elems.checked_mul(std::mem::size_of::<f64>()) else {
            *info = -1;
            return;
        };
        let Ok(_workspace_reservation) = reserve_native_workspace(workspace_bytes) else {
            // The high-level LAPACK adapter maps any nonzero INFO to the
            // standard recoverable R error. Keep this distinct from success.
            *info = -100;
            return;
        };

        let k = m_val.min(n_val);

        // Read matrix into a flat column-major buffer we can modify
        let mut buf = vec![0.0f64; matrix_elems];
        for j in 0..n_val {
            for i in 0..m_val {
                buf[i + j * m_val] = *a.add(i + j * lda_val);
            }
        }

        // LAPACK: a nonzero jpvt entry is a fixed column and stays in place.
        // Only jpvt == 0 columns are free to move. lm() fills 1..p, so a
        // full-rank model must not swap the intercept with a larger predictor.
        let fixed: Vec<bool> = (0..n_val).map(|j| *jpvt.add(j) != 0).collect();
        for j in 0..n_val {
            if *jpvt.add(j) == 0 {
                *jpvt.add(j) = (j + 1) as core::ffi::c_int;
            }
        }

        // Compute column squared norms
        let mut col_norms_sq = vec![0.0f64; n_val];
        for j in 0..n_val {
            let mut sum = 0.0;
            for i in 0..m_val {
                sum += buf[i + j * m_val] * buf[i + j * m_val];
            }
            col_norms_sq[j] = sum;
        }

        // Householder QR with column pivoting
        for jj in 0..k {
            crate::eval::limits::poll_computation();
            // Find pivot column (largest remaining free norm)
            let mut max_norm = -1.0f64;
            let mut pivot = jj;
            if !fixed[jj] {
                for j in jj..n_val {
                    if fixed[j] {
                        continue;
                    }
                    if col_norms_sq[j] > max_norm {
                        max_norm = col_norms_sq[j];
                        pivot = j;
                    }
                }
            }

            // Swap columns jj and pivot
            if pivot != jj {
                for i in 0..m_val {
                    buf.swap(i + jj * m_val, i + pivot * m_val);
                }
                col_norms_sq.swap(jj, pivot);
                let tmp = *jpvt.add(jj);
                *jpvt.add(jj) = *jpvt.add(pivot);
                *jpvt.add(pivot) = tmp;
            }

            // Compute Householder reflection for column jj, rows jj..m
            let remaining = m_val - jj;
            if remaining == 0 {
                *tau.add(jj) = 0.0;
                continue;
            }

            // Extract the vector x = buf[jj:m, jj]
            let mut x = vec![0.0f64; remaining];
            for i in 0..remaining {
                x[i] = buf[jj + i + jj * m_val];
            }

            let norm_x = {
                let mut sum = 0.0;
                for &v in &x {
                    sum += v * v;
                }
                sum.sqrt()
            };

            if norm_x == 0.0 {
                *tau.add(jj) = 0.0;
                continue;
            }

            let alpha = x[0];
            let sign = if alpha >= 0.0 { 1.0 } else { -1.0 };
            let beta = -sign
                * (alpha * alpha + {
                    let mut s = 0.0;
                    for i in 1..remaining {
                        s += x[i] * x[i];
                    }
                    s
                })
                .sqrt();

            // Compute Householder vector and tau
            let u1 = alpha - beta;
            if u1 == 0.0 {
                *tau.add(jj) = 0.0;
                buf[jj + jj * m_val] = beta;
                continue;
            }

            // Normalize: v = x / u1, v[0] = 1
            // Reuse x: the reservation includes one Householder vector, not two.
            let mut v = x;
            v[0] = 1.0;
            for i in 1..remaining {
                v[i] /= u1;
            }

            // tau = -u1 / beta
            let tau_val = -u1 / beta;

            // Store R diagonal and Householder vector below diagonal
            buf[jj + jj * m_val] = beta;
            for i in 1..remaining {
                buf[jj + i + jj * m_val] = v[i];
            }
            *tau.add(jj) = tau_val;

            // Apply reflection to remaining columns
            for col in (jj + 1)..n_val {
                // w = v^T * buf[jj:m, col]
                let mut w = buf[jj + col * m_val]; // v[0] = 1
                for i in 1..remaining {
                    w += v[i] * buf[jj + i + col * m_val];
                }
                w *= tau_val;

                // buf[jj:m, col] -= w * v
                buf[jj + col * m_val] -= w; // v[0] = 1
                for i in 1..remaining {
                    buf[jj + i + col * m_val] -= w * v[i];
                }
            }

            // Update column norms
            for col in (jj + 1)..n_val {
                col_norms_sq[col] -= buf[jj + col * m_val] * buf[jj + col * m_val];
                if col_norms_sq[col] < 0.0 {
                    col_norms_sq[col] = 0.0;
                }
            }
        }

        // Write back to a
        for j in 0..n_val {
            for i in 0..m_val {
                *a.add(i + j * lda_val) = buf[i + j * m_val];
            }
        }

        // Zero out unused tau entries
        for j in k..n_val {
            *tau.add(j) = 0.0;
        }
        *info = 0;
    }
}

#[cfg(test)]
mod dgeqp3_safety_tests {
    use super::*;
    use crate::sexp::memory::ArenaBudget;
    use crate::sexp::session::RSession;

    #[test]
    fn rejects_negative_dimensions_before_pointer_access() {
        let m = -1;
        let n = 2;
        let lda = 1;
        let lwork = -1;
        let mut work = [0.0];
        let mut info = 0;
        unsafe {
            dgeqp3_(
                &m,
                &n,
                std::ptr::null_mut(),
                &lda,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                work.as_mut_ptr(),
                &lwork,
                &mut info,
            );
        }
        assert_eq!(info, -1);
    }

    #[test]
    fn budget_denial_returns_lapack_error_and_keeps_query_available() {
        let session = RSession::new();
        session.with_active(|| unsafe {
            crate::sexp::memory::with_arena(|arena| {
                arena.set_budget(ArenaBudget::new(1, 0));
            });
            let m = 2;
            let n = 2;
            let lda = 2;
            let mut info = 0;
            let mut query = [0.0];
            let query_lwork = -1;
            dgeqp3_(
                &m,
                &n,
                std::ptr::null_mut(),
                &lda,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                query.as_mut_ptr(),
                &query_lwork,
                &mut info,
            );
            assert_eq!(info, 0);
            assert!(query[0] >= 4.0);

            let mut work = [0.0; 16];
            let lwork = 16;
            dgeqp3_(
                &m,
                &n,
                std::ptr::null_mut(),
                &lda,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                work.as_mut_ptr(),
                &lwork,
                &mut info,
            );
            assert_eq!(info, -100);
            crate::sexp::memory::with_arena(|arena| arena.set_budget(ArenaBudget::new(0, 0)));
            let mut matrix = [1.0, 0.0, 0.0, 2.0];
            let mut pivots = [0, 0];
            let mut tau = [0.0, 0.0];
            dgeqp3_(
                &m,
                &n,
                matrix.as_mut_ptr(),
                &lda,
                pivots.as_mut_ptr(),
                tau.as_mut_ptr(),
                work.as_mut_ptr(),
                &lwork,
                &mut info,
            );
            assert_eq!(info, 0);
            assert_eq!(pivots, [2, 1]);
        });
    }
}

/// DORMQR — apply Q from QR factorization to a matrix.
pub unsafe fn dormqr_(
    side: *const u8,
    trans: *const u8,
    m: *const core::ffi::c_int,
    n: *const core::ffi::c_int,
    k: *const core::ffi::c_int,
    a: *const f64,
    lda: *const core::ffi::c_int,
    tau: *const f64,
    c__: *mut f64,
    ldc: *const core::ffi::c_int,
    work: *mut f64,
    lwork: *const core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let m_val = *m as usize;
        let n_val = *n as usize;
        let k_val = *k as usize;
        let lda_val = *lda as usize;
        let ldc_val = *ldc as usize;
        let side_byte = *side;
        let trans_byte = *trans;
        let lwork_val = *lwork;

        if m_val == 0 || n_val == 0 || k_val == 0 {
            *info = 0;
            return;
        }

        // Workspace query
        if lwork_val == -1 {
            *work = (m_val * n_val) as f64;
            *info = 0;
            return;
        }

        let is_left = side_byte == b'L' || side_byte == b'l';
        let is_trans = trans_byte == b'T' || trans_byte == b't';

        // Apply Householder reflections from the QR factorization
        if is_left {
            // C = Q * C or C = Q^T * C
            let range: Vec<usize> = if is_trans {
                (0..k_val).rev().collect()
            } else {
                (0..k_val).collect()
            };

            for j in range {
                let tau_j = *tau.add(j);
                if tau_j == 0.0 {
                    continue;
                }

                // Householder vector v = [1, a[j+1:m, j]] stored in column j
                let remaining = m_val - j;

                for col in 0..n_val {
                    // w = v^T * C[j:m, col]
                    let mut w = *c__.add(j + col * ldc_val); // v[0] = 1
                    for i in 1..remaining {
                        w += *a.add(j + i + j * lda_val) * *c__.add(j + i + col * ldc_val);
                    }
                    w *= tau_j;

                    // C[j:m, col] -= w * v
                    *c__.add(j + col * ldc_val) -= w;
                    for i in 1..remaining {
                        *c__.add(j + i + col * ldc_val) -= w * *a.add(j + i + j * lda_val);
                    }
                }
            }
        } else {
            // Right: C = C * Q or C = C * Q^T
            let range: Vec<usize> = if !is_trans {
                (0..k_val).rev().collect()
            } else {
                (0..k_val).collect()
            };

            for j in range {
                let tau_j = *tau.add(j);
                if tau_j == 0.0 {
                    continue;
                }

                let remaining = n_val - j;

                for row in 0..m_val {
                    // w = C[row, j:n] * v
                    let mut w = *c__.add(row + j * ldc_val); // v[0] = 1
                    for i in 1..remaining {
                        w += *c__.add(row + (j + i) * ldc_val) * *a.add(j + i + j * lda_val);
                    }
                    w *= tau_j;

                    *c__.add(row + j * ldc_val) -= w;
                    for i in 1..remaining {
                        *c__.add(row + (j + i) * ldc_val) -= w * *a.add(j + i + j * lda_val);
                    }
                }
            }
        }
        *info = 0;
    }
}

/// DGECON — condition number estimate.
pub unsafe fn dgecon_(
    norm: *const u8,
    n: *const core::ffi::c_int,
    a: *const f64,
    lda: *const core::ffi::c_int,
    anorm: *const f64,
    rcond: *mut f64,
    _work: *mut f64,
    _iwork: *mut core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        *info = 0;
        *rcond = 0.0;
        if !matches!(*norm, b'1' | b'O' | b'o' | b'I' | b'i') {
            *info = -1;
            return;
        }
        if *n < 0 {
            *info = -2;
            return;
        }
        if *lda < (*n).max(1) {
            *info = -4;
            return;
        }
        if *anorm < 0.0 {
            *info = -5;
            return;
        }
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let anorm_val = *anorm;
        let _norm_byte = *norm;

        if n_val == 0 {
            *rcond = 1.0;
            *info = 0;
            return;
        }

        if anorm_val == 0.0 {
            *rcond = 0.0;
            *info = 0;
            return;
        }

        // A contains packed LU, not the original matrix. Solve L U X = I.
        // The omitted pivot only permutes inverse columns, leaving both
        // the one-norm and infinity-norm unchanged.
        let mut inverse = Mat::<f64>::from_fn(n_val, n_val, |i, j| if i == j { 1.0 } else { 0.0 });
        for j in 0..n_val {
            for i in 0..n_val {
                for k in 0..i {
                    inverse[(i, j)] -= *a.add(i + k * lda_val) * inverse[(k, j)];
                }
            }
            for i in (0..n_val).rev() {
                for k in i + 1..n_val {
                    inverse[(i, j)] -= *a.add(i + k * lda_val) * inverse[(k, j)];
                }
                inverse[(i, j)] /= *a.add(i + i * lda_val);
            }
        }
        let inf = matches!(*norm, b'I' | b'i');
        let inv_norm = (0..n_val)
            .map(|j| {
                (0..n_val)
                    .map(|i| {
                        if inf {
                            inverse[(j, i)].abs()
                        } else {
                            inverse[(i, j)].abs()
                        }
                    })
                    .sum::<f64>()
            })
            .fold(0.0, f64::max);
        *rcond = (1.0 / inv_norm) / anorm_val;
        *info = 0;
    }
}

/// DTRCON — triangular condition number.
pub unsafe fn dtrcon_(
    _norm: *const u8,
    uplo: *const u8,
    diag: *const u8,
    n: *const core::ffi::c_int,
    a: *const f64,
    lda: *const core::ffi::c_int,
    rcond: *mut f64,
    _work: *mut f64,
    _iwork: *mut core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let norm_byte = *_norm;
        let inf_norm = norm_byte == b'I' || norm_byte == b'i';
        let is_upper = *uplo == b'U' || *uplo == b'u';
        let is_unit = *diag == b'U' || *diag == b'u';
        *info = 0;
        if n_val == 0 {
            *rcond = 1.0;
            return;
        }
        let aij = |i: usize, j: usize| -> f64 {
            if i == j && is_unit {
                return 1.0;
            }
            let stored = if is_upper { i <= j } else { i >= j };
            if stored { *a.add(i + j * lda_val) } else { 0.0 }
        };
        let mut anorm = 0.0f64;
        if inf_norm {
            for i in 0..n_val {
                let mut s = 0.0f64;
                for j in 0..n_val {
                    s += aij(i, j).abs();
                }
                anorm = anorm.max(s);
            }
        } else {
            for j in 0..n_val {
                let mut s = 0.0f64;
                for i in 0..n_val {
                    s += aij(i, j).abs();
                }
                anorm = anorm.max(s);
            }
        }
        if anorm == 0.0 {
            *rcond = 0.0;
            return;
        }
        // Solve op(T) y = x. trans: apply T^T.
        let mut solve = |x: &mut [f64], trans: bool| {
            let upper = if trans { !is_upper } else { is_upper };
            if upper {
                for i in (0..n_val).rev() {
                    let mut s = x[i];
                    for j in (i + 1)..n_val {
                        let c = if trans { aij(j, i) } else { aij(i, j) };
                        s -= c * x[j];
                    }
                    let d = if is_unit { 1.0 } else if trans { aij(i, i) } else { aij(i, i) };
                    x[i] = if d == 0.0 { 0.0 } else { s / d };
                }
            } else {
                for i in 0..n_val {
                    let mut s = x[i];
                    for j in 0..i {
                        let c = if trans { aij(j, i) } else { aij(i, j) };
                        s -= c * x[j];
                    }
                    let d = if is_unit { 1.0 } else { aij(i, i) };
                    x[i] = if d == 0.0 { 0.0 } else { s / d };
                }
            }
        };
        // DLACN2 on inv(T). KASE 1: inv(T)*x, KASE 2: inv(T)^T*x.
        let mut x = vec![0.0f64; n_val];
        let mut v = vec![0.0f64; n_val];
        let mut isgn = vec![0i32; n_val];
        let mut isave = [0i32; 3];
        let mut est = 0.0f64;
        let mut kase = 0i32;
        let sgn = |z: f64| if z >= 0.0 { 1.0 } else { -1.0 };
        for _ in 0..n_val.saturating_mul(8).max(16) {
            if kase == 0 {
                let s = 1.0 / n_val as f64;
                x.fill(s);
                kase = 1;
                isave[0] = 1;
            } else {
                match isave[0] {
                    1 => {
                        if n_val == 1 {
                            est = x[0].abs();
                            kase = 0;
                        } else {
                            est = x.iter().map(|z| z.abs()).sum();
                            for i in 0..n_val {
                                x[i] = sgn(x[i]);
                                isgn[i] = x[i] as i32;
                            }
                            kase = 2;
                            isave[0] = 2;
                        }
                    }
                    2 => {
                        isave[1] = x.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).map(|(i, _)| i).unwrap_or(0) as i32;
                        isave[2] = 2;
                        x.fill(0.0);
                        x[isave[1] as usize] = 1.0;
                        kase = 1;
                        isave[0] = 3;
                    }
                    3 => {
                        v.copy_from_slice(&x);
                        let estold = est;
                        est = v.iter().map(|z| z.abs()).sum();
                        let repeated = (0..n_val).all(|i| (sgn(x[i]) as i32) == isgn[i]);
                        if repeated || est <= estold {
                            let mut altsgn = 1.0f64;
                            for i in 0..n_val {
                                x[i] = altsgn * ((i as f64) / (n_val as f64 - 1.0) + 1.0);
                                altsgn = -altsgn;
                            }
                            kase = 1;
                            isave[0] = 5;
                        } else {
                            for i in 0..n_val {
                                x[i] = sgn(x[i]);
                                isgn[i] = x[i] as i32;
                            }
                            kase = 2;
                            isave[0] = 4;
                        }
                    }
                    4 => {
                        let jlast = isave[1] as usize;
                        isave[1] = x.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).map(|(i, _)| i).unwrap_or(0) as i32;
                        if x[jlast] != x[isave[1] as usize].abs() && isave[2] < 5 {
                            isave[2] += 1;
                            x.fill(0.0);
                            x[isave[1] as usize] = 1.0;
                            kase = 1;
                            isave[0] = 3;
                        } else {
                            let mut altsgn = 1.0f64;
                            for i in 0..n_val {
                                x[i] = altsgn * ((i as f64) / (n_val as f64 - 1.0) + 1.0);
                                altsgn = -altsgn;
                            }
                            kase = 1;
                            isave[0] = 5;
                        }
                    }
                    _ => {
                        let temp = x.iter().map(|z| z.abs()).sum::<f64>() / (n_val as f64 * 3.0) * 2.0;
                        if temp > est {
                            est = temp;
                        }
                        kase = 0;
                    }
                }
            }
            if kase == 0 {
                break;
            }
            solve(&mut x, kase == 2);
        }
        *rcond = if est == 0.0 { 0.0 } else { 1.0 / (anorm * est) };
    }
}

/// DTRTRS — triangular solve.
pub unsafe fn dtrtrs_(
    uplo: *const u8,
    trans: *const u8,
    diag: *const u8,
    n: *const core::ffi::c_int,
    nrhs: *const core::ffi::c_int,
    a: *const f64,
    lda: *const core::ffi::c_int,
    b: *mut f64,
    ldb: *const core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let n_val = *n as usize;
        let nrhs_val = *nrhs as usize;
        let lda_val = *lda as usize;
        let ldb_val = *ldb as usize;
        let uplo_byte = *uplo;
        let trans_byte = *trans;
        let diag_byte = *diag;

        if n_val == 0 {
            *info = 0;
            return;
        }

        let is_upper = uplo_byte == b'U' || uplo_byte == b'u';
        let is_trans = trans_byte == b'T' || trans_byte == b't';
        let is_unit = diag_byte == b'U' || diag_byte == b'u';

        // Check for zero diagonal
        if !is_unit {
            for i in 0..n_val {
                if *a.add(i + i * lda_val) == 0.0 {
                    *info = (i + 1) as core::ffi::c_int;
                    return;
                }
            }
        }

        // Read A as triangular matrix
        let a_mat = {
            let mut m = Mat::zeros(n_val, n_val);
            for j in 0..n_val {
                for i in 0..n_val {
                    let val = if is_upper {
                        if i <= j { *a.add(i + j * lda_val) } else { 0.0 }
                    } else {
                        if i >= j { *a.add(i + j * lda_val) } else { 0.0 }
                    };
                    m[(i, j)] = val;
                }
            }
            if is_unit {
                for i in 0..n_val {
                    m[(i, i)] = 1.0;
                }
            }
            m
        };

        let b_mat = read_mat_f64(b, n_val, nrhs_val, ldb_val);

        // Solve using faer's triangular solve
        let tri = a_mat.partial_piv_lu();
        let x = if is_trans {
            // A^T x = B
            let at = a_mat.transpose();
            at.partial_piv_lu().solve(&b_mat)
        } else {
            tri.solve(&b_mat)
        };

        write_owned_f64(&x, b, n_val, nrhs_val, ldb_val);
        *info = 0;
    }
}

// ============================================================
// Complex LAPACK routines (z-prefixed)
// ============================================================

/// ZLANGE — complex matrix norm.
pub unsafe fn zlange_(
    norm: *const u8,
    m: *const core::ffi::c_int,
    n: *const core::ffi::c_int,
    a: *const Rcomplex,
    lda: *const core::ffi::c_int,
    _work: *mut f64,
) -> f64 {
    unsafe {
        let m = *m as usize;
        let n = *n as usize;
        let lda = *lda as usize;
        let norm_byte = *norm;

        let abs_val = |rc: Rcomplex| (rc.r * rc.r + rc.i * rc.i).sqrt();

        match norm_byte {
            b'M' | b'm' => {
                let mut max_val: f64 = 0.0;
                for j in 0..n {
                    for i in 0..m {
                        max_val = max_val.max(abs_val(*a.add(i + j * lda)));
                    }
                }
                max_val
            }
            b'O' | b'o' | b'1' => {
                let mut result: f64 = 0.0;
                for j in 0..n {
                    let mut col_sum: f64 = 0.0;
                    for i in 0..m {
                        col_sum += abs_val(*a.add(i + j * lda));
                    }
                    result = result.max(col_sum);
                }
                result
            }
            b'I' | b'i' => {
                let mut row_sums = vec![0.0f64; m];
                for j in 0..n {
                    for i in 0..m {
                        row_sums[i] += abs_val(*a.add(i + j * lda));
                    }
                }
                row_sums.into_iter().fold(0.0f64, f64::max)
            }
            b'F' | b'f' | b'E' | b'e' => {
                let mut sum_sq: f64 = 0.0;
                for j in 0..n {
                    for i in 0..m {
                        let rc = *a.add(i + j * lda);
                        sum_sq += rc.r * rc.r + rc.i * rc.i;
                    }
                }
                sum_sq.sqrt()
            }
            _ => 0.0,
        }
    }
}

/// ZGETRF — complex LU factorization.
pub unsafe fn zgetrf_(
    m: *const core::ffi::c_int,
    n: *const core::ffi::c_int,
    a: *mut Rcomplex,
    lda: *const core::ffi::c_int,
    ipiv: *mut core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let m = *m as usize;
        let n = *n as usize;
        let lda = *lda as usize;

        if m == 0 || n == 0 {
            *info = 0;
            return;
        }

        let mat = read_mat_c64(a, m, n, lda);
        let lu = mat.partial_piv_lu();
        let l = lu.L();
        let u = lu.U();
        let p = lu.P();

        let k = m.min(n);
        for j in 0..n {
            for i in 0..m {
                let val = if i > j {
                    if i < k && j < k {
                        l[(i, j)]
                    } else {
                        c64::new(0.0, 0.0)
                    }
                } else if j < u.ncols() && i < u.nrows() {
                    u[(i, j)]
                } else {
                    c64::new(0.0, 0.0)
                };
                *a.add(i + j * lda) = Rcomplex {
                    r: val.re,
                    i: val.im,
                };
            }
        }

        let bwd = get_bwd_perm(p);
        // Same IPIV dimension contract as DGETRF: decompose over the full
        // row permutation, write only the min(M, N) column-step entries.
        let pivots = perm_bwd_to_ipiv(&bwd, m.min(bwd.len()));
        for (i, &p) in pivots.iter().take(m.min(n)).enumerate() {
            *ipiv.add(i) = p;
        }
        let k = m.min(n);
        let mut info_val = 0;
        for i in 0..k {
            let d = u[(i, i)];
            if d.re == 0.0 && d.im == 0.0 {
                info_val = (i + 1) as core::ffi::c_int;
                break;
            }
        }
        *info = info_val;
    }
}

/// ZGESV — complex linear solve.
pub unsafe fn zgesv_(
    n: *const core::ffi::c_int,
    nrhs: *const core::ffi::c_int,
    a: *mut Rcomplex,
    lda: *const core::ffi::c_int,
    ipiv: *mut core::ffi::c_int,
    b: *mut Rcomplex,
    ldb: *const core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let n_val = *n as usize;
        let nrhs_val = *nrhs as usize;
        let lda_val = *lda as usize;
        let ldb_val = *ldb as usize;

        if n_val == 0 {
            *info = 0;
            return;
        }

        let a_mat = read_mat_c64(a, n_val, n_val, lda_val);
        let b_mat = read_mat_c64(b, n_val, nrhs_val, ldb_val);

        let lu = a_mat.partial_piv_lu();
        let u = lu.U();
        for i in 0..n_val {
            if u[(i, i)].re == 0.0 && u[(i, i)].im == 0.0 {
                *info = (i + 1) as core::ffi::c_int;
                return;
            }
        }

        let x = lu.solve(&b_mat);
        write_owned_c64(&x, b, n_val, nrhs_val, ldb_val);

        let l = lu.L();
        for j in 0..n_val {
            for i in 0..n_val {
                let val = if i > j { l[(i, j)] } else { u[(i, j)] };
                *a.add(i + j * lda_val) = Rcomplex {
                    r: val.re,
                    i: val.im,
                };
            }
        }
        let bwd = get_bwd_perm(lu.P());
        let pivots = perm_bwd_to_ipiv(&bwd, n_val);
        for (i, &p) in pivots.iter().enumerate() {
            *ipiv.add(i) = p;
        }
        *info = 0;
    }
}

/// ZGESDD — complex SVD.
pub unsafe fn zgesdd_(
    jobz: *const u8,
    m: *const core::ffi::c_int,
    n: *const core::ffi::c_int,
    a: *mut Rcomplex,
    lda: *const core::ffi::c_int,
    s: *mut f64,
    u: *mut Rcomplex,
    ldu: *const core::ffi::c_int,
    vt: *mut Rcomplex,
    ldvt: *const core::ffi::c_int,
    work: *mut Rcomplex,
    lwork: *const core::ffi::c_int,
    _rwork: *mut f64,
    _iwork: *mut core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let m_val = *m as usize;
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let ldu_val = *ldu as usize;
        let ldvt_val = *ldvt as usize;
        let jobz_byte = *jobz;
        let lwork_val = *lwork;

        if m_val == 0 || n_val == 0 {
            *info = 0;
            return;
        }

        // Workspace query
        if lwork_val == -1 {
            let min_mn = m_val.min(n_val);
            let tmp = Rcomplex {
                r: (2 * min_mn * min_mn + 2 * min_mn + m_val.max(n_val)) as f64,
                i: 0.0,
            };
            *work = tmp;
            *info = 0;
            return;
        }

        let mat = read_mat_c64(a, m_val, n_val, lda_val);
        let min_mn = m_val.min(n_val);

        let svals = match mat.singular_values() {
            Ok(v) => v,
            Err(_) => {
                *info = 1;
                return;
            }
        };

        for i in 0..min_mn {
            *s.add(i) = svals[i];
        }

        if jobz_byte != b'N' && jobz_byte != b'n' {
            let svd = match mat.svd() {
                Ok(s) => s,
                Err(_) => {
                    *info = 1;
                    return;
                }
            };
            let u_mat = svd.U();
            let v_mat = svd.V();

            match jobz_byte {
                b'A' | b'a' => {
                    write_matref_c64(u_mat, u, m_val, m_val, ldu_val);
                    let vt_mat = v_mat.adjoint();
                    let vt_owned = vt_mat.to_owned();
                    write_owned_c64(&vt_owned, vt, n_val, n_val, ldvt_val);
                }
                b'S' | b's' | _ => {
                    let mut u_thin = Mat::zeros(m_val, min_mn);
                    for j in 0..min_mn {
                        for i in 0..m_val {
                            u_thin[(i, j)] = u_mat[(i, j)];
                        }
                    }
                    write_owned_c64(&u_thin, u, m_val, min_mn, ldu_val);

                    let v_h = v_mat.adjoint();
                    let v_h_owned = v_h.to_owned();
                    let mut vt_thin = Mat::zeros(min_mn, n_val);
                    for j in 0..n_val {
                        for i in 0..min_mn {
                            vt_thin[(i, j)] = v_h_owned[(i, j)];
                        }
                    }
                    write_owned_c64(&vt_thin, vt, min_mn, n_val, ldvt_val);
                }
            }
        }
        *info = 0;
    }
}

/// ZHEEV — Hermitian eigenvalue decomposition.
pub unsafe fn zheev_(
    jobz: *const u8,
    uplo: *const u8,
    n: *const core::ffi::c_int,
    a: *mut Rcomplex,
    lda: *const core::ffi::c_int,
    w: *mut f64,
    work: *mut Rcomplex,
    lwork: *const core::ffi::c_int,
    _rwork: *mut f64,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let jobz_byte = *jobz;
        let uplo_byte = *uplo;
        let lwork_val = *lwork;

        if n_val == 0 {
            *info = 0;
            return;
        }

        // Workspace query
        if lwork_val == -1 {
            *work = Rcomplex {
                r: (2 * n_val + n_val * n_val) as f64,
                i: 0.0,
            };
            *info = 0;
            return;
        }

        let side = if uplo_byte == b'U' || uplo_byte == b'u' {
            Side::Upper
        } else {
            Side::Lower
        };

        let mat = read_mat_c64(a, n_val, n_val, lda_val);

        // Get eigenvalues (always real for Hermitian)
        let evals = match mat.self_adjoint_eigenvalues(side) {
            Ok(v) => v,
            Err(_) => {
                *info = 1;
                return;
            }
        };

        // Write eigenvalues
        for i in 0..n_val {
            *w.add(i) = evals[i];
        }

        // Compute eigenvectors if requested
        if jobz_byte == b'V' || jobz_byte == b'v' {
            let eigen = match mat.self_adjoint_eigen(side) {
                Ok(e) => e,
                Err(_) => {
                    *info = 1;
                    return;
                }
            };
            let evecs = eigen.U();
            write_matref_c64(evecs, a, n_val, n_val, lda_val);
        }
        *info = 0;
    }
}

/// ZGEEV — complex general eigenvalue decomposition.
pub unsafe fn zgeev_(
    jobvl: *const u8,
    jobvr: *const u8,
    n: *const core::ffi::c_int,
    a: *mut Rcomplex,
    lda: *const core::ffi::c_int,
    w: *mut Rcomplex,
    vl: *mut Rcomplex,
    ldvl: *const core::ffi::c_int,
    vr: *mut Rcomplex,
    ldvr: *const core::ffi::c_int,
    work: *mut Rcomplex,
    lwork: *const core::ffi::c_int,
    _rwork: *mut f64,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let ldvl_val = *ldvl as usize;
        let ldvr_val = *ldvr as usize;
        let jobvr_byte = *jobvr;
        let lwork_val = *lwork;

        if n_val == 0 {
            *info = 0;
            return;
        }

        // Workspace query
        if lwork_val == -1 {
            *work = Rcomplex {
                r: (2 * n_val * n_val + n_val) as f64,
                i: 0.0,
            };
            *info = 0;
            return;
        }

        let mat = read_mat_c64(a, n_val, n_val, lda_val);

        let eigen = match mat.eigen() {
            Ok(e) => e,
            Err(_) => {
                *info = 1;
                return;
            }
        };

        let evals: Vec<c64> = match mat.eigenvalues() {
            Ok(v) => v,
            Err(_) => {
                *info = 1;
                return;
            }
        };
        let evecs = eigen.U();

        // Write eigenvalues
        for i in 0..n_val {
            *w.add(i) = Rcomplex {
                r: evals[i].re,
                i: evals[i].im,
            };
        }

        // Write right eigenvectors
        if jobvr_byte == b'V' || jobvr_byte == b'v' {
            write_matref_c64(evecs, vr, n_val, n_val, ldvr_val);
        }

        // Left eigenvectors (not typically requested)
        if *jobvl == b'V' || *jobvl == b'v' {
            for j in 0..n_val {
                for i in 0..n_val {
                    *vl.add(i + j * ldvl_val) = Rcomplex { r: 0.0, i: 0.0 };
                }
            }
        }
        *info = 0;
    }
}

/// ZGEQP3 — complex QR with column pivoting.
pub unsafe fn zgeqp3_(
    m: *const core::ffi::c_int,
    n: *const core::ffi::c_int,
    a: *mut Rcomplex,
    lda: *const core::ffi::c_int,
    jpvt: *mut core::ffi::c_int,
    tau: *mut Rcomplex,
    work: *mut Rcomplex,
    lwork: *const core::ffi::c_int,
    _rwork: *mut f64,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let lwork_val = *lwork;

        // Validate the signed LAPACK arguments before converting them to
        // usize.  In particular, a negative dimension must be reported as an
        // argument error rather than becoming a giant allocation request.
        if *m < 0 {
            *info = -1;
            return;
        }
        if *n < 0 {
            *info = -2;
            return;
        }
        if *lda < (*m).max(1) {
            *info = -4;
            return;
        }
        if lwork_val < -1 {
            *info = -8;
            return;
        }
        // LAPACK ZGEQP3 requires N+1 complex work values for nonempty
        // matrices, and one for empty matrices (RWORK separately has 2*N).
        let minimum_work = if *m == 0 || *n == 0 {
            1
        } else {
            i64::from(*n) + 1
        };
        if lwork_val != -1 && i64::from(lwork_val) < minimum_work {
            *info = -8;
            return;
        }

        let Some(m_val) = usize::try_from(*m).ok() else {
            *info = -1;
            return;
        };
        let Some(n_val) = usize::try_from(*n).ok() else {
            *info = -2;
            return;
        };
        let Some(lda_val) = usize::try_from(*lda).ok() else {
            *info = -4;
            return;
        };

        // This unblocked implementation does not require a larger work array.
        *work = Rcomplex {
            r: minimum_work as f64,
            i: 0.0,
        };
        if lwork_val == -1 || m_val == 0 || n_val == 0 {
            *info = 0;
            return;
        }
        let Some(matrix_elems) = m_val.checked_mul(n_val) else {
            *info = -1;
            return;
        };

        let k = m_val.min(n_val);

        // Account for every local Vec held concurrently below: the copied
        // matrix, column norms, and the x/v Householder work vectors.  The
        // caller-owned LAPACK work arrays are not included here; this
        // reservation covers the additional native scratch before allocating
        // any of it.
        let Some(matrix_bytes) = matrix_elems.checked_mul(std::mem::size_of::<c64>()) else {
            *info = -1;
            return;
        };
        let Some(norm_bytes) = n_val.checked_mul(std::mem::size_of::<f64>()) else {
            *info = -1;
            return;
        };
        let Some(householder_elems) = m_val.checked_mul(2) else {
            *info = -1;
            return;
        };
        let Some(householder_bytes) = householder_elems.checked_mul(std::mem::size_of::<c64>())
        else {
            *info = -1;
            return;
        };
        let Some(workspace_bytes) = matrix_bytes
            .checked_add(norm_bytes)
            .and_then(|bytes| bytes.checked_add(householder_bytes))
        else {
            *info = -1;
            return;
        };
        let Ok(_workspace_reservation) = reserve_native_workspace(workspace_bytes) else {
            *info = -100;
            return;
        };

        // Read into buffer
        let mut buf = vec![c64::new(0.0, 0.0); matrix_elems];
        for j in 0..n_val {
            for i in 0..m_val {
                let rc = *a.add(i + j * lda_val);
                buf[i + j * m_val] = c64::new(rc.r, rc.i);
            }
        }

        // Initialize jpvt
        for j in 0..n_val {
            if *jpvt.add(j) == 0 {
                *jpvt.add(j) = (j + 1) as core::ffi::c_int;
            }
        }

        // Reference LAPACK ZLAQP2 (unblocked ZGEQP3 path): full column
        // norms in VN1/VN2, first-max pivoting, ZLARFG reflectors with
        // real beta, H(i)**H application through conjugated tau, and the
        // LAPACK Working Note 176 partial norm downdate.
        let mut vn1 = vec![0.0f64; n_val];
        let mut vn2 = vec![0.0f64; n_val];
        for j in 0..n_val {
            let mut sum = 0.0;
            for i in 0..m_val {
                let c = buf[i + j * m_val];
                sum += c.re * c.re + c.im * c.im;
            }
            vn1[j] = sum.sqrt();
            vn2[j] = vn1[j];
        }
        let tol3z = f64::EPSILON.sqrt();

        for i in 0..k {
            // IDAMAX over VN1(i:n) keeps the first maximum.
            let mut pivot = i;
            let mut best = vn1[i];
            for j in (i + 1)..n_val {
                if vn1[j] > best {
                    best = vn1[j];
                    pivot = j;
                }
            }
            if pivot != i {
                for r in 0..m_val {
                    buf.swap(r + i * m_val, r + pivot * m_val);
                }
                vn1.swap(i, pivot);
                vn2.swap(i, pivot);
                let tmp = *jpvt.add(i);
                *jpvt.add(i) = *jpvt.add(pivot);
                *jpvt.add(pivot) = tmp;
            }

            // ZLARFG on buf[i:m, i]: xnorm covers the tail only, and beta
            // is real with sign opposite to Re(alpha).
            let alpha = buf[i + i * m_val];
            let alphar = alpha.re;
            let alphi = alpha.im;
            let mut xnorm = 0.0f64;
            for r in (i + 1)..m_val {
                let c = buf[r + i * m_val];
                xnorm += c.re * c.re + c.im * c.im;
            }
            xnorm = xnorm.sqrt();
            let tau_i = if xnorm == 0.0 && alphi == 0.0 {
                c64::new(0.0, 0.0)
            } else {
                let norm3 = (alphar * alphar + alphi * alphi + xnorm * xnorm).sqrt();
                // SIGN(A, B) follows B's sign bit (verified against the
                // pinned oracle for B = -0.0), which is copysign exactly.
                let beta = -norm3.copysign(alphar);
                let tau = c64::new((beta - alphar) / beta, -alphi / beta);
                // v_tail = x_tail / (alpha - beta); the diagonal stores
                // the real beta.
                let inv = c64::new(1.0, 0.0) / c64::new(alpha.re - beta, alpha.im);
                for r in (i + 1)..m_val {
                    buf[r + i * m_val] = buf[r + i * m_val] * inv;
                }
                buf[i + i * m_val] = c64::new(beta, 0.0);
                tau
            };
            *tau.add(i) = Rcomplex {
                r: tau_i.re,
                i: tau_i.im,
            };

            // ZLARF1F with CONJG(TAU): C <- C - conj(tau) v (v^H C), with
            // the implicit v[0] = 1.
            let ctau = c64::new(tau_i.re, -tau_i.im);
            for col in (i + 1)..n_val {
                let mut w = buf[i + col * m_val];
                for r in (i + 1)..m_val {
                    let v = buf[r + i * m_val];
                    w = w + c64::new(v.re, -v.im) * buf[r + col * m_val];
                }
                w = ctau * w;
                buf[i + col * m_val] = buf[i + col * m_val] - w;
                for r in (i + 1)..m_val {
                    buf[r + col * m_val] = buf[r + col * m_val] - w * buf[r + i * m_val];
                }
            }

            // LAWN 176 partial norm downdate.
            for j in (i + 1)..n_val {
                if vn1[j] != 0.0 {
                    let a = buf[i + j * m_val];
                    let absr = (a.re * a.re + a.im * a.im).sqrt();
                    let mut temp = 1.0 - (absr / vn1[j]) * (absr / vn1[j]);
                    if temp < 0.0 {
                        temp = 0.0;
                    }
                    let temp2 = temp * (vn1[j] / vn2[j]) * (vn1[j] / vn2[j]);
                    if temp2 <= tol3z {
                        if i + 1 < m_val {
                            let mut s = 0.0;
                            for r in (i + 1)..m_val {
                                let c = buf[r + j * m_val];
                                s += c.re * c.re + c.im * c.im;
                            }
                            vn1[j] = s.sqrt();
                            vn2[j] = vn1[j];
                        } else {
                            vn1[j] = 0.0;
                            vn2[j] = 0.0;
                        }
                    } else {
                        vn1[j] *= temp.sqrt();
                    }
                }
            }
        }

        // Write back
        for j in 0..n_val {
            for i in 0..m_val {
                *a.add(i + j * lda_val) = Rcomplex {
                    r: buf[i + j * m_val].re,
                    i: buf[i + j * m_val].im,
                };
            }
        }

        for j in k..n_val {
            *tau.add(j) = Rcomplex { r: 0.0, i: 0.0 };
        }
        *info = 0;
    }
}

/// ZUNMQR — apply Q from complex QR.
pub unsafe fn zunmqr_(
    side: *const u8,
    trans: *const u8,
    m: *const core::ffi::c_int,
    n: *const core::ffi::c_int,
    k: *const core::ffi::c_int,
    a: *const Rcomplex,
    lda: *const core::ffi::c_int,
    tau: *const Rcomplex,
    c__: *mut Rcomplex,
    ldc: *const core::ffi::c_int,
    work: *mut Rcomplex,
    lwork: *const core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let m_val = *m as usize;
        let n_val = *n as usize;
        let k_val = *k as usize;
        let lda_val = *lda as usize;
        let ldc_val = *ldc as usize;
        let side_byte = *side;
        let trans_byte = *trans;
        let lwork_val = *lwork;

        if m_val == 0 || n_val == 0 || k_val == 0 {
            *info = 0;
            return;
        }

        if lwork_val == -1 {
            *work = Rcomplex {
                r: (m_val * n_val) as f64,
                i: 0.0,
            };
            *info = 0;
            return;
        }

        let is_left = side_byte == b'L' || side_byte == b'l';
        // For complex: 'C' = conjugate transpose, 'N' = no transpose
        let is_conj = trans_byte == b'C' || trans_byte == b'c';

        if is_left {
            // ZUNM2R: Q = H(1)...H(k); 'N' applies H(k) first (backward),
            // 'C' applies H(1)**H first (forward), with conjugated tau.
            let range: Vec<usize> = if is_conj {
                (0..k_val).collect()
            } else {
                (0..k_val).rev().collect()
            };

            for j in range {
                let tau_raw = {
                    let rc = *tau.add(j);
                    c64::new(rc.r, rc.i)
                };
                let tau_j = if is_conj {
                    c64::new(tau_raw.re, -tau_raw.im)
                } else {
                    tau_raw
                };
                if tau_j.re == 0.0 && tau_j.im == 0.0 {
                    continue;
                }

                let remaining = m_val - j;
                for col in 0..n_val {
                    let c0 = *c__.add(j + col * ldc_val);
                    let mut w = c64::new(c0.r, c0.i); // v[0] = 1
                    for i in 1..remaining {
                        let vi = {
                            let rc = *a.add(j + i + j * lda_val);
                            c64::new(rc.r, rc.i)
                        };
                        let ci = {
                            let rc = *c__.add(j + i + col * ldc_val);
                            c64::new(rc.r, rc.i)
                        };
                        w = w + c64::new(vi.re, -vi.im) * ci;
                    }
                    w = tau_j * w;

                    let new_c0 = {
                        let rc = *c__.add(j + col * ldc_val);
                        c64::new(rc.r, rc.i) - w
                    };
                    *c__.add(j + col * ldc_val) = Rcomplex {
                        r: new_c0.re,
                        i: new_c0.im,
                    };
                    for i in 1..remaining {
                        let vi = {
                            let rc = *a.add(j + i + j * lda_val);
                            c64::new(rc.r, rc.i)
                        };
                        let ci = {
                            let rc = *c__.add(j + i + col * ldc_val);
                            c64::new(rc.r, rc.i)
                        };
                        let new_ci = ci - w * vi;
                        *c__.add(j + i + col * ldc_val) = Rcomplex {
                            r: new_ci.re,
                            i: new_ci.im,
                        };
                    }
                }
            }
        } else {
            // ZUNM2R right side: 'N' applies H(1) first (forward), 'C'
            // applies H(k)**H first (backward), with conjugated tau.
            let range: Vec<usize> = if is_conj {
                (0..k_val).rev().collect()
            } else {
                (0..k_val).collect()
            };

            for j in range {
                let tau_raw = {
                    let rc = *tau.add(j);
                    c64::new(rc.r, rc.i)
                };
                let tau_j = if is_conj {
                    c64::new(tau_raw.re, -tau_raw.im)
                } else {
                    tau_raw
                };
                if tau_j.re == 0.0 && tau_j.im == 0.0 {
                    continue;
                }

                let remaining = n_val - j;
                for row in 0..m_val {
                    let c0 = *c__.add(row + j * ldc_val);
                    let mut w = c64::new(c0.r, c0.i);
                    for i in 1..remaining {
                        let vi = {
                            let rc = *a.add(j + i + j * lda_val);
                            c64::new(rc.r, rc.i)
                        };
                        let ci = {
                            let rc = *c__.add(row + (j + i) * ldc_val);
                            c64::new(rc.r, rc.i)
                        };
                        w = w + ci * vi;
                    }
                    w = tau_j * w;

                    let new_c0 = {
                        let rc = *c__.add(row + j * ldc_val);
                        c64::new(rc.r, rc.i) - w
                    };
                    *c__.add(row + j * ldc_val) = Rcomplex {
                        r: new_c0.re,
                        i: new_c0.im,
                    };
                    for i in 1..remaining {
                        let vi = {
                            let rc = *a.add(j + i + j * lda_val);
                            c64::new(rc.r, rc.i)
                        };
                        let ci = {
                            let rc = *c__.add(row + (j + i) * ldc_val);
                            c64::new(rc.r, rc.i)
                        };
                        let new_ci = ci - w * c64::new(vi.re, -vi.im);
                        *c__.add(row + (j + i) * ldc_val) = Rcomplex {
                            r: new_ci.re,
                            i: new_ci.im,
                        };
                    }
                }
            }
        }
        *info = 0;
    }
}

/// ZGECON — complex condition number estimate.
pub unsafe fn zgecon_(
    norm: *const u8,
    n: *const core::ffi::c_int,
    a: *const Rcomplex,
    lda: *const core::ffi::c_int,
    anorm: *const f64,
    rcond: *mut f64,
    _work: *mut Rcomplex,
    _rwork: *mut f64,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        *info = 0;
        *rcond = 0.0;
        if !matches!(*norm, b'1' | b'O' | b'o' | b'I' | b'i') {
            *info = -1;
            return;
        }
        if *n < 0 {
            *info = -2;
            return;
        }
        if *lda < (*n).max(1) {
            *info = -4;
            return;
        }
        if *anorm < 0.0 {
            *info = -5;
            return;
        }
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let anorm_val = *anorm;

        if n_val == 0 {
            *rcond = 1.0;
            return;
        }
        if anorm_val == 0.0 {
            *rcond = 0.0;
            *info = 0;
            return;
        }

        let mut inverse = Mat::<c64>::from_fn(n_val, n_val, |i, j| {
            c64::new(if i == j { 1.0 } else { 0.0 }, 0.0)
        });
        for j in 0..n_val {
            for i in 0..n_val {
                for k in 0..i {
                    let v = *a.add(i + k * lda_val);
                    let term = c64::new(v.r, v.i) * inverse[(k, j)];
                    inverse[(i, j)] -= term;
                }
            }
            for i in (0..n_val).rev() {
                for k in i + 1..n_val {
                    let v = *a.add(i + k * lda_val);
                    let term = c64::new(v.r, v.i) * inverse[(k, j)];
                    inverse[(i, j)] -= term;
                }
                let v = *a.add(i + i * lda_val);
                inverse[(i, j)] /= c64::new(v.r, v.i);
            }
        }
        let inf = matches!(*norm, b'I' | b'i');
        let invnorm = (0..n_val)
            .map(|j| {
                (0..n_val)
                    .map(|i| {
                        let z = if inf {
                            inverse[(j, i)]
                        } else {
                            inverse[(i, j)]
                        };
                        z.re.hypot(z.im)
                    })
                    .sum::<f64>()
            })
            .fold(0.0, f64::max);
        *rcond = (1.0 / invnorm) / anorm_val;
        *info = 0;
    }
}

/// ZTRCON — complex triangular condition number.
pub unsafe fn ztrcon_(
    _norm: *const u8,
    _uplo: *const u8,
    diag: *const u8,
    n: *const core::ffi::c_int,
    a: *const Rcomplex,
    lda: *const core::ffi::c_int,
    rcond: *mut f64,
    _work: *mut Rcomplex,
    _rwork: *mut f64,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let is_upper = *_uplo == b'U' || *_uplo == b'u';
        let is_unit = *diag == b'U' || *diag == b'u';
        *info = 0;
        if n_val == 0 {
            *rcond = 1.0;
            return;
        }
        let cabs = |z: Rcomplex| (z.r * z.r + z.i * z.i).sqrt();
        let cmul = |a: Rcomplex, b: Rcomplex| Rcomplex {
            r: a.r * b.r - a.i * b.i,
            i: a.r * b.i + a.i * b.r,
        };
        let cdiv = |a: Rcomplex, b: Rcomplex| {
            let d = b.r * b.r + b.i * b.i;
            if d == 0.0 {
                Rcomplex { r: 0.0, i: 0.0 }
            } else {
                Rcomplex {
                    r: (a.r * b.r + a.i * b.i) / d,
                    i: (a.i * b.r - a.r * b.i) / d,
                }
            }
        };
        let aij = |i: usize, j: usize| -> Rcomplex {
            if i == j && is_unit {
                return Rcomplex { r: 1.0, i: 0.0 };
            }
            let stored = if is_upper { i <= j } else { i >= j };
            if stored {
                *a.add(i + j * lda_val)
            } else {
                Rcomplex { r: 0.0, i: 0.0 }
            }
        };
        let mut anorm = 0.0f64;
        for j in 0..n_val {
            let mut s = 0.0f64;
            for i in 0..n_val {
                s += cabs(aij(i, j));
            }
            anorm = anorm.max(s);
        }
        if anorm == 0.0 {
            *rcond = 0.0;
            return;
        }
        let mut solve = |x: &mut [Rcomplex], conj_trans: bool| {
            let upper = if conj_trans { !is_upper } else { is_upper };
            let coeff = |row: usize, col: usize| {
                let z = if conj_trans { aij(col, row) } else { aij(row, col) };
                if conj_trans {
                    Rcomplex { r: z.r, i: -z.i }
                } else {
                    z
                }
            };
            if upper {
                for i in (0..n_val).rev() {
                    let mut s = x[i];
                    for j in (i + 1)..n_val {
                        let p = cmul(coeff(i, j), x[j]);
                        s.r -= p.r;
                        s.i -= p.i;
                    }
                    x[i] = cdiv(s, coeff(i, i));
                }
            } else {
                for i in 0..n_val {
                    let mut s = x[i];
                    for j in 0..i {
                        let p = cmul(coeff(i, j), x[j]);
                        s.r -= p.r;
                        s.i -= p.i;
                    }
                    x[i] = cdiv(s, coeff(i, i));
                }
            }
        };
        let mut x = vec![Rcomplex { r: 0.0, i: 0.0 }; n_val];
        let scale = 1.0 / n_val as f64;
        for z in &mut x {
            z.r = scale;
        }
        solve(&mut x, false);
        let mut est: f64 = x.iter().map(|z| cabs(*z)).sum();
        for z in &mut x {
            let m = cabs(*z);
            if m == 0.0 {
                *z = Rcomplex { r: 1.0, i: 0.0 };
            } else {
                *z = Rcomplex { r: z.r / m, i: -z.i / m };
            }
        }
        solve(&mut x, true);
        let j = x
            .iter()
            .enumerate()
            .max_by(|a, b| cabs(*a.1).total_cmp(&cabs(*b.1)))
            .map(|(i, _)| i)
            .unwrap_or(0);
        for (i, z) in x.iter_mut().enumerate() {
            *z = if i == j {
                Rcomplex { r: 1.0, i: 0.0 }
            } else {
                Rcomplex { r: 0.0, i: 0.0 }
            };
        }
        solve(&mut x, false);
        est = est.max(x.iter().map(|z| cabs(*z)).sum());
        *rcond = if est == 0.0 { 0.0 } else { 1.0 / (anorm * est) };
    }
}

/// ZTRTRS — complex triangular solve.
pub unsafe fn ztrtrs_(
    uplo: *const u8,
    trans: *const u8,
    diag: *const u8,
    n: *const core::ffi::c_int,
    nrhs: *const core::ffi::c_int,
    a: *const Rcomplex,
    lda: *const core::ffi::c_int,
    b: *mut Rcomplex,
    ldb: *const core::ffi::c_int,
    info: *mut core::ffi::c_int,
) {
    unsafe {
        let n_val = *n as usize;
        let nrhs_val = *nrhs as usize;
        let lda_val = *lda as usize;
        let ldb_val = *ldb as usize;
        let uplo_byte = *uplo;
        let trans_byte = *trans;
        let diag_byte = *diag;

        if n_val == 0 {
            *info = 0;
            return;
        }

        let is_upper = uplo_byte == b'U' || uplo_byte == b'u';
        let is_unit = diag_byte == b'U' || diag_byte == b'u';

        // Check for zero diagonal
        if !is_unit {
            for i in 0..n_val {
                let rc = *a.add(i + i * lda_val);
                if rc.r == 0.0 && rc.i == 0.0 {
                    *info = (i + 1) as core::ffi::c_int;
                    return;
                }
            }
        }

        // Build triangular matrix
        let a_mat = {
            let mut m = Mat::zeros(n_val, n_val);
            for j in 0..n_val {
                for i in 0..n_val {
                    let rc = *a.add(i + j * lda_val);
                    let val = if is_upper {
                        if i <= j {
                            c64::new(rc.r, rc.i)
                        } else {
                            c64::new(0.0, 0.0)
                        }
                    } else {
                        if i >= j {
                            c64::new(rc.r, rc.i)
                        } else {
                            c64::new(0.0, 0.0)
                        }
                    };
                    m[(i, j)] = val;
                }
            }
            if is_unit {
                for i in 0..n_val {
                    m[(i, i)] = c64::new(1.0, 0.0);
                }
            }
            m
        };

        let b_mat = read_mat_c64(b, n_val, nrhs_val, ldb_val);

        let x = match trans_byte {
            b'C' | b'c' => {
                let adj = a_mat.adjoint();
                let owned = adj.to_owned();
                owned.partial_piv_lu().solve(&b_mat)
            }
            b'T' | b't' => {
                let tr = a_mat.transpose();
                let mut owned = Mat::zeros(n_val, n_val);
                for j in 0..n_val {
                    for i in 0..n_val {
                        owned[(i, j)] = tr[(i, j)];
                    }
                }
                owned.partial_piv_lu().solve(&b_mat)
            }
            _ => a_mat.partial_piv_lu().solve(&b_mat),
        };
        write_owned_c64(&x, b, n_val, nrhs_val, ldb_val);
        *info = 0;
    }
}

#[cfg(test)]
mod zgeqp3_tests {
    use super::zgeqp3_;
    use crate::modules::lapack::lapack::Rcomplex;
    use crate::sexp::memory::ArenaBudget;
    use crate::sexp::session::RSession;

    fn call_small(lwork: i32, info: &mut i32) {
        let m = 2;
        let n = 1;
        let lda = 2;
        let mut a = [Rcomplex { r: 1.0, i: 1.0 }, Rcomplex { r: 2.0, i: 0.0 }];
        let mut jpvt = [0; 1];
        let mut tau = [Rcomplex { r: 0.0, i: 0.0 }];
        let mut work = [Rcomplex { r: 0.0, i: 0.0 }; 3];
        let mut rwork = [0.0; 2];
        unsafe {
            zgeqp3_(
                &m,
                &n,
                a.as_mut_ptr(),
                &lda,
                jpvt.as_mut_ptr(),
                tau.as_mut_ptr(),
                work.as_mut_ptr(),
                &lwork,
                rwork.as_mut_ptr(),
                info,
            );
        }
    }

    #[test]
    fn zgeqp3_rejects_signed_arguments_before_casting() {
        let m = -1;
        let n = 1;
        let lda = 1;
        let lwork = 3;
        let mut info = 0;
        unsafe {
            zgeqp3_(
                &m,
                &n,
                std::ptr::null_mut(),
                &lda,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &lwork,
                std::ptr::null_mut(),
                &mut info,
            );
        }
        assert_eq!(info, -1);
    }

    #[test]
    fn zgeqp3_query_and_insufficient_workspace_follow_lapack_contract() {
        let m = 2;
        let n = 1;
        let lda = 2;
        let mut work = [Rcomplex { r: 0.0, i: 0.0 }];
        let lwork = -1;
        let mut info = 0;
        unsafe {
            zgeqp3_(
                &m,
                &n,
                std::ptr::null_mut(),
                &lda,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                work.as_mut_ptr(),
                &lwork,
                std::ptr::null_mut(),
                &mut info,
            );
        }
        assert_eq!(info, 0);
        assert_eq!(work[0].r, 2.0);

        let mut info = 0;
        call_small(1, &mut info);
        assert_eq!(info, -8);
    }

    #[test]
    fn zgeqp3_rejects_native_workspace_before_local_vec_allocations() {
        let mut session = RSession::new();
        session.set_arena_budget(ArenaBudget::new(1, 0));
        let mut info = 0;
        session.with_active(|| call_small(3, &mut info));
        assert_eq!(info, -100);
    }

    #[test]
    fn zgeqp3_valid_small_inputs_are_accepted() {
        let mut info = 0;
        call_small(3, &mut info);
        assert_eq!(info, 0);
    }

    #[test]
    fn zgeqp3_validates_all_dimensions_and_empty_workspace_query() {
        for (m, n, lda, lwork, expected) in [(1, -1, 1, 3, -2), (2, 1, 1, 3, -4), (1, 1, 1, -2, -8)]
        {
            let mut info = 0;
            unsafe {
                zgeqp3_(
                    &m,
                    &n,
                    std::ptr::null_mut(),
                    &lda,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &lwork,
                    std::ptr::null_mut(),
                    &mut info,
                );
            }
            assert_eq!(info, expected);
        }
        for (m, n) in [(0, 3), (3, 0), (i32::MAX, i32::MAX)] {
            let lda = m.max(1);
            let mut work = Rcomplex { r: 0.0, i: 0.0 };
            let mut info = 0;
            unsafe {
                zgeqp3_(
                    &m,
                    &n,
                    std::ptr::null_mut(),
                    &lda,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut work,
                    &-1,
                    std::ptr::null_mut(),
                    &mut info,
                );
            }
            assert_eq!(info, 0);
            assert_eq!(
                work.r,
                if m == 0 || n == 0 {
                    1.0
                } else {
                    f64::from(n) + 1.0
                }
            );
        }
    }
}
