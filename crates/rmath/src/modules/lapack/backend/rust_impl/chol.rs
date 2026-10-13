use super::*;

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
pub(crate) fn invert_upper_tri(u: &[f64], n: usize) -> Vec<f64> {
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
pub(crate) fn invert_lower_tri(l: &[f64], n: usize) -> Vec<f64> {
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
pub(crate) fn multiply_upper_by_transpose(inv: &[f64], n: usize) -> Vec<f64> {
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
pub(crate) fn multiply_lower_transpose_by_self(inv: &[f64], n: usize) -> Vec<f64> {
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
