//! Portable acceptance tests for the FMA-affected kernels.
//!
//! These check NUMERICAL PROPERTIES (monotonicity, round-trip identities,
//! orthogonality, convergence invariants), never decimal strings, so they
//! hold under both fused and unfused scalar arithmetic. They are the
//! executable form of docs/numerical-contracts.md: loose enough that 1-ulp
//! input differences (the only effect toggling FMA contraction has on these
//! quantities) pass, tight enough to catch real defects.
//!
//! Exact-oracle reproduction (bit-identical iterates, fn-evaluation counts)
//! is a separate, narrower contract owned by the conformance corpus and the
//! in-module golden tests (e.g. optim.rs `bfgs_restarts_...`); it is NOT
//! re-asserted here.

use rmath::appl::linpack_qr::{dqrdc2, dqrsl};
use rmath::appl::optim::{OptimFn, OptimGr, vmmin};
use rmath::dist::beta::{dbeta_inner, pbeta_inner, qbeta_inner};

// ===========================================================================
// qbeta / pbeta / dbeta: tail behavior, d/p/q identity, extremes, endpoints
// ===========================================================================

const SHAPE_CASES: &[(f64, f64)] = &[
    (1.0, 1.0),
    (2.0, 2.0),
    (0.5, 0.5),
    (2.0, 5.0),
    (5.0, 2.0),
    (3.0, 10.0),
    (100.0, 1.0),
    (1.0, 100.0),
    (0.05, 2.0),
    (1e6, 1e6),
];

/// p grid concentrated near the tails where FMA contraction changes the
/// rounding path the most.
fn p_grid() -> Vec<f64> {
    let mut ps: Vec<f64> = (1..=19).map(|i| i as f64 / 20.0).collect();
    for e in [3i32, 6, 9, 12] {
        let t = 10f64.powi(-e);
        ps.push(t);
        ps.push(1.0 - t);
    }
    ps.push(0.5);
    ps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ps
}

/// Round-trip tolerance derived from the conditioning: a perturbation dq of
/// the quantile perturbs pbeta(q) by about dbeta(q)*dq, and qbeta's own
/// accuracy in q is a few relative ulps plus the Newton convergence floor,
/// so |pbeta(qbeta(p)) - p| is bounded by ~1e-12 * (1 + q*density).
fn roundtrip_tol(q: f64, a: f64, b: f64) -> f64 {
    let density = dbeta_inner(q, a, b, false);
    1e-12 + (q * density).abs() * 1e-9
}

/// qbeta's documented accuracy is ~1e-8 relative for central p but degrades
/// in the deep tails (p < 1e-6); scale closed-form comparisons accordingly.
fn tail_rel(p: f64) -> f64 {
    if p >= 1e-6 && p <= 1.0 - 1e-6 {
        1e-7
    } else {
        1e-3
    }
}

#[test]
fn qbeta_roundtrips_through_pbeta() {
    for &(a, b) in SHAPE_CASES {
        for &p in &p_grid() {
            let q = qbeta_inner(p, a, b, true, false);
            assert!(
                q.is_finite() && (0.0..=1.0).contains(&q),
                "q not in (0,1): p={p} a={a} b={b} q={q}"
            );
            let p1 = pbeta_inner(q, a, b, true, false);
            assert!(
                (p1 - p).abs() <= roundtrip_tol(q, a, b),
                "roundtrip: a={a} b={b} p={p} q={q} pbeta(q)={p1}"
            );
            // upper-tail / log-parametrized entry points must agree
            let p1u = pbeta_inner(q, a, b, false, false);
            assert!(
                (p1u - (1.0 - p)).abs() <= roundtrip_tol(q, a, b).max(1e-12),
                "upper tail: a={a} b={b} p={p} pbeta_upper={p1u}"
            );
        }
    }
}

#[test]
fn qbeta_log_p_entry_matches_plain_entry() {
    for &(a, b) in SHAPE_CASES {
        for &p in &p_grid() {
            let q_plain = qbeta_inner(p, a, b, true, false);
            let q_log = qbeta_inner(p.ln(), a, b, true, true);
            // log-space p loses a few ulp; the quantile inherits only that.
            assert!(
                (q_log - q_plain).abs() <= 1e-9 * q_plain.max(1e-300),
                "log.p: a={a} b={b} p={p} plain={q_plain} log={q_log}"
            );
            let lp = pbeta_inner(q_plain, a, b, true, true);
            // pbeta(log.p) represents p to high absolute accuracy on the
            // probability scale; the log magnifies rounding near p=1 by
            // 1/(1-p), so bound exp(lp) against p rather than lp against ln p.
            assert!(
                (lp.exp() - p).abs() <= 1e-9,
                "pbeta log.p: a={a} b={b} p={p} log-pbeta={lp}"
            );
        }
    }
}

#[test]
fn qbeta_is_monotone_in_p_including_tails() {
    for &(a, b) in SHAPE_CASES {
        let ps = p_grid();
        for w in ps.windows(2) {
            let (p_lo, p_hi) = (w[0], w[1]);
            let q_lo = qbeta_inner(p_lo, a, b, true, false);
            let q_hi = qbeta_inner(p_hi, a, b, true, false);
            assert!(
                q_lo <= q_hi || (q_hi - q_lo).abs() < 1e-15,
                "lower.tail monotonicity broken: a={a} b={b} p={p_lo}->{p_hi} q={q_lo}->{q_hi}"
            );
            // upper-tail parametrization: larger p => smaller upper quantile
            let qu_lo = qbeta_inner(p_lo, a, b, false, false);
            let qu_hi = qbeta_inner(p_hi, a, b, false, false);
            assert!(
                qu_lo >= qu_hi || (qu_hi - qu_lo).abs() < 1e-15,
                "upper.tail monotonicity broken: a={a} b={b} p={p_lo}->{p_hi}"
            );
            // upper/lower + parameter-swap identity: for X ~ Beta(a,b),
            // q_upper^{a,b}(u) = 1 - q_lower^{b,a}(u).
            let q_u = qbeta_inner(p_lo, a, b, false, false);
            let q_inv = 1.0 - qbeta_inner(p_lo, b, a, true, false);
            assert!(
                (q_u - q_inv).abs() <= 1e-9,
                "upper-tail inversion: a={a} b={b} p={p_lo} q_u={q_u} 1-q_swap(p)={q_inv}"
            );
        }
    }
}

#[test]
fn qbeta_matches_closed_form_references() {
    // Independently justified closed forms, not oracle captures.
    for &p in &p_grid() {
        // Beta(1,1) is Uniform(0,1): qbeta(p) = p.
        let q = qbeta_inner(p, 1.0, 1.0, true, false);
        assert!((q - p).abs() <= 1e-12, "uniform: p={p} q={q}");
        // Beta(a,1) has CDF x^a, so qbeta(p) = p^(1/a).
        for &a in &[2.0, 0.5, 10.0] {
            let q = qbeta_inner(p, a, 1.0, true, false);
            let want = p.powf(1.0 / a);
            // qbeta's documented accuracy is ~1e-8 relative for central p
            // but degrades in the deep tails (p < 1e-6); bound accordingly.
            assert!(
                (q - want).abs() <= tail_rel(p) * want,
                "Beta(a,1): a={a} p={p} q={q} want={want}"
            );
        }
        // Beta(1,b) has CDF 1-(1-x)^b, so qbeta(p) = 1-(1-p)^(1/b).
        for &b in &[2.0, 0.5, 10.0] {
            let q = qbeta_inner(p, 1.0, b, true, false);
            let want = 1.0 - (1.0 - p).powf(1.0 / b);
            assert!(
                (q - want).abs() <= tail_rel(p) * want,
                "Beta(1,b): b={b} p={p} q={q} want={want}"
            );
        }
    }
    // Symmetric shapes have median exactly 1/2.
    for &a in &[2.0, 0.5, 7.5, 1e6] {
        let m = qbeta_inner(0.5, a, a, true, false);
        assert!((m - 0.5).abs() <= 1e-9, "symmetric median: a={a} m={m}");
    }
}

#[test]
fn qbeta_endpoint_and_extreme_parameter_behavior() {
    for &(a, b) in SHAPE_CASES {
        // CDF endpoints map to support endpoints.
        assert_eq!(qbeta_inner(0.0, a, b, true, false), 0.0);
        assert_eq!(qbeta_inner(1.0, a, b, true, false), 1.0);
        // log.p = TRUE accepts -Inf / 0 as p = 0 / 1.
        assert_eq!(qbeta_inner(f64::NEG_INFINITY, a, b, true, true), 0.0);
        assert_eq!(qbeta_inner(0.0, a, b, true, true), 1.0);
    }
    // p outside [0,1] is rejected with NaN (R_DOMAIN_ERROR path).
    assert!(qbeta_inner(-1e-3, 2.0, 2.0, true, false).is_nan());
    assert!(qbeta_inner(1.0 + 1e-3, 2.0, 2.0, true, false).is_nan());
    // Extreme but finite shapes stay inside the open support.
    for &(a, b) in &[(1e-3, 1e-3), (1e8, 1.0), (1.0, 1e8), (1e10, 1e10)] {
        for &p in &[1e-9, 0.5, 1.0 - 1e-9] {
            let q = qbeta_inner(p, a, b, true, false);
            assert!(
                q.is_finite() && (0.0..=1.0).contains(&q),
                "extreme shapes: a={a} b={b} p={p} q={q}"
            );
            assert!(
                (pbeta_inner(q, a, b, true, false) - p).abs() <= 1e-7,
                "extreme roundtrip: a={a} b={b} p={p}"
            );
        }
    }
}

#[test]
fn dbeta_is_the_derivative_of_pbeta() {
    // d/p identity by central differences on the log scale (keeps the
    // finite-difference error uniform across the support).
    for &(a, b) in &[(2.0, 5.0), (0.5, 0.5), (3.0, 3.0), (10.0, 1.5)] {
        for i in 1..10 {
            let x = i as f64 / 10.0;
            let h = 1e-6;
            let d_num = (pbeta_inner(x + h, a, b, true, false)
                - pbeta_inner(x - h, a, b, true, false))
                / (2.0 * h);
            let d = dbeta_inner(x, a, b, false);
            assert!(
                (d_num - d).abs() <= 1e-4 * d.max(1e-3),
                "d/p identity: a={a} b={b} x={x} analytic={d} numeric={d_num}"
            );
        }
    }
}

// ===========================================================================
// optim BFGS (vmmin): convergence code, decrease, gradient norm, optimum
// ===========================================================================

unsafe extern "C-unwind" fn rosenbrock(
    n: std::os::raw::c_int,
    point: *mut f64,
    _ex: *mut std::ffi::c_void,
) -> f64 {
    let (x, y) = unsafe { (*point, *point.add(1)) };
    debug_assert_eq!(n, 2);
    100.0 * (y - x * x).powi(2) + (1.0 - x).powi(2)
}

unsafe extern "C-unwind" fn rosenbrock_gr(
    _n: std::os::raw::c_int,
    point: *mut f64,
    g: *mut f64,
    _ex: *mut std::ffi::c_void,
) {
    unsafe {
        let (x, y) = (*point, *point.add(1));
        *g = -400.0 * x * (y - x * x) - 2.0 * (1.0 - x);
        *g.add(1) = 200.0 * (y - x * x);
    }
}

unsafe extern "C-unwind" fn quadratic(
    n: std::os::raw::c_int,
    point: *mut f64,
    _ex: *mut std::ffi::c_void,
) -> f64 {
    debug_assert_eq!(n, 2);
    let (x, y) = unsafe { (*point, *point.add(1)) };
    (x - 3.0).powi(2) + 10.0 * (y + 2.0).powi(2)
}

unsafe extern "C-unwind" fn quadratic_gr(
    _n: std::os::raw::c_int,
    point: *mut f64,
    g: *mut f64,
    _ex: *mut std::ffi::c_void,
) {
    unsafe {
        let (x, y) = (*point, *point.add(1));
        *g = 2.0 * (x - 3.0);
        *g.add(1) = 20.0 * (y + 2.0);
    }
}

fn run_vmmin(
    n: std::os::raw::c_int,
    start: &mut [f64],
    f: OptimFn,
    g: OptimGr,
    maxit: std::os::raw::c_int,
) -> (f64, Vec<f64>, i32, i32, i32) {
    let mask = vec![1i32; n as usize];
    let (mut fmin, mut fncount, mut grcount, mut fail) = (f64::NAN, 0, 0, -1);
    // SAFETY: buffers outlive the call and match the declared dimensions;
    // vmmin touches no other global state on the success path checked here.
    unsafe {
        vmmin(
            n,
            start.as_mut_ptr(),
            &mut fmin,
            f,
            g,
            maxit,
            0,
            mask.as_ptr(),
            f64::NEG_INFINITY,
            1e-8,
            10,
            std::ptr::null_mut(),
            &mut fncount,
            &mut grcount,
            &mut fail,
        );
    }
    (fmin, start.to_vec(), fncount, grcount, fail)
}

#[test]
fn bfgs_rosenbrock_satisfies_portable_convergence_contract() {
    // SAFETY: two initialized coordinates are supplied for n=2.
    let f0 = unsafe { rosenbrock(2, [-1.2, 1.0].as_mut_ptr(), std::ptr::null_mut()) };
    let mut start = [-1.2f64, 1.0];
    let (fmin, point, _fn, _gr, fail) = run_vmmin(2, &mut start, rosenbrock, rosenbrock_gr, 200);

    // Convergence code: 0 = success (both fused and unfused line searches
    // converge here; only the evaluation count differs between them).
    assert_eq!(fail, 0, "vmmin reported failure on Rosenbrock");
    // Objective decrease from the (finite) start.
    assert!(fmin < f0, "no decrease: f0={f0} fmin={fmin}");
    // Statistical tolerance on the optimum (1,1) — the analytic minimum.
    assert!(
        point.iter().all(|c| (c - 1.0).abs() < 1e-4),
        "optimum: {point:?}"
    );
    assert!(fmin < 1e-10, "objective at optimum: {fmin}");
    // Gradient norm bound at the returned point, computed independently.
    let mut grad = [0.0f64; 2];
    let mut point = point;
    // SAFETY: two coordinates and two writable gradient slots are supplied.
    unsafe {
        rosenbrock_gr(
            2,
            point.as_mut_ptr(),
            grad.as_mut_ptr(),
            std::ptr::null_mut(),
        )
    };
    let gnorm = (grad[0] * grad[0] + grad[1] * grad[1]).sqrt();
    assert!(gnorm < 1e-3, "gradient norm at result: {gnorm}");
}

#[test]
fn bfgs_quadratic_recovers_the_exact_minimum() {
    let mut start = [0.0f64, 0.0];
    let (fmin, point, _fn, _gr, fail) = run_vmmin(2, &mut start, quadratic, quadratic_gr, 100);
    assert_eq!(fail, 0);
    assert!((point[0] - 3.0).abs() < 1e-8, "x: {point:?}");
    assert!((point[1] + 2.0).abs() < 1e-8, "y: {point:?}");
    assert!(fmin < 1e-16, "fmin: {fmin}");
}

#[test]
fn bfgs_maxit_zero_reports_the_starting_value_without_iterating() {
    let mut start = [-1.2f64, 1.0];
    let (fmin, point, fncount, grcount, fail) =
        run_vmmin(2, &mut start, rosenbrock, rosenbrock_gr, 0);
    assert_eq!((fail, fncount, grcount), (0, 0, 0));
    assert_eq!(point, vec![-1.2, 1.0]);
    assert!((fmin - 24.2).abs() < 1e-12); // rosenbrock(-1.2, 1), up to rounding
}

// ===========================================================================
// lm QR (dqrdc2/dqrsl): rank recovery, residual orthogonality, fitted values
// ===========================================================================

/// Fits y on X (column-major, ldx = n) by the LINPACK pivoted QR path lm
/// uses, returning (rank, unpivoted coefficients, residuals, fitted).
fn qr_fit(
    x_orig: &[f64],
    n: usize,
    p: usize,
    tol: f64,
    y: &[f64],
) -> (usize, Vec<f64>, Vec<f64>, Vec<f64>) {
    let mut x = x_orig.to_vec();
    let mut jpvt: Vec<i32> = (1..=p as i32).collect();
    let mut qraux = vec![0.0f64; p];
    let mut work = vec![0.0f64; 2 * p];
    let mut k = 0i32;
    // SAFETY: all buffers are fully initialized and match the declared
    // leading dimension n.
    unsafe {
        dqrdc2(
            x.as_mut_ptr(),
            n as i32,
            n as i32,
            p as i32,
            tol,
            &mut k,
            qraux.as_mut_ptr(),
            jpvt.as_mut_ptr(),
            work.as_mut_ptr(),
        );
        let kk = k as usize;
        let mut b = vec![0.0f64; kk.max(1)];
        let mut rsd = vec![0.0f64; n];
        let mut xb = vec![0.0f64; n];
        let mut qty = vec![0.0f64; n];
        let mut qy = vec![0.0f64; n];
        let mut info = 0i32;
        dqrsl(
            x.as_mut_ptr(),
            n as i32,
            n as i32,
            k,
            qraux.as_ptr(),
            y.as_ptr(),
            qy.as_mut_ptr(),
            qty.as_mut_ptr(),
            b.as_mut_ptr(),
            rsd.as_mut_ptr(),
            xb.as_mut_ptr(),
            11111,
            &mut info,
        );
        assert_eq!(info, 0, "dqrsl info: {info}");
        let mut beta = vec![f64::NAN; p];
        for j in 0..kk {
            beta[(jpvt[j] - 1) as usize] = b[j];
        }
        (kk, beta, rsd, xb)
    }
}

#[test]
fn lm_qr_recovers_rank_on_known_rank_problems() {
    let n = 12usize;
    // Full-rank design: x1 ramp, x2 independent, x3 = noise-free mixture
    // column that is still independent of (x1, x2).
    let x1: Vec<f64> = (0..n).map(|i| i as f64 / (n as f64 - 1.0) - 0.5).collect();
    let x2: Vec<f64> = (0..n)
        .map(|i| ((i * 7 + 3) % n) as f64 / n as f64 - 0.4)
        .collect();
    let x3: Vec<f64> = x1
        .iter()
        .zip(&x2)
        .map(|(a, b)| a * 0.3 - b * 1.1 + 0.05)
        .collect();
    let mut x_full = Vec::with_capacity(3 * n);
    for col in [&x1, &x2, &x3] {
        x_full.extend_from_slice(col);
    }
    let y: Vec<f64> = (0..n)
        .map(|i| 1.5 * x1[i] - 2.0 * x2[i] + 3.0 * x3[i] + 0.1)
        .collect();
    let (k, _, _, _) = qr_fit(&x_full, n, 3, 1e-7, &y);
    assert_eq!(k, 3, "full-rank design misranked");

    // Exactly collinear third column: rank must drop to 2.
    let x3_dep: Vec<f64> = x1.iter().zip(&x2).map(|(a, b)| a + 2.0 * b).collect();
    let mut x_dep = Vec::with_capacity(3 * n);
    for col in [&x1, &x2, &x3_dep] {
        x_dep.extend_from_slice(col);
    }
    let y_dep: Vec<f64> = (0..n).map(|i| x1[i] - x2[i]).collect();
    let (k2, _, _, _) = qr_fit(&x_dep, n, 3, 1e-7, &y_dep);
    assert_eq!(k2, 2, "collinear design misranked");
}

#[test]
fn lm_qr_residuals_are_orthogonal_to_the_column_space() {
    // Noisy regression: residual must be orthogonal to all columns of X,
    // relatively bounded — the invariant ddot/daxpy contraction must not
    // disturb (a fused or unfused dot product both give X'y inexact only at
    // rounding level; orthogonality itself is enforced by the reflections).
    let n = 40usize;
    let mut x = Vec::with_capacity(3 * n);
    // deterministic pseudo-random design (LCG), fixed for reproducibility
    let mut seed = 12345u64;
    let mut lcg = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as f64 / (u32::MAX >> 1) as f64) - 1.0
    };
    let x1: Vec<f64> = (0..n).map(|_| lcg()).collect();
    let x2: Vec<f64> = (0..n).map(|_| lcg() * 3.0).collect();
    let ones: Vec<f64> = vec![1.0; n];
    for col in [&ones, &x1, &x2] {
        x.extend_from_slice(col);
    }
    let beta_true = [2.0, -1.5, 0.75];
    let y: Vec<f64> = (0..n)
        .map(|i| beta_true[0] + beta_true[1] * x1[i] + beta_true[2] * x2[i] + 0.3 * lcg())
        .collect();

    let (k, beta, rsd, xb) = qr_fit(&x, n, 3, 1e-7, &y);
    assert_eq!(k, 3);

    // ||X' r||_inf relative to the scale of the computation.
    for (j, col) in [&ones, &x1, &x2].iter().enumerate() {
        let xtr: f64 = (0..n).map(|i| col[i] * rsd[i]).sum();
        let col_norm: f64 = (0..n).map(|i| col[i] * col[i]).sum::<f64>().sqrt();
        let rsd_norm: f64 = (0..n).map(|i| rsd[i] * rsd[i]).sum::<f64>().sqrt();
        assert!(
            xtr.abs() <= 1e-10 * col_norm * rsd_norm * (n as f64),
            "orthogonality broken: column {j} X'r={xtr}"
        );
    }
    // Fitted values decompose y exactly: xb + rsd == y.
    for i in 0..n {
        assert!(
            (xb[i] + rsd[i] - y[i]).abs() <= 1e-12 * y[i].abs().max(1.0),
            "y decomposition: i={i} xb={:?} rsd={:?}",
            xb[i],
            rsd[i]
        );
    }
    // Coefficients within statistical tolerance of the truth (noise sd 0.3
    // on n=40 gives standard errors well below 0.05 for this design).
    for (got, want) in beta.iter().zip(&beta_true) {
        assert!(
            (got - want).abs() < 0.15,
            "coefficient estimate: got {got}, want {want}"
        );
    }
}

#[test]
fn lm_qr_exact_system_has_machine_level_residual() {
    // Consistent system: y lies exactly in the column space, so the residual
    // is pure rounding — bounded relative to ||y||, not a statistical bound.
    let n = 20usize;
    let x1: Vec<f64> = (1..=n).map(|i| i as f64).collect();
    let x2: Vec<f64> = x1.iter().map(|v| v * 0.5 + 1.0).collect();
    let mut x = Vec::with_capacity(2 * n);
    x.extend_from_slice(&x1);
    x.extend_from_slice(&x2);
    let y: Vec<f64> = (0..n).map(|i| 3.0 * x1[i] - 1.0 * x2[i]).collect();
    let (k, beta, rsd, _) = qr_fit(&x, n, 2, 1e-7, &y);
    assert_eq!(k, 2);
    assert!((beta[0] - 3.0).abs() < 1e-10, "beta1: {beta:?}");
    assert!((beta[1] + 1.0).abs() < 1e-10, "beta2: {beta:?}");
    let y_norm: f64 = y.iter().map(|v| v * v).sum::<f64>().sqrt();
    let r_norm: f64 = rsd.iter().map(|v| v * v).sum::<f64>().sqrt();
    assert!(
        r_norm <= 1e-12 * y_norm,
        "consistent system residual: {r_norm} vs ||y|| {y_norm}"
    );
}
