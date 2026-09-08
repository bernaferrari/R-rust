use crate::comparisons::*;
use rmath::dist::logistic::*;
use rmath::rng::*;

pub fn run_tests() -> Result<(), String> {
    // dlogis tests
    assert_nan(dlogis_inner(f64::NAN, 0.0, 1.0, false), "dlogis(NaN,...)");
    assert_nan(dlogis_inner(0.0, f64::NAN, 1.0, false), "dlogis(0,NaN,1)");
    assert_nan(dlogis_inner(0.0, 0.0, 0.0, false), "dlogis(0,0,0)");
    assert_nan(dlogis_inner(0.0, 0.0, -1.0, false), "dlogis(0,0,-1)");
    if dlogis_inner(0.0, 0.0, 1.0, false) <= 0.0 {
        return Err("dlogis(0,0,1) > 0".into());
    }

    // plogis tests
    assert_equiv(
        plogis_inner(0.0, 0.0, 1.0, true, false),
        0.5,
        "plogis(0,0,1,lower)",
    );
    assert_equiv(
        plogis_inner(0.0, 0.0, 1.0, false, false),
        0.5,
        "plogis(0,0,1,upper)",
    );
    if plogis_inner(10.0, 0.0, 1.0, true, false) <= 0.999 {
        return Err("plogis(10)~1".into());
    }
    if plogis_inner(-10.0, 0.0, 1.0, true, false) >= 0.001 {
        return Err("plogis(-10)~0".into());
    }
    assert_nan(plogis_inner(f64::NAN, 0.0, 1.0, true, false), "plogis(NaN)");
    assert_nan(plogis_inner(0.0, 0.0, 0.0, true, false), "plogis(0,0,0)");

    // qlogis tests
    assert_equiv(qlogis_inner(0.5, 0.0, 1.0, true, false), 0.0, "qlogis(0.5)");
    assert_posinf(qlogis_inner(1.0, 0.0, 1.0, true, false), "qlogis(1)");
    assert_neginf(qlogis_inner(0.0, 0.0, 1.0, true, false), "qlogis(0)");
    assert_nan(qlogis_inner(f64::NAN, 0.0, 1.0, true, false), "qlogis(NaN)");
    assert_nan(qlogis_inner(-1.0, 0.0, 1.0, true, false), "qlogis(-1)");

    // rlogis tests
    set_seed(42, 24);
    let r1 = rlogis_inner(0.0, 1.0);
    if !(r1.is_finite()) {
        return Err("rlogis(0,1) is finite".into());
    }
    set_seed(42, 24);
    let r2 = rlogis_inner(0.0, 1.0);
    assert_equiv(r1, r2, "rlogis reproducible");
    assert_nan(rlogis_inner(f64::NAN, 1.0), "rlogis(NaN,1)");
    assert_equiv(rlogis_inner(0.0, 0.0), 0.0, "rlogis(0,0)");

    Ok(())
}
