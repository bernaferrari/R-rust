//! Native `nlminb` entry points (`C_port_ivset`, `C_port_nlminb`) from
//! r-source/src/library/stats/src/port.c.
//!
//! GNU drives the PORT Fortran library (`divset`/`drmnf`) through the `iv`/`v`
//! working vectors. The Rust port keeps the observable R contract — objective
//! and gradient callbacks evaluated in `rho` against `.par`, bounds, and the
//! `iv`/`v` result slots read by `nlminb()` — and runs the optimizer on the
//! bundled L-BFGS-B translation instead of the unported Fortran kernel.

use std::os::raw::{c_char, c_double, c_int};

use crate::appl::lbfgsb::lbfgsb;
use crate::sexp::accessors::{INTEGER, LENGTH, REAL, Rf_isNull, TYPEOF};
use crate::sexp::constructors::Rf_allocVector3;
use crate::sexp::envir::{R_findVar, defineVar};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

fn error(msg: &str) -> ! {
    crate::mainutils::errors::errorcall_str(
        unsafe { crate::mainutils::errors::R_getCurrentCall() },
        msg,
    )
}

fn is_null(x: SEXP) -> bool {
    unsafe { Rf_isNull(x) != 0 }
}

// ---------------------------------------------------------------------------
// port_ivset — initialize the iv/v working vectors
// ---------------------------------------------------------------------------

/// `C_port_ivset(kind, iv, v)`: GNU calls `Rf_divset`; the port marks the
/// vectors initialized (`iv[0] = 1`) since the R level only relies on the
/// values written back by `port_nlminb`.
pub unsafe fn port_ivset(kind: SEXP, iv: SEXP, v: SEXP) -> SEXP {
    unsafe {
        let _kind = crate::mainutils::coerce::asInteger(kind);
        if TYPEOF(iv) != SEXPTYPE::INTSXP || LENGTH(iv) < 1 {
            error("'iv' must be a nonempty integer vector");
        }
        if TYPEOF(v) != SEXPTYPE::REALSXP || LENGTH(v) < 1 {
            error("'v' must be a nonempty numeric vector");
        }
        *INTEGER(iv) = 1;
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// port_nlminb — run the minimization
// ---------------------------------------------------------------------------

struct PortCall {
    expr: SEXP,
    rho: SEXP,
    n: usize,
    count: usize,
}

/// Publish `x` as `.par` in `rho` (GNU duplicates before each publication so
/// callbacks cannot alias the working vector) and evaluate `expr`.
unsafe fn port_eval(call: &mut PortCall, x: &[c_double], sym: SEXP) -> c_double {
    unsafe {
        let par = Rf_allocVector3(SEXPTYPE::REALSXP, call.n as i64);
        let _p = protect(par);
        for i in 0..call.n {
            *REAL(par).add(i) = x[i];
        }
        defineVar(sym, par, call.rho);
        let value = crate::eval::eval::Rf_eval(call.expr, call.rho);
        call.count += 1;
        let f = crate::mainutils::coerce::asReal(value);
        if f.is_nan() {
            f64::INFINITY
        } else {
            f
        }
    }
}

/// `C_port_nlminb(fn, gr, hs, rho, lower, upper, d, iv, v)`.
#[allow(clippy::too_many_arguments)]
pub unsafe fn port_nlminb(
    fn_expr: SEXP,
    gr_expr: SEXP,
    hs_expr: SEXP,
    rho: SEXP,
    lowerb: SEXP,
    upperb: SEXP,
    d: SEXP,
    iv: SEXP,
    v: SEXP,
) -> SEXP {
    unsafe {
        if is_null(rho) {
            error("use of NULL environment is defunct");
        }
        let n = LENGTH(d) as usize;
        if TYPEOF(d) != SEXPTYPE::REALSXP || n < 1 {
            error("'d' must be a nonempty numeric (double) vector");
        }
        if !is_null(hs_expr) && is_null(gr_expr) {
            error("When Hessian defined must also have gradient defined");
        }
        let sym = Rf_install(b".par\0".as_ptr() as *const c_char);
        let xpt = R_findVar(sym, rho);
        if xpt.is_null() || xpt == R_UnboundValue() {
            error("environment 'rho' must contain a numeric (double) vector '.par'");
        }
        if TYPEOF(xpt) != SEXPTYPE::REALSXP || LENGTH(xpt) as usize != n {
            error(&format!(
                "environment 'rho' must contain a numeric (double) vector '.par' of length {n}"
            ));
        }
        let mut x = vec![0.0 as c_double; n];
        for i in 0..n {
            x[i] = *REAL(xpt).add(i);
        }

        let bounded = LENGTH(lowerb) as usize == n && LENGTH(upperb) as usize == n;
        let mut lower = vec![f64::NEG_INFINITY; n];
        let mut upper = vec![f64::INFINITY; n];
        let mut nbd = vec![0 as c_int; n];
        if bounded {
            if TYPEOF(lowerb) != SEXPTYPE::REALSXP || TYPEOF(upperb) != SEXPTYPE::REALSXP {
                error("'lower' and 'upper' must be numeric (double) vectors");
            }
            for i in 0..n {
                lower[i] = *REAL(lowerb).add(i);
                upper[i] = *REAL(upperb).add(i);
                nbd[i] = match (lower[i].is_finite(), upper[i].is_finite()) {
                    (false, false) => 0,
                    (false, true) => 3,
                    (true, false) => 1,
                    (true, true) => 2,
                };
            }
        }

        // PORT control slots filled by nlminb() (iv[17]=eval.max, iv[18]=
        // iter.max, iv[19]=trace).
        let eval_max = {
            let raw = *INTEGER(iv).add(17);
            if raw > 0 { raw } else { 200 }
        };
        let iter_max = {
            let raw = *INTEGER(iv).add(18);
            if raw > 0 { raw } else { 150 }
        };
        let trace = *INTEGER(iv).add(19);

        let mut call = PortCall {
            expr: fn_expr,
            rho,
            n,
            count: 0,
        };
        let gradient = |call: &mut PortCall, x: &[c_double], g: &mut [c_double], sym: SEXP| {
            if !is_null(gr_expr) {
                let par = Rf_allocVector3(SEXPTYPE::REALSXP, call.n as i64);
                let _p = protect(par);
                for i in 0..call.n {
                    *REAL(par).add(i) = x[i];
                }
                defineVar(sym, par, call.rho);
                let value = crate::eval::eval::Rf_eval(gr_expr, call.rho);
                for i in 0..call.n {
                    g[i] = *REAL(value).add(i);
                }
            } else {
                // Forward differences with PORT-style scaled steps.
                for i in 0..call.n {
                    let h = 1e-5 * (x[i].abs() + 1e-5);
                    let mut xp = x.to_vec();
                    xp[i] += h;
                    let fp = port_eval(call, &xp, sym);
                    let f0 = port_eval(call, x, sym);
                    g[i] = (fp - f0) / h;
                }
            }
        };

        let lmm = 10 as c_int;
        let mut f = 0.0 as c_double;
        let mut g = vec![0.0 as c_double; n];
        let wa_len = (2 * lmm * n as c_int + 5 * n as c_int + 11 * lmm * lmm + 8 * lmm + 1) as usize;
        let mut wa = vec![0.0 as c_double; wa_len];
        let mut iwa = vec![0 as c_int; 3 * n];
        let mut task: [c_char; 60] = [0; 60];
        for (slot, byte) in b"START\0".iter().enumerate() {
            task[slot] = *byte as c_char;
        }
        let mut pgtol = 1e-8 as c_double;
        let factr = 1e7 as c_double;
        let mut isave: [c_int; 44] = [0; 44];
        let mut converged = false;
        // lbfgsb only prints when iprint >= 0; PORT/optim default to silent.
        let iprint = if trace != 0 { trace } else { -1 };
        loop {
            lbfgsb(
                n as c_int,
                lmm,
                x.as_mut_ptr(),
                lower.as_ptr() as *mut c_double,
                upper.as_ptr() as *mut c_double,
                nbd.as_mut_ptr(),
                &mut f,
                g.as_mut_ptr(),
                factr,
                &mut pgtol,
                wa.as_mut_ptr(),
                iwa.as_mut_ptr(),
                task.as_mut_ptr(),
                iprint,
                isave.as_mut_ptr(),
            );
            let task_len = task.iter().position(|&c| c == 0).unwrap_or(60);
            let task_bytes: &[u8] =
                std::slice::from_raw_parts(task.as_ptr() as *const u8, task_len);
            if task_bytes.starts_with(b"CONVERGENCE") {
                converged = true;
                break;
            }
            if task_bytes.starts_with(b"ERROR") {
                break;
            }
            if call.count as c_int >= eval_max || isave[29] >= iter_max {
                break;
            }
            f = port_eval(&mut call, &x, sym);
            gradient(&mut call, &x, &mut g, sym);
        }

        // Publish the final iterate as '.par'.
        let par = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _p = protect(par);
        for i in 0..n {
            *REAL(par).add(i) = x[i];
        }
        defineVar(sym, par, rho);

        // Report through the iv/v slots read by nlminb().
        *INTEGER(iv).add(1) = if converged { 3 } else { 9 };
        *INTEGER(iv).add(31) = isave[29];
        *INTEGER(iv).add(6) = call.count as c_int;
        *INTEGER(iv).add(30) = call.count as c_int;
        if LENGTH(v) as usize > 10 {
            *REAL(v).add(10) = f;
        }
        R_NilValue()
    }
}
