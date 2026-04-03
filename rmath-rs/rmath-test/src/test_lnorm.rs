use crate::comparisons::*;
use rmath::dist::lnorm::*;
use rmath::rng::*;

pub fn run_tests() -> Result<(), String> {
    // dlnorm tests
    assert_nan(dlnorm_inner(f64::NAN, 0.0, 1.0, false), "dlnorm(NaN,...)");
    assert_nan(dlnorm_inner(0.0, f64::NAN, 1.0, false), "dlnorm(0,NaN,1)");
    assert_nan(dlnorm_inner(0.0, 0.0, 0.0, false), "dlnorm(0,0,0)");
    assert_nan(dlnorm_inner(0.0, 0.0, -1.0, false), "dlnorm(0,0,-1)");
    assert_equiv(dlnorm_inner(0.0, 0.0, 1.0, false), 0.0, "dlnorm(0,0,1)");
    assert_equiv(dlnorm_inner(-1.0, 0.0, 1.0, false), 0.0, "dlnorm(-1,0,1)");
    if !(dlnorm_inner(1.0, 0.0, 1.0, false) > 0.0) {
        return Err("dlnorm(1,0,1) > 0".into());
    }

    // plnorm tests
    assert_nan(
        plnorm_inner(f64::NAN, 0.0, 1.0, true, false),
        "plnorm(NaN,...)",
    );
    assert_nan(plnorm_inner(0.0, 0.0, 0.0, true, false), "plnorm(0,0,0)");
    assert_nan(plnorm_inner(0.0, 0.0, -1.0, true, false), "plnorm(0,0,-1)");
    assert_equiv(
        plnorm_inner(1.0, 0.0, 1.0, true, false),
        0.5,
        "plnorm(1,0,1,lower)",
    );
    assert_equiv(
        plnorm_inner(0.0, 0.0, 1.0, true, false),
        0.0,
        "plnorm(0,0,1)",
    );
    if !(plnorm_inner(std::f64::consts::E, 0.0, 1.0, true, false) > 0.84) {
        return Err("plnorm(e,0,1)~0.84".into());
    }

    // qlnorm tests
    assert_nan(
        qlnorm_inner(f64::NAN, 0.0, 1.0, true, false),
        "qlnorm(NaN,...)",
    );
    assert_nan(qlnorm_inner(0.0, 0.0, 0.0, true, false), "qlnorm(0,0,0)");
    assert_nan(qlnorm_inner(-1.0, 0.0, 1.0, true, false), "qlnorm(-1,...)");
    assert_nan(qlnorm_inner(2.0, 0.0, 1.0, true, false), "qlnorm(2,...)");
    assert_equiv(
        qlnorm_inner(0.5, 0.0, 1.0, true, false),
        1.0,
        "qlnorm(0.5,0,1)",
    );
    assert_neginf(qlnorm_inner(0.0, 0.0, 1.0, true, false), "qlnorm(0)");
    assert_posinf(qlnorm_inner(1.0, 0.0, 1.0, true, false), "qlnorm(1)");

    // rlnorm tests
    set_seed(42, 24);
    let r1 = rlnorm_inner(0.0, 1.0);
    if !(r1 >= 0.0) {
        return Err("rlnorm(0,1) >= 0".into());
    }
    set_seed(42, 24);
    let r2 = rlnorm_inner(0.0, 1.0);
    assert_equiv(r1, r2, "rlnorm reproducible");
    assert_nan(rlnorm_inner(f64::NAN, 1.0), "rlnorm(NaN,1)");
    assert_nan(rlnorm_inner(0.0, 0.0), "rlnorm(0,0)");
    assert_nan(rlnorm_inner(0.0, -1.0), "rlnorm(0,-1)");

    Ok(())
}
