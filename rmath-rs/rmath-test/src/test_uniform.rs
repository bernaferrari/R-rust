use crate::comparisons::*;

use rmath::dist::uniform::*;
use rmath::rng::*;

pub fn run_tests() -> Result<(), String> {
    // dunif tests
    assert_equiv(
        dunif_inner(0.5, 0.0, 1.0, false),
        1.0,
        "dunif(0.5, 0, 1, 0)",
    );
    assert_equiv(
        dunif_inner(0.5, 0.0, 1.0, true),
        -0.0_f64,
        "dunif(0.5, 0, 1, 1)",
    );
    assert_equiv(
        dunif_inner(0.5, 0.0, 2.0, false),
        0.5,
        "dunif(0.5, 0, 2, 0)",
    );
    assert_equiv(
        dunif_inner(-1.0, 0.0, 1.0, false),
        0.0,
        "dunif(-1, 0, 1, 0)",
    );
    assert_equiv(dunif_inner(2.0, 0.0, 1.0, false), 0.0, "dunif(2, 0, 1, 0)");
    assert_equiv(dunif_inner(0.0, 0.0, 1.0, false), 1.0, "dunif(0, 0, 1, 0)");
    assert_equiv(dunif_inner(1.0, 0.0, 1.0, false), 1.0, "dunif(1, 0, 1, 0)");
    assert_equiv(
        dunif_inner(0.0, 0.0, 1.0, true),
        -0.0_f64,
        "dunif(0, 0, 1, 1)",
    );
    assert_nan(
        dunif_inner(f64::NAN, 0.0, 1.0, false),
        "dunif(NaN, 0, 1, 0)",
    );
    assert_nan(
        dunif_inner(0.5, f64::NAN, 1.0, false),
        "dunif(0.5, NaN, 1, 0)",
    );
    assert_nan(dunif_inner(0.5, 1.0, 0.0, false), "dunif(0.5, 1, 0, 0)");

    // punif tests
    assert_equiv(
        punif_inner(0.5, 0.0, 1.0, true, false),
        0.5,
        "punif(0.5,0,1,lower,linear)",
    );
    assert_equiv(
        punif_inner(0.5, 0.0, 1.0, false, false),
        0.5,
        "punif(0.5,0,1,upper,linear)",
    );
    assert_equiv(
        punif_inner(0.0, 0.0, 1.0, true, false),
        0.0,
        "punif(0,0,1,lower,linear)",
    );
    assert_equiv(
        punif_inner(1.0, 0.0, 1.0, true, false),
        1.0,
        "punif(1,0,1,lower,linear)",
    );
    assert_equiv(
        punif_inner(1.5, 0.0, 1.0, true, false),
        1.0,
        "punif(1.5,0,1,lower,linear)",
    );
    assert_equiv(
        punif_inner(-0.5, 0.0, 1.0, true, false),
        0.0,
        "punif(-0.5,0,1,lower,linear)",
    );
    assert_equiv(
        punif_inner(0.5, 0.0, 2.0, true, false),
        0.25,
        "punif(0.5,0,2,lower,linear)",
    );
    assert_nan(
        punif_inner(f64::NAN, 0.0, 1.0, true, false),
        "punif(NaN,...)",
    );
    assert_nan(
        punif_inner(0.5, f64::NAN, 1.0, true, false),
        "punif(0.5,NaN,...)",
    );
    assert_nan(punif_inner(0.5, 1.0, 0.0, true, false), "punif(0.5,1,0)");

    // punif log_p tests
    let log_half = libm::log(0.5);
    assert_equiv(
        punif_inner(0.5, 0.0, 1.0, true, true),
        log_half,
        "punif(0.5,0,1,lower,log)",
    );
    assert_neginf(
        punif_inner(0.0, 0.0, 1.0, true, true),
        "punif(0,0,1,lower,log)",
    );
    assert_equiv(
        punif_inner(1.0, 0.0, 1.0, true, true),
        0.0,
        "punif(1,0,1,lower,log)",
    );

    // qunif tests
    assert_equiv(
        qunif_inner(0.5, 0.0, 1.0, true, false),
        0.5,
        "qunif(0.5,0,1,lower,linear)",
    );
    assert_equiv(
        qunif_inner(0.0, 0.0, 1.0, true, false),
        0.0,
        "qunif(0,0,1,lower,linear)",
    );
    assert_equiv(
        qunif_inner(1.0, 0.0, 1.0, true, false),
        1.0,
        "qunif(1,0,1,lower,linear)",
    );
    assert_equiv(
        qunif_inner(0.5, 0.0, 2.0, true, false),
        1.0,
        "qunif(0.5,0,2,lower,linear)",
    );
    assert_nan(
        qunif_inner(f64::NAN, 0.0, 1.0, true, false),
        "qunif(NaN,...)",
    );
    assert_nan(qunif_inner(-1.0, 0.0, 1.0, true, false), "qunif(-1,...)");
    assert_nan(qunif_inner(2.0, 0.0, 1.0, true, false), "qunif(2,...)");
    assert_nan(qunif_inner(0.5, 1.0, 0.0, true, false), "qunif(0.5,1,0)");

    // runif: test that it produces values in range, and is reproducible
    set_seed(42, 24);
    let r1 = runif_inner(0.0, 1.0);
    let r2 = runif_inner(0.0, 1.0);
    assert!(r1 > 0.0 && r1 < 1.0, "runif(0,1) in range (1st)");
    assert!(r2 > 0.0 && r2 < 1.0, "runif(0,1) in range (2nd)");

    // Reproducibility: same seed -> same values
    set_seed(42, 24);
    let r3 = runif_inner(0.0, 1.0);
    let r4 = runif_inner(0.0, 1.0);
    assert_equiv(r1, r3, "runif reproducible (1st)");
    assert_equiv(r2, r4, "runif reproducible (2nd)");

    // runif with different range
    set_seed(42, 24);
    let r5 = runif_inner(10.0, 20.0);
    let expected = 10.0 + r1 * 10.0;
    assert_equiv(r5, expected, "runif(10,20) scaling");

    // runif edge cases
    assert_nan(runif_inner(f64::NAN, 1.0), "runif(NaN,1)");
    assert_nan(runif_inner(1.0, f64::NAN), "runif(1,NaN)");
    assert_nan(runif_inner(1.0, 0.0), "runif(1,0) b<a");

    Ok(())
}
