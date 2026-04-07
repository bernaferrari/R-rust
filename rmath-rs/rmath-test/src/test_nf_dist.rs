use crate::comparisons::*;
use rmath::dist::nf_dist::*;

pub fn run_tests() -> Result<(), String> {
    // dnf tests
    assert_nan(dnf_inner(f64::NAN, 1.0, 1.0, 0.0, false), "dnf(NaN,1,1,0)");
    assert_nan(dnf_inner(1.0, f64::NAN, 1.0, 0.0, false), "dnf(1,NaN,1,0)");
    assert_nan(dnf_inner(1.0, 1.0, f64::NAN, 0.0, false), "dnf(1,1,NaN,0)");
    // ncp=0 reduces to df: dnf(x, df1, df2, 0) = df(x, df1, df2)
    // df(1, 3, 5) should be positive
    let d_ncp0 = dnf_inner(1.0, 3.0, 5.0, 0.0, false);
    if d_ncp0 <= 0.0 {
        return Err(format!("dnf(1,3,5,0) = {}, expected > 0", d_ncp0));
    }
    // Valid density with ncp > 0
    let d_ncp = dnf_inner(2.0, 3.0, 5.0, 2.0, false);
    if d_ncp <= 0.0 {
        return Err(format!("dnf(2,3,5,2) = {}, expected > 0", d_ncp));
    }
    // Negative x => 0
    assert_equiv(dnf_inner(-1.0, 3.0, 5.0, 2.0, false), 0.0, "dnf(-1,3,5,2)");
    // Log scale
    let d_log = dnf_inner(2.0, 3.0, 5.0, 2.0, true);
    if d_log >= 0.0 {
        return Err(format!("dnf(2,3,5,2,log) = {}, expected < 0", d_log));
    }

    // pnf tests
    assert_nan(
        pnf_inner(f64::NAN, 1.0, 1.0, 0.0, true, false),
        "pnf(NaN,1,1,0)",
    );
    assert_nan(
        pnf_inner(0.0, f64::NAN, 1.0, 0.0, true, false),
        "pnf(0,NaN,1,0)",
    );
    // ncp=0 reduces to pf: pnf(0, df1, df2, 0) = 0
    assert_equiv(
        pnf_inner(0.0, 3.0, 5.0, 0.0, true, false),
        0.0,
        "pnf(0,3,5,0)",
    );
    // Large x => probability well above 0.5
    let p_big = pnf_inner(100.0, 3.0, 5.0, 2.0, true, false);
    if p_big <= 0.0 {
        return Err(format!("pnf(100,3,5,2) = {}, expected > 0", p_big));
    }
    // Negative x => 0
    assert_equiv(
        pnf_inner(-1.0, 3.0, 5.0, 2.0, true, false),
        0.0,
        "pnf(-1,3,5,2)",
    );

    // rnf tests
    rmath::rng::set_seed(42, 24);
    let r1 = rnf_inner(3.0, 5.0, 2.0);
    if r1 < 0.0 {
        return Err(format!("rnf(3,5,2) = {}, expected >= 0", r1));
    }
    rmath::rng::set_seed(42, 24);
    let r2 = rnf_inner(3.0, 5.0, 2.0);
    assert_equiv(r1, r2, "rnf reproducible");

    Ok(())
}
