use super::*;

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
