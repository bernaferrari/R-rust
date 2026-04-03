use crate::comparisons::*;
use rmath::dist::tukey::*;

pub fn run_tests() -> Result<(), String> {
    // ptukey tests
    assert_nan(
        ptukey_inner(f64::NAN, 3.0, 5.0, 10.0, true, false),
        "ptukey(NaN,3,5,10)",
    );
    assert_nan(
        ptukey_inner(0.0, f64::NAN, 5.0, 10.0, true, false),
        "ptukey(0,NaN,5,10)",
    );
    assert_nan(
        ptukey_inner(0.0, 3.0, f64::NAN, 10.0, true, false),
        "ptukey(0,3,NaN,10)",
    );
    assert_nan(
        ptukey_inner(0.0, 3.0, 5.0, f64::NAN, true, false),
        "ptukey(0,3,5,NaN)",
    );
    // ptukey(0,...) should be near 0 for lower tail
    let p0 = ptukey_inner(0.0, 3.0, 5.0, 10.0, true, false);
    if !(p0 >= 0.0 && p0 < 0.5) {
        return Err(format!("ptukey(0,3,5,10) = {}, expected near 0", p0));
    }
    // Large q => probability near 1 for lower tail
    let p_big = ptukey_inner(100.0, 3.0, 5.0, 10.0, true, false);
    if !(p_big > 0.99 && p_big <= 1.0) {
        return Err(format!("ptukey(100,3,5,10) = {}, expected ~1", p_big));
    }
    // Upper tail of large q should be near 0
    let p_upper = ptukey_inner(100.0, 3.0, 5.0, 10.0, false, false);
    if !(p_upper >= 0.0 && p_upper < 0.01) {
        return Err(format!("ptukey(100,3,5,10,upper) = {}", p_upper));
    }

    // qtukey tests
    assert_nan(
        qtukey_inner(f64::NAN, 3.0, 5.0, 10.0, true, false),
        "qtukey(NaN,3,5,10)",
    );
    assert_nan(
        qtukey_inner(-0.5, 3.0, 5.0, 10.0, true, false),
        "qtukey(-0.5,3,5,10)",
    );
    assert_nan(
        qtukey_inner(1.5, 3.0, 5.0, 10.0, true, false),
        "qtukey(1.5,3,5,10)",
    );
    // qtukey(0.95,...) should give a positive value
    let q1 = qtukey_inner(0.95, 3.0, 5.0, 10.0, true, false);
    if !(q1 > 0.0) {
        return Err(format!("qtukey(0.95,3,5,10) = {}, expected > 0", q1));
    }

    Ok(())
}
