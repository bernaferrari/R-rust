use crate::comparisons::*;
use rmath::dist::nbeta::*;

pub fn run_tests() -> Result<(), String> {
    // dnbeta tests
    assert_nan(
        dnbeta_inner(f64::NAN, 1.0, 1.0, 0.0, false),
        "dnbeta(NaN,1,1,0)",
    );
    assert_nan(
        dnbeta_inner(0.5, f64::NAN, 1.0, 0.0, false),
        "dnbeta(0.5,NaN,1,0)",
    );
    assert_nan(
        dnbeta_inner(0.5, 1.0, f64::NAN, 0.0, false),
        "dnbeta(0.5,1,NaN,0)",
    );
    assert_nan(
        dnbeta_inner(0.5, 1.0, 1.0, f64::NAN, false),
        "dnbeta(0.5,1,1,NaN)",
    );
    // Invalid params: a <= 0
    assert_nan(dnbeta_inner(0.5, 0.0, 1.0, 0.0, false), "dnbeta(0.5,0,1,0)");
    // Invalid params: b <= 0
    assert_nan(dnbeta_inner(0.5, 1.0, 0.0, 0.0, false), "dnbeta(0.5,1,0,0)");
    // Invalid params: ncp < 0
    assert_nan(
        dnbeta_inner(0.5, 1.0, 1.0, -1.0, false),
        "dnbeta(0.5,1,1,-1)",
    );
    // ncp=0 reduces to dbeta: dnbeta(x, a, b, 0) = dbeta(x, a, b)
    // dbeta(0.5, 1, 1) = 1
    let d_ncp0 = dnbeta_inner(0.5, 1.0, 1.0, 0.0, false);
    if !(d_ncp0 > 0.9 && d_ncp0 < 1.1) {
        return Err(format!("dnbeta(0.5,1,1,0) = {}, expected ~1.0", d_ncp0));
    }
    // x < 0 => 0
    assert_equiv(
        dnbeta_inner(-0.5, 1.0, 1.0, 1.0, false),
        0.0,
        "dnbeta(-0.5,1,1,1)",
    );
    // x > 1 => 0
    assert_equiv(
        dnbeta_inner(1.5, 1.0, 1.0, 1.0, false),
        0.0,
        "dnbeta(1.5,1,1,1)",
    );
    // Valid density with ncp > 0
    let d_ncp = dnbeta_inner(0.5, 2.0, 3.0, 2.0, false);
    if d_ncp <= 0.0 {
        return Err(format!("dnbeta(0.5,2,3,2) = {}, expected > 0", d_ncp));
    }

    // pnbeta tests
    assert_nan(
        pnbeta_inner(f64::NAN, 1.0, 1.0, 0.0, true, false),
        "pnbeta(NaN,1,1,0)",
    );
    assert_nan(
        pnbeta_inner(0.5, f64::NAN, 1.0, 0.0, true, false),
        "pnbeta(0.5,NaN,1,0)",
    );
    // ncp=0 reduces to pbeta: pnbeta(0, a, b, 0) = 0
    assert_equiv(
        pnbeta_inner(0.0, 2.0, 3.0, 0.0, true, false),
        0.0,
        "pnbeta(0,2,3,0)",
    );
    // pnbeta(1, a, b, 0) = 1
    assert_equiv(
        pnbeta_inner(1.0, 2.0, 3.0, 0.0, true, false),
        1.0,
        "pnbeta(1,2,3,0)",
    );
    // x < 0 => 0
    assert_equiv(
        pnbeta_inner(-0.5, 2.0, 3.0, 1.0, true, false),
        0.0,
        "pnbeta(-0.5,2,3,1)",
    );
    // x > 1 => 1
    assert_equiv(
        pnbeta_inner(1.5, 2.0, 3.0, 1.0, true, false),
        1.0,
        "pnbeta(1.5,2,3,1)",
    );

    Ok(())
}
