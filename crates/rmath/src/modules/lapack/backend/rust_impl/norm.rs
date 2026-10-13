use super::*;

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
