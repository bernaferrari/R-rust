//! LINPACK/BLAS kernels exactly as the Fortran reference arranges — the
//! column-pivoted QR decomposition dqrdc and the dqrsl solve application the
//! loess weighted least squares assembles from, with the vector BLAS they
//! share. All indexing inside is zero-based over the leading dimension; the
//! arithmetic order follows the original statement-for-statement exactly so
//! rounding matches bit-for-bit.

/// ddot — double precision dot product with unit strides.
pub fn ddot(x: &[f64], y: &[f64]) -> f64 {
    x.iter().zip(y.iter()).map(|(a, b)| a * b).sum()
}

/// daxpy — y += a * x with unit strides.
pub fn daxpy(a: f64, x: &[f64], y: &mut [f64]) {
    for (yi, xi) in y.iter_mut().zip(x.iter()) {
        *yi += a * xi;
    }
}

/// Euclidean norm exactly as dnrm2's scaled accumulation.
pub fn dnrm2(x: &[f64]) -> f64 {
    let mut scale = 0.0f64;
    let mut ssq = 1.0f64;
    for &v in x {
        if v != 0.0 {
            let av = v.abs();
            if scale < av {
                let t = scale / av;
                ssq = 1.0 + ssq * t * t;
                scale = av;
            } else {
                let t = av / scale;
                ssq += t * t;
            }
        }
    }
    scale * ssq.sqrt()
}

/// dqrdc — LINPACK QR decomposition with column pivoting, exactly as the
/// reference implementation arranges the arithmetic: the subcolumn scaled by
/// its signed norm including the diagonal, the diagonal incremented by one,
/// reflections applied through the dot/daxpy pair, norms downdated with the
/// 0.05 recompute threshold, qraux carrying the pre-swap diagonal and the R
/// diagonal left as the negative norm. `x` is n×p column-major with leading
/// dimension n; `jpvt` answers the original 1-based column index now sitting
/// at each position.
pub fn dqrdc(x: &mut [f64], n: usize, p: usize, qraux: &mut [f64], jpvt: &mut [i64], pivot: bool) {
    let at = |i: usize, j: usize| i + j * n;
    let mut work = vec![0.0f64; p];
    let mut nrm = vec![0.0f64; p];
    for j in 0..p {
        jpvt[j] = (j + 1) as i64;
        qraux[j] = 0.0;
    }
    for j in 0..p {
        let col: Vec<f64> = (0..n).map(|i| x[at(i, j)]).collect();
        nrm[j] = dnrm2(&col);
        work[j] = nrm[j];
    }
    let lup = n.min(p);
    for l in 0..lup {
        if pivot && l + 1 < p {
            let mut maxj = l;
            for j in (l + 1)..p {
                if nrm[j] > nrm[maxj] {
                    maxj = j;
                }
            }
            if maxj != l {
                for i in 0..n {
                    x.swap(at(i, l), at(i, maxj));
                }
                nrm.swap(l, maxj);
                work.swap(l, maxj);
                jpvt.swap(l, maxj);
            }
        }
        qraux[l] = 0.0;
        if l + 1 == n {
            continue;
        }
        let col: Vec<f64> = (l..n).map(|i| x[at(i, l)]).collect();
        let mut nrmxl = dnrm2(&col);
        if nrmxl == 0.0 {
            continue;
        }
        if x[at(l, l)] != 0.0 {
            nrmxl = nrmxl.copysign(x[at(l, l)]);
        }
        // Scale the entire subcolumn including the diagonal, then increment.
        for i in l..n {
            x[at(i, l)] /= nrmxl;
        }
        x[at(l, l)] += 1.0;
        // Apply the reflection to the remaining columns.
        for j in (l + 1)..p {
            let mut t = 0.0;
            for i in l..n {
                t += x[at(i, l)] * x[at(i, j)];
            }
            t = -t / x[at(l, l)];
            for i in l..n {
                x[at(i, j)] += t * x[at(i, l)];
            }
            if pivot && x[at(l, j)] != 0.0 && nrm[j] != 0.0 {
                let mut tt = 1.0 - (x[at(l, j)].abs() / nrm[j]).powi(2);
                tt = tt.max(0.0);
                let t = tt;
                let ratio = nrm[j] / work[j];
                let tt2 = 1.0 + 0.05 * tt * ratio * ratio;
                if tt2 != 1.0 {
                    nrm[j] *= t.sqrt();
                } else {
                    let colj: Vec<f64> = ((l + 1)..n).map(|i| x[at(i, j)]).collect();
                    nrm[j] = dnrm2(&colj);
                    work[j] = nrm[j];
                }
            }
        }
        qraux[l] = x[at(l, l)];
        x[at(l, l)] = -nrmxl;
    }
}

/// The dqrsl pair the loess kernel consumes: qty = Qᵀ b exactly as LINPACK's
/// accumulation dividing by qraux, then R c = qty[0..k] by back substitution
/// with the pivot permutation undone in the answer order.
pub fn qr_solve(x: &[f64], qraux: &[f64], jpvt: &[i64], b: &[f64], n: usize, p: usize, k: usize) -> Vec<f64> {
    let at = |i: usize, j: usize| i + j * n;
    let mut qy = b.to_vec();
    let ju = k.min(if n > 0 { n - 1 } else { 0 });
    for j in 0..ju {
        if qraux[j] != 0.0 {
            // LINPACK restores x(j,j) = qraux(j) for the reflection exactly —
            // the packed R diagonal is not the Householder component.
            let mut t = qraux[j] * qy[j];
            for i in (j + 1)..n {
                t += x[at(i, j)] * qy[i];
            }
            t = -t / qraux[j];
            qy[j] += t * qraux[j];
            for i in (j + 1)..n {
                qy[i] += t * x[at(i, j)];
            }
        }
    }
    let mut c = vec![0.0f64; k];
    if k > 0 {
        c[k - 1] = qy[k - 1] / x[at(k - 1, k - 1)];
        for jj in (0..k - 1).rev() {
            let mut acc = qy[jj];
            for i in (jj + 1)..k {
                acc -= x[at(jj, i)] * c[i];
            }
            c[jj] = acc / x[at(jj, jj)];
        }
    }
    // Un-permute: position j of the pivoted factorization answers original
    // column jpvt[j]; coefficients beyond rank k are zero exactly as LINPACK.
    let mut coef = vec![0.0f64; p];
    for j in 0..p {
        let orig = jpvt[j].abs() as usize - 1;
        coef[orig] = if j < k { c[j] } else { 0.0 };
    }
    coef
}

/// qty_only — the Qᵀ b application alone, answering the transformed vector.
/// The covariance machinery consumes it for the leverage computations.
pub fn qty(x: &[f64], qraux: &[f64], b: &[f64], n: usize, k: usize) -> Vec<f64> {
    let at = |i: usize, j: usize| i + j * n;
    let mut qy = b.to_vec();
    let ju = k.min(if n > 0 { n - 1 } else { 0 });
    for j in 0..ju {
        if qraux[j] != 0.0 {
            let mut t = qraux[j] * qy[j];
            for i in (j + 1)..n {
                t += x[at(i, j)] * qy[i];
            }
            t = -t / qraux[j];
            qy[j] += t * qraux[j];
            for i in (j + 1)..n {
                qy[i] += t * x[at(i, j)];
            }
        }
    }
    qy
}
