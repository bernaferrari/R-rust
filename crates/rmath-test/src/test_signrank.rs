use crate::comparisons::*;
use r_runtime::nmath::dist::signrank::*;

pub fn run_tests() -> Result<(), String> {
    let mut ms_state = r_runtime::nmath::state::MathState::default();
    // dsignrank tests
    assert_nan(dsignrank_inner(&mut ms_state, f64::NAN, 5.0, false), "dsignrank(&mut ms_state, NaN,5)");
    assert_nan(dsignrank_inner(&mut ms_state, 0.0, f64::NAN, false), "dsignrank(&mut ms_state, 0,NaN)");
    // Valid density: dsignrank should be > 0 for a valid statistic value
    // The signed rank statistic for n=3 ranges from 0 to 6
    let d1 = dsignrank_inner(&mut ms_state, 3.0, 3.0, false);
    if d1 <= 0.0 {
        return Err(format!("dsignrank(&mut ms_state, 3,3) = {}, expected > 0", d1));
    }
    // Out of range: x < 0 or x > n*(n+1)/2 => 0
    assert_equiv(dsignrank_inner(&mut ms_state, -1.0, 3.0, false), 0.0, "dsignrank(&mut ms_state, -1,3)");
    assert_equiv(dsignrank_inner(&mut ms_state, 100.0, 3.0, false), 0.0, "dsignrank(&mut ms_state, 100,3)");

    // psignrank tests
    assert_nan(
        psignrank_inner(&mut ms_state, f64::NAN, 5.0, true, false),
        "psignrank(&mut ms_state, NaN,5)",
    );
    assert_nan(
        psignrank_inner(&mut ms_state, 0.0, f64::NAN, true, false),
        "psignrank(&mut ms_state, 0,NaN)",
    );
    // psignrank(&mut ms_state, 0, n) should be > 0 (there is a probability at 0)
    let p0 = psignrank_inner(&mut ms_state, 0.0, 3.0, true, false);
    if !(p0 > 0.0 && p0 < 1.0) {
        return Err(format!("psignrank(&mut ms_state, 0,3) = {}, expected in (0,1)", p0));
    }
    // Boundary: psignrank at maximum should be 1
    let p_max = psignrank_inner(&mut ms_state, 15.0, 5.0, true, false);
    if !(p_max > 0.99 && p_max <= 1.0) {
        return Err(format!("psignrank(&mut ms_state, 15,5) = {}, expected ~1", p_max));
    }

    // qsignrank tests
    assert_nan(
        qsignrank_inner(&mut ms_state, f64::NAN, 5.0, true, false),
        "qsignrank(&mut ms_state, NaN,5)",
    );
    assert_nan(qsignrank_inner(&mut ms_state, -0.5, 5.0, true, false), "qsignrank(&mut ms_state, -0.5,5)");
    assert_nan(qsignrank_inner(&mut ms_state, 1.5, 5.0, true, false), "qsignrank(&mut ms_state, 1.5,5)");
    // qsignrank boundary: qsignrank(&mut ms_state, 0, n) = 0
    assert_equiv(
        qsignrank_inner(&mut ms_state, 0.0, 3.0, true, false),
        0.0,
        "qsignrank(&mut ms_state, 0,3)",
    );
    // qsignrank(&mut ms_state, 1, n) = n*(n+1)/2
    assert_equiv(
        qsignrank_inner(&mut ms_state, 1.0, 3.0, true, false),
        6.0,
        "qsignrank(&mut ms_state, 1,3)",
    );

    // rsignrank tests
    let mut rng_state = r_runtime::nmath::rng::RngState(42, 24);
    let r1 = rsignrank_inner(&mut || rng_state.unif(), 5.0);
    if r1 < 0.0 {
        return Err(format!("rsignrank(5) = {}, expected >= 0", r1));
    }
    let mut rng_state = r_runtime::nmath::rng::RngState(42, 24);
    let r2 = rsignrank_inner(&mut || rng_state.unif(), 5.0);
    assert_equiv(r1, r2, "rsignrank reproducible");
    assert_nan(rsignrank_inner(&mut || rng_state.unif(), f64::NAN), "rsignrank(NaN)");

    Ok(())
}
