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
        *REAL(vr)
    }
}

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

/// GNU `nlm(f, p)` for one-dimensional p via optimize.
pub unsafe fn do_nlm(
    call: crate::sexp::ffi::SEXP,
    op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    rho: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, INTEGER, REAL, SET_VECTOR_ELT, TYPEOF, VECTOR_ELT, XLENGTH};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_cons};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::protect;
        let fun = CAR(args);
        let p = CAR(CDR(args));
        if XLENGTH(p) != 1 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "nlm currently supports one-dimensional p only",
            );
        }
        let p0 = if TYPEOF(p) == SEXPTYPE::REALSXP {
            *REAL(p)
        } else {
            *INTEGER(p) as f64
        };
        let span = 10.0 * p0.abs() + 1.0;
        let interval = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _iv = protect(interval);
        *REAL(interval) = p0 - span;
        *REAL(interval).add(1) = p0 + span;
        let opt_args = Rf_cons(fun, Rf_cons(interval, R_NilValue()));
        let _oa = protect(opt_args);
        let opt = do_optimize(call, op, opt_args, rho);
        let _o = protect(opt);
        let xmin = VECTOR_ELT(opt, 0);
        let fmin = VECTOR_ELT(opt, 1);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, fmin);
        SET_VECTOR_ELT(result, 1, xmin);
        crate::mainutils::essentials::set_string_names(
            result,
            &["minimum".to_string(), "estimate".to_string()],
        );
        result
    }
}



