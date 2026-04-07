use crate::comparisons::*;
use rmath::dist::nt_dist::*;

pub fn run_tests() -> Result<(), String> {
    // dnt tests
    assert_nan(dnt_inner(f64::NAN, 3.0, 0.0, false), "dnt(NaN,3,0)");
    assert_nan(dnt_inner(0.0, f64::NAN, 0.0, false), "dnt(0,NaN,0)");
    assert_nan(dnt_inner(0.0, -1.0, 0.0, false), "dnt(0,-1,0)");
    // ncp=0 reduces to dt: dnt(0, df, 0) = dt(0, df)
    // dt(0, 3) = 1/(sqrt(3)*Beta(1/2, 3/2)) ~ 0.3676
    let d_ncp0 = dnt_inner(0.0, 3.0, 0.0, false);
    if !(d_ncp0 > 0.30 && d_ncp0 < 0.45) {
        return Err(format!("dnt(0,3,0) = {}, expected ~0.368", d_ncp0));
    }
    // Valid density with ncp > 0
    let d_ncp = dnt_inner(1.0, 3.0, 2.0, false);
    if d_ncp <= 0.0 {
        return Err(format!("dnt(1,3,2) = {}, expected > 0", d_ncp));
    }

    // pnt tests
    assert_nan(pnt_inner(f64::NAN, 3.0, 0.0, true, false), "pnt(NaN,3,0)");
    assert_nan(pnt_inner(0.0, f64::NAN, 0.0, true, false), "pnt(0,NaN,0)");
    assert_nan(pnt_inner(0.0, -1.0, 0.0, true, false), "pnt(0,-1,0)");
    // ncp=0 reduces to pt: pnt(0, df, 0) = pt(0, df) = 0.5
    let p_ncp0 = pnt_inner(0.0, 3.0, 0.0, true, false);
    if !(p_ncp0 > 0.49 && p_ncp0 < 0.51) {
        return Err(format!("pnt(0,3,0) = {}, expected ~0.5", p_ncp0));
    }
    // Large t => probability well above 0.5
    let p_big = pnt_inner(100.0, 3.0, 2.0, true, false);
    if p_big <= 0.5 {
        return Err(format!("pnt(100,3,2) = {}, expected > 0.5", p_big));
    }
    // Very negative t => probability near 0
    let p_neg = pnt_inner(-100.0, 3.0, 2.0, true, false);
    if !(0.0..0.01).contains(&p_neg) {
        return Err(format!("pnt(-100,3,2) = {}", p_neg));
    }

    // rnt tests
    rmath::rng::set_seed(42, 24);
    let r1 = rnt_inner(3.0, 2.0);
    if !(r1.is_finite()) {
        return Err(format!("rnt(3,2) = {}, expected finite", r1));
    }
    rmath::rng::set_seed(42, 24);
    let r2 = rnt_inner(3.0, 2.0);
    assert_equiv(r1, r2, "rnt reproducible");
    assert_nan(rnt_inner(f64::NAN, 2.0), "rnt(NaN,2)");

    Ok(())
}
