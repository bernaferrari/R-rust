//! Brent's root-finding method (zeroin).
//! Port of r-source/src/library/stats/src/zeroin.c

use core::ffi::{c_double, c_int, c_void};

/// Function pointer type for the objective function passed to R_zeroin2.
pub type R_zeroin2_fn = unsafe extern "C" fn(f64, *mut c_void) -> f64;

/// Brent's method for finding a root of a function in a given interval.
///
/// Port of `R_zeroin2` from R's `src/library/stats/src/zeroin.c`.
///
/// # Safety
/// - `f` must be a valid function pointer.
/// - `info` must be a valid pointer (or null) as expected by `f`.
/// - `Tol` and `Maxit` must be valid pointers to a `c_double` and `c_int` respectively.
pub unsafe fn R_zeroin2(
    mut ax: c_double,
    mut bx: c_double,
    mut fa: c_double,
    mut fb: c_double,
    f: R_zeroin2_fn,
    info: *mut c_void,
    Tol: *mut c_double,
    Maxit: *mut c_int,
) -> c_double {
    let mut a: c_double;
    let mut b: c_double;
    let mut c: c_double;
    let mut fc: c_double;
    let tol: c_double;
    let mut maxit: c_int;

    a = ax;
    b = bx;
    c = a;
    fc = fa;
    maxit = *Maxit + 1;
    let tol = *Tol;

    if fa == 0.0 {
        *Tol = 0.0;
        *Maxit = 0;
        return a;
    }
    if fb == 0.0 {
        *Tol = 0.0;
        *Maxit = 0;
        return b;
    }

    while maxit > 0 {
        maxit -= 1;

        let prev_step = b - a;
        let tol_act;
        let p: c_double;
        let q: c_double;
        let mut new_step: c_double;

        if libm::fabs(fc) < libm::fabs(fb) {
            let tmp_a = a;
            a = b;
            b = c;
            c = tmp_a;

            let tmp_fa = fa;
            fa = fb;
            fb = fc;
            fc = tmp_fa;
        }
        tol_act = 2.0 * f64::EPSILON * libm::fabs(b) + tol / 2.0;
        new_step = (c - b) / 2.0;

        if libm::fabs(new_step) <= tol_act || fb == 0.0 {
            *Maxit -= maxit;
            *Tol = libm::fabs(c - b);
            return b;
        }

        if libm::fabs(prev_step) >= tol_act && libm::fabs(fa) > libm::fabs(fb) {
            let t1: c_double;
            let cb: c_double;
            let t2: c_double;

            cb = c - b;
            if a == c {
                t1 = fb / fa;
                let p_val = cb * t1;
                let q_val = 1.0 - t1;

                // Inline the rest of the logic using p_val and q_val
                let mut p_local = p_val;
                let mut q_local = q_val;

                if p_local > 0.0 {
                    q_local = -q_local;
                } else {
                    p_local = -p_local;
                }

                if p_local < (0.75 * cb * q_local - libm::fabs(tol_act * q_local) / 2.0)
                    && p_local < libm::fabs(prev_step * q_local / 2.0)
                {
                    new_step = p_local / q_local;
                }
            } else {
                let q_val = fa / fc;
                let t1_val = fb / fc;
                let t2_val = fb / fa;
                let p_val = t2_val * (cb * q_val * (q_val - t1_val) - (b - a) * (t1_val - 1.0));
                let q_val = (q_val - 1.0) * (t1_val - 1.0) * (t2_val - 1.0);

                let mut p_local = p_val;
                let mut q_local = q_val;

                if p_local > 0.0 {
                    q_local = -q_local;
                } else {
                    p_local = -p_local;
                }

                if p_local < (0.75 * cb * q_local - libm::fabs(tol_act * q_local) / 2.0)
                    && p_local < libm::fabs(prev_step * q_local / 2.0)
                {
                    new_step = p_local / q_local;
                }
            }
        }

        if libm::fabs(new_step) < tol_act {
            if new_step > 0.0 {
                new_step = tol_act;
            } else {
                new_step = -tol_act;
            }
        }
        a = b;
        fa = fb;
        b += new_step;
        fb = f(b, info);
        if (fb > 0.0 && fc > 0.0) || (fb < 0.0 && fc < 0.0) {
            c = a;
            fc = fa;
        }
    }
    *Tol = libm::fabs(c - b);
    *Maxit = -1;
    b
}
