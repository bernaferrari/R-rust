use super::*;

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
