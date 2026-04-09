use crate::comparisons::*;
use rmath::dist::weibull::*;
use rmath::rng::*;

pub fn run_tests() -> Result<(), String> {
    // dweibull tests
    assert_nan(
        dweibull_inner(f64::NAN, 1.0, 1.0, false),
        "dweibull(NaN,...)",
    );
    assert_nan(dweibull_inner(0.0, 0.0, 1.0, false), "dweibull(0,0,1)");
    assert_nan(dweibull_inner(0.0, -1.0, 1.0, false), "dweibull(0,-1,1)");
    assert_nan(dweibull_inner(0.0, 1.0, 0.0, false), "dweibull(0,1,0)");
    assert_nan(dweibull_inner(0.0, 1.0, -1.0, false), "dweibull(0,1,-1)");
    if dweibull_inner(1.0, 1.0, 1.0, false) <= 0.0 {
        return Err("dweibull(1,1,1) > 0".into());
    }
    assert_equiv(dweibull_inner(0.0, 1.0, 1.0, false), 1.0, "dweibull(0,1,1)");
    assert_equiv(
        dweibull_inner(-1.0, 1.0, 1.0, false),
        0.0,
        "dweibull(-1,1,1)",
    );

    // pweibull tests
    assert_nan(
        pweibull_inner(f64::NAN, 1.0, 1.0, true, false),
        "pweibull(NaN,...)",
    );
    assert_nan(
        pweibull_inner(0.0, 0.0, 1.0, true, false),
        "pweibull(0,0,1)",
    );
    assert_nan(
        pweibull_inner(0.0, -1.0, 1.0, true, false),
        "pweibull(0,-1,1)",
    );
    assert_nan(
        pweibull_inner(0.0, 1.0, 0.0, true, false),
        "pweibull(0,1,0)",
    );
    assert_equiv(
        pweibull_inner(0.0, 1.0, 1.0, true, false),
        0.0,
        "pweibull(0,1,1,lower)",
    );
    assert_equiv(
        pweibull_inner(0.0, 1.0, 1.0, false, false),
        1.0,
        "pweibull(0,1,1,upper)",
    );
    if pweibull_inner(1.0, 1.0, 1.0, true, false) <= 0.63 {
        return Err("pweibull(1,1,1)~0.632".into());
    }

    // qweibull tests
    assert_nan(
        qweibull_inner(f64::NAN, 1.0, 1.0, true, false),
        "qweibull(NaN,...)",
    );
    assert_nan(
        qweibull_inner(0.0, 0.0, 1.0, true, false),
        "qweibull(0,0,1)",
    );
    assert_nan(
        qweibull_inner(-1.0, 1.0, 1.0, true, false),
        "qweibull(-1,1,1)",
    );
    assert_nan(
        qweibull_inner(2.0, 1.0, 1.0, true, false),
        "qweibull(2,1,1)",
    );
    assert_nan(
        qweibull_inner(0.5, 1.0, 0.0, true, false),
        "qweibull(0.5,1,0)",
    );
    assert_equiv(
        qweibull_inner(0.5, 1.0, 1.0, true, false),
        libm::log(2.0),
        "qweibull(0.5,1,1)",
    );

    // rweibull tests
    set_seed(42, 24);
    let r1 = rweibull_inner(1.0, 1.0);
    if r1 < 0.0 {
        return Err("rweibull(1,1) >= 0".into());
    }
    set_seed(42, 24);
    let r2 = rweibull_inner(1.0, 1.0);
    assert_equiv(r1, r2, "rweibull reproducible");
    assert_nan(rweibull_inner(0.0, 1.0), "rweibull(0,1)");
    assert_nan(rweibull_inner(-1.0, 1.0), "rweibull(-1,1)");
    assert_equiv(rweibull_inner(1.0, 0.0), 0.0, "rweibull(1,0)");

    Ok(())
}
