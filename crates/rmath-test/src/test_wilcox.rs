use crate::comparisons::*;
use rmath::nmath::dist::wilcox::*;

pub fn run_tests() -> Result<(), String> {
    let mut ms_state = rmath::nmath::state::MathState::default();
    // dwilcox tests
    assert_nan(dwilcox_inner(&mut ms_state, f64::NAN, 5.0, 5.0, false), "dwilcox(&mut ms_state, NaN,5,5)");
    assert_nan(dwilcox_inner(&mut ms_state, 0.0, f64::NAN, 5.0, false), "dwilcox(&mut ms_state, 0,NaN,5)");
    assert_nan(dwilcox_inner(&mut ms_state, 0.0, 5.0, f64::NAN, false), "dwilcox(&mut ms_state, 0,5,NaN)");
    // Out of range: x < 0 => 0
    assert_equiv(dwilcox_inner(&mut ms_state, -1.0, 5.0, 5.0, false), 0.0, "dwilcox(&mut ms_state, -1,5,5)");
    // Out of range: x > m*n => 0
    assert_equiv(
        dwilcox_inner(&mut ms_state, 100.0, 5.0, 5.0, false),
        0.0,
        "dwilcox(&mut ms_state, 100,5,5)",
    );
    // Invalid params: m <= 0
    assert_nan(dwilcox_inner(&mut ms_state, 0.0, 0.0, 5.0, false), "dwilcox(&mut ms_state, 0,0,5)");

    // pwilcox tests
    assert_nan(
        pwilcox_inner(&mut ms_state, f64::NAN, 5.0, 5.0, true, false),
        "pwilcox(&mut ms_state, NaN,5,5)",
    );
    assert_nan(
        pwilcox_inner(&mut ms_state, 0.0, f64::NAN, 5.0, true, false),
        "pwilcox(&mut ms_state, 0,NaN,5)",
    );
    // Negative q => 0
    assert_equiv(
        pwilcox_inner(&mut ms_state, -1.0, 5.0, 5.0, true, false),
        0.0,
        "pwilcox(&mut ms_state, -1,5,5)",
    );
    // q >= m*n => 1
    assert_equiv(
        pwilcox_inner(&mut ms_state, 50.0, 5.0, 5.0, true, false),
        1.0,
        "pwilcox(&mut ms_state, 50,5,5)",
    );
    // Invalid params: m <= 0
    assert_nan(pwilcox_inner(&mut ms_state, 0.0, 0.0, 5.0, true, false), "pwilcox(&mut ms_state, 0,0,5)");

    // qwilcox tests
    assert_nan(
        qwilcox_inner(&mut ms_state, f64::NAN, 5.0, 5.0, true, false),
        "qwilcox(&mut ms_state, NaN,5,5)",
    );
    assert_nan(
        qwilcox_inner(&mut ms_state, -0.5, 5.0, 5.0, true, false),
        "qwilcox(&mut ms_state, -0.5,5,5)",
    );
    assert_nan(
        qwilcox_inner(&mut ms_state, 1.5, 5.0, 5.0, true, false),
        "qwilcox(&mut ms_state, 1.5,5,5)",
    );

    // rwilcox tests
    let mut rng_state = rmath::nmath::rng::RngState(42, 24);
    let r1 = rwilcox_inner(&mut || rng_state.unif(), 5.0, 5.0);
    if r1 < 0.0 {
        return Err(format!("rwilcox(5,5) = {}, expected >= 0", r1));
    }
    let mut rng_state = rmath::nmath::rng::RngState(42, 24);
    let r2 = rwilcox_inner(&mut || rng_state.unif(), 5.0, 5.0);
    assert_equiv(r1, r2, "rwilcox reproducible");
    assert_nan(rwilcox_inner(&mut || rng_state.unif(), f64::NAN, 5.0), "rwilcox(NaN,5)");

    Ok(())
}
