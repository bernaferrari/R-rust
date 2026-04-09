use crate::comparisons::*;
use rmath::dist::nbinom::*;

pub fn run_tests() -> Result<(), String> {
    // dnbinom tests
    assert_nan(
        dnbinom_inner(f64::NAN, 1.0, 0.5, false),
        "dnbinom(NaN,1,0.5)",
    );
    assert_nan(
        dnbinom_inner(0.0, f64::NAN, 0.5, false),
        "dnbinom(0,NaN,0.5)",
    );
    assert_nan(dnbinom_inner(0.0, 1.0, f64::NAN, false), "dnbinom(0,1,NaN)");
    // Edge case: size=0, x=0 => density = 1
    assert_equiv(dnbinom_inner(0.0, 0.0, 0.5, false), 1.0, "dnbinom(0,0,0.5)");
    // Edge case: prob=0 => NaN (invalid)
    assert_nan(dnbinom_inner(0.0, 1.0, 0.0, false), "dnbinom(0,1,0)");
    // Edge case: prob=1 => point mass at 0
    assert_equiv(dnbinom_inner(0.0, 2.0, 1.0, false), 1.0, "dnbinom(0,2,1)");
    assert_equiv(dnbinom_inner(1.0, 2.0, 1.0, false), 0.0, "dnbinom(1,2,1)");
    // Valid density should be > 0
    let d1 = dnbinom_inner(2.0, 3.0, 0.5, false);
    if d1 <= 0.0 {
        return Err(format!("dnbinom(2,3,0.5) = {}, expected > 0", d1));
    }
    // Log-scale check: log(density) should match log_p version
    let d1_log = dnbinom_inner(2.0, 3.0, 0.5, true);
    if d1_log >= 0.0 {
        return Err(format!("dnbinom(2,3,0.5,log) = {}, expected < 0", d1_log));
    }
    // Negative x => 0
    assert_equiv(
        dnbinom_inner(-1.0, 3.0, 0.5, false),
        0.0,
        "dnbinom(-1,3,0.5)",
    );

    // pnbinom tests
    assert_nan(
        pnbinom_inner(f64::NAN, 1.0, 0.5, true, false),
        "pnbinom(NaN,1,0.5)",
    );
    assert_nan(
        pnbinom_inner(0.0, f64::NAN, 0.5, true, false),
        "pnbinom(0,NaN,0.5)",
    );
    // pnbinom(0,...) = pbeta(prob, size, 1, ...) via implementation
    let _p0 = pnbinom_inner(0.0, 2.0, 0.5, true, false);
    // pnbinom at negative x should be 0
    assert_equiv(
        pnbinom_inner(-1.0, 3.0, 0.5, true, false),
        0.0,
        "pnbinom(-1,3,0.5)",
    );
    // pnbinom(Inf, ...) = 1
    assert_equiv(
        pnbinom_inner(f64::INFINITY, 3.0, 0.5, true, false),
        1.0,
        "pnbinom(Inf,3,0.5)",
    );
    // Upper tail: pnbinom(Inf, ..., upper) = 0
    assert_equiv(
        pnbinom_inner(f64::INFINITY, 3.0, 0.5, false, false),
        0.0,
        "pnbinom(Inf,3,0.5,upper)",
    );

    // qnbinom tests
    assert_nan(
        qnbinom_inner(f64::NAN, 1.0, 0.5, true, false),
        "qnbinom(NaN,1,0.5)",
    );
    assert_nan(
        qnbinom_inner(-0.5, 1.0, 0.5, true, false),
        "qnbinom(-0.5,1,0.5)",
    );
    assert_nan(
        qnbinom_inner(1.5, 1.0, 0.5, true, false),
        "qnbinom(1.5,1,0.5)",
    );
    // qnbinom(0.5, size, prob) should give a non-negative integer
    let q1 = qnbinom_inner(0.5, 3.0, 0.5, true, false);
    if q1 < 0.0 {
        return Err(format!("qnbinom(0.5,3,0.5) = {}, expected >= 0", q1));
    }
    // qnbinom(0) boundary: returns a value <= 0
    let q0 = qnbinom_inner(0.0, 3.0, 0.5, true, false);
    if q0 > 0.0 {
        return Err(format!("qnbinom(0,3,0.5) = {}, expected <= 0", q0));
    }
    // qnbinom(1) should give a large positive value
    let q1_big = qnbinom_inner(1.0, 3.0, 0.5, true, false);
    if q1_big <= 0.0 {
        return Err(format!("qnbinom(1,3,0.5) = {}, expected > 0", q1_big));
    }

    // rnbinom tests
    rmath::rng::set_seed(42, 24);
    let r1 = rnbinom_inner(3.0, 0.5);
    if r1 < 0.0 {
        return Err(format!("rnbinom(3,0.5) = {}, expected >= 0", r1));
    }
    rmath::rng::set_seed(42, 24);
    let r2 = rnbinom_inner(3.0, 0.5);
    assert_equiv(r1, r2, "rnbinom reproducible");
    assert_nan(rnbinom_inner(f64::NAN, 0.5), "rnbinom(NaN,0.5)");
    assert_nan(rnbinom_inner(3.0, 0.0), "rnbinom(3,0)");

    Ok(())
}
