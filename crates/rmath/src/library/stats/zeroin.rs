//! Brent's root-finding method (zeroin).
//! Port of r-source/src/library/stats/src/zeroin.c

use core::ffi::{c_double, c_int, c_void};

/// Function pointer type for the objective function passed to R_zeroin2.
pub type R_zeroin2_fn = unsafe extern "C-unwind" fn(f64, *mut c_void) -> f64;

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

#[derive(Clone, Copy)]
enum ScalarCallbackKind {
    Optimize,
    Zeroin,
}

struct ZeroinCtx<'session> {
    fun: crate::sexp::object::Sexp<'session>,
    rho: crate::sexp::object::Sexp<'session>,
    owner: crate::sexp::owner::StoredOwner<'session>,
    kind: ScalarCallbackKind,
}

impl<'session> ZeroinCtx<'session> {
    unsafe fn new(
        fun: crate::sexp::ffi::SEXP,
        rho: crate::sexp::ffi::SEXP,
        kind: ScalarCallbackKind,
    ) -> Self {
        let owner = unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        Self {
            fun: owner
                .sexp(fun)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string())),
            rho: owner
                .sexp(rho)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string())),
            owner: crate::sexp::owner::StoredOwner::from_token(owner),
            kind,
        }
    }

    fn evaluate(&self, x: f64) -> f64 {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.owner.with_projection(|pointer| {
                // The stored owner retains the exact originating interpreter during
                // evaluation, ALTREP element access, and warning handlers.
                let owner = unsafe { crate::sexp::owner::OwnerToken::from_raw(pointer) };
                owner.require_active()?;
                let xv = unsafe { crate::sexp::constructors::Rf_ScalarReal(x) };
                owner.require_active()?;
                let _xv = unsafe { crate::sexp::protect::protect(xv) };
                let call = unsafe { crate::sexp::constructors::Rf_lang2(self.fun.as_raw(), xv) };
                owner.require_active()?;
                let _call = unsafe { crate::sexp::protect::protect(call) };
                let raw = unsafe { crate::eval::eval::Rf_eval(call, self.rho.as_raw()) };
                owner.require_active()?;
                let value = owner.sexp(raw)?;
                let ty = value.typeof_();
                if value.len() != 1
                    || !matches!(
                        ty,
                        crate::sexp::ffi::SEXPTYPE::INTSXP | crate::sexp::ffi::SEXPTYPE::REALSXP
                    )
                {
                    let message = match self.kind {
                        ScalarCallbackKind::Optimize => "invalid function value in 'optimize'",
                        ScalarCallbackKind::Zeroin => "invalid function value in 'zeroin'",
                    };
                    crate::mainutils::errors::errorcall_str(
                        unsafe { crate::mainutils::errors::R_getCurrentCall() },
                        message,
                    );
                }
                let result = if ty == crate::sexp::ffi::SEXPTYPE::INTSXP {
                    let number = value.try_integer_elt(0);
                    owner.require_active()?;
                    let number = number?;
                    if number == crate::sexp::ffi::NA_INTEGER {
                        unsafe {
                            crate::mainutils::errors::Rf_warning1(
                                c"NA replaced by maximum positive value".as_ptr(),
                            )
                        };
                        owner.require_active()?;
                        f64::MAX
                    } else {
                        number as f64
                    }
                } else {
                    let number = value.try_real_elt(0);
                    owner.require_active()?;
                    let number = number?;
                    let number = finite_uniroot(number);
                    owner.require_active()?;
                    number
                };
                Ok(result)
            })
        }));
        self.owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        match outcome {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => crate::sexp::context::r_error(error.to_string()),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

unsafe extern "C-unwind" fn zeroin_call(x: f64, info: *mut core::ffi::c_void) -> f64 {
    let ctx = unsafe { &*(info as *const ZeroinCtx<'_>) };
    ctx.evaluate(x)
}
fn finite_uniroot(v: f64) -> f64 {
    let (message, replacement) = if v.is_nan() {
        (c"NA/NaN replaced by maximum positive value", f64::MAX)
    } else if v == f64::NEG_INFINITY {
        (c"-Inf replaced by maximally negative value", f64::MIN)
    } else if v.is_infinite() {
        (c"Inf replaced by maximum positive value", f64::MAX)
    } else {
        return v;
    };
    // GNU signals at the evaluation that produced the value.  In particular,
    // handlers may run nested optimizers or turn this warning into an error.
    unsafe { crate::mainutils::errors::Rf_warning1(message.as_ptr()) };
    replacement
}

/// GNU `uniroot(f, interval)`.
pub unsafe fn do_uniroot(
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
        let mut ctx = ZeroinCtx::new(fun, rho, ScalarCallbackKind::Zeroin);
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
pub unsafe fn do_optimize(
    _call: crate::sexp::ffi::SEXP,
    _op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    rho: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
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
        let mut ctx = ZeroinCtx::new(fun, rho, ScalarCallbackKind::Optimize);
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

// ---------------------------------------------------------------------------
// nlm() — faithful port of r-source/src/library/stats/src/optimize.c's nlm()
// (the Schnabel/Dennis `uncmin` translation in src/appl/uncmin.c, optif9).
//
// .External2(C_nlm, function(x) f(x, ...), p, hessian, typsize, fscale,
//            msg, ndigit, gradtol, stepmax, steptol, iterlim)
// ---------------------------------------------------------------------------

/// Default size of the table of stored function values (FT_SIZE in optimize.c).
const NLM_FT_SIZE: usize = 5;

struct NlmFtEntry {
    fval: f64,
    x: Vec<f64>,
    grad: Option<Vec<f64>>,
    hess: Option<Vec<f64>>,
}

/// `function_info` from optimize.c: the objective callback plus the cache of
/// computed function values shared with the analytic gradient/hessian
/// handlers.
struct NlmState {
    r_fcall: crate::sexp::ffi::SEXP,
    r_env: crate::sexp::ffi::SEXP,
    have_gradient: bool,
    have_hessian: bool,
    ft_size: usize,
    ft_last: isize,
    ftable: Vec<NlmFtEntry>,
}

/// FT_init: initialize the table of computed function values.
unsafe fn nlm_ft_init(n: usize, ft_size: usize, state: &mut NlmState) {
    let mut ftable = Vec::with_capacity(ft_size);
    for _ in 0..ft_size {
        // initialize to unlikely parameter values
        ftable.push(NlmFtEntry {
            fval: 0.0,
            x: vec![f64::MAX; n],
            grad: if state.have_gradient {
                Some(vec![0.0; n])
            } else {
                None
            },
            hess: if state.have_gradient && state.have_hessian {
                Some(vec![0.0; n * n])
            } else {
                None
            },
        });
    }
    state.ftable = ftable;
    state.ft_size = ft_size;
    state.ft_last = -1;
}

/// FT_store: store an entry in the table of computed function values.
unsafe fn nlm_ft_store(
    n: usize,
    f: f64,
    x: &[f64],
    grad: Option<&[f64]>,
    hess: Option<&[f64]>,
    state: &mut NlmState,
) {
    state.ft_last += 1;
    let ind = (state.ft_last % state.ft_size as isize) as usize;
    let e = &mut state.ftable[ind];
    e.fval = f;
    e.x[..n].copy_from_slice(&x[..n]);
    if let Some(g) = grad {
        if let Some(eg) = &mut e.grad {
            eg[..n].copy_from_slice(&g[..n]);
            if let Some(h) = hess {
                if let Some(eh) = &mut e.hess {
                    eh[..n * n].copy_from_slice(&h[..n * n]);
                }
            }
        }
    }
}

/// FT_lookup: check for stored values; returns the table index or None.
unsafe fn nlm_ft_lookup(n: usize, x: &[f64], state: &NlmState) -> Option<usize> {
    for i in 0..state.ft_size as isize {
        let mut ind = (state.ft_last - i) % state.ft_size as isize;
        /* why can't they define modulus correctly */
        if ind < 0 {
            ind += state.ft_size as isize;
        }
        if state.ftable[ind as usize].x[..n] == x[..n] {
            return Some(ind as usize);
        }
    }
    None
}

/// Read a `gradient`/`hessian` attribute as doubles (GNU does
/// `REAL(coerceVector(getAttrib(...), REALSXP))`).
unsafe fn nlm_attr_real(v: crate::sexp::ffi::SEXP, len: usize) -> Vec<f64> {
    use crate::sexp::accessors::{REAL, TYPEOF, XLENGTH};
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::R_NilValue;
    unsafe {
        let mut out = vec![0.0f64; len];
        if v.is_null() || v == R_NilValue() {
            return out;
        }
        let vr = if TYPEOF(v) == SEXPTYPE::REALSXP {
            v
        } else {
            crate::main::coerce::coerceVector(v, SEXPTYPE::REALSXP.as_c_int())
        };
        if vr.is_null() || vr == R_NilValue() || XLENGTH(vr) < len as i64 {
            return out;
        }
        for i in 0..len {
            out[i] = *REAL(vr).add(i);
        }
        out
    }
}

/// The optimizer's objective handler (`fcn` in optimize.c).
unsafe extern "C" fn nlm_fcn(
    n: core::ffi::c_int,
    x: *mut core::ffi::c_double,
    f: *mut core::ffi::c_double,
    arg_state: *mut core::ffi::c_void,
) {
    unsafe {
        use crate::sexp::accessors::{CDR, INTEGER, REAL, SETCAR, TYPEOF, XLENGTH};
        use crate::sexp::constructors::{Rf_allocVector3, Rf_lang2};
        use crate::sexp::ffi::{ISNAN, NA_INTEGER, SEXPTYPE};
        use crate::sexp::protect::protect;
        let n = n as usize;
        let state = &mut *(arg_state as *mut NlmState);
        let xs = core::slice::from_raw_parts(x, n);
        if let Some(ind) = nlm_ft_lookup(n, xs, state) {
            *f = state.ftable[ind].fval;
            return;
        }
        /* calculate for a new value of x */
        let s = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _ps = protect(s);
        for i in 0..n {
            if !xs[i].is_finite() {
                crate::mainutils::errors::errorcall_str(
                    crate::mainutils::errors::R_getCurrentCall(),
                    "non-finite value supplied by 'nlm'",
                );
            }
            *REAL(s).add(i) = xs[i];
        }
        SETCAR(CDR(state.r_fcall), s);
        let value = crate::eval::eval::Rf_eval(state.r_fcall, state.r_env);
        let _pv = protect(value);
        match TYPEOF(value) {
            t if t == SEXPTYPE::INTSXP => {
                if XLENGTH(value) != 1 {
                    nlm_bad_value();
                }
                let iv = *INTEGER(value);
                if iv == NA_INTEGER {
                    crate::mainutils::errors::Rf_warning1(
                        c"NA replaced by maximum positive value".as_ptr(),
                    );
                    *f = f64::MAX;
                } else {
                    *f = iv as f64;
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                if XLENGTH(value) != 1 {
                    nlm_bad_value();
                }
                let rv = *REAL(value);
                if !rv.is_finite() {
                    if rv == f64::NEG_INFINITY {
                        // keep sign for root finding !
                        crate::mainutils::errors::Rf_warning1(
                            c"-Inf replaced by maximally negative value".as_ptr(),
                        );
                        *f = -f64::MAX;
                    } else {
                        crate::mainutils::errors::Rf_warning1(if ISNAN(rv) {
                            c"NA/NaN replaced by maximum positive value".as_ptr()
                        } else {
                            c"Inf replaced by maximum positive value".as_ptr()
                        });
                        *f = f64::MAX;
                    }
                } else {
                    *f = rv;
                }
            }
            _ => nlm_bad_value(),
        }
        let mut grad: Option<Vec<f64>> = None;
        let mut hess: Option<Vec<f64>> = None;
        if state.have_gradient {
            let gsym = crate::sexp::symbol::Rf_install(b"gradient\0".as_ptr() as *const _);
            grad = Some(nlm_attr_real(
                crate::sexp::attrib_core::getAttrib(value, gsym),
                n,
            ));
            if state.have_hessian {
                let hsym = crate::sexp::symbol::Rf_install(b"hessian\0".as_ptr() as *const _);
                hess = Some(nlm_attr_real(
                    crate::sexp::attrib_core::getAttrib(value, hsym),
                    n * n,
                ));
            }
        }
        nlm_ft_store(n, *f, xs, grad.as_deref(), hess.as_deref(), state);
    }
}

unsafe fn nlm_bad_value() -> ! {
    unsafe {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "invalid function value in 'nlm' optimizer",
        )
    }
}

/// Handler for the analytic gradient (`Cd1fcn` in optimize.c).
unsafe extern "C" fn nlm_d1fcn(
    n: core::ffi::c_int,
    x: *mut core::ffi::c_double,
    g: *mut core::ffi::c_double,
    arg_state: *mut core::ffi::c_void,
) {
    unsafe {
        let n = n as usize;
        let state = &mut *(arg_state as *mut NlmState);
        let xs = core::slice::from_raw_parts(x, n);
        let mut ind = nlm_ft_lookup(n, xs, state);
        if ind.is_none() {
            /* shouldn't happen */
            let mut tmp = 0.0f64;
            nlm_fcn(n as core::ffi::c_int, x, &mut tmp, arg_state);
            ind = nlm_ft_lookup(n, xs, state);
        }
        let gs = core::slice::from_raw_parts_mut(g, n);
        match ind.and_then(|i| state.ftable[i].grad.as_ref()) {
            Some(v) => gs.copy_from_slice(v),
            None => crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "function value caching for optimization is seriously confused",
            ),
        }
    }
}

/// Handler for the analytic hessian (`Cd2fcn` in optimize.c).
unsafe extern "C" fn nlm_d2fcn(
    nr: core::ffi::c_int,
    n: core::ffi::c_int,
    x: *mut core::ffi::c_double,
    h: *mut core::ffi::c_double,
    arg_state: *mut core::ffi::c_void,
) {
    unsafe {
        let n = n as usize;
        let state = &mut *(arg_state as *mut NlmState);
        let xs = core::slice::from_raw_parts(x, n);
        let mut ind = nlm_ft_lookup(n, xs, state);
        if ind.is_none() {
            /* shouldn't happen */
            let mut tmp = 0.0f64;
            nlm_fcn(n as core::ffi::c_int, x, &mut tmp, arg_state);
            ind = nlm_ft_lookup(n, xs, state);
        }
        let hs = core::slice::from_raw_parts_mut(h, nr as usize * n);
        let he = match ind.and_then(|i| state.ftable[i].hess.as_ref()) {
            Some(v) => v,
            None => crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "function value caching for optimization is seriously confused",
            ),
        };
        for j in 0..n {
            /* fill in lower triangle only */
            let off = j * (n + 1);
            hs[off..off + (n - j)].copy_from_slice(&he[off..off + (n - j)]);
        }
    }
}

/// fixparam from optimize.c: check/extract a numeric parameter vector.
unsafe fn nlm_fixparam(p: crate::sexp::ffi::SEXP, n: &mut usize) -> Vec<f64> {
    use crate::sexp::accessors::{INTEGER, REAL, TYPEOF, XLENGTH};
    use crate::sexp::ffi::SEXPTYPE;
    unsafe {
        let bad = |msg: &str| -> ! {
            unsafe {
                crate::mainutils::errors::errorcall_str(
                    crate::mainutils::errors::R_getCurrentCall(),
                    msg,
                )
            }
        };
        let t = TYPEOF(p);
        if !(t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP || t == SEXPTYPE::CPLXSXP) {
            bad("numeric parameter expected");
        }
        let len = XLENGTH(p) as usize;
        if *n != 0 {
            if len != *n {
                bad("conflicting parameter lengths");
            }
        } else {
            if len == 0 {
                bad("invalid parameter length");
            }
            *n = len;
        }
        let mut x = vec![0.0f64; *n];
        match t {
            t if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP => {
                for i in 0..*n {
                    if *INTEGER(p).add(i) == crate::sexp::ffi::NA_INTEGER {
                        bad("missing value in parameter");
                    }
                    x[i] = *INTEGER(p).add(i) as f64;
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                for i in 0..*n {
                    if !(*REAL(p).add(i)).is_finite() {
                        bad("missing value in parameter");
                    }
                    x[i] = *REAL(p).add(i);
                }
            }
            _ => bad("invalid parameter type"),
        }
        x
    }
}

/// Fatal errors — we don't deliver an answer (opterror in optimize.c).
unsafe fn nlm_opterror(nerr: i32) -> ! {
    let msg = match nerr {
        -1 => "non-positive number of parameters in nlm",
        -2 => "nlm is inefficient for 1-d problems",
        -3 => "invalid gradient tolerance in nlm",
        -4 => "invalid iteration limit in nlm",
        -5 => "minimization function has no good digits in nlm",
        -6 => "no analytic gradient to check in nlm!",
        -7 => "no analytic Hessian to check in nlm!",
        -21 => "probable coding error in analytic gradient",
        -22 => "probable coding error in analytic Hessian",
        _ => "*** unknown error message (msg = %d) in nlm()\n*** should not happen!",
    };
    unsafe {
        crate::mainutils::errors::errorcall_str(crate::mainutils::errors::R_getCurrentCall(), msg)
    }
}

/// Warnings — we return a value, but print a warning (optcode in optimize.c).
unsafe fn nlm_optcode(code: i32) {
    let text = match code {
        1 => "Relative gradient close to zero.\nCurrent iterate is probably solution.\n",
        2 => "Successive iterates within tolerance.\nCurrent iterate is probably solution.\n",
        3 => {
            "Last global step failed to locate a point lower than x.\nEither x is an approximate local minimum of the function,\nthe function is too non-linear for this algorithm,\nor steptol is too large.\n"
        }
        4 => "Iteration limit exceeded.  Algorithm failed.\n",
        5 => {
            "Maximum step size exceeded 5 consecutive times.\nEither the function is unbounded below,\nbecomes asymptotic to a finite value\nfrom above in some direction,\nor stepmx is too small.\n"
        }
        _ => "",
    };
    crate::sexp::output::emit(text);
    crate::sexp::output::emit("\n");
}

/// GNU `nlm(f, p, ...)` — Dennis–Schnabel minimizer (statement-level port of
/// `nlm()` in r-source/src/library/stats/src/optimize.c driving `optif9`).
pub unsafe fn do_nlm(
    _call: crate::sexp::ffi::SEXP,
    _op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    rho: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe {
        use crate::appl::uncmin::{fdhess, optif9};
        use crate::sexp::accessors::{CAR, CDR, INTEGER, REAL, SET_VECTOR_ELT, TYPEOF, XLENGTH};
        use crate::sexp::constructors::{
            Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector3, Rf_lang2,
        };
        use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_IsNA, SEXPTYPE};
        use crate::sexp::protect::protect;
        crate::mainutils::print::PrintDefaults();

        /* the function to be minimized */
        let v = CAR(args);
        if !crate::mainutils::coerce::isFunction(v) {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "attempt to minimize non-function",
            );
        }
        let r_fcall = Rf_lang2(v, crate::sexp::globals::R_NilValue());
        let _pf = protect(r_fcall);
        let mut args = CDR(args);

        /* `p' : initial parameter value */
        let mut n: usize = 0;
        let mut x = nlm_fixparam(CAR(args), &mut n);
        args = CDR(args);

        /* `hessian' : H. required? */
        let mut want_hessian = crate::mainutils::coerce::asLogical(CAR(args));
        if want_hessian == NA_LOGICAL {
            want_hessian = 0;
        }
        args = CDR(args);

        /* `typsize' : typical size of parameter elements */
        let mut typsiz = nlm_fixparam(CAR(args), &mut n);
        args = CDR(args);

        /* `fscale' : expected function size */
        let mut fscale = crate::mainutils::coerce::asReal(CAR(args));
        if R_IsNA(fscale) {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid NA value in parameter",
            );
        }
        args = CDR(args);

        /* `msg' (bit pattern) */
        let mut msg = crate::mainutils::coerce::asInteger(CAR(args));
        let omsg = msg;
        if msg == NA_INTEGER {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid NA value in parameter",
            );
        }
        args = CDR(args);

        let mut ndigit = crate::mainutils::coerce::asInteger(CAR(args));
        if ndigit == NA_INTEGER {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid NA value in parameter",
            );
        }
        args = CDR(args);

        let gradtl = crate::mainutils::coerce::asReal(CAR(args));
        if R_IsNA(gradtl) {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid NA value in parameter",
            );
        }
        args = CDR(args);

        let mut stepmx = crate::mainutils::coerce::asReal(CAR(args));
        if R_IsNA(stepmx) {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid NA value in parameter",
            );
        }
        args = CDR(args);

        let steptol = crate::mainutils::coerce::asReal(CAR(args));
        if R_IsNA(steptol) {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid NA value in parameter",
            );
        }
        args = CDR(args);

        /* `iterlim' (def. 100) */
        let itnlim = crate::mainutils::coerce::asInteger(CAR(args));
        if itnlim == NA_INTEGER {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid NA value in parameter",
            );
        }

        /* force one evaluation to check for the gradient and hessian */
        let mut iagflg: i32 = 0; /* No analytic gradient */
        let mut iahflg: i32 = 0; /* No analytic hessian */
        let mut state = NlmState {
            r_fcall,
            r_env: rho,
            have_gradient: false,
            have_hessian: false,
            ft_size: 0,
            ft_last: -1,
            ftable: Vec::new(),
        };
        let gsym = crate::sexp::symbol::Rf_install(b"gradient\0".as_ptr() as *const _);
        let hsym = crate::sexp::symbol::Rf_install(b"hessian\0".as_ptr() as *const _);

        let xv = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _px = protect(xv);
        for i in 0..n {
            *REAL(xv).add(i) = x[i];
        }
        crate::sexp::accessors::SETCAR(crate::sexp::accessors::CDR(r_fcall), xv);
        let value = crate::eval::eval::Rf_eval(r_fcall, state.r_env);
        let _pval = protect(value);

        let ga = crate::sexp::attrib_core::getAttrib(value, gsym);
        if ga != crate::sexp::globals::R_NilValue() {
            let g_ok = XLENGTH(ga) == n as i64
                && (TYPEOF(ga) == SEXPTYPE::REALSXP || TYPEOF(ga) == SEXPTYPE::INTSXP);
            if g_ok {
                iagflg = 1;
                state.have_gradient = true;
                let ha = crate::sexp::attrib_core::getAttrib(value, hsym);
                if ha != crate::sexp::globals::R_NilValue() {
                    let h_ok = XLENGTH(ha) == (n * n) as i64
                        && (TYPEOF(ha) == SEXPTYPE::REALSXP || TYPEOF(ha) == SEXPTYPE::INTSXP);
                    if h_ok {
                        iahflg = 1;
                        state.have_hessian = true;
                    } else {
                        crate::mainutils::errors::Rf_warning1(
                            c"hessian supplied is of the wrong length or mode, so ignored".as_ptr(),
                        );
                    }
                }
            } else {
                crate::mainutils::errors::Rf_warning1(
                    c"gradient supplied is of the wrong length or mode, so ignored".as_ptr(),
                );
            }
        }
        if (msg / 4) % 2 != 0 && iahflg == 0 {
            /* skip check of analytic Hessian */
            msg -= 4;
        }
        if (msg / 2) % 2 != 0 && iagflg == 0 {
            /* skip check of analytic gradient */
            msg -= 2;
        }
        nlm_ft_init(n, NLM_FT_SIZE, &mut state);

        /* Plug in the call to the optimizer here */
        let method: i32 = 1; /* Line Search */
        let iexp: i32 = if iahflg != 0 { 0 } else { 1 }; /* calls expensive */
        let dlt: f64 = 1.0;

        let mut xpls = vec![0.0f64; n];
        let mut gpls = vec![0.0f64; n];
        let mut a = vec![0.0f64; n * n];
        let mut wrk = vec![0.0f64; 8 * n];
        let mut fpls: f64 = 0.0;
        let mut code: core::ffi::c_int = 0;
        let mut itncnt: core::ffi::c_int = 0;
        let state_ptr = &mut state as *mut NlmState as *mut core::ffi::c_void;

        optif9(
            n as core::ffi::c_int,
            n as core::ffi::c_int,
            x.as_mut_ptr(),
            nlm_fcn,
            nlm_d1fcn,
            nlm_d2fcn,
            state_ptr,
            typsiz.as_mut_ptr(),
            fscale,
            method,
            iexp,
            &mut msg,
            ndigit,
            itnlim,
            iagflg,
            iahflg,
            dlt,
            gradtl,
            stepmx,
            steptol,
            xpls.as_mut_ptr(),
            &mut fpls,
            gpls.as_mut_ptr(),
            &mut code,
            a.as_mut_ptr(),
            wrk.as_mut_ptr(),
            &mut itncnt,
        );

        if msg < 0 {
            nlm_opterror(msg);
        }
        if code != 0 && (omsg & 8) == 0 {
            nlm_optcode(code);
        }

        let mut slot = 0isize;
        let result = if want_hessian != 0 {
            fdhess(
                n as core::ffi::c_int,
                xpls.as_mut_ptr(),
                fpls,
                nlm_fcn,
                state_ptr,
                a.as_mut_ptr(),
                n as core::ffi::c_int,
                wrk.as_mut_ptr(),
                wrk.as_mut_ptr().add(n),
                ndigit,
                typsiz.as_ptr(),
            );
            for i in 0..n {
                for j in 0..i {
                    a[i + j * n] = a[j + i * n];
                }
            }
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 6);
            let _r = protect(result);
            let hess = Rf_allocVector3(SEXPTYPE::REALSXP, (n * n) as i64);
            let _h = protect(hess);
            for i in 0..n * n {
                *REAL(hess).add(i) = a[i];
            }
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            let _d = protect(dim);
            *INTEGER(dim) = n as i32;
            *INTEGER(dim).add(1) = n as i32;
            crate::sexp::attrib_core::setAttrib(
                hess,
                crate::sexp::symbol::Rf_install(b"dim\0".as_ptr() as *const _),
                dim,
            );
            SET_VECTOR_ELT(result, 3, hess);
            slot = 4;
            result
        } else {
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
            let _r = protect(result);
            slot = 3;
            result
        };
        let _r = protect(result);

        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(fpls));

        let est = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _e = protect(est);
        for i in 0..n {
            *REAL(est).add(i) = xpls[i];
        }
        SET_VECTOR_ELT(result, 1, est);

        let gr = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _g = protect(gr);
        for i in 0..n {
            *REAL(gr).add(i) = gpls[i];
        }
        SET_VECTOR_ELT(result, 2, gr);

        if want_hessian != 0 {
            // hessian was set at index 3 above (before minimum/estimate/
            // gradient were filled); GNU fills in order, we set it early
            // but the slot content is identical.
        }

        SET_VECTOR_ELT(result, slot as i64, Rf_ScalarInteger(code));
        SET_VECTOR_ELT(result, slot as i64 + 1, Rf_ScalarInteger(itncnt));

        let mut names = vec![
            "minimum".to_string(),
            "estimate".to_string(),
            "gradient".to_string(),
        ];
        if want_hessian != 0 {
            names.push("hessian".to_string());
        }
        names.push("code".to_string());
        names.push("iterations".to_string());
        crate::mainutils::essentials::set_string_names(result, &names);
        result
    }
}
pub unsafe extern "C-unwind" fn c_nlm(
    call: crate::sexp::ffi::SEXP,
    op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    rho: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { do_nlm(call, op, crate::sexp::accessors::CDR(args), rho) }
}

/// GNU `nlminb(start, objective)` 1-d via nlm (rport fallback; the real
/// nlminb dispatches to C_port_nlminb).
pub unsafe fn do_nlminb(
    call: crate::sexp::ffi::SEXP,
    op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    rho: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, REAL, SET_VECTOR_ELT, VECTOR_ELT, XLENGTH};
        use crate::sexp::constructors::{
            Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector3, Rf_cons,
        };
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::protect;
        let start = CAR(args);
        let fun = CAR(CDR(args));
        let n = XLENGTH(start).max(1) as usize;
        // typsize = rep(1, length(p)) — the R-level nlm() default
        let typsiz = Rf_allocVector3(crate::sexp::ffi::SEXPTYPE::REALSXP, n as i64);
        let _t = protect(typsiz);
        for i in 0..n {
            *REAL(typsiz).add(i) = 1.0;
        }
        // .External2 payload with nlm()'s R-level defaults:
        // hessian=FALSE, typsize, fscale=1, msg=9, ndigit=12, gradtol=1e-6,
        // stepmax=0 (auto), steptol=1e-6, iterlim=100
        let nlm_args = Rf_cons(
            fun,
            Rf_cons(
                start,
                Rf_cons(
                    Rf_ScalarInteger(0),
                    Rf_cons(
                        typsiz,
                        Rf_cons(
                            Rf_ScalarReal(1.0),
                            Rf_cons(
                                Rf_ScalarInteger(9),
                                Rf_cons(
                                    Rf_ScalarInteger(12),
                                    Rf_cons(
                                        Rf_ScalarReal(1e-6),
                                        Rf_cons(
                                            Rf_ScalarReal(0.0),
                                            Rf_cons(
                                                Rf_ScalarReal(1e-6),
                                                Rf_cons(Rf_ScalarInteger(100), R_NilValue()),
                                            ),
                                        ),
                                    ),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        );
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

unsafe extern "C-unwind" fn zeroin_r_fn(x: f64, info: *mut c_void) -> f64 {
    let ctx = unsafe { &*(info as *const ZeroinCtx<'_>) };
    ctx.evaluate(x)
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
        let info = ZeroinCtx::new(f, env, ScalarCallbackKind::Zeroin);
        let root = R_zeroin2(
            lower,
            upper,
            fa,
            fb,
            zeroin_r_fn,
            &info as *const ZeroinCtx<'_> as *mut c_void,
            &mut tol,
            &mut maxit,
        );
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
        if y.is_finite() {
            y
        } else if y == f64::NEG_INFINITY {
            f64::MIN
        } else {
            f64::MAX
        }
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
            if q > 0.0 {
                p = -p;
            } else {
                q = -q;
            }
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
        let u = if d.abs() >= tol1 {
            x + d
        } else if d > 0.0 {
            x + tol1
        } else {
            x - tol1
        };
        let fu = eval(u);
        if fu <= fx {
            if u < x {
                b = x;
            } else {
                a = x;
            }
            v = w;
            w = x;
            x = u;
            fv = fw;
            fw = fx;
            fx = fu;
        } else {
            if u < x {
                a = u;
            } else {
                b = u;
            }
            if fu <= fw || w == x {
                v = w;
                fv = fw;
                w = u;
                fw = fu;
            } else if fu <= fv || v == x || v == w {
                v = u;
                fv = fu;
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
        let mut ctx = ZeroinCtx::new(fun, rho, ScalarCallbackKind::Optimize);
        let info = &mut ctx as *mut _ as *mut core::ffi::c_void;
        let x = brent_fmin(xmin, xmax, info, tol);
        let out = Rf_allocVector(SEXPTYPE::REALSXP, 1);
        *REAL(out) = x;
        out
    }
}

#[cfg(test)]
#[path = "zeroin_scalar_tests.rs"]
mod zeroin_scalar_tests;
