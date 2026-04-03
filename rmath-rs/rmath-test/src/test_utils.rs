use crate::comparisons::*;

// Import Rust functions directly from rmath crate
use rmath::special::cospi::*;
use rmath::special::mlutils::*;
use rmath::utils::*;

pub fn run_tests() -> Result<(), String> {
    let mut _errors = Vec::<String>::new();

    // fmax2 tests
    assert_equiv(fmax2(3.0, 5.0), 5.0, "fmax2(3,5)");
    assert_equiv(fmax2(5.0, 3.0), 5.0, "fmax2(5,3)");
    assert_equiv(fmax2(-1.0, -2.0), -1.0, "fmax2(-1,-2)");
    assert_nan(fmax2(f64::NAN, 1.0), "fmax2(NaN,1)");
    assert_nan(fmax2(1.0, f64::NAN), "fmax2(1,NaN)");

    // fmin2 tests
    assert_equiv(fmin2(3.0, 5.0), 3.0, "fmin2(3,5)");
    assert_equiv(fmin2(5.0, 3.0), 3.0, "fmin2(5,3)");
    assert_equiv(fmin2(-1.0, -2.0), -2.0, "fmin2(-1,-2)");
    assert_nan(fmin2(f64::NAN, 1.0), "fmin2(NaN,1)");
    assert_nan(fmin2(1.0, f64::NAN), "fmin2(1,NaN)");

    // imax2 / imin2 tests
    assert!(imax2(3, 5) == 5, "imax2(3,5)");
    assert!(imax2(5, 3) == 5, "imax2(5,3)");
    assert!(imax2(-1, -2) == -1, "imax2(-1,-2)");
    assert!(imin2(3, 5) == 3, "imin2(3,5)");
    assert!(imin2(5, 3) == 3, "imin2(5,3)");
    assert!(imin2(-1, -2) == -2, "imin2(-1,-2)");

    // sign tests
    assert_equiv(sign(3.0), 1.0, "sign(3)");
    assert_equiv(sign(-3.0), -1.0, "sign(-3)");
    assert_equiv(sign(0.0), 0.0, "sign(0)");
    assert_nan(sign(f64::NAN), "sign(NaN)");

    // fsign tests
    assert_equiv(fsign(3.0, 1.0), 3.0, "fsign(3,1)");
    assert_equiv(fsign(3.0, -1.0), -3.0, "fsign(3,-1)");
    assert_equiv(fsign(-3.0, 1.0), 3.0, "fsign(-3,1)");
    assert_nan(fsign(f64::NAN, 1.0), "fsign(NaN,1)");
    assert_nan(fsign(1.0, f64::NAN), "fsign(1,NaN)");

    // ftrunc tests
    assert_equiv(ftrunc(3.7), 3.0, "ftrunc(3.7)");
    assert_equiv(ftrunc(-3.7), -3.0, "ftrunc(-3.7)");
    assert_equiv(ftrunc(0.5), 0.0, "ftrunc(0.5)");

    // R_pow tests
    assert_equiv(R_pow(2.0, 3.0), 8.0, "R_pow(2,3)");
    assert_equiv(R_pow(2.0, 0.0), 1.0, "R_pow(2,0)");
    assert_equiv(R_pow(1.0, 5.0), 1.0, "R_pow(1,5)");
    assert_nan(R_pow(f64::NAN, 1.0), "R_pow(NaN,1)");
    // R_pow(1, NaN) returns 1.0 in C (x==1 check short-circuits before NaN check)
    assert_equiv(R_pow(1.0, f64::NAN), 1.0, "R_pow(1,NaN)");
    assert_posinf(R_pow(f64::INFINITY, 1.0), "R_pow(Inf,1)");
    assert_equiv(R_pow(f64::INFINITY, -1.0), 0.0, "R_pow(Inf,-1)");

    // R_pow_di tests
    assert_equiv(R_pow_di(2.0, 3), 8.0, "R_pow_di(2,3)");
    assert_equiv(R_pow_di(2.0, 0), 1.0, "R_pow_di(2,0)");
    assert_equiv(R_pow_di(2.0, -2), 0.25, "R_pow_di(2,-2)");
    assert_nan(R_pow_di(f64::NAN, 3), "R_pow_di(NaN,3)");

    // cospi tests
    assert_equiv(cospi(0.0), 1.0, "cospi(0)");
    assert_equiv(cospi(1.0), -1.0, "cospi(1)");
    assert_equiv(cospi(0.5), 0.0, "cospi(0.5)");
    assert_nan(cospi(f64::NAN), "cospi(NaN)");

    // sinpi tests
    assert_equiv(sinpi(0.0), 0.0, "sinpi(0)");
    assert_equiv(sinpi(0.5), 1.0, "sinpi(0.5)");
    assert_equiv(sinpi(-0.5), -1.0, "sinpi(-0.5)");
    assert_equiv(sinpi(1.0), 0.0, "sinpi(1)");
    assert_nan(sinpi(f64::NAN), "sinpi(NaN)");

    // tanpi tests
    assert_equiv(tanpi(0.0), 0.0, "tanpi(0)");
    assert_nan(tanpi(0.5), "tanpi(0.5)");
    assert_equiv(tanpi(0.25), 1.0, "tanpi(0.25)");
    assert_equiv(tanpi(-0.25), -1.0, "tanpi(-0.25)");
    assert_nan(tanpi(f64::NAN), "tanpi(NaN)");

    Ok(())
}
