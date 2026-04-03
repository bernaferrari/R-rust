use crate::comparisons::*;

use rmath::dist::cauchy::*;
use rmath::rng::*;

pub fn run_tests() -> Result<(), String> {
    // dcauchy tests
    assert_equiv(
        dcauchy_inner(0.0, 0.0, 1.0, false),
        1.0 / std::f64::consts::PI,
        "dcauchy(0,0,1,0)",
    );
    assert_equiv(
        dcauchy_inner(1.0, 0.0, 1.0, false),
        1.0 / (std::f64::consts::PI * 2.0),
        "dcauchy(1,0,1,0)",
    );
    assert_equiv(
        dcauchy_inner(0.0, 0.0, 2.0, false),
        1.0 / (std::f64::consts::PI * 2.0),
        "dcauchy(0,0,2,0)",
    );
    assert_nan(dcauchy_inner(f64::NAN, 0.0, 1.0, false), "dcauchy(NaN,...)");
    assert_nan(
        dcauchy_inner(0.0, f64::NAN, 1.0, false),
        "dcauchy(0,NaN,...)",
    );
    assert_nan(dcauchy_inner(0.0, 0.0, 0.0, false), "dcauchy(0,0,0)");
    assert_nan(dcauchy_inner(0.0, 0.0, -1.0, false), "dcauchy(0,0,-1)");

    // pcauchy tests
    assert_equiv(
        pcauchy_inner(0.0, 0.0, 1.0, true, false),
        0.5,
        "pcauchy(0,0,1,lower,linear)",
    );
    assert_equiv(
        pcauchy_inner(0.0, 0.0, 1.0, false, false),
        0.5,
        "pcauchy(0,0,1,upper,linear)",
    );
    assert_equiv(
        pcauchy_inner(1.0, 0.0, 1.0, true, false),
        0.75,
        "pcauchy(1,0,1,lower,linear)",
    );
    assert_equiv(
        pcauchy_inner(-1.0, 0.0, 1.0, true, false),
        0.25,
        "pcauchy(-1,0,1,lower,linear)",
    );
    assert_equiv(
        pcauchy_inner(f64::INFINITY, 0.0, 1.0, true, false),
        1.0,
        "pcauchy(Inf,...)",
    );
    assert_equiv(
        pcauchy_inner(f64::NEG_INFINITY, 0.0, 1.0, true, false),
        0.0,
        "pcauchy(-Inf,...)",
    );
    assert_equiv(
        pcauchy_inner(f64::INFINITY, 0.0, 1.0, false, false),
        0.0,
        "pcauchy(Inf,upper)",
    );
    assert_nan(
        pcauchy_inner(f64::NAN, 0.0, 1.0, true, false),
        "pcauchy(NaN,...)",
    );
    assert_nan(pcauchy_inner(0.0, 0.0, 0.0, true, false), "pcauchy(0,0,0)");

    // pcauchy with location != 0
    assert_equiv(
        pcauchy_inner(5.0, 5.0, 1.0, true, false),
        0.5,
        "pcauchy(5,5,1,lower,linear)",
    );
    assert_equiv(
        pcauchy_inner(7.0, 5.0, 2.0, true, false),
        0.75,
        "pcauchy(7,5,2,lower,linear)",
    );

    // pcauchy log_p
    assert_equiv(
        pcauchy_inner(0.0, 0.0, 1.0, true, true),
        libm::log(0.5),
        "pcauchy(0,0,1,lower,log)",
    );
    assert_equiv(
        pcauchy_inner(f64::INFINITY, 0.0, 1.0, true, true),
        0.0,
        "pcauchy(Inf,lower,log)",
    );
    assert_neginf(
        pcauchy_inner(f64::NEG_INFINITY, 0.0, 1.0, true, true),
        "pcauchy(-Inf,lower,log)",
    );

    // qcauchy tests
    assert_equiv(
        qcauchy_inner(0.5, 0.0, 1.0, true, false),
        0.0,
        "qcauchy(0.5,0,1,lower,linear)",
    );
    assert_equiv(
        qcauchy_inner(0.75, 0.0, 1.0, true, false),
        1.0,
        "qcauchy(0.75,0,1,lower,linear)",
    );
    assert_equiv(
        qcauchy_inner(0.25, 0.0, 1.0, true, false),
        -1.0,
        "qcauchy(0.25,0,1,lower,linear)",
    );
    assert_neginf(
        qcauchy_inner(0.0, 0.0, 1.0, true, false),
        "qcauchy(0,0,1,lower,linear)",
    );
    assert_posinf(
        qcauchy_inner(1.0, 0.0, 1.0, true, false),
        "qcauchy(1,0,1,lower,linear)",
    );
    assert_equiv(
        qcauchy_inner(0.5, 5.0, 2.0, true, false),
        5.0,
        "qcauchy(0.5,5,2)",
    );
    assert_equiv(
        qcauchy_inner(0.75, 5.0, 2.0, true, false),
        7.0,
        "qcauchy(0.75,5,2)",
    );
    assert_nan(
        qcauchy_inner(f64::NAN, 0.0, 1.0, true, false),
        "qcauchy(NaN,...)",
    );
    assert_nan(
        qcauchy_inner(-1.0, 0.0, 1.0, true, false),
        "qcauchy(-1,...)",
    );
    assert_nan(qcauchy_inner(2.0, 0.0, 1.0, true, false), "qcauchy(2,...)");
    assert_nan(
        qcauchy_inner(0.5, 0.0, -1.0, true, false),
        "qcauchy(0.5,0,-1)",
    );
    assert_equiv(
        qcauchy_inner(0.5, 3.0, 0.0, true, false),
        3.0,
        "qcauchy(0.5,3,0)",
    );

    // rcauchy tests
    set_seed(42, 24);
    let r1 = rcauchy_inner(0.0, 1.0);
    assert!(r1.is_finite(), "rcauchy(0,1) is finite");
    // Reproducibility
    set_seed(42, 24);
    let r2 = rcauchy_inner(0.0, 1.0);
    assert_equiv(r1, r2, "rcauchy reproducible");
    // rcauchy with location/scale
    set_seed(42, 24);
    let r3 = rcauchy_inner(5.0, 2.0);
    set_seed(42, 24);
    let r4 = rcauchy_inner(0.0, 1.0);
    assert_equiv(r3, 5.0 + 2.0 * r4, "rcauchy(5,2) scaling");
    // Edge cases
    assert_nan(rcauchy_inner(f64::NAN, 1.0), "rcauchy(NaN,1)");
    assert_nan(rcauchy_inner(0.0, -1.0), "rcauchy(0,-1)");
    assert_nan(rcauchy_inner(0.0, f64::INFINITY), "rcauchy(0,Inf)");
    assert_equiv(rcauchy_inner(5.0, 0.0), 5.0, "rcauchy(5,0)");

    Ok(())
}
