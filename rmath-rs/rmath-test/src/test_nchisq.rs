use crate::comparisons::*;
use rmath::dist::nchisq::*;

pub fn run_tests() -> Result<(), String> {
    // dnchisq tests
    assert_nan(dnchisq_inner(f64::NAN, 1.0, 0.0, false), "dnchisq(NaN,1,0)");
    assert_nan(dnchisq_inner(1.0, f64::NAN, 0.0, false), "dnchisq(1,NaN,0)");
    assert_nan(dnchisq_inner(1.0, 1.0, f64::NAN, false), "dnchisq(1,1,NaN)");
    // ncp=0 reduces to dchisq: dnchisq(x, df, 0) = dchisq(x, df)
    let d_ncp0 = dnchisq_inner(1.0, 1.0, 0.0, false);
    if d_ncp0 <= 0.0 {
        return Err(format!("dnchisq(1,1,0) = {}, expected > 0", d_ncp0));
    }
    // Valid density with ncp > 0
    let d_ncp = dnchisq_inner(2.0, 3.0, 2.0, false);
    if d_ncp <= 0.0 {
        return Err(format!("dnchisq(2,3,2) = {}, expected > 0", d_ncp));
    }
    // Negative x => 0
    assert_equiv(dnchisq_inner(-1.0, 3.0, 2.0, false), 0.0, "dnchisq(-1,3,2)");
    // Log scale
    let d_log = dnchisq_inner(2.0, 3.0, 2.0, true);
    if d_log >= 0.0 {
        return Err(format!("dnchisq(2,3,2,log) = {}, expected < 0", d_log));
    }

    // pnchisq tests
    assert_nan(
        pnchisq_inner(f64::NAN, 1.0, 0.0, true, false),
        "pnchisq(NaN,1,0)",
    );
    assert_nan(
        pnchisq_inner(0.0, f64::NAN, 0.0, true, false),
        "pnchisq(0,NaN,0)",
    );
    // ncp=0 reduces to pchisq: pnchisq(0, df, 0) = 0
    assert_equiv(
        pnchisq_inner(0.0, 3.0, 0.0, true, false),
        0.0,
        "pnchisq(0,3,0)",
    );
    // Large x => probability near 1
    let p_big = pnchisq_inner(100.0, 3.0, 2.0, true, false);
    if !(0.99..=1.0).contains(&p_big) {
        return Err(format!("pnchisq(100,3,2) = {}, expected ~1", p_big));
    }
    // Negative x => 0
    assert_equiv(
        pnchisq_inner(-1.0, 3.0, 2.0, true, false),
        0.0,
        "pnchisq(-1,3,2)",
    );
    // Upper tail
    let p_upper = pnchisq_inner(100.0, 3.0, 2.0, false, false);
    if !(0.0..0.01).contains(&p_upper) {
        return Err(format!("pnchisq(100,3,2,upper) = {}", p_upper));
    }

    // rnchisq tests
    rmath::rng::set_seed(42, 24);
    let r1 = rnchisq_inner(3.0, 2.0);
    if r1 < 0.0 {
        return Err(format!("rnchisq(3,2) = {}, expected >= 0", r1));
    }
    rmath::rng::set_seed(42, 24);
    let r2 = rnchisq_inner(3.0, 2.0);
    assert_equiv(r1, r2, "rnchisq reproducible");
    assert_nan(rnchisq_inner(f64::NAN, 2.0), "rnchisq(NaN,2)");

    Ok(())
}
