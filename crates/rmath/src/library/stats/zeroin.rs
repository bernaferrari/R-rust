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
    unsafe {
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
                a = b;
                fa = fb;
                b = c;
                fb = fc;
                c = a;
                fc = fa;
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
}

struct ZeroinCtx {
    fun: crate::sexp::ffi::SEXP,
    rho: crate::sexp::ffi::SEXP,
}

unsafe extern "C" fn zeroin_call(x: f64, info: *mut core::ffi::c_void) -> f64 {
    unsafe {
        use crate::sexp::accessors::{REAL, TYPEOF, XLENGTH};
        use crate::sexp::constructors::{Rf_ScalarReal, Rf_lang2};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::protect::protect;
        let ctx = &*(info as *const ZeroinCtx);
        let xv = Rf_ScalarReal(x);
        let _xv = protect(xv);
        let call = Rf_lang2(ctx.fun, xv);
        let _c = protect(call);
        let v = crate::eval::eval::Rf_eval(call, ctx.rho);
        let _v = protect(v);
        let vr = if TYPEOF(v) == SEXPTYPE::REALSXP {
            v
        } else {
            crate::main::coerce::coerceVector(v, SEXPTYPE::REALSXP.as_c_int())
        };
        let _vr = protect(vr);
        if XLENGTH(vr) < 1 {
            return f64::NAN;
        }
        finite_uniroot(*REAL(vr))
    }
}
fn finite_uniroot(v: f64) -> f64 {
    if v.is_nan() {
        UNIROOT_NOTE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        f64::MAX
    } else if v == f64::NEG_INFINITY {
        UNIROOT_NEG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        f64::MIN
    } else if v.is_infinite() {
        UNIROOT_POS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        f64::MAX
    } else {
        v
    }
}

fn flush_uniroot_warnings() {
    let na = UNIROOT_NOTE.swap(0, std::sync::atomic::Ordering::Relaxed);
    let neg = UNIROOT_NEG.swap(0, std::sync::atomic::Ordering::Relaxed);
    let pos = UNIROOT_POS.swap(0, std::sync::atomic::Ordering::Relaxed);
    unsafe {
        for _ in 0..na {
            crate::mainutils::errors::Rf_warning1(c"NA replaced by maximum positive value".as_ptr());
        }
        for _ in 0..neg {
            crate::mainutils::errors::Rf_warning1(
                c"-Inf replaced by maximally negative value".as_ptr(),
            );
        }
        for _ in 0..pos {
            crate::mainutils::errors::Rf_warning1(c"Inf replaced by maximum positive value".as_ptr());
        }
    }
}

static UNIROOT_NOTE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static UNIROOT_NEG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static UNIROOT_POS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);


/// GNU `uniroot(f, interval)`.
pub unsafe fn do_uniroot(_call: crate::sexp::ffi::SEXP, _op: crate::sexp::ffi::SEXP, args: crate::sexp::ffi::SEXP, rho: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, INTEGER, REAL, SET_VECTOR_ELT, TYPEOF, XLENGTH};
        use crate::sexp::constructors::{Rf_ScalarReal, Rf_allocVector3};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::protect::protect;
        let fun = CAR(args);
        let interval = CAR(CDR(args));
        let ax = if TYPEOF(interval) == SEXPTYPE::REALSXP {
            *REAL(interval)
        } else {
            *INTEGER(interval) as f64
        };
        let bx = if TYPEOF(interval) == SEXPTYPE::REALSXP {
            *REAL(interval).add(1)
        } else {
            *INTEGER(interval).add(1) as f64
        };
        let mut ctx = ZeroinCtx { fun, rho };
        let fa = zeroin_call(ax, &mut ctx as *mut _ as *mut core::ffi::c_void);
        let fb = zeroin_call(bx, &mut ctx as *mut _ as *mut core::ffi::c_void);
        let mut tol = 1e-8;
        let mut maxit: core::ffi::c_int = 1000;
        let root = R_zeroin2(
            ax,
            bx,
            fa,
            fb,
            zeroin_call,
            &mut ctx as *mut _ as *mut core::ffi::c_void,
            &mut tol,
            &mut maxit,
        );
        let froot = zeroin_call(root, &mut ctx as *mut _ as *mut core::ffi::c_void);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(root));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(froot));
        crate::mainutils::essentials::set_string_names(
            result,
            &["root".to_string(), "f.root".to_string()],
        );
        result
    }
}

/// GNU `optimize(f, interval)` golden-section min.
pub unsafe fn do_optimize(_call: crate::sexp::ffi::SEXP, _op: crate::sexp::ffi::SEXP, args: crate::sexp::ffi::SEXP, rho: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, INTEGER, REAL, SET_VECTOR_ELT, TYPEOF};
        use crate::sexp::constructors::{Rf_ScalarReal, Rf_allocVector3};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::protect::protect;
        let fun = CAR(args);
        let interval = CAR(CDR(args));
        let mut lo = if TYPEOF(interval) == SEXPTYPE::REALSXP {
            *REAL(interval)
        } else {
            *INTEGER(interval) as f64
        };
        let mut hi = if TYPEOF(interval) == SEXPTYPE::REALSXP {
            *REAL(interval).add(1)
        } else {
            *INTEGER(interval).add(1) as f64
        };
        let mut ctx = ZeroinCtx { fun, rho };
        let info = &mut ctx as *mut _ as *mut core::ffi::c_void;
        let gr = (5.0f64.sqrt() - 1.0) / 2.0;
        let mut x1 = hi - gr * (hi - lo);
        let mut x2 = lo + gr * (hi - lo);
        let mut f1 = zeroin_call(x1, info);
        let mut f2 = zeroin_call(x2, info);
        for _ in 0..80 {
            if f1 < f2 {
                hi = x2;
                x2 = x1;
                f2 = f1;
                x1 = hi - gr * (hi - lo);
                f1 = zeroin_call(x1, info);
            } else {
                lo = x1;
                x1 = x2;
                f1 = f2;
                x2 = lo + gr * (hi - lo);
                f2 = zeroin_call(x2, info);
            }
        }
        let xmin = 0.5 * (lo + hi);
        let fmin = zeroin_call(xmin, info);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(xmin));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(fmin));
        crate::mainutils::essentials::set_string_names(
            result,
            &["minimum".to_string(), "objective".to_string()],
        );
        result
    }
}

/// GNU `nlm(f, p)` — 1-d Newton with finite-difference Hessian.
pub unsafe fn do_nlm(
    _call: crate::sexp::ffi::SEXP,
    _op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    rho: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, INTEGER, REAL, SET_VECTOR_ELT, TYPEOF, XLENGTH};
        use crate::sexp::constructors::{Rf_ScalarReal, Rf_allocVector3};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::protect::protect;
        let fun = CAR(args);
        let p = CAR(CDR(args));
        if XLENGTH(p) != 1 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "nlm currently supports one-dimensional p only",
            );
        }
        let mut x = if TYPEOF(p) == SEXPTYPE::REALSXP {
            *REAL(p)
        } else {
            *INTEGER(p) as f64
        };
        let mut ctx = ZeroinCtx { fun, rho };
        let info = &mut ctx as *mut _ as *mut core::ffi::c_void;
        for _ in 0..40 {
            let f0 = zeroin_call(x, info);
            let eps = 1e-6 * (x.abs() + 1.0);
            let fp = zeroin_call(x + eps, info);
            let fm = zeroin_call(x - eps, info);
            let g = (fp - fm) / (2.0 * eps);
            let h = (fp - 2.0 * f0 + fm) / (eps * eps);
            if !g.is_finite() {
                break;
            }
            let step = if h.abs() > 1e-12 {
                g / h
            } else {
                0.1 * g
            };
            x -= step;
            if step.abs() < 1e-12 {
                break;
            }
        }
        let fmin = zeroin_call(x, info);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(fmin));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(x));
        crate::mainutils::essentials::set_string_names(
            result,
            &["minimum".to_string(), "estimate".to_string()],
        );
        result
    }
}


/// GNU `nlminb(start, objective)` 1-d via nlm.
pub unsafe fn do_nlminb(
    call: crate::sexp::ffi::SEXP,
    op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    rho: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, SET_VECTOR_ELT, VECTOR_ELT};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_cons};
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::protect;
        let start = CAR(args);
        let fun = CAR(CDR(args));
        let nlm_args = Rf_cons(fun, Rf_cons(start, R_NilValue()));
        let _na = protect(nlm_args);
        let nlm = do_nlm(call, op, nlm_args, rho);
        let _n = protect(nlm);
        let result = Rf_allocVector3(crate::sexp::ffi::SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, VECTOR_ELT(nlm, 1));
        SET_VECTOR_ELT(result, 1, VECTOR_ELT(nlm, 0));
        crate::mainutils::essentials::set_string_names(
            result,
            &["par".to_string(), "objective".to_string()],
        );
        result
    }
}

struct ZeroinCall {
    f: crate::sexp::ffi::SEXP,
    env: crate::sexp::ffi::SEXP,
}

unsafe extern "C" fn zeroin_r_fn(x: f64, info: *mut c_void) -> f64 {
    unsafe {
        let call = &*(info as *const ZeroinCall);
        let arg = crate::sexp::constructors::Rf_ScalarReal(x);
        let _a = crate::sexp::protect::protect(arg);
        let expr = crate::sexp::constructors::Rf_lang2(call.f, arg);
        let _e = crate::sexp::protect::protect(expr);
        let result = crate::eval::eval::Rf_eval(expr, call.env);
        finite_uniroot(crate::mainutils::coerce::asReal(result))
    }
}

/// `.External2(C_zeroin2, f, lower, upper, f.lower, f.upper, tol, maxiter)`.
pub unsafe fn zeroin2(
    _call: crate::sexp::ffi::SEXP,
    _op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    env: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    use crate::sexp::accessors::{CAR, CDR, REAL};
    use crate::sexp::constructors::Rf_allocVector;
    use crate::sexp::ffi::SEXPTYPE;
    unsafe {
        let mut a = CDR(args);
        let f = CAR(a);
        a = CDR(a);
        let lower = crate::mainutils::coerce::asReal(CAR(a));
        a = CDR(a);
        let upper = crate::mainutils::coerce::asReal(CAR(a));
        a = CDR(a);
        let fa = crate::mainutils::coerce::asReal(CAR(a));
        a = CDR(a);
        let fb = crate::mainutils::coerce::asReal(CAR(a));
        a = CDR(a);
        let mut tol = crate::mainutils::coerce::asReal(CAR(a));
        a = CDR(a);
        let mut maxit = crate::mainutils::coerce::asInteger(CAR(a));
        let info = ZeroinCall { f, env };
        let root = R_zeroin2(
            lower,
            upper,
            fa,
            fb,
            zeroin_r_fn,
            &info as *const ZeroinCall as *mut c_void,
            &mut tol,
            &mut maxit,
        );
        flush_uniroot_warnings();
        let out = Rf_allocVector(SEXPTYPE::REALSXP, 3);
        *REAL(out) = root;
        *REAL(out).add(1) = maxit as f64;
        *REAL(out).add(2) = tol;
        out
    }
}

fn brent_fmin(ax: f64, bx: f64, info: *mut core::ffi::c_void, tol: f64) -> f64 {
    let c = (3.0 - 5.0_f64.sqrt()) * 0.5;
    let mut eps = f64::EPSILON.sqrt();
    let mut a = ax;
    let mut b = bx;
    let mut v = a + c * (b - a);
    let mut w = v;
    let mut x = v;
    let mut d: f64 = 0.0;
    let mut e: f64 = 0.0;
    let eval = |z: f64| {
        let y = unsafe { zeroin_call(z, info) };
        if y.is_finite() { y } else if y == f64::NEG_INFINITY { f64::MIN } else { f64::MAX }
    };
    let mut fx = eval(x);
    let mut fv = fx;
    let mut fw = fx;
    let tol3 = tol / 3.0;
    loop {
        let xm = (a + b) * 0.5;
        let tol1 = eps * x.abs() + tol3;
        let t2 = tol1 * 2.0;
        if (x - xm).abs() <= t2 - (b - a) * 0.5 {
            break;
        }
        let mut p = 0.0;
        let mut q = 0.0;
        let mut r = 0.0;
        if e.abs() > tol1 {
            r = (x - w) * (fx - fv);
            q = (x - v) * (fx - fw);
            p = (x - v) * q - (x - w) * r;
            q = (q - r) * 2.0;
            if q > 0.0 { p = -p; } else { q = -q; }
            r = e;
            e = d;
        }
        if p.abs() >= (q * 0.5 * r).abs() || p <= q * (a - x) || p >= q * (b - x) {
            e = if x < xm { b - x } else { a - x };
            d = c * e;
        } else {
            d = p / q;
            let u = x + d;
            if u - a < t2 || b - u < t2 {
                d = if x >= xm { -tol1 } else { tol1 };
            }
        }
        let u = if d.abs() >= tol1 { x + d } else if d > 0.0 { x + tol1 } else { x - tol1 };
        let fu = eval(u);
        if fu <= fx {
            if u < x { b = x; } else { a = x; }
            v = w; w = x; x = u;
            fv = fw; fw = fx; fx = fu;
        } else {
            if u < x { a = u; } else { b = u; }
            if fu <= fw || w == x {
                v = w; fv = fw;
                w = u; fw = fu;
            } else if fu <= fv || v == x || v == w {
                v = u; fv = fu;
            }
        }
    }
    let _ = eps;
    x
}


/// `.External2(C_do_fmin, f, lower, upper, tol)` — scalar minimizer.
pub unsafe fn do_fmin(
    _call: crate::sexp::ffi::SEXP,
    _op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    rho: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, REAL};
        use crate::sexp::constructors::Rf_allocVector;
        use crate::sexp::ffi::SEXPTYPE;
        let mut a = CDR(args);
        let fun = CAR(a);
        a = CDR(a);
        let xmin = crate::mainutils::coerce::asReal(CAR(a));
        a = CDR(a);
        let xmax = crate::mainutils::coerce::asReal(CAR(a));
        a = CDR(a);
        let tol = crate::mainutils::coerce::asReal(CAR(a));
        let mut ctx = ZeroinCtx { fun, rho };
        let info = &mut ctx as *mut _ as *mut core::ffi::c_void;
        let x = brent_fmin(xmin, xmax, info, tol);
        let out = Rf_allocVector(SEXPTYPE::REALSXP, 1);
        *REAL(out) = x;
        out
    }
}




