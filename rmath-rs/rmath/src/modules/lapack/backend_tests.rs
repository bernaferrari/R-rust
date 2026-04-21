//! Conformance tests for LAPACK backend parity.
//!
//! Tests verify that whichever backend is active — Fortran FFI or pure-Rust faer —
//! produces numerically correct results for standard LAPACK routines.
//! Both backends are exercised against the same mathematical ground truth,
//! ensuring output parity.

use super::backend;

// ── Tolerances ──────────────────────────────────────────────────
const REL_TOL: f64 = 1e-10;
const ABS_TOL: f64 = 1e-8;

// ── Assertion helpers ───────────────────────────────────────────

fn assert_close(a: f64, b: f64, ctx: &str) {
    let diff = (a - b).abs();
    let scale = a.abs().max(b.abs()).max(1.0);
    assert!(
        diff <= ABS_TOL || diff <= REL_TOL * scale,
        "{ctx}: {a} !≈ {b}  (diff={diff}, rel={})",
        diff / scale,
    );
}

fn assert_close_slice(got: &[f64], expected: &[f64], ctx: &str) {
    assert_eq!(got.len(), expected.len(), "{ctx}: length mismatch");
    for (i, (g, e)) in got.iter().zip(expected.iter()).enumerate() {
        assert_close(*g, *e, &format!("{ctx}[{i}]"));
    }
}

/// Verify that A * P ≈ Q * R for a QR factorisation with column pivoting.
/// `a` is the output of dgeqp3 (column-major, lda = m).
/// `jpvt` is 1-based, `tau` has length k = min(m,n).
fn verify_qr_pivot(a_orig: &[f64], a: &[f64], m: usize, n: usize, jpvt: &[i32], tau: &[f64]) {
    let k = m.min(n);

    // Extract R (upper triangle, including diagonal)
    let mut r = vec![0.0f64; m * n];
    for j in 0..n {
        for i in 0..=j.min(m - 1) {
            r[i + j * m] = a[i + j * m];
        }
    }

    // Build Q explicitly from Householder vectors stored below diagonal
    let mut q = vec![0.0f64; m * m];
    for i in 0..m {
        q[i + i * m] = 1.0;
    }

    for jj in (0..k).rev() {
        let remaining = m - jj;
        if tau[jj] == 0.0 {
            continue;
        }
        let mut v = vec![0.0f64; remaining];
        v[0] = 1.0;
        for i in 1..remaining {
            v[i] = a[jj + i + jj * m];
        }
        // Apply H = I - tau * v * v^T  to Q[jj:m, :]
        for col in 0..m {
            let mut w = q[jj + col * m];
            for i in 1..remaining {
                w += v[i] * q[jj + i + col * m];
            }
            w *= tau[jj];
            q[jj + col * m] -= w;
            for i in 1..remaining {
                q[jj + i + col * m] -= w * v[i];
            }
        }
    }

    // Compute Q * R
    let mut qr = vec![0.0f64; m * n];
    for j in 0..n {
        for i in 0..m {
            let mut sum = 0.0;
            for l in 0..=j.min(m - 1) {
                sum += q[i + l * m] * r[l + j * m];
            }
            qr[i + j * m] = sum;
        }
    }

    // Permute original columns according to jpvt (1-based)
    let mut ap = vec![0.0f64; m * n];
    for j in 0..n {
        let src_col = (jpvt[j] - 1) as usize;
        for i in 0..m {
            ap[i + j * m] = a_orig[i + src_col * m];
        }
    }

    assert_close_slice(&qr, &ap, "Q*R vs A*P");
}

// ── Matrix helpers (column-major throughout) ────────────────────

/// Deterministic pseudo-random values in [0.5, 5.0).
fn seed_matrix(m: usize, n: usize, seed: u64) -> Vec<f64> {
    let mut s = seed;
    (0..m * n)
        .map(|_| {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            0.5 + ((s >> 40) as f64) / ((1u64 << 24) as f64) * 4.5
        })
        .collect()
}

/// Symmetric positive definite: A = BᵀB + n·I  (B ∈ ℝ^{2n × n}).
fn make_spd(n: usize, seed: u64) -> Vec<f64> {
    let b = seed_matrix(2 * n, n, seed);
    let mut a = vec![0.0; n * n];
    for i in 0..n {
        for j in 0..n {
            let mut sum = 0.0;
            for k in 0..(2 * n) {
                sum += b[k + i * (2 * n)] * b[k + j * (2 * n)];
            }
            a[i + j * n] = sum;
        }
    }
    for i in 0..n {
        a[i + i * n] += n as f64;
    }
    a
}

/// Symmetric (not necessarily PD): A = (B + Bᵀ) / 2.
fn make_symmetric(n: usize, seed: u64) -> Vec<f64> {
    let raw = seed_matrix(n, n, seed);
    let mut a = vec![0.0; n * n];
    for i in 0..n {
        for j in 0..n {
            a[i + j * n] = (raw[i + j * n] + raw[j + i * n]) / 2.0;
        }
    }
    a
}

/// C = A · B   (A: m×k, B: k×n, C: m×n, all column-major with lda=m, ldb=k, ldc=m).
fn mat_mul(a: &[f64], b: &[f64], m: usize, k: usize, n: usize) -> Vec<f64> {
    let mut c = vec![0.0; m * n];
    for j in 0..n {
        for i in 0..m {
            let mut sum = 0.0;
            for p in 0..k {
                sum += a[i + p * m] * b[p + j * k];
            }
            c[i + j * m] = sum;
        }
    }
    c
}

/// Frobenius norm.
fn frob(a: &[f64], m: usize, n: usize) -> f64 {
    let mut s = 0.0;
    for j in 0..n {
        for i in 0..m {
            s += a[i + j * m] * a[i + j * m];
        }
    }
    s.sqrt()
}

/// Relative residual ‖AX−B‖_F / (‖A‖_F · ‖X‖_F).
fn solve_residual(a: &[f64], x: &[f64], b: &[f64], n: usize, nrhs: usize) -> f64 {
    let ax = mat_mul(a, x, n, n, nrhs);
    let mut diff = vec![0.0; n * nrhs];
    for i in 0..(n * nrhs) {
        diff[i] = ax[i] - b[i];
    }
    frob(&diff, n, nrhs) / (frob(a, n, n) * frob(x, n, nrhs).max(1e-15))
}

// ════════════════════════════════════════════════════════════════
// dlange_  –  matrix norms
// ════════════════════════════════════════════════════════════════

#[test]
fn test_dlange_3x3_known() {
    // Row view: [[1,2,3],[4,5,6],[7,8,9]]
    // Column-major: [1,4,7, 2,5,8, 3,6,9]
    let a: Vec<f64> = vec![1.0, 4.0, 7.0, 2.0, 5.0, 8.0, 3.0, 6.0, 9.0];
    let n = 3i32;
    let mut work = vec![0.0; 3];

    // 1-norm (max column sum): max(1+4+7, 2+5+8, 3+6+9) = 18
    let norm_one = unsafe {
        backend::dlange_(&b'O', &n, &n, a.as_ptr(), &n, work.as_mut_ptr())
    };
    assert_close(norm_one, 18.0, "dlange 1-norm");

    // ∞-norm (max row sum): max(1+2+3, 4+5+6, 7+8+9) = 24
    let norm_inf = unsafe {
        backend::dlange_(&b'I', &n, &n, a.as_ptr(), &n, work.as_mut_ptr())
    };
    assert_close(norm_inf, 24.0, "dlange inf-norm");

    // Frobenius: sqrt(1²+4²+7²+2²+5²+8²+3²+6²+9²) = sqrt(285)
    let norm_f = unsafe {
        backend::dlange_(&b'F', &n, &n, a.as_ptr(), &n, work.as_mut_ptr())
    };
    assert_close(norm_f, 285.0f64.sqrt(), "dlange F-norm");

    // Max element: 9
    let norm_m = unsafe {
        backend::dlange_(&b'M', &n, &n, a.as_ptr(), &n, work.as_mut_ptr())
    };
    assert_close(norm_m, 9.0, "dlange max");
}

#[test]
fn test_dlange_5x3_random() {
    let a = seed_matrix(5, 3, 42);
    let m = 5i32;
    let n = 3i32;
    let mut work = vec![0.0; 5];

    // Verify 1-norm against manual computation
    let mut expected_one = 0.0f64;
    for j in 0..3 {
        let mut col_sum = 0.0;
        for i in 0..5 {
            col_sum += a[i + j * 5].abs();
        }
        expected_one = expected_one.max(col_sum);
    }
    let got = unsafe {
        backend::dlange_(&b'O', &m, &n, a.as_ptr(), &m, work.as_mut_ptr())
    };
    assert_close(got, expected_one, "dlange 5×3 1-norm");

    // Verify Frobenius
    let mut expected_f = 0.0;
    for &v in &a {
        expected_f += v * v;
    }
    let got_f = unsafe {
        backend::dlange_(&b'F', &m, &n, a.as_ptr(), &m, work.as_mut_ptr())
    };
    assert_close(got_f, expected_f.sqrt(), "dlange 5×3 F-norm");
}

// ════════════════════════════════════════════════════════════════
// dgetrf_  –  LU factorisation
// ════════════════════════════════════════════════════════════════

#[test]
fn test_dgetrf_3x3() {
    let a_orig = seed_matrix(3, 3, 100);
    let mut a = a_orig.clone();
    let n = 3i32;
    let mut ipiv = vec![0i32; 3];
    let mut info = 0i32;

    unsafe {
        backend::dgetrf_(&n, &n, a.as_mut_ptr(), &n, ipiv.as_mut_ptr(), &mut info);
    }
    assert_eq!(info, 0, "dgetrf info");

    // Pivot indices must be valid (1-based, 1..=n)
    for i in 0..3 {
        assert!(
            (1..=3).contains(&ipiv[i]),
            "dgetrf ipiv[{i}] = {} out of range",
            ipiv[i],
        );
    }

    // Verify L has unit diagonal (implicit) and U diagonal is non-zero
    for i in 0..3 {
        assert!(a[i + i * 3].abs() > 1e-15, "dgetrf U diagonal [{i}] is ~zero");
    }
}

// ════════════════════════════════════════════════════════════════
// dgesv_  –  solve Ax = B via LU
// ════════════════════════════════════════════════════════════════

#[test]
fn test_dgesv_3x3() {
    let a_orig = seed_matrix(3, 3, 200);
    let b_orig = seed_matrix(3, 1, 201);
    let mut a = a_orig.clone();
    let mut b = b_orig.clone();
    let n = 3i32;
    let nrhs = 1i32;
    let mut ipiv = vec![0i32; 3];
    let mut info = 0i32;

    unsafe {
        backend::dgesv_(
            &n, &nrhs,
            a.as_mut_ptr(), &n,
            ipiv.as_mut_ptr(),
            b.as_mut_ptr(), &n,
            &mut info,
        );
    }
    assert_eq!(info, 0, "dgesv 3×3 info");

    let res = solve_residual(&a_orig, &b, &b_orig, 3, 1);
    assert!(res < 1e-10, "dgesv 3×3 residual: {res}");
}

#[test]
fn test_dgesv_5x5() {
    let a_orig = seed_matrix(5, 5, 300);
    let b_orig = seed_matrix(5, 2, 301);
    let mut a = a_orig.clone();
    let mut b = b_orig.clone();
    let n = 5i32;
    let nrhs = 2i32;
    let mut ipiv = vec![0i32; 5];
    let mut info = 0i32;

    unsafe {
        backend::dgesv_(
            &n, &nrhs,
            a.as_mut_ptr(), &n,
            ipiv.as_mut_ptr(),
            b.as_mut_ptr(), &n,
            &mut info,
        );
    }
    assert_eq!(info, 0, "dgesv 5×5 info");

    let res = solve_residual(&a_orig, &b, &b_orig, 5, 2);
    assert!(res < 1e-10, "dgesv 5×5 residual: {res}");
}

#[test]
fn test_dgesv_10x10() {
    let a_orig = seed_matrix(10, 10, 400);
    let b_orig = seed_matrix(10, 1, 401);
    let mut a = a_orig.clone();
    let mut b = b_orig.clone();
    let n = 10i32;
    let nrhs = 1i32;
    let mut ipiv = vec![0i32; 10];
    let mut info = 0i32;

    unsafe {
        backend::dgesv_(
            &n, &nrhs,
            a.as_mut_ptr(), &n,
            ipiv.as_mut_ptr(),
            b.as_mut_ptr(), &n,
            &mut info,
        );
    }
    assert_eq!(info, 0, "dgesv 10×10 info");

    let res = solve_residual(&a_orig, &b, &b_orig, 10, 1);
    assert!(res < 1e-9, "dgesv 10×10 residual: {res}");
}

// ════════════════════════════════════════════════════════════════
// dpotrf_  –  Cholesky factorisation
// ════════════════════════════════════════════════════════════════

#[test]
fn test_dpotrf_3x3_upper() {
    let a_orig = make_spd(3, 500);
    let mut a = a_orig.clone();
    let n = 3i32;
    let mut info = 0i32;

    unsafe {
        backend::dpotrf_(&b'U', &n, a.as_mut_ptr(), &n, &mut info);
    }
    assert_eq!(info, 0, "dpotrf 3×3 info");

    // Extract upper triangle U
    let mut u = vec![0.0; 9];
    for j in 0..3 {
        for i in 0..=j {
            u[i + j * 3] = a[i + j * 3];
        }
    }

    // Verify Uᵀ·U = A
    let mut ut_u = vec![0.0; 9];
    for j in 0..3 {
        for i in 0..3 {
            let mut s = 0.0;
            for k in 0..3 {
                s += u[k + i * 3] * u[k + j * 3]; // Uᵀ[i,k] · U[k,j]
            }
            ut_u[i + j * 3] = s;
        }
    }
    assert_close_slice(&ut_u, &a_orig, "dpotrf UᵀU = A");
}

#[test]
fn test_dpotrf_5x5_lower() {
    let a_orig = make_spd(5, 600);
    let mut a = a_orig.clone();
    let n = 5i32;
    let mut info = 0i32;

    unsafe {
        backend::dpotrf_(&b'L', &n, a.as_mut_ptr(), &n, &mut info);
    }
    assert_eq!(info, 0, "dpotrf 5×5 L info");

    // Extract lower triangle L
    let mut l = vec![0.0; 25];
    for j in 0..5 {
        for i in j..5 {
            l[i + j * 5] = a[i + j * 5];
        }
    }

    // Verify L·Lᵀ = A
    let mut ll_t = vec![0.0; 25];
    for j in 0..5 {
        for i in 0..5 {
            let mut s = 0.0;
            for k in 0..5 {
                s += l[i + k * 5] * l[j + k * 5]; // L[i,k] · Lᵀ[k,j]
            }
            ll_t[i + j * 5] = s;
        }
    }
    assert_close_slice(&ll_t, &a_orig, "dpotrf LLᵀ = A");
}

// ════════════════════════════════════════════════════════════════
// dpotri_  –  inverse from Cholesky factor
// ════════════════════════════════════════════════════════════════

#[test]
fn test_dpotri_3x3() {
    let a_orig = make_spd(3, 700);
    let n = 3i32;
    let mut info = 0i32;

    // Cholesky
    let mut a = a_orig.clone();
    unsafe {
        backend::dpotrf_(&b'U', &n, a.as_mut_ptr(), &n, &mut info);
    }
    assert_eq!(info, 0, "dpotri: dpotrf step");

    // Inverse
    unsafe {
        backend::dpotri_(&b'U', &n, a.as_mut_ptr(), &n, &mut info);
    }
    assert_eq!(info, 0, "dpotri info");

    // Symmetrise (dpotri writes only the specified triangle)
    let mut ainv = a.clone();
    for j in 0..3 {
        for i in (j + 1)..3 {
            ainv[i + j * 3] = a[j + i * 3];
        }
    }

    // Verify A · A⁻¹ ≈ I
    let product = mat_mul(&a_orig, &ainv, 3, 3, 3);
    for j in 0..3 {
        for i in 0..3 {
            let expected = if i == j { 1.0 } else { 0.0 };
            assert_close(product[i + j * 3], expected, "dpotri A·A⁻¹ ≈ I");
        }
    }
}

#[test]
fn test_dpotri_5x5() {
    let a_orig = make_spd(5, 750);
    let n = 5i32;
    let mut info = 0i32;

    let mut a = a_orig.clone();
    unsafe {
        backend::dpotrf_(&b'U', &n, a.as_mut_ptr(), &n, &mut info);
    }
    assert_eq!(info, 0, "dpotri 5: dpotrf step");

    unsafe {
        backend::dpotri_(&b'U', &n, a.as_mut_ptr(), &n, &mut info);
    }
    assert_eq!(info, 0, "dpotri 5 info");

    let mut ainv = a.clone();
    for j in 0..5 {
        for i in (j + 1)..5 {
            ainv[i + j * 5] = a[j + i * 5];
        }
    }

    let product = mat_mul(&a_orig, &ainv, 5, 5, 5);
    for j in 0..5 {
        for i in 0..5 {
            let expected = if i == j { 1.0 } else { 0.0 };
            assert_close(product[i + j * 5], expected, "dpotri 5 A·A⁻¹ ≈ I");
        }
    }
}

// ════════════════════════════════════════════════════════════════
// dgesdd_  –  SVD
// ════════════════════════════════════════════════════════════════

#[test]
fn test_dgesdd_3x3() {
    let a_orig = seed_matrix(3, 3, 800);
    let m = 3i32;
    let n = 3i32;
    let minmn = 3;
    let mut s = vec![0.0; minmn];
    let mut u = vec![0.0; 9];
    let mut vt = vec![0.0; 9];
    let mut iwork = vec![0i32; 8 * minmn];
    let mut info = 0i32;

    // Workspace query
    let mut work = vec![0.0; 1];
    let mut lwork = -1i32;
    {
        let mut a = a_orig.clone();
        unsafe {
            backend::dgesdd_(
                &b'A', &m, &n,
                a.as_mut_ptr(), &m,
                s.as_mut_ptr(),
                u.as_mut_ptr(), &m,
                vt.as_mut_ptr(), &n,
                work.as_mut_ptr(), &lwork,
                iwork.as_mut_ptr(),
                &mut info,
            );
        }
    }
    assert_eq!(info, 0, "dgesdd 3×3 workspace query");
    lwork = work[0] as i32;
    work.resize(lwork as usize, 0.0);

    // Actual computation
    unsafe {
        let mut a = a_orig.clone();
        backend::dgesdd_(
            &b'A', &m, &n,
            a.as_mut_ptr(), &m,
            s.as_mut_ptr(),
            u.as_mut_ptr(), &m,
            vt.as_mut_ptr(), &n,
            work.as_mut_ptr(), &lwork,
            iwork.as_mut_ptr(),
            &mut info,
        );
    }
    assert_eq!(info, 0, "dgesdd 3×3 info");

    // Singular values must be non-negative and descending
    for i in 0..minmn {
        assert!(s[i] >= 0.0, "dgesdd s[{i}] = {} is negative", s[i]);
    }
    for i in 0..(minmn - 1) {
        assert!(
            s[i] >= s[i + 1] - 1e-14,
            "dgesdd s[{i}]={} < s[{}]={}",
            s[i], i + 1, s[i + 1],
        );
    }

    // Reconstruction: A ≈ U · diag(s) · VT
    let mut sv = vec![0.0; 9];
    for i in 0..3 {
        sv[i + i * 3] = s[i];
    }
    let usv = mat_mul(&u, &sv, 3, 3, 3);
    let recon = mat_mul(&usv, &vt, 3, 3, 3);

    let mut diff = vec![0.0; 9];
    for i in 0..9 {
        diff[i] = recon[i] - a_orig[i];
    }
    let residual = frob(&diff, 3, 3) / frob(&a_orig, 3, 3).max(1e-15);
    assert!(residual < 1e-10, "dgesdd 3×3 reconstruction residual: {residual}");
}

#[test]
fn test_dgesdd_5x3() {
    let a_orig = seed_matrix(5, 3, 850);
    let m = 5i32;
    let n = 3i32;
    let minmn = 3;
    let mut s = vec![0.0; minmn];
    let mut u = vec![0.0; 25]; // 5×5
    let mut vt = vec![0.0; 9]; // 3×3
    let mut iwork = vec![0i32; 8 * minmn];
    let mut info = 0i32;

    // Workspace query
    let mut work = vec![0.0; 1];
    let mut lwork = -1i32;
    {
        let mut a = a_orig.clone();
        unsafe {
            backend::dgesdd_(
                &b'A', &m, &n,
                a.as_mut_ptr(), &m,
                s.as_mut_ptr(),
                u.as_mut_ptr(), &m,
                vt.as_mut_ptr(), &n,
                work.as_mut_ptr(), &lwork,
                iwork.as_mut_ptr(),
                &mut info,
            );
        }
    }
    lwork = work[0] as i32;
    work.resize(lwork as usize, 0.0);

    unsafe {
        let mut a = a_orig.clone();
        backend::dgesdd_(
            &b'A', &m, &n,
            a.as_mut_ptr(), &m,
            s.as_mut_ptr(),
            u.as_mut_ptr(), &m,
            vt.as_mut_ptr(), &n,
            work.as_mut_ptr(), &lwork,
            iwork.as_mut_ptr(),
            &mut info,
        );
    }
    assert_eq!(info, 0, "dgesdd 5×3 info");

    // Singular values non-negative and descending
    for i in 0..minmn {
        assert!(s[i] >= 0.0, "dgesdd 5×3 s[{i}] negative");
    }
    for i in 0..(minmn - 1) {
        assert!(s[i] >= s[i + 1] - 1e-14, "dgesdd 5×3 not descending");
    }
}

// ════════════════════════════════════════════════════════════════
// dsyevr_  –  symmetric eigenvalue decomposition
// ════════════════════════════════════════════════════════════════

#[test]
fn test_dsyevr_3x3() {
    let a_orig = make_symmetric(3, 1000);
    let n = 3i32;
    let mut w = vec![0.0; 3];
    let mut z = vec![0.0; 9];
    let mut isuppz = vec![0i32; 6];
    let mut m = 0i32;
    let mut info = 0i32;

    // Workspace query
    let mut work = vec![0.0; 1];
    let mut iwork = vec![0i32; 1];
    let mut lwork = -1i32;
    let mut liwork = -1i32;
    {
        let mut a = a_orig.clone();
        unsafe {
            backend::dsyevr_(
                &b'V', &b'A', &b'L',
                &n, a.as_mut_ptr(), &n,
                &0.0, &0.0, &0, &0, &0.0,
                &mut m, w.as_mut_ptr(), z.as_mut_ptr(), &n,
                isuppz.as_mut_ptr(),
                work.as_mut_ptr(), &lwork,
                iwork.as_mut_ptr(), &liwork,
                &mut info,
            );
        }
    }
    lwork = work[0] as i32;
    liwork = iwork[0];
    work.resize(lwork as usize, 0.0);
    iwork.resize(liwork as usize, 0);

    // Actual computation
    {
        let mut a = a_orig.clone();
        unsafe {
            backend::dsyevr_(
                &b'V', &b'A', &b'L',
                &n, a.as_mut_ptr(), &n,
                &0.0, &0.0, &0, &0, &0.0,
                &mut m, w.as_mut_ptr(), z.as_mut_ptr(), &n,
                isuppz.as_mut_ptr(),
                work.as_mut_ptr(), &lwork,
                iwork.as_mut_ptr(), &liwork,
                &mut info,
            );
        }
    }
    assert_eq!(info, 0, "dsyevr 3×3 info");
    assert_eq!(m, 3, "dsyevr 3×3 m");

    // Eigenvalues ascending
    for i in 0..2 {
        assert!(w[i] <= w[i + 1] + 1e-14, "dsyevr eigenvalues not ascending");
    }

    // Verify A · v_j ≈ λ_j · v_j for each eigenvector
    for j in 0..3 {
        for i in 0..3 {
            let mut av_i = 0.0;
            for k in 0..3 {
                av_i += a_orig[i + k * 3] * z[k + j * 3];
            }
            assert_close(
                av_i,
                w[j] * z[i + j * 3],
                &format!("dsyevr 3×3 A·v[{}] at row {}", j, i),
            );
        }
    }
}

#[test]
fn test_dsyevr_5x5() {
    let a_orig = make_symmetric(5, 1050);
    let n = 5i32;
    let mut w = vec![0.0; 5];
    let mut z = vec![0.0; 25];
    let mut isuppz = vec![0i32; 10];
    let mut m = 0i32;
    let mut info = 0i32;

    // Workspace query
    let mut work = vec![0.0; 1];
    let mut iwork = vec![0i32; 1];
    let mut lwork = -1i32;
    let mut liwork = -1i32;
    {
        let mut a = a_orig.clone();
        unsafe {
            backend::dsyevr_(
                &b'V', &b'A', &b'L',
                &n, a.as_mut_ptr(), &n,
                &0.0, &0.0, &0, &0, &0.0,
                &mut m, w.as_mut_ptr(), z.as_mut_ptr(), &n,
                isuppz.as_mut_ptr(),
                work.as_mut_ptr(), &lwork,
                iwork.as_mut_ptr(), &liwork,
                &mut info,
            );
        }
    }
    lwork = work[0] as i32;
    liwork = iwork[0];
    work.resize(lwork as usize, 0.0);
    iwork.resize(liwork as usize, 0);

    {
        let mut a = a_orig.clone();
        unsafe {
            backend::dsyevr_(
                &b'V', &b'A', &b'L',
                &n, a.as_mut_ptr(), &n,
                &0.0, &0.0, &0, &0, &0.0,
                &mut m, w.as_mut_ptr(), z.as_mut_ptr(), &n,
                isuppz.as_mut_ptr(),
                work.as_mut_ptr(), &lwork,
                iwork.as_mut_ptr(), &liwork,
                &mut info,
            );
        }
    }
    assert_eq!(info, 0, "dsyevr 5×5 info");
    assert_eq!(m, 5, "dsyevr 5×5 m");

    // Verify A · V ≈ V · diag(w)
    for j in 0..5 {
        for i in 0..5 {
            let mut av_i = 0.0;
            for k in 0..5 {
                av_i += a_orig[i + k * 5] * z[k + j * 5];
            }
            assert_close(
                av_i,
                w[j] * z[i + j * 5],
                &format!("dsyevr 5×5 A·v[{}] row {}", j, i),
            );
        }
    }
}

// ════════════════════════════════════════════════════════════════
// dgeev_  –  general eigenvalue decomposition
// ════════════════════════════════════════════════════════════════

#[test]
fn test_dgeev_3x3() {
    let a_orig = seed_matrix(3, 3, 1100);
    let n = 3i32;
    let mut wr = vec![0.0; 3];
    let mut wi = vec![0.0; 3];
    let mut vl = vec![0.0; 9]; // not referenced (jobvl='N') but allocate for safety
    let mut vr = vec![0.0; 9];
    let mut info = 0i32;

    // Workspace query
    let mut work = vec![0.0; 1];
    let mut lwork = -1i32;
    {
        let mut a = a_orig.clone();
        unsafe {
            backend::dgeev_(
                &b'N', &b'V',
                &n, a.as_mut_ptr(), &n,
                wr.as_mut_ptr(), wi.as_mut_ptr(),
                vl.as_mut_ptr(), &n,
                vr.as_mut_ptr(), &n,
                work.as_mut_ptr(), &lwork,
                &mut info,
            );
        }
    }
    lwork = work[0] as i32;
    work.resize(lwork as usize, 0.0);

    // Actual computation
    {
        let mut a = a_orig.clone();
        unsafe {
            backend::dgeev_(
                &b'N', &b'V',
                &n, a.as_mut_ptr(), &n,
                wr.as_mut_ptr(), wi.as_mut_ptr(),
                vl.as_mut_ptr(), &n,
                vr.as_mut_ptr(), &n,
                work.as_mut_ptr(), &lwork,
                &mut info,
            );
        }
    }
    assert_eq!(info, 0, "dgeev 3×3 info");

    // For real eigenvalues, verify A · v = λ · v
    let mut j = 0usize;
    while j < 3 {
        if wi[j] == 0.0 {
            // Real eigenvalue: check A·v_j = λ_j·v_j
            for i in 0..3 {
                let mut av_i = 0.0;
                for k in 0..3 {
                    av_i += a_orig[i + k * 3] * vr[k + j * 3];
                }
                assert_close(
                    av_i,
                    wr[j] * vr[i + j * 3],
                    &format!("dgeev real eigenpair [{j}] row {i}"),
                );
            }
            j += 1;
        } else {
            // Complex conjugate pair – skip vector check (storage format is complex)
            j += 2;
        }
    }
}

#[test]
fn test_dgeev_5x5() {
    let a_orig = seed_matrix(5, 5, 1150);
    let n = 5i32;
    let mut wr = vec![0.0; 5];
    let mut wi = vec![0.0; 5];
    let mut vl = vec![0.0; 25];
    let mut vr = vec![0.0; 25];
    let mut info = 0i32;

    // Workspace query
    let mut work = vec![0.0; 1];
    let mut lwork = -1i32;
    {
        let mut a = a_orig.clone();
        unsafe {
            backend::dgeev_(
                &b'N', &b'V',
                &n, a.as_mut_ptr(), &n,
                wr.as_mut_ptr(), wi.as_mut_ptr(),
                vl.as_mut_ptr(), &n,
                vr.as_mut_ptr(), &n,
                work.as_mut_ptr(), &lwork,
                &mut info,
            );
        }
    }
    lwork = work[0] as i32;
    work.resize(lwork as usize, 0.0);

    {
        let mut a = a_orig.clone();
        unsafe {
            backend::dgeev_(
                &b'N', &b'V',
                &n, a.as_mut_ptr(), &n,
                wr.as_mut_ptr(), wi.as_mut_ptr(),
                vl.as_mut_ptr(), &n,
                vr.as_mut_ptr(), &n,
                work.as_mut_ptr(), &lwork,
                &mut info,
            );
        }
    }
    assert_eq!(info, 0, "dgeev 5×5 info");

    // Verify real eigenpairs
    let mut j = 0usize;
    while j < 5 {
        if wi[j] == 0.0 {
            for i in 0..5 {
                let mut av_i = 0.0;
                for k in 0..5 {
                    av_i += a_orig[i + k * 5] * vr[k + j * 5];
                }
                assert_close(
                    av_i,
                    wr[j] * vr[i + j * 5],
                    &format!("dgeev 5×5 eigenpair [{j}] row {i}"),
                );
            }
            j += 1;
        } else {
            j += 2;
        }
    }
}

// ════════════════════════════════════════════════════════════════
// dgeqp3_  –  QR factorisation with column pivoting
// ════════════════════════════════════════════════════════════════

#[test]
fn test_dgeqp3_3x3() {
    let a_orig = seed_matrix(3, 3, 1200);
    let m = 3i32;
    let n = 3i32;
    let mut jpvt = vec![0i32; 3];
    let mut tau = vec![0.0; 3];
    let mut info = 0i32;

    // Workspace query
    let mut work = vec![0.0; 1];
    let mut lwork = -1i32;
    {
        let mut a = a_orig.clone();
        unsafe {
            backend::dgeqp3_(
                &m, &n,
                a.as_mut_ptr(), &m,
                jpvt.as_mut_ptr(),
                tau.as_mut_ptr(),
                work.as_mut_ptr(), &lwork,
                &mut info,
            );
        }
    }
    lwork = work[0] as i32;
    work.resize(lwork as usize, 0.0);

    // Actual computation
    let mut a = a_orig.clone();
    unsafe {
        backend::dgeqp3_(
            &m, &n,
            a.as_mut_ptr(), &m,
            jpvt.as_mut_ptr(),
            tau.as_mut_ptr(),
            work.as_mut_ptr(), &lwork,
            &mut info,
        );
    }
    assert_eq!(info, 0, "dgeqp3 3×3 info");

    // jpvt is a valid permutation of {1,2,3}
    let mut perm: Vec<i32> = jpvt.iter().copied().collect();
    perm.sort();
    assert_eq!(perm, vec![1, 2, 3], "dgeqp3 3×3: jpvt not a permutation");

    verify_qr_pivot(&a_orig, &a, 3, 3, &jpvt, &tau);
}

#[test]
fn test_dgeqp3_5x3() {
    let a_orig = seed_matrix(5, 3, 1250);
    let m = 5i32;
    let n = 3i32;
    let mut jpvt = vec![0i32; 3];
    let mut tau = vec![0.0; 3];
    let mut info = 0i32;

    // Workspace query
    let mut work = vec![0.0; 1];
    let mut lwork = -1i32;
    {
        let mut a = a_orig.clone();
        unsafe {
            backend::dgeqp3_(
                &m, &n,
                a.as_mut_ptr(), &m,
                jpvt.as_mut_ptr(),
                tau.as_mut_ptr(),
                work.as_mut_ptr(), &lwork,
                &mut info,
            );
        }
    }
    lwork = work[0] as i32;
    work.resize(lwork as usize, 0.0);

    let mut a = a_orig.clone();
    unsafe {
        backend::dgeqp3_(
            &m, &n,
            a.as_mut_ptr(), &m,
            jpvt.as_mut_ptr(),
            tau.as_mut_ptr(),
            work.as_mut_ptr(), &lwork,
            &mut info,
        );
    }
    assert_eq!(info, 0, "dgeqp3 5×3 info");

    // jpvt valid permutation of {1,2,3}
    let mut perm: Vec<i32> = jpvt.iter().copied().collect();
    perm.sort();
    assert_eq!(perm, vec![1, 2, 3], "dgeqp3 5×3: jpvt not a permutation");

    verify_qr_pivot(&a_orig, &a, 5, 3, &jpvt, &tau);
}

// ════════════════════════════════════════════════════════════════
// dtrtrs_  –  triangular solve
// ════════════════════════════════════════════════════════════════

#[test]
fn test_dtrtrs_3x3_upper() {
    // Upper-triangular (row view): [[2,1,3],[0,4,1],[0,0,5]]
    // Column-major: [2,0,0, 1,4,0, 3,1,5]
    let a: Vec<f64> = vec![2.0, 0.0, 0.0, 1.0, 4.0, 0.0, 3.0, 1.0, 5.0];
    let mut b: Vec<f64> = vec![1.0, 2.0, 3.0];
    let n = 3i32;
    let nrhs = 1i32;
    let mut info = 0i32;

    unsafe {
        backend::dtrtrs_(
            &b'U', &b'N', &b'N',
            &n, &nrhs,
            a.as_ptr(), &n,
            b.as_mut_ptr(), &n,
            &mut info,
        );
    }
    assert_eq!(info, 0, "dtrtrs 3×3 upper info");

    // Manual solution: Ux=b
    // 5x₃ = 3 → x₃ = 0.6
    // 4x₂ + 0.6 = 2 → x₂ = 0.35
    // 2x₁ + 0.35 + 1.8 = 1 → x₁ = −0.575
    assert_close(b[0], -0.575, "dtrtrs 3×3 x[0]");
    assert_close(b[1], 0.35, "dtrtrs 3×3 x[1]");
    assert_close(b[2], 0.6, "dtrtrs 3×3 x[2]");
}

#[test]
fn test_dtrtrs_5x5_random() {
    // Upper-triangular matrix with non-zero diagonal
    let raw = seed_matrix(5, 5, 1300);
    let mut a = vec![0.0; 25];
    for j in 0..5 {
        for i in 0..=j {
            a[i + j * 5] = raw[i + j * 5];
        }
        if a[j + j * 5].abs() < 0.5 {
            a[j + j * 5] = 1.0;
        }
    }
    let b_orig = seed_matrix(5, 1, 1400);
    let mut b = b_orig.clone();
    let n = 5i32;
    let nrhs = 1i32;
    let mut info = 0i32;

    unsafe {
        backend::dtrtrs_(
            &b'U', &b'N', &b'N',
            &n, &nrhs,
            a.as_ptr(), &n,
            b.as_mut_ptr(), &n,
            &mut info,
        );
    }
    assert_eq!(info, 0, "dtrtrs 5×5 info");

    // Verify U · x = b
    let ux = mat_mul(&a, &b, 5, 5, 1);
    for i in 0..5 {
        assert_close(ux[i], b_orig[i], &format!("dtrtrs 5×5 verify [{i}]"));
    }
}

#[test]
fn test_dtrtrs_lower_transpose() {
    // L = [[3,0,0],[1,4,0],[2,1,5]]  (col-major: [3,1,2, 0,4,1, 0,0,5])
    let a: Vec<f64> = vec![3.0, 1.0, 2.0, 0.0, 4.0, 1.0, 0.0, 0.0, 5.0];
    let mut b: Vec<f64> = vec![6.0, 10.0, 25.0];
    let n = 3i32;
    let nrhs = 1i32;
    let mut info = 0i32;

    unsafe {
        backend::dtrtrs_(
            &b'L', &b'T', &b'N',
            &n, &nrhs,
            a.as_ptr(), &n,
            b.as_mut_ptr(), &n,
            &mut info,
        );
    }
    assert_eq!(info, 0, "dtrtrs lower transpose info");

    // Lᵀ = [[3,1,2],[0,4,1],[0,0,5]]
    // 5x₃ = 25 → x₃ = 5
    // 4x₂ + 5 = 10 → x₂ = 1.25
    // 3x₁ + 1.25 + 10 = 6 → x₁ = −1.75
    assert_close(b[0], -1.75, "dtrtrs Lᵀ x[0]");
    assert_close(b[1], 1.25, "dtrtrs Lᵀ x[1]");
    assert_close(b[2], 5.0, "dtrtrs Lᵀ x[2]");
}
