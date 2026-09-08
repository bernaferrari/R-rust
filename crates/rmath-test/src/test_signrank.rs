use crate::comparisons::*;
use rmath::dist::signrank::*;

pub fn run_tests() -> Result<(), String> {
    // dsignrank tests
    assert_nan(dsignrank_inner(f64::NAN, 5.0, false), "dsignrank(NaN,5)");
    assert_nan(dsignrank_inner(0.0, f64::NAN, false), "dsignrank(0,NaN)");
    // Valid density: dsignrank should be > 0 for a valid statistic value
    // The signed rank statistic for n=3 ranges from 0 to 6
    let d1 = dsignrank_inner(3.0, 3.0, false);
    if d1 <= 0.0 {
        return Err(format!("dsignrank(3,3) = {}, expected > 0", d1));
    }
    // Out of range: x < 0 or x > n*(n+1)/2 => 0
    assert_equiv(dsignrank_inner(-1.0, 3.0, false), 0.0, "dsignrank(-1,3)");
    assert_equiv(dsignrank_inner(100.0, 3.0, false), 0.0, "dsignrank(100,3)");

    // psignrank tests
    assert_nan(
        psignrank_inner(f64::NAN, 5.0, true, false),
        "psignrank(NaN,5)",
    );
    assert_nan(
        psignrank_inner(0.0, f64::NAN, true, false),
        "psignrank(0,NaN)",
    );
    // psignrank(0, n) should be > 0 (there is a probability at 0)
    let p0 = psignrank_inner(0.0, 3.0, true, false);
    if !(p0 > 0.0 && p0 < 1.0) {
        return Err(format!("psignrank(0,3) = {}, expected in (0,1)", p0));
    }
    // Boundary: psignrank at maximum should be 1
    let p_max = psignrank_inner(15.0, 5.0, true, false);
    if !(p_max > 0.99 && p_max <= 1.0) {
        return Err(format!("psignrank(15,5) = {}, expected ~1", p_max));
    }

    // qsignrank tests
    assert_nan(
        qsignrank_inner(f64::NAN, 5.0, true, false),
        "qsignrank(NaN,5)",
    );
    assert_nan(qsignrank_inner(-0.5, 5.0, true, false), "qsignrank(-0.5,5)");
    assert_nan(qsignrank_inner(1.5, 5.0, true, false), "qsignrank(1.5,5)");
    // qsignrank boundary: qsignrank(0, n) = 0
    assert_equiv(
        qsignrank_inner(0.0, 3.0, true, false),
        0.0,
        "qsignrank(0,3)",
    );
    // qsignrank(1, n) = n*(n+1)/2
    assert_equiv(
        qsignrank_inner(1.0, 3.0, true, false),
        6.0,
        "qsignrank(1,3)",
    );

    // rsignrank tests
    rmath::rng::set_seed(42, 24);
    let r1 = rsignrank_inner(5.0);
    if r1 < 0.0 {
        return Err(format!("rsignrank(5) = {}, expected >= 0", r1));
    }
    rmath::rng::set_seed(42, 24);
    let r2 = rsignrank_inner(5.0);
    assert_equiv(r1, r2, "rsignrank reproducible");
    assert_nan(rsignrank_inner(f64::NAN), "rsignrank(NaN)");

    Ok(())
}
