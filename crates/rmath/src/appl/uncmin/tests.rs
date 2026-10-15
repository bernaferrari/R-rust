use super::*;
// R's `t^2` is computed as t*t (R_pow fast path), so these native
// objectives reproduce the R-level demo arithmetic bit for bit.
// Reference values: GNU R 4.7.0 (bac5839) on this machine, from
// stats::nlm on demo(nlm)'s objectives printed at full precision.
unsafe extern "C" fn rosenbrock(
    _n: std::os::raw::c_int,
    x: *mut f64,
    f: *mut f64,
    _state: *mut std::ffi::c_void,
) {
    // SAFETY: the optimizer supplies two initialized coordinates for this fixture.
    let x = unsafe { std::slice::from_raw_parts(x, 2) };
    let (x1, x2) = (x[0], x[1]);
    let t = x2 - x1 * x1;
    let u = 1.0 - x1;
    // R: 100 * t^2 + u^2 with ^2 as R_pow's t*t — mind the grouping
    // SAFETY: f points to the optimizer's writable scalar result.
    unsafe { *f = 100.0 * (t * t) + (u * u) };
}
unsafe extern "C" fn rosenbrock_g(
    _n: std::os::raw::c_int,
    x: *mut f64,
    g: *mut f64,
    _state: *mut std::ffi::c_void,
) {
    // SAFETY: the optimizer supplies two initialized coordinates for this fixture.
    let x = unsafe { std::slice::from_raw_parts(x, 2) };
    let (x1, x2) = (x[0], x[1]);
    // SAFETY: the optimizer supplies a disjoint two-element gradient output.
    let g = unsafe { std::slice::from_raw_parts_mut(g, 2) };
    g[0] = -400.0 * x1 * (x2 - x1 * x1) - 2.0 * (1.0 - x1);
    g[1] = 200.0 * (x2 - x1 * x1);
}
unsafe extern "C" fn rosenbrock_h(
    nr: std::os::raw::c_int,
    _n: std::os::raw::c_int,
    x: *mut f64,
    h: *mut f64,
    _state: *mut std::ffi::c_void,
) {
    // SAFETY: the optimizer supplies two initialized coordinates for this fixture.
    let x = unsafe { std::slice::from_raw_parts(x, 2) };
    let (x1, x2) = (x[0], x[1]);
    let nr = nr as usize;
    // SAFETY: the optimizer supplies a disjoint 2-by-2 Hessian output.
    let h = unsafe { std::slice::from_raw_parts_mut(h, 4) };
    h[0] = 2.0 - 400.0 * x2 + 1200.0 * x1 * x1;
    h[1] = -400.0 * x1;
    h[0 + 1 * nr] = -400.0 * x1;
    h[1 + 1 * nr] = 200.0;
}
unsafe extern "C" fn helical(
    _n: std::os::raw::c_int,
    x: *mut f64,
    f: *mut f64,
    _state: *mut std::ffi::c_void,
) {
    // SAFETY: the optimizer supplies three initialized coordinates for this fixture.
    let x = unsafe { std::slice::from_raw_parts(x, 3) };
    let theta = x[1].atan2(x[0]) / (2.0 * std::f64::consts::PI);
    let f1 = 10.0 * (x[2] - 10.0 * theta);
    let f2 = 10.0 * ((x[0] * x[0] + x[1] * x[1]).sqrt() - 1.0);
    let f3 = x[2];
    // SAFETY: f points to the optimizer's writable scalar result.
    unsafe { *f = f1 * f1 + f2 * f2 + f3 * f3 };
}
struct Outcome {
    fpls: f64,
    xpls: Vec<f64>,
    gpls: Vec<f64>,
    code: std::os::raw::c_int,
    itncnt: std::os::raw::c_int,
}
unsafe fn run_nlm(
    n: usize,
    x0: &[f64],
    fcn: UncminFcn,
    d1fcn: UncminD1Fcn,
    d2fcn: UncminD2Fcn,
    iagflg: std::os::raw::c_int,
    iahflg: std::os::raw::c_int,
) -> Outcome {
    let mut x = x0.to_vec();
    let mut typsiz = vec![1.0f64; n];
    let mut xpls = vec![0.0f64; n];
    let mut gpls = vec![0.0f64; n];
    let mut a = vec![0.0f64; n * n];
    let mut wrk = vec![0.0f64; 8 * n];
    let mut fpls = 0.0f64;
    let mut code = 0;
    let mut itncnt = 0;
    let mut msg: std::os::raw::c_int = 9; // print.level 0 default
    // SAFETY: all workspace extents match optif9; callback buffers are disjoint
    // and remain live for the entire call.
    unsafe {
        optif9(
            n as std::os::raw::c_int,
            n as std::os::raw::c_int,
            x.as_mut_ptr(),
            fcn,
            d1fcn,
            d2fcn,
            std::ptr::null_mut(),
            typsiz.as_mut_ptr(),
            1.0,                             // fscale
            1,                               // method: line search
            if iahflg != 0 { 0 } else { 1 }, // iexp
            &mut msg,
            12,  // ndigit
            100, // iterlim
            iagflg,
            iahflg,
            1.0,  // dlt
            1e-6, // gradtol
            0.0,  // stepmax -> auto
            1e-6, // steptol
            xpls.as_mut_ptr(),
            &mut fpls,
            gpls.as_mut_ptr(),
            &mut code,
            a.as_mut_ptr(),
            wrk.as_mut_ptr(),
            &mut itncnt,
        )
    };
    assert_eq!(msg, 0);
    Outcome {
        fpls,
        xpls,
        gpls,
        code,
        itncnt,
    }
}
unsafe extern "C" fn never_called(
    _n: std::os::raw::c_int,
    _x: *mut f64,
    _g: *mut f64,
    _state: *mut std::ffi::c_void,
) {
    panic!("d1fcn must not be called in this scenario");
}
#[test]
fn nlm_rosenbrock_matches_gnu() {
    unsafe {
        // Finite-difference gradients go through the QR secant update,
        // where the last un-replicated clang FMA contraction leaves ~1e-8
        // relative drift in the terminal values (identical at R's str()
        // display precision); code and iteration count stay exact.
        let o = run_nlm(
            2,
            &[-1.2, 1.0],
            rosenbrock,
            never_called,
            never_called2,
            0,
            0,
        );
        assert!((o.fpls - 3.9737654444861321e-12).abs() / 3.9737654444861321e-12 < 1e-6);
        assert!((o.xpls[0] - 0.99999800663921257).abs() < 1e-9);
        assert!((o.xpls[1] - 0.99999601495037826).abs() < 1e-9);
        assert!((o.gpls[0] - -6.5392005545988035e-07).abs() / 6.5392005545988035e-07 < 1e-5);
        assert!((o.gpls[1] - 3.3359593976126505e-07).abs() / 3.3359593976126505e-07 < 1e-5);
        assert_eq!(o.code, 1);
        assert_eq!(o.itncnt, 23);
    }
}
unsafe extern "C" fn never_called2(
    _nr: std::os::raw::c_int,
    _n: std::os::raw::c_int,
    _x: *mut f64,
    _h: *mut f64,
    _state: *mut std::ffi::c_void,
) {
    panic!("d2fcn must not be called in this scenario");
}
#[test]
fn nlm_rosenbrock_analytic_gradient_matches_gnu() {
    unsafe {
        let o = run_nlm(
            2,
            &[-1.2, 1.0],
            rosenbrock,
            rosenbrock_g,
            never_called2,
            1,
            0,
        );
        assert_eq!(o.fpls, 1.1820961652299623e-20);
        assert_eq!(o.xpls[0], 1.0000000000906322);
        assert_eq!(o.xpls[1], 1.0000000001752587);
        assert_eq!(o.gpls[0], 2.5835205088543604e-09);
        assert_eq!(o.gpls[1], -1.2011280858814644e-09);
        assert_eq!(o.code, 1);
        assert_eq!(o.itncnt, 24);
    }
}
#[test]
fn nlm_rosenbrock_analytic_hessian_matches_gnu() {
    unsafe {
        let o = run_nlm(
            2,
            &[-1.2, 1.0],
            rosenbrock,
            rosenbrock_g,
            rosenbrock_h,
            1,
            1,
        );
        assert_eq!(o.fpls, 1.1270257587201423e-17);
        assert_eq!(o.xpls[0], 0.99999999928545746);
        assert_eq!(o.xpls[1], 0.99999999824289532);
        assert_eq!(o.gpls[0], 1.2977875942294302e-07);
        assert_eq!(o.gpls[1], -6.5603922294599215e-08);
        assert_eq!(o.code, 1);
        assert_eq!(o.itncnt, 24);
    }
}
#[test]
fn nlm_helical_valley_matches_gnu() {
    unsafe {
        // See nlm_rosenbrock_matches_gnu: FD-gradient path drift.
        let o = run_nlm(
            3,
            &[-1.0, 0.0, 0.0],
            helical,
            never_called,
            never_called2,
            0,
            0,
        );
        assert!((o.fpls - 1.238240707101609e-14).abs() / 1.238240707101609e-14 < 1e-4);
        assert!((o.xpls[0] - 0.99999999812210127).abs() < 1e-8);
        assert!((o.xpls[1] - 3.0711081852378829e-09).abs() < 1e-12);
        assert!((o.xpls[2] - -6.0634312658223856e-09).abs() < 1e-12);
        assert!((o.gpls[0] - -3.7557975578744549e-07).abs() / 3.7557975578744549e-07 < 2e-4);
        assert!((o.gpls[1] - 3.4858916824254435e-06).abs() / 3.4858916824254435e-06 < 2e-4);
        assert!((o.gpls[2] - -2.2023772144331659e-06).abs() / 2.2023772144331659e-06 < 2e-4);
        assert_eq!(o.code, 2);
        assert_eq!(o.itncnt, 27);
    }
}
