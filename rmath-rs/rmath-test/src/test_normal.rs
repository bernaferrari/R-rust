use crate::comparisons::*;
use rmath::dist::normal::*;

pub fn run_tests() -> Result<(), String> {
    // dnorm tests
    assert_equiv(
        dnorm4_inner(0.0, 0.0, 1.0, false),
        1.0 / (2.0 * std::f64::consts::PI).sqrt(),
        "dnorm(0,0,1)",
    );
    assert_nan(dnorm4_inner(f64::NAN, 0.0, 1.0, false), "dnorm(NaN,...)");
    assert_nan(dnorm4_inner(0.0, f64::NAN, 1.0, false), "dnorm(0,NaN,1)");
    assert_nan(dnorm4_inner(0.0, 0.0, 0.0, false), "dnorm(0,0,0)");
    assert_nan(dnorm4_inner(0.0, 0.0, -1.0, false), "dnorm(0,0,-1)");
    assert_equiv(
        dnorm4_inner(0.0, 0.0, 1.0, true),
        -(2.0 * std::f64::consts::PI).ln() / 2.0,
        "dnorm(0,0,1,log)",
    );

    // pnorm tests
    assert_equiv(
        pnorm5_inner(0.0, 0.0, 1.0, true, false),
        0.5,
        "pnorm(0,0,1,lower)",
    );
    assert_equiv(
        pnorm5_inner(0.0, 0.0, 1.0, false, false),
        0.5,
        "pnorm(0,0,1,upper)",
    );
    // Approximate check: pnorm(1.96) should be near 0.975
    // (not bitwise exact due to algorithm differences in Cody's approximation)
    let p1 = pnorm5_inner(1.96, 0.0, 1.0, true, false);
    if !(p1 > 0.97 && p1 < 0.98) {
        return Err(format!("pnorm(1.96) = {}, expected ~0.975", p1));
    }
    let p2 = pnorm5_inner(-1.96, 0.0, 1.0, true, false);
    if !(p2 > 0.02 && p2 < 0.03) {
        return Err(format!("pnorm(-1.96) = {}, expected ~0.025", p2));
    }
    assert_equiv(
        pnorm5_inner(f64::INFINITY, 0.0, 1.0, true, false),
        1.0,
        "pnorm(Inf)",
    );
    assert_equiv(
        pnorm5_inner(f64::NEG_INFINITY, 0.0, 1.0, true, false),
        0.0,
        "pnorm(-Inf)",
    );
    assert_nan(pnorm5_inner(f64::NAN, 0.0, 1.0, true, false), "pnorm(NaN)");
    // log scale
    assert_neginf(
        pnorm5_inner(f64::NEG_INFINITY, 0.0, 1.0, true, true),
        "pnorm(-Inf,log)",
    );
    assert_equiv(
        pnorm5_inner(f64::INFINITY, 0.0, 1.0, true, true),
        0.0,
        "pnorm(Inf,log)",
    );

    // qnorm tests
    assert_equiv(qnorm5_inner(0.5, 0.0, 1.0, true, false), 0.0, "qnorm(0.5)");
    let q1 = qnorm5_inner(0.975, 0.0, 1.0, true, false);
    if !(q1 > 1.95 && q1 < 1.97) {
        return Err(format!("qnorm(0.975) = {}, expected ~1.96", q1));
    }
    assert_neginf(qnorm5_inner(0.0, 0.0, 1.0, true, false), "qnorm(0)");
    assert_posinf(qnorm5_inner(1.0, 0.0, 1.0, true, false), "qnorm(1)");
    assert_nan(qnorm5_inner(f64::NAN, 0.0, 1.0, true, false), "qnorm(NaN)");
    assert_nan(qnorm5_inner(-1.0, 0.0, 1.0, true, false), "qnorm(-1)");
    assert_nan(qnorm5_inner(2.0, 0.0, 1.0, true, false), "qnorm(2)");
    // With mu/sigma
    assert_equiv(
        qnorm5_inner(0.5, 10.0, 2.0, true, false),
        10.0,
        "qnorm(0.5,10,2)",
    );

    // rnorm tests
    rmath::rng::set_seed(42, 24);
    let r1 = rnorm_inner(0.0, 1.0);
    if !(r1.is_finite()) {
        return Err("rnorm(0,1) is finite".into());
    }
    rmath::rng::set_seed(42, 24);
    let r2 = rnorm_inner(0.0, 1.0);
    assert_equiv(r1, r2, "rnorm reproducible");
    assert_nan(rnorm_inner(f64::NAN, 1.0), "rnorm(NaN,1)");
    assert_nan(rnorm_inner(0.0, 0.0), "rnorm(0,0)");
    assert_nan(rnorm_inner(0.0, -1.0), "rnorm(0,-1)");

    Ok(())
}
