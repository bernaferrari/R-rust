use super::*;

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
                    let d = if is_unit {
                        1.0
                    } else if trans {
                        aij(i, i)
                    } else {
                        aij(i, i)
                    };
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
                        isave[1] = x
                            .iter()
                            .enumerate()
                            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                            .map(|(i, _)| i)
                            .unwrap_or(0) as i32;
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
                        isave[1] = x
                            .iter()
                            .enumerate()
                            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                            .map(|(i, _)| i)
                            .unwrap_or(0) as i32;
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
                        let temp =
                            x.iter().map(|z| z.abs()).sum::<f64>() / (n_val as f64 * 3.0) * 2.0;
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
                let z = if conj_trans {
                    aij(col, row)
                } else {
                    aij(row, col)
                };
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
                *z = Rcomplex {
                    r: z.r / m,
                    i: -z.i / m,
                };
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
