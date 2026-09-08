use crate::comparisons::*;

use rmath::dist::exponential::*;
use rmath::rng::*;

pub fn run_tests() -> Result<(), String> {
    // dexp tests
    assert_equiv(dexp_inner(0.0, 1.0, false), 1.0, "dexp(0,1,0)");
    assert_equiv(dexp_inner(0.0, 1.0, true), -0.0_f64, "dexp(0,1,1)");
    assert_equiv(dexp_inner(1.0, 1.0, false), libm::exp(-1.0), "dexp(1,1,0)");
    assert_equiv(
        dexp_inner(1.0, 2.0, false),
        libm::exp(-0.5) / 2.0,
        "dexp(1,2,0)",
    );
    assert_equiv(dexp_inner(-1.0, 1.0, false), 0.0, "dexp(-1,1,0)");
    assert_neginf(dexp_inner(-1.0, 1.0, true), "dexp(-1,1,1)");
    assert_nan(dexp_inner(f64::NAN, 1.0, false), "dexp(NaN,1,0)");
    assert_nan(dexp_inner(0.0, f64::NAN, false), "dexp(0,NaN,0)");
    assert_nan(dexp_inner(0.0, 0.0, false), "dexp(0,0,0)");
    assert_nan(dexp_inner(0.0, -1.0, false), "dexp(0,-1,0)");

    // pexp tests
    assert_equiv(
        pexp_inner(0.0, 1.0, true, false),
        0.0,
        "pexp(0,1,lower,linear)",
    );
    assert_equiv(
        pexp_inner(0.0, 1.0, false, false),
        1.0,
        "pexp(0,1,upper,linear)",
    );
    assert_equiv(
        pexp_inner(1.0, 1.0, true, false),
        -libm::expm1(-1.0),
        "pexp(1,1,lower,linear)",
    );
    assert_equiv(
        pexp_inner(-1.0, 1.0, true, false),
        0.0,
        "pexp(-1,1,lower,linear)",
    );
    assert_nan(pexp_inner(f64::NAN, 1.0, true, false), "pexp(NaN,...)");
    assert_nan(pexp_inner(0.0, -1.0, true, false), "pexp(0,-1,...)");

    // pexp log_p
    assert_neginf(pexp_inner(0.0, 1.0, true, true), "pexp(0,1,lower,log)");
    assert_equiv(
        pexp_inner(0.0, 1.0, false, true),
        0.0,
        "pexp(0,1,upper,log)",
    );

    // qexp tests
    assert_equiv(
        qexp_inner(0.5, 1.0, true, false),
        libm::log(2.0),
        "qexp(0.5,1,lower,linear)",
    );
    assert_equiv(
        qexp_inner(0.0, 1.0, true, false),
        0.0,
        "qexp(0,1,lower,linear)",
    );
    assert_posinf(qexp_inner(1.0, 1.0, true, false), "qexp(1,1,lower,linear)");
    assert_nan(qexp_inner(f64::NAN, 1.0, true, false), "qexp(NaN,...)");
    assert_nan(qexp_inner(-1.0, 1.0, true, false), "qexp(-1,...)");
    assert_nan(qexp_inner(2.0, 1.0, true, false), "qexp(2,...)");
    assert_nan(qexp_inner(0.5, -1.0, true, false), "qexp(0.5,-1,...)");

    // rexp tests
    set_seed(42, 24);
    let r1 = rexp_inner(1.0);
    assert!(r1 > 0.0, "rexp(1) > 0");

    // Reproducibility
    set_seed(42, 24);
    let r2 = rexp_inner(1.0);
    assert_equiv(r1, r2, "rexp reproducible");

    // rexp with scale != 1
    set_seed(42, 24);
    let r3 = rexp_inner(2.0);
    set_seed(42, 24);
    let er1 = exp_rand();
    assert_equiv(r3, 2.0 * er1, "rexp(2) = 2*exp_rand()");

    // Edge cases
    assert_equiv(rexp_inner(0.0), 0.0, "rexp(0)");
    assert_nan(rexp_inner(-1.0), "rexp(-1)");
    assert_nan(rexp_inner(f64::NAN), "rexp(NaN)");

    Ok(())
}
