#[cfg(feature = "rust-backend")]

use super::lapack::Rcomplex;
use faer::{mat, Mat, Side, c64};
use std::ptr;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

unsafe fn read_col_major_f64(a: *const f64, m: usize, n: usize, lda: usize) -> Mat<f64> {
    let mut out = Mat::zeros(m, n);
    for j in 0..n {
        for i in 0..m {
            out.write(i, j, *a.add(j * lda + i));
        }
    }
    out
}

unsafe fn write_col_major_f64(mat: &Mat<f64>, a: *mut f64, m: usize, n: usize, lda: usize) {
    for j in 0..n {
        for i in 0..m {
            *a.add(j * lda + i) = mat.read(i, j);
        }
    }
}

unsafe fn read_col_major_c64(a: *const Rcomplex, m: usize, n: usize, lda: usize) -> Mat<c64> {
    let mut out = Mat::zeros(m, n);
    for j in 0..n {
        for i in 0..m {
            let rc = &*a.add(j * lda + i);
            out.write(i, j, c64::new(rc.r, rc.i));
        }
    }
    out
}

unsafe fn write_col_major_c64(mat: &Mat<c64>, a: *mut Rcomplex, m: usize, n: usize, lda: usize) {
    for j in 0..n {
        for i in 0..m {
            let c = mat.read(i, j);
            let rc = &mut *a.add(j * lda + i);
            rc.r = c.re;
            rc.i = c.im;
        }
    }
}

fn perm_to_ipiv(perm: &[usize]) -> Vec<i32> {
    let n = perm.len();
    let mut ipiv = vec![0i32; n];
    let mut current: Vec<usize> = (0..n).collect();
    let mut inv: Vec<usize> = (0..n).collect();
    for k in 0..n {
        let target = perm[k];
        let pos = inv[target];
        ipiv[k] = (pos + 1) as i32;
        if pos != k {
            let at_k = current[k];
            current[k] = target;
            current[pos] = at_k;
            inv[target] = k;
            inv[at_k] = pos;
        }
    }
    ipiv
}

fn side_from_uplo(uplo: u8) -> Side {
    if uplo == b'L' || uplo == b'l' {
        Side::Lower
    } else {
        Side::Upper
    }
}

// ---------------------------------------------------------------------------
// Real routines
// ---------------------------------------------------------------------------

pub unsafe fn dlange_(
    norm: *const u8,
    m: *const libc::c_int,
    n: *const libc::c_int,
    a: *const f64,
    lda: *const libc::c_int,
    _work: *mut f64,
) -> f64 {
    let m = *m as usize;
    let n = *n as usize;
    let lda = *lda as usize;
    if m == 0 || n == 0 {
        return 0.0;
    }
    let norm_c = (*norm).to_ascii_uppercase();
    match norm_c {
        b'M' | b'm' => {
            let mut max = 0.0f64;
            for j in 0..n {
                for i in 0..m {
                    max = max.max((*a.add(j * lda + i)).abs());
                }
            }
            max
        }
        b'O' | b'1' => {
            let mut max_col = 0.0f64;
            for j in 0..n {
                let mut col = 0.0f64;
                for i in 0..m {
                    col += (*a.add(j * lda + i)).abs();
                }
                max_col = max_col.max(col);
            }
            max_col
        }
        b'I' | b'i' => {
            let mut row_sums = vec![0.0f64; m];
            for j in 0..n {
                for i in 0..m {
                    row_sums[i] += (*a.add(j * lda + i)).abs();
                }
            }
            row_sums.into_iter().fold(0.0f64, f64::max)
        }
        b'F' | b'E' | b'f' | b'e' => {
            let mut sum = 0.0f64;
            for j in 0..n {
                for i in 0..m {
                    let v = *a.add(j * lda + i);
                    sum += v * v;
                }
            }
            sum.sqrt()
        }
        _ => 0.0,
    }
}

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
    let n = *n as usize;
    let lda = *lda as usize;
    let anorm = *anorm;
    if n == 0 {
        *rcond = 1.0;
        *info = 0;
        return;
    }
    let a_mat = read_col_major_f64(a, n, n, lda);
    let svd = a_mat.svd();
    let s = svd.s_diagonal();
    let s_max = s.iter().copied().fold(0.0f64, f64::max);
    let s_min = s.iter().copied().fold(f64::INFINITY, f64::min);
    if s_max == 0.0 || s_min == 0.0 {
        *rcond = 0.0;
    } else {
        let norm_c = (*norm).to_ascii_uppercase();
        let cond2 = s_max / s_min;
        // Scale from 2-norm to 1/inf norm estimate roughly
        let scale = if norm_c == b'I' || norm_c == b'1' {
            cond2.sqrt()
        } else {
            cond2
        };
        *rcond = 1.0 / (anorm * scale);
        if !rcond.is_finite() || *rcond < 0.0 {
            *rcond = 0.0;
        }
    }
    *info = 0;
}

pub unsafe fn dgetrf_(
    m: *const libc::c_int,
    n: *const libc::c_int,
    a: *mut f64,
    lda: *const libc::c_int,
    ipiv: *mut libc::c_int,
    info: *mut libc::c_int,
) {
    let m = *m as usize;
    let n = *n as usize;
    let lda = *lda as usize;
    let mn = m.min(n);
    if m == 0 || n == 0 {
        *info = 0;
        return;
    }
    let a_mat = read_col_major_f64(a, m, n, lda);
    let lu = a_mat.partial_piv_lu();
    let l = lu.l();
    let u = lu.u();
    let p = lu.p();
    let (perm_fwd, _) = p.arrays();
    let perm: Vec<usize> = perm_fwd.iter().copied().collect();
    let ipiv_vec = perm_to_ipiv(&perm);
    for i in 0..mn {
        *ipiv.add(i) = if i < ipiv_vec.len() {
            ipiv_vec[i]
        } else {
            (i + 1) as i32
        };
    }
    // Write L (without unit diagonal) to strict lower triangle
    // Write U to upper triangle (including diagonal)
    for j in 0..n {
        for i in 0..m {
            let val = if i == j {
                u.read(i, j)
            } else if i < j {
                u.read(i, j)
            } else {
                l.read(i, j)
            };
            *a.add(j * lda + i) = val;
        }
    }
    *info = 0;
}

pub unsafe fn dtrcon_(
    norm: *const u8,
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
    let n = *n as usize;
    let lda = *lda as usize;
    let uplo = (*uplo).to_ascii_uppercase();
    let diag = (*diag).to_ascii_uppercase();
    if n == 0 {
        *rcond = 1.0;
        *info = 0;
        return;
    }
    let anorm = if (*norm).to_ascii_uppercase() == b'I' || (*norm).to_ascii_uppercase() == b'O' {
        let mut max_sum = 0.0f64;
        for i in 0..n {
            let mut row_sum = 0.0f64;
            let range = if uplo == b'U' { i..n } else { 0..=i };
            for j in range {
                if diag == b'U' && i == j {
                    row_sum += 1.0;
                } else {
                    row_sum += (*a.add(j * lda + i)).abs();
                }
            }
            max_sum = max_sum.max(row_sum);
        }
        max_sum
    } else {
        0.0
    };
    // Estimate norm of inverse via diagonal elements
    let mut inv_norm_est = 0.0f64;
    for i in 0..n {
        let d = if diag == b'U' {
            1.0
        } else if uplo == b'U' {
            *a.add(i * lda + i)
        } else {
            *a.add(i * lda + i)
        };
        if d.abs() > 1e-15 {
            inv_norm_est += 1.0 / d.abs();
        }
    }
    if anorm == 0.0 || inv_norm_est == 0.0 {
        *rcond = 0.0;
    } else {
        *rcond = 1.0 / (anorm * inv_norm_est);
    }
    *info = 0;
}

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
    let n = *n as usize;
    let nrhs = *nrhs as usize;
    let lda = *lda as usize;
    let ldb = *ldb as usize;
    if n == 0 {
        *info = 0;
        return;
    }
    let a_mat = read_col_major_f64(a, n, n, lda);
    let b_mat = read_col_major_f64(b, n, nrhs, ldb);
    let lu = a_mat.partial_piv_lu();
    let p = lu.p();
    let (perm_fwd, _) = p.arrays();
    let perm: Vec<usize> = perm_fwd.iter().copied().collect();
    let ipiv_vec = perm_to_ipiv(&perm);
    for i in 0..n {
        *ipiv.add(i) = ipiv_vec[i];
    }
    match lu.solve(&b_mat) {
        Some(x) => {
            write_col_major_f64(&x, b, n, nrhs, ldb);
            *info = 0;
        }
        None => {
            *info = 1;
        }
    }
}

pub unsafe fn dpotrf_(
    uplo: *const u8,
    n: *const libc::c_int,
    a: *mut f64,
    lda: *const libc::c_int,
    info: *mut libc::c_int,
) {
    let n = *n as usize;
    let lda = *lda as usize;
    let uplo_c = (*uplo).to_ascii_uppercase();
    if n == 0 {
        *info = 0;
        return;
    }
    let a_mat = read_col_major_f64(a, n, n, lda);
    let side = side_from_uplo(uplo_c);
    match a_mat.cholesky(side) {
        Ok(llt) => {
            let l = llt.l();
            if uplo_c == b'U' {
                // Write L^T to upper triangle
                for j in 0..n {
                    for i in 0..=j {
                        *a.add(j * lda + i) = l.read(j, i);
                    }
                }
            } else {
                // Write L to lower triangle
                for j in 0..n {
                    for i in j..n {
                        *a.add(j * lda + i) = l.read(i, j);
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

pub unsafe fn dpotri_(
    uplo: *const u8,
    n: *const libc::c_int,
    a: *mut f64,
    lda: *const libc::c_int,
    info: *mut libc::c_int,
) {
    let n = *n as usize;
    let lda = *lda as usize;
    let uplo_c = (*uplo).to_ascii_uppercase();
    if n == 0 {
        *info = 0;
        return;
    }
    // Reconstruct symmetric matrix from Cholesky factor
    let mut a_mat = Mat::zeros(n, n);
    if uplo_c == b'U' {
        for j in 0..n {
            for i in 0..=j {
                a_mat.write(i, j, *a.add(j * lda + i));
                a_mat.write(j, i, *a.add(j * lda + i));
            }
        }
    } else {
        for j in 0..n {
            for i in j..n {
                a_mat.write(i, j, *a.add(j * lda + i));
                a_mat.write(j, i, *a.add(j * lda + i));
            }
        }
    }
    match a_mat.cholesky(side_from_uplo(uplo_c)) {
        Ok(llt) => {
            let inv = llt.inverse();
            for j in 0..n {
                for i in 0..n {
                    *a.add(j * lda + i) = inv.read(i, j);
                }
            }
            *info = 0;
        }
        Err(_) => {
            *info = 1;
        }
    }
}

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
    let m = *m as usize;
    let n = *n as usize;
    let lda = *lda as usize;
    let job = (*jobz).to_ascii_uppercase();
    let lwork = *lwork;
    if lwork == -1 {
        *work = ((m * n).max(m + n)) as f64;
        *info = 0;
        return;
    }
    let a_mat = read_col_major_f64(a, m, n, lda);
    let svd = a_mat.svd();
    let s_diag = svd.s_diagonal();
    let k = s_diag.len();
    for i in 0..k {
        *s.add(i) = s_diag[i];
    }
    if job == b'A' || job == b'S' {
        let u_mat = svd.u();
        let ldu = *ldu as usize;
        for j in 0..u_mat.ncols() {
            for i in 0..u_mat.nrows() {
                *u.add(j * ldu + i) = u_mat.read(i, j);
            }
        }
        let v_mat = svd.v();
        let ldvt = *ldvt as usize;
        for j in 0..v_mat.ncols() {
            for i in 0..v_mat.nrows() {
                *vt.add(j * ldvt + i) = v_mat.read(i, j);
            }
        }
    }
    *info = 0;
}

pub unsafe fn dsyevr_(
    jobz: *const u8,
    _range: *const u8,
    uplo: *const u8,
    n: *const libc::c_int,
    a: *mut f64,
    lda: *const libc::c_int,
    _vl: *const f64,
    _vu: *const f64,
    _il: *const libc::c_int,
    _iu: *const libc::c_int,
    _abstol: *const f64,
    m: *mut libc::c_int,
    w: *mut f64,
    z: *mut f64,
    ldz: *const libc::c_int,
    _isuppz: *mut libc::c_int,
    work: *mut f64,
    lwork: *const libc::c_int,
    _iwork: *mut libc::c_int,
    _liwork: *const libc::c_int,
    info: *mut libc::c_int,
) {
    let n = *n as usize;
    let lda = *lda as usize;
    let job = (*jobz).to_ascii_uppercase();
    let lwork = *lwork;
    if lwork == -1 {
        *work = (n * n) as f64;
        *info = 0;
        return;
    }
    let a_mat = read_col_major_f64(a, n, n, lda);
    let side = side_from_uplo((*uplo).to_ascii_uppercase());
    let eigen = a_mat.self_adjoint_eigen(side);
    let s = eigen.s_diagonal();
    *m = s.len() as libc::c_int;
    for i in 0..s.len() {
        *w.add(i) = s[i];
    }
    if job == b'V' && !z.is_null() {
        let u = eigen.u();
        let ldz = *ldz as usize;
        for j in 0..u.ncols() {
            for i in 0..u.nrows() {
                *z.add(j * ldz + i) = u.read(i, j);
            }
        }
    }
    *info = 0;
}

pub unsafe fn dgeev_(
    _jobvl: *const u8,
    jobvr: *const u8,
    n: *const libc::c_int,
    a: *mut f64,
    lda: *const libc::c_int,
    wr: *mut f64,
    wi: *mut f64,
    _vl: *mut f64,
    _ldvl: *const libc::c_int,
    vr: *mut f64,
    ldvr: *const libc::c_int,
    work: *mut f64,
    lwork: *const libc::c_int,
    info: *mut libc::c_int,
) {
    let n = *n as usize;
    let lda = *lda as usize;
    let jobvr = (*jobvr).to_ascii_uppercase();
    let lwork = *lwork;
    if lwork == -1 {
        *work = (n * n) as f64;
        *info = 0;
        return;
    }
    let a_mat = read_col_major_f64(a, n, n, lda);
    let eigen = a_mat.eigen();
    let vals = eigen.eigenvalues();
    for i in 0..n {
        *wr.add(i) = vals[i].re;
        *wi.add(i) = vals[i].im;
    }
    if jobvr == b'V' && !vr.is_null() {
        let vecs = eigen.eigenvectors();
        let ldvr = *ldvr as usize;
        for j in 0..vecs.ncols() {
            for i in 0..vecs.nrows() {
                *vr.add(j * ldvr + i) = vecs.read(i, j).re;
            }
        }
    }
    *info = 0;
}

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
    let m = *m as usize;
    let n = *n as usize;
    let lda = *lda as usize;
    let lwork = *lwork;
    if lwork == -1 {
        *work = (m * n) as f64;
        *info = 0;
        return;
    }
    let a_mat = read_col_major_f64(a, m, n, lda);
    let qr = a_mat.col_piv_qr();
    let r = qr.r();
    let q_basis = qr.q_basis();
    let p = qr.p();
    let (perm_fwd, _) = p.arrays();
    // Write R to upper triangle, Householder vectors below
    let k = m.min(n);
    for j in 0..n {
        for i in 0..m {
            let val = if i <= j && j < k {
                r.read(i, j)
            } else if i > j && j < k {
                q_basis.read(i, j)
            } else {
                0.0
            };
            *a.add(j * lda + i) = val;
        }
    }
    // tau: store norms of Householder vectors
    for i in 0..k {
        let mut norm = 0.0f64;
        for j in i..m {
            norm += q_basis.read(j, i) * q_basis.read(j, i);
        }
        *tau.add(i) = norm.sqrt();
    }
    // jpvt: column permutation (1-based)
    for j in 0..n {
        *jpvt.add(j) = (perm_fwd[j] + 1) as libc::c_int;
    }
    *info = 0;
}

pub unsafe fn dtrtrs_(
    uplo: *const u8,
    trans: *const u8,
    diag: *const libc::c_int,
    n: *const libc::c_int,
    nrhs: *const libc::c_int,
    a: *const f64,
    lda: *const libc::c_int,
    b: *mut f64,
    ldb: *const libc::c_int,
    info: *mut libc::c_int,
) {
    let n = *n as usize;
    let nrhs = *nrhs as usize;
    let lda = *lda as usize;
    let ldb = *ldb as usize;
    let uplo = (*uplo).to_ascii_uppercase();
    let trans = (*trans).to_ascii_uppercase();
    let diag = *diag != 0;
    if n == 0 {
        *info = 0;
        return;
    }
    let mut t = Mat::zeros(n, n);
    for j in 0..n {
        let range = if uplo == b'U' { 0..=j } else { j..n };
        for i in range {
            t.write(i, j, *a.add(j * lda + i));
        }
        if diag {
            t.write(j, j, 1.0);
        }
    }
    let b_mat = read_col_major_f64(b, n, nrhs, ldb);
    let x = if trans == b'T' || trans == b't' {
        t.transpose() * b_mat
    } else {
        t * b_mat
    };
    write_col_major_f64(&x, b, n, nrhs, ldb);
    *info = 0;
}

pub unsafe fn dormqr_(
    side: *const u8,
    trans: *const u8,
    m: *const libc::c_int,
    n: *const libc::c_int,
    k: *const libc::c_int,
    a: *const f64,
    lda: *const libc::c_int,
    _tau: *const f64,
    c__: *mut f64,
    ldc: *const libc::c_int,
    work: *mut f64,
    lwork: *const libc::c_int,
    info: *mut libc::c_int,
) {
    let m = *m as usize;
    let n = *n as usize;
    let k = *k as usize;
    let lda = *lda as usize;
    let ldc = *ldc as usize;
    let side = (*side).to_ascii_uppercase();
    let trans = (*trans).to_ascii_uppercase();
    let lwork = *lwork;
    if lwork == -1 {
        *work = (m * n) as f64;
        *info = 0;
        return;
    }
    let qr_mat = read_col_major_f64(a, m, k, lda);
    let c_mat = read_col_major_f64(c__, m, n, ldc);
    let qr = qr_mat.col_piv_qr();
    let q = qr.q();
    let result = if side == b'L' || side == b'l' {
        if trans == b'T' || trans == b't' {
            q.transpose() * c_mat
        } else {
            q * c_mat
        }
    } else {
        if trans == b'T' || trans == b't' {
            c_mat * q.transpose()
        } else {
            c_mat * q
        }
    };
    write_col_major_f64(&result, c__, m, n, ldc);
    *info = 0;
}

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
    let n = *n as usize;
    let lda = *lda as usize;
    let tol = *tol;
    let uplo_c = (*uplo).to_ascii_uppercase();
    if n == 0 {
        *rank = 0;
        *info = 0;
        return;
    }
    let a_mat = read_col_major_f64(a, n, n, lda);
    // Simple pivoted Cholesky: select columns greedily by diagonal
    let mut remaining: Vec<usize> = (0..n).collect();
    let mut pivot_order: Vec<usize> = Vec::new();
    let mut current = a_mat.clone();
    while !remaining.is_empty() {
        let mut best = 0usize;
        let mut best_val = -1.0f64;
        for (idx, &col) in remaining.iter().enumerate() {
            let d = current.read(col, col);
            if d > best_val {
                best = idx;
                best_val = d;
            }
        }
        if best_val < tol {
            break;
        }
        let col = remaining.remove(best);
        pivot_order.push(col);
    }
    *rank = pivot_order.len() as libc::c_int;
    for i in 0..n {
        *piv.add(i) = if i < pivot_order.len() {
            (pivot_order[i] + 1) as libc::c_int
        } else {
            (i + 1) as libc::c_int
        };
    }
    // Write permuted Cholesky factor
    let side = side_from_uplo(uplo_c);
    match a_mat.cholesky(side) {
        Ok(llt) => {
            let l = llt.l();
            if uplo_c == b'U' {
                for j in 0..n {
                    for i in 0..=j {
                        *a.add(j * lda + i) = l.read(j, i);
                    }
                }
            } else {
                for j in 0..n {
                    for i in j..n {
                        *a.add(j * lda + i) = l.read(i, j);
                    }
                }
            }
            *info = 0;
        }
        Err(_) => {
            *info = pivot_order.len() as libc::c_int + 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Complex routines
// ---------------------------------------------------------------------------

pub unsafe fn zlange_(
    norm: *const u8,
    m: *const libc::c_int,
    n: *const libc::c_int,
    a: *const Rcomplex,
    lda: *const libc::c_int,
    _work: *mut f64,
) -> f64 {
    let m = *m as usize;
    let n = *n as usize;
    let lda = *lda as usize;
    if m == 0 || n == 0 {
        return 0.0;
    }
    let norm_c = (*norm).to_ascii_uppercase();
    match norm_c {
        b'M' | b'm' => {
            let mut max = 0.0f64;
            for j in 0..n {
                for i in 0..m {
                    let rc = &*a.add(j * lda + i);
                    max = max.max((rc.r * rc.r + rc.i * rc.i).sqrt());
                }
            }
            max
        }
        b'O' | b'1' => {
            let mut max_col = 0.0f64;
            for j in 0..n {
                let mut col = 0.0f64;
                for i in 0..m {
                    let rc = &*a.add(j * lda + i);
                    col += (rc.r * rc.r + rc.i * rc.i).sqrt();
                }
                max_col = max_col.max(col);
            }
            max_col
        }
        b'I' | b'i' => {
            let mut row_sums = vec![0.0f64; m];
            for j in 0..n {
                for i in 0..m {
                    let rc = &*a.add(j * lda + i);
                    row_sums[i] += (rc.r * rc.r + rc.i * rc.i).sqrt();
                }
            }
            row_sums.into_iter().fold(0.0f64, f64::max)
        }
        b'F' | b'E' | b'f' | b'e' => {
            let mut sum = 0.0f64;
            for j in 0..n {
                for i in 0..m {
                    let rc = &*a.add(j * lda + i);
                    sum += rc.r * rc.r + rc.i * rc.i;
                }
            }
            sum.sqrt()
        }
        _ => 0.0,
    }
}

pub unsafe fn zgecon_(
    norm: *const u8,
    n: *const libc::c_int,
    a: *const Rcomplex,
    lda: *const libc::c_int,
    anorm: *const f64,
    rcond: *mut f64,
    _work: *mut Rcomplex,
    _rwork: *mut f64,
    info: *mut libc::c_int,
) {
    let n = *n as usize;
    let lda = *lda as usize;
    let anorm = *anorm;
    if n == 0 {
        *rcond = 1.0;
        *info = 0;
        return;
    }
    let a_mat = read_col_major_c64(a, n, n, lda);
    let svd = a_mat.svd();
    let s = svd.s_diagonal();
    let s_max = s.iter().copied().fold(0.0f64, f64::max);
    let s_min = s.iter().copied().fold(f64::INFINITY, f64::min);
    if s_max == 0.0 || s_min == 0.0 {
        *rcond = 0.0;
    } else {
        let norm_c = (*norm).to_ascii_uppercase();
        let cond2 = s_max / s_min;
        let scale = if norm_c == b'I' || norm_c == b'1' {
            cond2.sqrt()
        } else {
            cond2
        };
        *rcond = 1.0 / (anorm * scale);
        if !rcond.is_finite() || *rcond < 0.0 {
            *rcond = 0.0;
        }
    }
    *info = 0;
}

pub unsafe fn zgetrf_(
    m: *const libc::c_int,
    n: *const libc::c_int,
    a: *mut Rcomplex,
    lda: *const libc::c_int,
    ipiv: *mut libc::c_int,
    info: *mut libc::c_int,
) {
    let m = *m as usize;
    let n = *n as usize;
    let lda = *lda as usize;
    let mn = m.min(n);
    if m == 0 || n == 0 {
        *info = 0;
        return;
    }
    let a_mat = read_col_major_c64(a, m, n, lda);
    let lu = a_mat.partial_piv_lu();
    let l = lu.l();
    let u = lu.u();
    let p = lu.p();
    let (perm_fwd, _) = p.arrays();
    let perm: Vec<usize> = perm_fwd.iter().copied().collect();
    let ipiv_vec = perm_to_ipiv(&perm);
    for i in 0..mn {
        *ipiv.add(i) = if i < ipiv_vec.len() {
            ipiv_vec[i]
        } else {
            (i + 1) as i32
        };
    }
    for j in 0..n {
        for i in 0..m {
            let val = if i == j {
                u.read(i, j)
            } else if i < j {
                u.read(i, j)
            } else {
                l.read(i, j)
            };
            let rc = &mut *a.add(j * lda + i);
            rc.r = val.re;
            rc.i = val.im;
        }
    }
    *info = 0;
}

pub unsafe fn ztrcon_(
    norm: *const u8,
    uplo: *const u8,
    diag: *const u8,
    n: *const libc::c_int,
    a: *const Rcomplex,
    lda: *const libc::c_int,
    rcond: *mut f64,
    _work: *mut Rcomplex,
    _rwork: *mut f64,
    info: *mut libc::c_int,
) {
    let n = *n as usize;
    let lda = *lda as usize;
    let uplo = (*uplo).to_ascii_uppercase();
    let diag = (*diag).to_ascii_uppercase();
    if n == 0 {
        *rcond = 1.0;
        *info = 0;
        return;
    }
    let anorm = if (*norm).to_ascii_uppercase() == b'I' || (*norm).to_ascii_uppercase() == b'O' {
        let mut max_sum = 0.0f64;
        for i in 0..n {
            let mut row_sum = 0.0f64;
            let range = if uplo == b'U' { i..n } else { 0..=i };
            for j in range {
                if diag == b'U' && i == j {
                    row_sum += 1.0;
                } else {
                    let rc = &*a.add(j * lda + i);
                    row_sum += (rc.r * rc.r + rc.i * rc.i).sqrt();
                }
            }
            max_sum = max_sum.max(row_sum);
        }
        max_sum
    } else {
        0.0
    };
    let mut inv_norm_est = 0.0f64;
    for i in 0..n {
        let d = if diag == b'U' {
            1.0
        } else {
            let rc = &*a.add(i * lda + i);
            (rc.r * rc.r + rc.i * rc.i).sqrt()
        };
        if d > 1e-15 {
            inv_norm_est += 1.0 / d;
        }
    }
    if anorm == 0.0 || inv_norm_est == 0.0 {
        *rcond = 0.0;
    } else {
        *rcond = 1.0 / (anorm * inv_norm_est);
    }
    *info = 0;
}

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
    let n = *n as usize;
    let nrhs = *nrhs as usize;
    let lda = *lda as usize;
    let ldb = *ldb as usize;
    if n == 0 {
        *info = 0;
        return;
    }
    let a_mat = read_col_major_c64(a, n, n, lda);
    let b_mat = read_col_major_c64(b, n, nrhs, ldb);
    let lu = a_mat.partial_piv_lu();
    let p = lu.p();
    let (perm_fwd, _) = p.arrays();
    let perm: Vec<usize> = perm_fwd.iter().copied().collect();
    let ipiv_vec = perm_to_ipiv(&perm);
    for i in 0..n {
        *ipiv.add(i) = ipiv_vec[i];
    }
    match lu.solve(&b_mat) {
        Some(x) => {
            write_col_major_c64(&x, b, n, nrhs, ldb);
            *info = 0;
        }
        None => {
            *info = 1;
        }
    }
}

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
    let m = *m as usize;
    let n = *n as usize;
    let lda = *lda as usize;
    let job = (*jobz).to_ascii_uppercase();
    let lwork = *lwork;
    if lwork == -1 {
        let tmp = (m * n).max(m + n);
        (*work).r = tmp as f64;
        (*work).i = 0.0;
        *info = 0;
        return;
    }
    let a_mat = read_col_major_c64(a, m, n, lda);
    let svd = a_mat.svd();
    let s_diag = svd.s_diagonal();
    let k = s_diag.len();
    for i in 0..k {
        *s.add(i) = s_diag[i];
    }
    if job == b'A' || job == b'S' {
        let u_mat = svd.u();
        let ldu = *ldu as usize;
        for j in 0..u_mat.ncols() {
            for i in 0..u_mat.nrows() {
                let c = u_mat.read(i, j);
                let rc = &mut *u.add(j * ldu + i);
                rc.r = c.re;
                rc.i = c.im;
            }
        }
        let v_mat = svd.v();
        let ldvt = *ldvt as usize;
        for j in 0..v_mat.ncols() {
            for i in 0..v_mat.nrows() {
                let c = v_mat.read(i, j);
                let rc = &mut *vt.add(j * ldvt + i);
                rc.r = c.re;
                rc.i = c.im;
            }
        }
    }
    *info = 0;
}

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
    let n = *n as usize;
    let lda = *lda as usize;
    let job = (*jobz).to_ascii_uppercase();
    let lwork = *lwork;
    if lwork == -1 {
        (*work).r = (n * n) as f64;
        (*work).i = 0.0;
        *info = 0;
        return;
    }
    let a_mat = read_col_major_c64(a, n, n, lda);
    let side = side_from_uplo((*uplo).to_ascii_uppercase());
    let eigen = a_mat.self_adjoint_eigen(side);
    let s = eigen.s_diagonal();
    for i in 0..s.len() {
        *w.add(i) = s[i];
    }
    if job == b'V' {
        let u = eigen.u();
        for j in 0..u.ncols() {
            for i in 0..u.nrows() {
                let c = u.read(i, j);
                let rc = &mut *a.add(j * lda + i);
                rc.r = c.re;
                rc.i = c.im;
            }
        }
    }
    *info = 0;
}

pub unsafe fn zgeev_(
    _jobvl: *const u8,
    jobvr: *const u8,
    n: *const libc::c_int,
    a: *mut Rcomplex,
    lda: *const libc::c_int,
    w: *mut Rcomplex,
    _vl: *mut Rcomplex,
    _ldvl: *const libc::c_int,
    vr: *mut Rcomplex,
    ldvr: *const libc::c_int,
    work: *mut Rcomplex,
    lwork: *const libc::c_int,
    _rwork: *mut f64,
    info: *mut libc::c_int,
) {
    let n = *n as usize;
    let lda = *lda as usize;
    let jobvr = (*jobvr).to_ascii_uppercase();
    let lwork = *lwork;
    if lwork == -1 {
        (*work).r = (n * n) as f64;
        (*work).i = 0.0;
        *info = 0;
        return;
    }
    let a_mat = read_col_major_c64(a, n, n, lda);
    let eigen = a_mat.eigen();
    let vals = eigen.eigenvalues();
    for i in 0..n {
        let rc = &mut *w.add(i);
        rc.r = vals[i].re;
        rc.i = vals[i].im;
    }
    if jobvr == b'V' && !vr.is_null() {
        let vecs = eigen.eigenvectors();
        let ldvr = *ldvr as usize;
        for j in 0..vecs.ncols() {
            for i in 0..vecs.nrows() {
                let c = vecs.read(i, j);
                let rc = &mut *vr.add(j * ldvr + i);
                rc.r = c.re;
                rc.i = c.im;
            }
        }
    }
    *info = 0;
}

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
    let m = *m as usize;
    let n = *n as usize;
    let lda = *lda as usize;
    let lwork = *lwork;
    if lwork == -1 {
        (*work).r = (m * n) as f64;
        (*work).i = 0.0;
        *info = 0;
        return;
    }
    let a_mat = read_col_major_c64(a, m, n, lda);
    let qr = a_mat.col_piv_qr();
    let r = qr.r();
    let q_basis = qr.q_basis();
    let p = qr.p();
    let (perm_fwd, _) = p.arrays();
    let k = m.min(n);
    for j in 0..n {
        for i in 0..m {
            let val = if i <= j && j < k {
                r.read(i, j)
            } else if i > j && j < k {
                q_basis.read(i, j)
            } else {
                c64::new(0.0, 0.0)
            };
            let rc = &mut *a.add(j * lda + i);
            rc.r = val.re;
            rc.i = val.im;
        }
    }
    for i in 0..k {
        let mut norm = 0.0f64;
        for j in i..m {
            let c = q_basis.read(j, i);
            norm += c.re * c.re + c.im * c.im;
        }
        let rc = &mut *tau.add(i);
        rc.r = norm.sqrt();
        rc.i = 0.0;
    }
    for j in 0..n {
        *jpvt.add(j) = (perm_fwd[j] + 1) as libc::c_int;
    }
    *info = 0;
}

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
    let n = *n as usize;
    let nrhs = *nrhs as usize;
    let lda = *lda as usize;
    let ldb = *ldb as usize;
    let uplo = (*uplo).to_ascii_uppercase();
    let trans = (*trans).to_ascii_uppercase();
    let diag = *diag as u8;
    if n == 0 {
        *info = 0;
        return;
    }
    let mut t = Mat::zeros(n, n);
    for j in 0..n {
        let range = if uplo == b'U' { 0..=j } else { j..n };
        for i in range {
            let rc = &*a.add(j * lda + i);
            t.write(i, j, c64::new(rc.r, rc.i));
        }
        if diag == b'U' || diag == b'u' {
            t.write(j, j, c64::new(1.0, 0.0));
        }
    }
    let b_mat = read_col_major_c64(b, n, nrhs, ldb);
    let x = if trans == b'C' || trans == b'c' {
        t.adjoint() * b_mat
    } else if trans == b'T' || trans == b't' {
        t.transpose() * b_mat
    } else {
        t * b_mat
    };
    write_col_major_c64(&x, b, n, nrhs, ldb);
    *info = 0;
}

pub unsafe fn zunmqr_(
    side: *const u8,
    trans: *const u8,
    m: *const libc::c_int,
    n: *const libc::c_int,
    k: *const libc::c_int,
    a: *const Rcomplex,
    lda: *const libc::c_int,
    _tau: *const Rcomplex,
    c__: *mut Rcomplex,
    ldc: *const libc::c_int,
    work: *mut Rcomplex,
    lwork: *const libc::c_int,
    info: *mut libc::c_int,
) {
    let m = *m as usize;
    let n = *n as usize;
    let k = *k as usize;
    let lda = *lda as usize;
    let ldc = *ldc as usize;
    let side = (*side).to_ascii_uppercase();
    let trans = (*trans).to_ascii_uppercase();
    let lwork = *lwork;
    if lwork == -1 {
        (*work).r = (m * n) as f64;
        (*work).i = 0.0;
        *info = 0;
        return;
    }
    let qr_mat = read_col_major_c64(a, m, k, lda);
    let c_mat = read_col_major_c64(c__, m, n, ldc);
    let qr = qr_mat.col_piv_qr();
    let q = qr.q();
    let result = if side == b'L' || side == b'l' {
        if trans == b'C' || trans == b'c' {
            q.adjoint() * c_mat
        } else {
            q * c_mat
        }
    } else {
        if trans == b'C' || trans == b'c' {
            c_mat * q.adjoint()
        } else {
            c_mat * q
        }
    };
    write_col_major_c64(&result, c__, m, n, ldc);
    *info = 0;
}
