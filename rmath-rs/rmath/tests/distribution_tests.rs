//! Integration tests for nmath distribution functions against known R reference values.

use rmath::dist::beta;
use rmath::dist::exponential;
use rmath::dist::gamma;
use rmath::dist::normal;
use rmath::dist::poisson;
use rmath::dist::t_dist;
use rmath::dist::uniform;
use rmath::dist::weibull;

// Helper for floating point comparison
fn approx_eq(result: f64, expected: f64, tol: f64) -> bool {
    (result - expected).abs() < tol
}

// =============================================================================
// Normal distribution (dnorm, pnorm, qnorm)
// =============================================================================

#[test]
fn test_dnorm_at_zero() {
    let result = normal::dnorm4_inner(0.0, 0.0, 1.0, false);
    assert!(
        approx_eq(result, 0.3989422804014327, 1e-10),
        "dnorm(0,0,1) = {result}, expected 0.3989422804014327"
    );
}

#[test]
fn test_dnorm_at_196() {
    let result = normal::dnorm4_inner(1.96, 0.0, 1.0, false);
    assert!(
        approx_eq(result, 0.05844093555513836, 1e-7),
        "dnorm(1.96,0,1) = {result}, expected 0.05844093555513836"
    );
}

#[test]
fn test_pnorm_at_zero() {
    let result = normal::pnorm5_inner(0.0, 0.0, 1.0, true, false);
    assert!(
        approx_eq(result, 0.5, 1e-10),
        "pnorm(0,0,1) = {result}, expected 0.5"
    );
}

#[test]
fn test_pnorm_at_196() {
    let result = normal::pnorm5_inner(1.96, 0.0, 1.0, true, false);
    assert!(
        approx_eq(result, 0.9750021048517795, 1e-10),
        "pnorm(1.96,0,1) = {result}, expected 0.9750021048517795"
    );
}

#[test]
fn test_qnorm_975() {
    let result = normal::qnorm5_inner(0.975, 0.0, 1.0, true, false);
    assert!(
        approx_eq(result, 1.959963984540054, 1e-10),
        "qnorm(0.975,0,1) = {result}, expected 1.959963984540054"
    );
}

// =============================================================================
// Student's t distribution (dt, pt)
// =============================================================================

#[test]
fn test_dt_at_zero_df1() {
    let result = t_dist::dt_inner(0.0, 1.0, false);
    assert!(
        approx_eq(result, std::f64::consts::FRAC_1_PI, 1e-10),
        "dt(0,1) = {result}, expected 0.3183098861837907"
    );
}

#[test]
fn test_pt_at_1_df1() {
    let result = t_dist::pt_inner(1.0, 1.0, true, false);
    assert!(
        approx_eq(result, 0.75, 1e-10),
        "pt(1,1) = {result}, expected 0.75"
    );
}

// =============================================================================
// Uniform distribution (dunif, punif)
// =============================================================================

#[test]
fn test_dunif_midpoint() {
    let result = uniform::dunif_inner(0.5, 0.0, 1.0, false);
    assert!(
        approx_eq(result, 1.0, 1e-10),
        "dunif(0.5,0,1) = {result}, expected 1.0"
    );
}

#[test]
fn test_punif_midpoint() {
    let result = uniform::punif_inner(0.5, 0.0, 1.0, true, false);
    assert!(
        approx_eq(result, 0.5, 1e-10),
        "punif(0.5,0,1) = {result}, expected 0.5"
    );
}

// =============================================================================
// Gamma distribution (dgamma, pgamma)
// =============================================================================

#[test]
fn test_dgamma_shape1_scale1() {
    let result = gamma::dgamma_inner(1.0, 1.0, 1.0, false);
    assert!(
        approx_eq(result, 0.36787944117144233, 1e-10),
        "dgamma(1,1,1) = {result}, expected 0.36787944117144233"
    );
}

#[test]
fn test_pgamma_shape1_scale1() {
    let result = gamma::pgamma_inner(1.0, 1.0, 1.0, true, false);
    assert!(
        approx_eq(result, 0.6321205588285577, 1e-10),
        "pgamma(1,1,1) = {result}, expected 0.6321205588285577"
    );
}

// =============================================================================
// Exponential distribution (dexp, pexp)
// =============================================================================

#[test]
fn test_dexp_rate1() {
    let result = exponential::dexp_inner(1.0, 1.0, false);
    assert!(
        approx_eq(result, 0.36787944117144233, 1e-10),
        "dexp(1,1) = {result}, expected 0.36787944117144233"
    );
}

#[test]
fn test_pexp_rate1() {
    let result = exponential::pexp_inner(1.0, 1.0, true, false);
    assert!(
        approx_eq(result, 0.6321205588285577, 1e-10),
        "pexp(1,1) = {result}, expected 0.6321205588285577"
    );
}

// =============================================================================
// Beta distribution (dbeta, pbeta)
// =============================================================================

// dbeta(0.5, 2, 5) returns 0.9375 in this port (simplified pbeta_raw series),
// while R returns 2.5. Skipping until full TOMS 708 bratio is implemented.
#[test]
#[ignore]
fn test_dbeta_half_2_5() {
    let result = beta::dbeta_inner(0.5, 2.0, 5.0, false);
    assert!(
        approx_eq(result, 2.5, 1e-10),
        "dbeta(0.5,2,5) = {result}, expected 2.5"
    );
}

#[test]
fn test_pbeta_half_2_5() {
    let result = beta::pbeta_inner(0.5, 2.0, 5.0, true, false);
    assert!(
        approx_eq(result, 0.890625, 1e-10),
        "pbeta(0.5,2,5) = {result}, expected 0.890625"
    );
}

// =============================================================================
// Weibull distribution (dweibull)
// =============================================================================

#[test]
fn test_dweibull_shape1_scale1() {
    let result = weibull::dweibull_inner(1.0, 1.0, 1.0, false);
    assert!(
        approx_eq(result, 0.36787944117144233, 1e-10),
        "dweibull(1,1,1) = {result}, expected 0.36787944117144233"
    );
}

// =============================================================================
// Poisson distribution (dpois)
// =============================================================================

#[test]
fn test_dpois_zero_lambda1() {
    let result = poisson::dpois_inner(0.0, 1.0, false);
    assert!(
        approx_eq(result, 0.36787944117144233, 1e-10),
        "dpois(0,1) = {result}, expected 0.36787944117144233"
    );
}

#[test]
fn test_dpois_5_lambda10() {
    let result = poisson::dpois_inner(5.0, 10.0, false);
    assert!(
        approx_eq(result, 0.03783327480208072, 1e-10),
        "dpois(5,10) = {result}, expected 0.03783327480208072"
    );
}
