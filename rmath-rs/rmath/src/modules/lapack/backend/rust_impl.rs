/*
 * Pure Rust LAPACK backend using faer 0.24.
 *
 * Provides the same function signatures as the Fortran FFI in lapack.rs,
 * dispatching to faer-rs for linear algebra computations.
 * Enabled via the `rust-backend` feature flag.
 */

use crate::modules::lapack::lapack::Rcomplex;
use faer::linalg::solvers::{DenseSolveCore, Solve};
use faer::{Mat, MatRef, Side, c64};
// ============================================================
// Helper functions
// ============================================================

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
    m: *const libc::c_int,
    n: *const libc::c_int,
    a: *const f64,
    lda: *const libc::c_int,
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
                        max_val = max_val.max((*a.add(i + j * lda)).abs());
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
                // Frobenius norm
                let mut sum_sq: f64 = 0.0;
                for j in 0..n {
                    for i in 0..m {
                        let v = *a.add(i + j * lda);
                        sum_sq += v * v;
                    }
                }
                sum_sq.sqrt()
            }
            _ => 0.0,
        }
    }
}

/// DGETRF — LU factorization with partial pivoting.
pub unsafe fn dgetrf_(
    m: *const libc::c_int,
    n: *const libc::c_int,
    a: *mut f64,
    lda: *const libc::c_int,
    ipiv: *mut libc::c_int,
    info: *mut libc::c_int,
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
        for j in 0..n {
            for i in 0..m {
                let val = if i > j {
                    // Strict lower triangle: from L
                    if i < k && j < k { l[(i, j)] } else { 0.0 }
                } else {
                    // Upper triangle (including diagonal): from U
                    if j < u.ncols() && i < u.nrows() {
                        u[(i, j)]
                    } else {
                        0.0
                    }
                };
                *a.add(i + j * lda) = val;
            }
        }

        // Write pivot array
        let bwd = get_bwd_perm(p);
        let pivots = perm_bwd_to_ipiv(&bwd, m.min(bwd.len()));
        for (i, &p) in pivots.iter().enumerate() {
            *ipiv.add(i) = p;
        }
        *info = 0;
    }
}

/// DGESV — solve Ax = B via LU.
pub unsafe fn dgesv_(
    n: *const libc::c_int,
    nrhs: *const libc::c_int,
    a: *mut f64,
    lda: *const libc::c_int,
    ipiv: *mut libc::c_int,
    b: *mut f64,
    ldb: *const libc::c_int,
    info: *mut libc::c_int,
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

        let a_mat = read_mat_f64(a, n, n, lda_val);
        let b_mat = read_mat_f64(b, n, nrhs, ldb_val);

        let lu = a_mat.partial_piv_lu();

        // Check for singularity
        let u = lu.U();
        for i in 0..n {
            if u[(i, i)].abs() == 0.0 {
                *info = (i + 1) as libc::c_int;
                return;
            }
        }

        let x = lu.solve(&b_mat);
        write_owned_f64(&x, b, n, nrhs, ldb_val);

        // Also write LU factors and pivot to a/ipiv
        let l = lu.L();
        for j in 0..n {
            for i in 0..n {
                let val = if i > j { l[(i, j)] } else { u[(i, j)] };
                *a.add(i + j * lda_val) = val;
            }
        }
        let bwd = get_bwd_perm(lu.P());
        let pivots = perm_bwd_to_ipiv(&bwd, n);
        for (i, &p) in pivots.iter().enumerate() {
            *ipiv.add(i) = p;
        }
        *info = 0;
    }
}

/// DPOTRF — Cholesky factorization.
pub unsafe fn dpotrf_(
    uplo: *const u8,
    n: *const libc::c_int,
    a: *mut f64,
    lda: *const libc::c_int,
    info: *mut libc::c_int,
) {
    unsafe {
        let n = *n as usize;
        let lda = *lda as usize;
        let uplo_byte = *uplo;

        if n == 0 {
            *info = 0;
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
                *info = 1;
            }
        }
    }
}

/// DPOTRI — inverse from Cholesky factor.
pub unsafe fn dpotri_(
    uplo: *const u8,
    n: *const libc::c_int,
    a: *mut f64,
    lda: *const libc::c_int,
    info: *mut libc::c_int,
) {
    unsafe {
        let n = *n as usize;
        let lda = *lda as usize;
        let uplo_byte = *uplo;

        if n == 0 {
            *info = 0;
            return;
        }

        // Reconstruct A from Cholesky factor, then compute inverse
        let chol = read_mat_f64(a, n, n, lda);
        let mut reconstructed = Mat::zeros(n, n);

        match uplo_byte {
            b'U' | b'u' => {
                // A = U^T U where U is upper triangle of chol
                for i in 0..n {
                    for j in 0..n {
                        let mut sum = 0.0;
                        for k in 0..n {
                            let u_ki = if k <= i { chol[(k, i)] } else { 0.0 };
                            let u_kj = if k <= j { chol[(k, j)] } else { 0.0 };
                            sum += u_ki * u_kj;
                        }
                        reconstructed[(i, j)] = sum;
                    }
                }
            }
            _ => {
                // A = L L^T where L is lower triangle of chol
                for i in 0..n {
                    for j in 0..n {
                        let mut sum = 0.0;
                        for k in 0..n {
                            let l_ik = if i >= k { chol[(i, k)] } else { 0.0 };
                            let l_jk = if j >= k { chol[(j, k)] } else { 0.0 };
                            sum += l_ik * l_jk;
                        }
                        reconstructed[(i, j)] = sum;
                    }
                }
            }
        }

        // Compute inverse via LU
        let a_inv = reconstructed.partial_piv_lu().inverse();

        // Write result to the specified triangle
        match uplo_byte {
            b'U' | b'u' => {
                for j in 0..n {
                    for i in 0..=j {
                        *a.add(i + j * lda) = a_inv[(i, j)];
                    }
                }
            }
            _ => {
                for j in 0..n {
                    for i in j..n {
                        *a.add(i + j * lda) = a_inv[(i, j)];
                    }
                }
            }
        }
        *info = 0;
    }
}

/// DPSTRF — pivoted Cholesky factorization.
pub unsafe fn dpstrf_(
    uplo: *const u8,
    n: *const libc::c_int,
    a: *mut f64,
    lda: *const libc::c_int,
    piv: *mut libc::c_int,
    rank: *mut libc::c_int,
    tol: *const f64,
    _work: *mut f64,
    info: *mut libc::c_int,
) {
    unsafe {
        let n = *n as usize;
        let lda = *lda as usize;
        let uplo_byte = *uplo;
        let tol_val = *tol;

        if n == 0 {
            *rank = 0;
            *info = 0;
            return;
        }

        // Initialize pivots (1-based)
        for i in 0..n {
            *piv.add(i) = (i + 1) as libc::c_int;
        }

        // Read the matrix
        let mut mat = read_mat_f64(a, n, n, lda);

        // Compute diagonal values for pivot selection
        let mut diag = vec![0.0f64; n];
        for i in 0..n {
            diag[i] = mat[(i, i)];
        }

        let mut r = 0usize;

        if uplo_byte == b'U' || uplo_byte == b'u' {
            // Upper triangular Cholesky: A[p,p] = U^T U
            for k in 0..n {
                // Find pivot (largest remaining diagonal)
                let mut max_val = 0.0f64;
                let mut max_idx = k;
                for j in k..n {
                    let p = (*piv.add(j) - 1) as usize;
                    if diag[p] > max_val {
                        max_val = diag[p];
                        max_idx = j;
                    }
                }

                if max_val <= tol_val.max(0.0) {
                    break;
                }

                // Swap pivots k and max_idx
                let pk = (*piv.add(k) - 1) as usize;
                let pm = (*piv.add(max_idx) - 1) as usize;
                {
                    let tmp = *piv.add(k);
                    *piv.add(k) = *piv.add(max_idx);
                    *piv.add(max_idx) = tmp;
                }

                // Swap rows and columns in mat
                if pk != pm {
                    for j in 0..n {
                        let tmp = mat[(pk, j)];
                        mat[(pk, j)] = mat[(pm, j)];
                        mat[(pm, j)] = tmp;
                    }
                    for i in 0..n {
                        let tmp = mat[(i, pk)];
                        mat[(i, pk)] = mat[(i, pm)];
                        mat[(i, pm)] = tmp;
                    }
                }

                let p = (*piv.add(k) - 1) as usize;
                let sqrt_diag = diag[p].sqrt();
                mat[(p, p)] = sqrt_diag;

                // Update remaining columns
                for j in (k + 1)..n {
                    let pj = (*piv.add(j) - 1) as usize;
                    let mut sum = mat[(p, pj)];
                    for i in 0..k {
                        let pi = (*piv.add(i) - 1) as usize;
                        sum -= mat[(pi, p)] * mat[(pi, pj)];
                    }
                    mat[(p, pj)] = sum / sqrt_diag;

                    // Update diagonal
                    let val = mat[(p, pj)];
                    diag[pj] -= val * val;
                    if diag[pj] < 0.0 {
                        diag[pj] = 0.0;
                    }
                }
                r += 1;
            }

            // Write upper triangle back
            for j in 0..n {
                for i in 0..n {
                    let p = (*piv.add(j) - 1) as usize;
                    let q = (*piv.add(i) - 1) as usize;
                    if i <= j {
                        *a.add(i + j * lda) = mat[(q, p)];
                    } else {
                        *a.add(i + j * lda) = 0.0;
                    }
                }
            }
        } else {
            // Lower triangular Cholesky: A[p,p] = L L^T
            for k in 0..n {
                let mut max_val = 0.0f64;
                let mut max_idx = k;
                for j in k..n {
                    let p = (*piv.add(j) - 1) as usize;
                    if diag[p] > max_val {
                        max_val = diag[p];
                        max_idx = j;
                    }
                }

                if max_val <= tol_val.max(0.0) {
                    break;
                }

                let pk = (*piv.add(k) - 1) as usize;
                let pm = (*piv.add(max_idx) - 1) as usize;
                {
                    let tmp = *piv.add(k);
                    *piv.add(k) = *piv.add(max_idx);
                    *piv.add(max_idx) = tmp;
                }

                if pk != pm {
                    for j in 0..n {
                        let tmp = mat[(pk, j)];
                        mat[(pk, j)] = mat[(pm, j)];
                        mat[(pm, j)] = tmp;
                    }
                    for i in 0..n {
                        let tmp = mat[(i, pk)];
                        mat[(i, pk)] = mat[(i, pm)];
                        mat[(i, pm)] = tmp;
                    }
                }

                let p = (*piv.add(k) - 1) as usize;
                let sqrt_diag = diag[p].sqrt();
                mat[(p, p)] = sqrt_diag;

                for j in (k + 1)..n {
                    let pj = (*piv.add(j) - 1) as usize;
                    let mut sum = mat[(pj, p)];
                    for i in 0..k {
                        let pi = (*piv.add(i) - 1) as usize;
                        sum -= mat[(pj, pi)] * mat[(p, pi)];
                    }
                    mat[(pj, p)] = sum / sqrt_diag;

                    let val = mat[(pj, p)];
                    diag[pj] -= val * val;
                    if diag[pj] < 0.0 {
                        diag[pj] = 0.0;
                    }
                }
                r += 1;
            }

            // Write lower triangle back
            for j in 0..n {
                for i in 0..n {
                    let p = (*piv.add(j) - 1) as usize;
                    let q = (*piv.add(i) - 1) as usize;
                    if i >= j {
                        *a.add(i + j * lda) = mat[(q, p)];
                    } else {
                        *a.add(i + j * lda) = 0.0;
                    }
                }
            }
        }

        *rank = r as libc::c_int;
        *info = if r < n { 1 } else { 0 };
    }
}

/// DGESDD — SVD.
pub unsafe fn dgesdd_(
    jobz: *const u8,
    m: *const libc::c_int,
    n: *const libc::c_int,
    a: *mut f64,
    lda: *const libc::c_int,
    s: *mut f64,
    u: *mut f64,
    ldu: *const libc::c_int,
    vt: *mut f64,
    ldvt: *const libc::c_int,
    work: *mut f64,
    lwork: *const libc::c_int,
    _iwork: *mut libc::c_int,
    info: *mut libc::c_int,
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
    n: *const libc::c_int,
    a: *mut f64,
    lda: *const libc::c_int,
    vl: *const f64,
    vu: *const f64,
    il: *const libc::c_int,
    iu: *const libc::c_int,
    _abstol: *const f64,
    m: *mut libc::c_int,
    w: *mut f64,
    z: *mut f64,
    ldz: *const libc::c_int,
    isuppz: *mut libc::c_int,
    work: *mut f64,
    lwork: *const libc::c_int,
    _iwork: *mut libc::c_int,
    liwork: *const libc::c_int,
    info: *mut libc::c_int,
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
                *_iwork = (10 * n_val) as libc::c_int;
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

        *m = selected.len() as libc::c_int;

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
                *isuppz.add(2 * idx + 1) = n_val as libc::c_int;
            }
        }
        *info = 0;
    }
}

/// DGEEV — general eigenvalue decomposition.
pub unsafe fn dgeev_(
    jobvl: *const u8,
    jobvr: *const u8,
    n: *const libc::c_int,
    a: *mut f64,
    lda: *const libc::c_int,
    wr: *mut f64,
    wi: *mut f64,
    vl: *mut f64,
    ldvl: *const libc::c_int,
    vr: *mut f64,
    ldvr: *const libc::c_int,
    work: *mut f64,
    lwork: *const libc::c_int,
    info: *mut libc::c_int,
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
    m: *const libc::c_int,
    n: *const libc::c_int,
    a: *mut f64,
    lda: *const libc::c_int,
    jpvt: *mut libc::c_int,
    tau: *mut f64,
    work: *mut f64,
    lwork: *const libc::c_int,
    info: *mut libc::c_int,
) {
    unsafe {
        let m_val = *m as usize;
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let lwork_val = *lwork;

        if m_val == 0 || n_val == 0 {
            *info = 0;
            return;
        }

        // Workspace query
        if lwork_val == -1 {
            *work = (3 * n_val + 1).max(m_val * n_val) as f64;
            *info = 0;
            return;
        }

        let k = m_val.min(n_val);

        // Read matrix into a flat column-major buffer we can modify
        let mut buf = vec![0.0f64; m_val * n_val];
        for j in 0..n_val {
            for i in 0..m_val {
                buf[i + j * m_val] = *a.add(i + j * lda_val);
            }
        }

        // Initialize jpvt to 1-based column indices
        for j in 0..n_val {
            let cur = *jpvt.add(j);
            if cur == 0 {
                *jpvt.add(j) = (j + 1) as libc::c_int;
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
            // Find pivot column (largest remaining norm)
            let mut max_norm = 0.0f64;
            let mut pivot = jj;
            for j in jj..n_val {
                if col_norms_sq[j] > max_norm {
                    max_norm = col_norms_sq[j];
                    pivot = j;
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
            let mut v = vec![0.0f64; remaining];
            v[0] = 1.0;
            for i in 1..remaining {
                v[i] = x[i] / u1;
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

/// DORMQR — apply Q from QR factorization to a matrix.
pub unsafe fn dormqr_(
    side: *const u8,
    trans: *const u8,
    m: *const libc::c_int,
    n: *const libc::c_int,
    k: *const libc::c_int,
    a: *const f64,
    lda: *const libc::c_int,
    tau: *const f64,
    c__: *mut f64,
    ldc: *const libc::c_int,
    work: *mut f64,
    lwork: *const libc::c_int,
    info: *mut libc::c_int,
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
    n: *const libc::c_int,
    a: *const f64,
    lda: *const libc::c_int,
    anorm: *const f64,
    rcond: *mut f64,
    _work: *mut f64,
    _iwork: *mut libc::c_int,
    info: *mut libc::c_int,
) {
    unsafe {
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let anorm_val = *anorm;
        let _norm_byte = *norm;

        if n_val == 0 {
            *rcond = 0.0;
            *info = 0;
            return;
        }

        if anorm_val == 0.0 {
            *rcond = 0.0;
            *info = 0;
            return;
        }

        // Use SVD to estimate condition number
        let mat = read_mat_f64(a, n_val, n_val, lda_val);
        let svals = match mat.singular_values() {
            Ok(v) => v,
            Err(_) => {
                *rcond = 0.0;
                *info = 1;
                return;
            }
        };

        let s_max = svals.first().copied().unwrap_or(0.0);
        let s_min = svals.last().copied().unwrap_or(0.0);

        if s_max == 0.0 {
            *rcond = 0.0;
        } else {
            *rcond = s_min / s_max;
            // Normalize by the provided anorm to get 1-norm or inf-norm condition number
            // rcond = s_min / (s_max * anorm / s_max) = s_min / anorm (approximate)
            // Actually: rcond = 1 / (anorm * ||A^{-1}||) ≈ s_min / (s_max * anorm / s_max)
            // Simplification: use ratio directly
            let ratio = s_min / s_max;
            // Adjust: LAPACK's rcond = 1 / (||A|| * ||A^-1||)
            // ||A||_2 = s_max, ||A^-1||_2 = 1/s_min
            // rcond_2 = s_min / s_max
            // For 1-norm/inf-norm: approximate with 2-norm ratio
            *rcond = ratio;
        }
        *info = 0;
    }
}

/// DTRCON — triangular condition number.
pub unsafe fn dtrcon_(
    _norm: *const u8,
    uplo: *const u8,
    diag: *const u8,
    n: *const libc::c_int,
    a: *const f64,
    lda: *const libc::c_int,
    rcond: *mut f64,
    _work: *mut f64,
    _iwork: *mut libc::c_int,
    info: *mut libc::c_int,
) {
    unsafe {
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let uplo_byte = *uplo;
        let diag_byte = *diag;

        if n_val == 0 {
            *rcond = 0.0;
            *info = 0;
            return;
        }

        // For triangular matrix, condition ≈ 1 / (||diag||_inf * ||T^-1||_inf)
        // Simplified: rcond ≈ min|diag| / max|diag| * (1/n)
        let mut min_diag = f64::INFINITY;
        let mut max_diag = 0.0f64;
        let is_unit = diag_byte == b'U' || diag_byte == b'u';

        for i in 0..n_val {
            let d = if is_unit {
                1.0
            } else {
                match uplo_byte {
                    b'U' | b'u' => *a.add(i + i * lda_val),
                    _ => *a.add(i + i * lda_val),
                }
            };
            min_diag = min_diag.min(d.abs());
            max_diag = max_diag.max(d.abs());
        }

        if max_diag == 0.0 {
            *rcond = 0.0;
        } else {
            *rcond = min_diag / (max_diag * n_val as f64);
        }
        *info = 0;
    }
}

/// DTRTRS — triangular solve.
pub unsafe fn dtrtrs_(
    uplo: *const u8,
    trans: *const u8,
    diag: *const u8,
    n: *const libc::c_int,
    nrhs: *const libc::c_int,
    a: *const f64,
    lda: *const libc::c_int,
    b: *mut f64,
    ldb: *const libc::c_int,
    info: *mut libc::c_int,
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
                    *info = (i + 1) as libc::c_int;
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
    m: *const libc::c_int,
    n: *const libc::c_int,
    a: *const Rcomplex,
    lda: *const libc::c_int,
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
    m: *const libc::c_int,
    n: *const libc::c_int,
    a: *mut Rcomplex,
    lda: *const libc::c_int,
    ipiv: *mut libc::c_int,
    info: *mut libc::c_int,
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
        let pivots = perm_bwd_to_ipiv(&bwd, m.min(bwd.len()));
        for (i, &p) in pivots.iter().enumerate() {
            *ipiv.add(i) = p;
        }
        *info = 0;
    }
}

/// ZGESV — complex linear solve.
pub unsafe fn zgesv_(
    n: *const libc::c_int,
    nrhs: *const libc::c_int,
    a: *mut Rcomplex,
    lda: *const libc::c_int,
    ipiv: *mut libc::c_int,
    b: *mut Rcomplex,
    ldb: *const libc::c_int,
    info: *mut libc::c_int,
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
                *info = (i + 1) as libc::c_int;
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
    m: *const libc::c_int,
    n: *const libc::c_int,
    a: *mut Rcomplex,
    lda: *const libc::c_int,
    s: *mut f64,
    u: *mut Rcomplex,
    ldu: *const libc::c_int,
    vt: *mut Rcomplex,
    ldvt: *const libc::c_int,
    work: *mut Rcomplex,
    lwork: *const libc::c_int,
    _rwork: *mut f64,
    _iwork: *mut libc::c_int,
    info: *mut libc::c_int,
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
    n: *const libc::c_int,
    a: *mut Rcomplex,
    lda: *const libc::c_int,
    w: *mut f64,
    work: *mut Rcomplex,
    lwork: *const libc::c_int,
    _rwork: *mut f64,
    info: *mut libc::c_int,
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
    n: *const libc::c_int,
    a: *mut Rcomplex,
    lda: *const libc::c_int,
    w: *mut Rcomplex,
    vl: *mut Rcomplex,
    ldvl: *const libc::c_int,
    vr: *mut Rcomplex,
    ldvr: *const libc::c_int,
    work: *mut Rcomplex,
    lwork: *const libc::c_int,
    _rwork: *mut f64,
    info: *mut libc::c_int,
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
    m: *const libc::c_int,
    n: *const libc::c_int,
    a: *mut Rcomplex,
    lda: *const libc::c_int,
    jpvt: *mut libc::c_int,
    tau: *mut Rcomplex,
    work: *mut Rcomplex,
    lwork: *const libc::c_int,
    _rwork: *mut f64,
    info: *mut libc::c_int,
) {
    unsafe {
        let m_val = *m as usize;
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let lwork_val = *lwork;

        if m_val == 0 || n_val == 0 {
            *info = 0;
            return;
        }

        // Workspace query
        if lwork_val == -1 {
            *work = Rcomplex {
                r: (m_val * n_val + n_val) as f64,
                i: 0.0,
            };
            *info = 0;
            return;
        }

        let k = m_val.min(n_val);

        // Read into buffer
        let mut buf = vec![c64::new(0.0, 0.0); m_val * n_val];
        for j in 0..n_val {
            for i in 0..m_val {
                let rc = *a.add(i + j * lda_val);
                buf[i + j * m_val] = c64::new(rc.r, rc.i);
            }
        }

        // Initialize jpvt
        for j in 0..n_val {
            if *jpvt.add(j) == 0 {
                *jpvt.add(j) = (j + 1) as libc::c_int;
            }
        }

        // Column norms (squared magnitude)
        let mut col_norms_sq = vec![0.0f64; n_val];
        for j in 0..n_val {
            let mut sum = 0.0;
            for i in 0..m_val {
                let c = buf[i + j * m_val];
                sum += c.re * c.re + c.im * c.im;
            }
            col_norms_sq[j] = sum;
        }

        for jj in 0..k {
            // Find pivot
            let mut max_norm = 0.0f64;
            let mut pivot = jj;
            for j in jj..n_val {
                if col_norms_sq[j] > max_norm {
                    max_norm = col_norms_sq[j];
                    pivot = j;
                }
            }

            if pivot != jj {
                for i in 0..m_val {
                    buf.swap(i + jj * m_val, i + pivot * m_val);
                }
                col_norms_sq.swap(jj, pivot);
                let tmp = *jpvt.add(jj);
                *jpvt.add(jj) = *jpvt.add(pivot);
                *jpvt.add(pivot) = tmp;
            }

            let remaining = m_val - jj;
            if remaining == 0 {
                *tau.add(jj) = Rcomplex { r: 0.0, i: 0.0 };
                continue;
            }

            // Extract x = buf[jj:m, jj]
            let mut x = vec![c64::new(0.0, 0.0); remaining];
            for i in 0..remaining {
                x[i] = buf[jj + i + jj * m_val];
            }

            // Complex norm
            let norm_x = {
                let mut s = 0.0;
                for c in &x {
                    s += c.re * c.re + c.im * c.im;
                }
                s.sqrt()
            };

            if norm_x == 0.0 {
                *tau.add(jj) = Rcomplex { r: 0.0, i: 0.0 };
                continue;
            }

            // Complex Householder: H = I - tau * v * v^H
            let alpha = x[0];
            let r_alpha = (alpha.re * alpha.re + alpha.im * alpha.im).sqrt();
            let sign = if r_alpha == 0.0 {
                1.0
            } else {
                alpha.re / r_alpha
            };
            let beta = -sign * norm_x;

            let u1 = c64::new(alpha.re - beta, alpha.im);

            if u1.re == 0.0 && u1.im == 0.0 {
                *tau.add(jj) = Rcomplex { r: 0.0, i: 0.0 };
                buf[jj + jj * m_val] = c64::new(beta, 0.0);
                continue;
            }

            let mut v = vec![c64::new(0.0, 0.0); remaining];
            v[0] = c64::new(1.0, 0.0);
            for i in 1..remaining {
                v[i] = x[i] / u1;
            }

            // tau = (beta - alpha) / beta ... simplified for complex
            // tau = conj(u1) / beta
            let tau_val = c64::new(u1.re, -u1.im) / c64::new(beta, 0.0);

            // Store
            buf[jj + jj * m_val] = c64::new(beta, 0.0);
            for i in 1..remaining {
                buf[jj + i + jj * m_val] = v[i];
            }
            *tau.add(jj) = Rcomplex {
                r: tau_val.re,
                i: tau_val.im,
            };

            // Apply: buf[jj:m, col] -= tau * v * (v^H * buf[jj:m, col])
            for col in (jj + 1)..n_val {
                // w = v^H * buf[jj:m, col]
                let mut w = buf[jj + col * m_val]; // v[0] = 1
                for i in 1..remaining {
                    let vi_conj = c64::new(v[i].re, -v[i].im);
                    w = w + vi_conj * buf[jj + i + col * m_val];
                }
                w = tau_val * w;

                buf[jj + col * m_val] = buf[jj + col * m_val] - w;
                for i in 1..remaining {
                    buf[jj + i + col * m_val] = buf[jj + i + col * m_val] - w * v[i];
                }
            }

            // Update norms
            for col in (jj + 1)..n_val {
                let c = buf[jj + col * m_val];
                col_norms_sq[col] -= c.re * c.re + c.im * c.im;
                if col_norms_sq[col] < 0.0 {
                    col_norms_sq[col] = 0.0;
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
    m: *const libc::c_int,
    n: *const libc::c_int,
    k: *const libc::c_int,
    a: *const Rcomplex,
    lda: *const libc::c_int,
    tau: *const Rcomplex,
    c__: *mut Rcomplex,
    ldc: *const libc::c_int,
    work: *mut Rcomplex,
    lwork: *const libc::c_int,
    info: *mut libc::c_int,
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
            let range: Vec<usize> = if is_conj {
                (0..k_val).rev().collect()
            } else {
                (0..k_val).collect()
            };

            for j in range {
                let tau_j = {
                    let rc = *tau.add(j);
                    c64::new(rc.r, rc.i)
                };
                if tau_j.re == 0.0 && tau_j.im == 0.0 {
                    continue;
                }

                let remaining = m_val - j;
                for col in 0..n_val {
                    // w = v^H * C[j:m, col]
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
            let range: Vec<usize> = if !is_conj {
                (0..k_val).rev().collect()
            } else {
                (0..k_val).collect()
            };

            for j in range {
                let tau_j = {
                    let rc = *tau.add(j);
                    c64::new(rc.r, rc.i)
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
    _norm: *const u8,
    n: *const libc::c_int,
    a: *const Rcomplex,
    lda: *const libc::c_int,
    anorm: *const f64,
    rcond: *mut f64,
    _work: *mut Rcomplex,
    _rwork: *mut f64,
    info: *mut libc::c_int,
) {
    unsafe {
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let anorm_val = *anorm;

        if n_val == 0 || anorm_val == 0.0 {
            *rcond = 0.0;
            *info = 0;
            return;
        }

        let mat = read_mat_c64(a, n_val, n_val, lda_val);
        let svals = match mat.singular_values() {
            Ok(v) => v,
            Err(_) => {
                *rcond = 0.0;
                *info = 1;
                return;
            }
        };

        let s_max = svals.first().copied().unwrap_or(0.0);
        let s_min = svals.last().copied().unwrap_or(0.0);

        if s_max == 0.0 {
            *rcond = 0.0;
        } else {
            *rcond = s_min / s_max;
        }
        *info = 0;
    }
}

/// ZTRCON — complex triangular condition number.
pub unsafe fn ztrcon_(
    _norm: *const u8,
    _uplo: *const u8,
    diag: *const u8,
    n: *const libc::c_int,
    a: *const Rcomplex,
    lda: *const libc::c_int,
    rcond: *mut f64,
    _work: *mut Rcomplex,
    _rwork: *mut f64,
    info: *mut libc::c_int,
) {
    unsafe {
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let diag_byte = *diag;

        if n_val == 0 {
            *rcond = 0.0;
            *info = 0;
            return;
        }

        let is_unit = diag_byte == b'U' || diag_byte == b'u';
        let mut min_diag = f64::INFINITY;
        let mut max_diag = 0.0f64;

        for i in 0..n_val {
            let rc = *a.add(i + i * lda_val);
            let d = if is_unit {
                1.0
            } else {
                (rc.r * rc.r + rc.i * rc.i).sqrt()
            };
            min_diag = min_diag.min(d);
            max_diag = max_diag.max(d);
        }

        if max_diag == 0.0 {
            *rcond = 0.0;
        } else {
            *rcond = min_diag / (max_diag * n_val as f64);
        }
        *info = 0;
    }
}

/// ZTRTRS — complex triangular solve.
pub unsafe fn ztrtrs_(
    uplo: *const u8,
    trans: *const u8,
    diag: *const u8,
    n: *const libc::c_int,
    nrhs: *const libc::c_int,
    a: *const Rcomplex,
    lda: *const libc::c_int,
    b: *mut Rcomplex,
    ldb: *const libc::c_int,
    info: *mut libc::c_int,
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
                    *info = (i + 1) as libc::c_int;
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
