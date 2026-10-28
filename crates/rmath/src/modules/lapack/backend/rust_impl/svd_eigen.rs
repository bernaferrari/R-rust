use super::*;

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
        let jobz_byte = (*jobz).to_ascii_uppercase();
        if !matches!(jobz_byte, b'A' | b'S' | b'O' | b'N') {
            *info = -1;
            return;
        }
        let m = *m as usize;
        let n = *n as usize;
        let lda = *lda as usize;
        let ldu_val = *ldu as usize;
        let ldvt_val = *ldvt as usize;
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
                    b'O' if m >= n => {
                        write_matref_f64(u_mat, a, m, n, lda);
                        write_matref_f64(v_mat.transpose(), vt, n, n, ldvt_val);
                    }
                    b'O' => {
                        write_matref_f64(u_mat, u, m, m, ldu_val);
                        write_matref_f64(v_mat.transpose(), a, m, n, lda);
                    }
                    b'S' => {
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
                    _ => unreachable!("validated SVD vector mode"),
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
                    // Real eigenvector, normalized to unit Euclidean norm
                    // (LAPACK contract for DGEEV right eigenvectors).
                    let mut norm_sq = 0.0f64;
                    for i in 0..n_val {
                        let re = evecs_c64[(i, idx)].re;
                        norm_sq += re * re;
                    }
                    let scale = if norm_sq > 0.0 {
                        1.0 / norm_sq.sqrt()
                    } else {
                        0.0
                    };
                    for i in 0..n_val {
                        *vr.add(i + j * ldvr_val) = evecs_c64[(i, idx)].re * scale;
                    }
                    j += 1;
                } else {
                    // Complex pair: real part in col j, imag part in col j+1
                    let imag_sign = if ev.im > 0.0 { 1.0 } else { -1.0 };
                    let mut norm_sq = 0.0f64;
                    for i in 0..n_val {
                        let c = evecs_c64[(i, idx)];
                        norm_sq += c.re * c.re + c.im * c.im;
                    }
                    let scale = if norm_sq > 0.0 {
                        1.0 / norm_sq.sqrt()
                    } else {
                        0.0
                    };
                    for i in 0..n_val {
                        let c = evecs_c64[(i, idx)];
                        *vr.add(i + j * ldvr_val) = c.re * scale;
                        *vr.add(i + (j + 1) * ldvr_val) = c.im * imag_sign * scale;
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
        let jobz_byte = (*jobz).to_ascii_uppercase();
        if !matches!(jobz_byte, b'A' | b'S' | b'O' | b'N') {
            *info = -1;
            return;
        }
        let m_val = *m as usize;
        let n_val = *n as usize;
        let lda_val = *lda as usize;
        let ldu_val = *ldu as usize;
        let ldvt_val = *ldvt as usize;
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
                b'O' if m_val >= n_val => {
                    write_matref_c64(u_mat, a, m_val, n_val, lda_val);
                    let vt_owned = v_mat.adjoint().to_owned();
                    write_owned_c64(&vt_owned, vt, n_val, n_val, ldvt_val);
                }
                b'O' => {
                    write_matref_c64(u_mat, u, m_val, m_val, ldu_val);
                    let vt_owned = v_mat.adjoint().to_owned();
                    write_owned_c64(&vt_owned, a, m_val, n_val, lda_val);
                }
                b'S' => {
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
                _ => unreachable!("validated SVD vector mode"),
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

        // Write right eigenvectors, each column normalized to unit
        // Euclidean norm (LAPACK contract for ZGEEV right eigenvectors).
        if jobvr_byte == b'V' || jobvr_byte == b'v' {
            for col in 0..n_val {
                let mut norm_sq = 0.0f64;
                for i in 0..n_val {
                    let c = evecs[(i, col)];
                    norm_sq += c.re * c.re + c.im * c.im;
                }
                let scale = if norm_sq > 0.0 {
                    1.0 / norm_sq.sqrt()
                } else {
                    0.0
                };
                for i in 0..n_val {
                    let c = evecs[(i, col)];
                    *vr.add(i + col * ldvr_val) = Rcomplex {
                        r: c.re * scale,
                        i: c.im * scale,
                    };
                }
            }
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
