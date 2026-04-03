use crate::comparisons::*;
use rmath::dist::wilcox::*;

pub fn run_tests() -> Result<(), String> {
    // dwilcox tests
    assert_nan(dwilcox_inner(f64::NAN, 5.0, 5.0, false), "dwilcox(NaN,5,5)");
    assert_nan(dwilcox_inner(0.0, f64::NAN, 5.0, false), "dwilcox(0,NaN,5)");
    assert_nan(dwilcox_inner(0.0, 5.0, f64::NAN, false), "dwilcox(0,5,NaN)");
    // Out of range: x < 0 => 0
    assert_equiv(dwilcox_inner(-1.0, 5.0, 5.0, false), 0.0, "dwilcox(-1,5,5)");
    // Out of range: x > m*n => 0
    assert_equiv(
        dwilcox_inner(100.0, 5.0, 5.0, false),
        0.0,
        "dwilcox(100,5,5)",
    );
    // Invalid params: m <= 0
    assert_nan(dwilcox_inner(0.0, 0.0, 5.0, false), "dwilcox(0,0,5)");

    // pwilcox tests
    assert_nan(
        pwilcox_inner(f64::NAN, 5.0, 5.0, true, false),
        "pwilcox(NaN,5,5)",
    );
    assert_nan(
        pwilcox_inner(0.0, f64::NAN, 5.0, true, false),
        "pwilcox(0,NaN,5)",
    );
    // Negative q => 0
    assert_equiv(
        pwilcox_inner(-1.0, 5.0, 5.0, true, false),
        0.0,
        "pwilcox(-1,5,5)",
    );
    // q >= m*n => 1
    assert_equiv(
        pwilcox_inner(50.0, 5.0, 5.0, true, false),
        1.0,
        "pwilcox(50,5,5)",
    );
    // Invalid params: m <= 0
    assert_nan(pwilcox_inner(0.0, 0.0, 5.0, true, false), "pwilcox(0,0,5)");

    // qwilcox tests
    assert_nan(
        qwilcox_inner(f64::NAN, 5.0, 5.0, true, false),
        "qwilcox(NaN,5,5)",
    );
    assert_nan(
        qwilcox_inner(-0.5, 5.0, 5.0, true, false),
        "qwilcox(-0.5,5,5)",
    );
    assert_nan(
        qwilcox_inner(1.5, 5.0, 5.0, true, false),
        "qwilcox(1.5,5,5)",
    );

    // rwilcox tests
    rmath::rng::set_seed(42, 24);
    let r1 = rwilcox_inner(5.0, 5.0);
    if !(r1 >= 0.0) {
        return Err(format!("rwilcox(5,5) = {}, expected >= 0", r1));
    }
    rmath::rng::set_seed(42, 24);
    let r2 = rwilcox_inner(5.0, 5.0);
    assert_equiv(r1, r2, "rwilcox reproducible");
    assert_nan(rwilcox_inner(f64::NAN, 5.0), "rwilcox(NaN,5)");

    Ok(())
}
