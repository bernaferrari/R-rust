//! Native `nlminb` entry points (`C_port_ivset`, `C_port_nlminb`) from
//! r-source/src/library/stats/src/port.c.
//!
//! GNU drives the PORT Fortran library (`divset`/`drmnf`) through the `iv`/`v`
//! working vectors. The Rust port keeps the observable R contract — objective
//! and gradient callbacks evaluated in `rho` against `.par`, bounds, and the
//! `iv`/`v` result slots read by `nlminb()` — and runs the optimizer on the
//! bundled L-BFGS-B translation instead of the unported Fortran kernel.

use crate::sexp::accessors::{INTEGER_ELT, REAL_ELT, SET_INTEGER_ELT, SET_REAL_ELT};
use std::os::raw::{c_char, c_double, c_int};

use crate::appl::lbfgsb::{LbfgsbState, lbfgsb, workspace_lengths};
use crate::eval::attrib_core::setAttrib;
use crate::sexp::accessors::{INTEGER, LENGTH, REAL, Rf_isNull, TYPEOF};
use crate::sexp::attrib_core::{R_NamesSymbol, getAttrib};
use crate::sexp::constructors::Rf_allocVector3;
use crate::sexp::envir::{R_findVar, defineVar};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

fn error(msg: &str) -> ! {
    crate::mainutils::errors::errorcall_str(crate::mainutils::errors::R_getCurrentCall(), msg)
}

fn is_null(x: SEXP) -> bool {
    Rf_isNull(x) != 0
}

// R counts vector slots from one. These offsets match stats::nlminb.
const RETURN_CODE: usize = 0;
const FUNCTION_COUNT: usize = 5;
const EVAL_MAX: usize = 16;
const ITER_MAX: usize = 17;
const TRACE: usize = 18;
const GRADIENT_COUNT: usize = 29;
const ITERATIONS: usize = 30;
const OBJECTIVE_VALUE: usize = 9;
const WORKSPACE_LIMIT: &str = "nlminb workspace exceeds the session memory limit";

fn workspace_vec<T: Copy>(len: usize, value: T) -> Vec<T> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .unwrap_or_else(|_| error(WORKSPACE_LIMIT));
    values.resize(len, value);
    values
}

// ---------------------------------------------------------------------------
// port_ivset — initialize the iv/v working vectors
// ---------------------------------------------------------------------------

/// `C_port_ivset(kind, iv, v)`: GNU calls `Rf_divset`; the port marks the
/// vectors initialized (`iv[0] = 1`) since the R level only relies on the
/// values written back by `port_nlminb`.
pub fn port_ivset(kind: SEXP, iv: SEXP, v: SEXP) -> SEXP {
    let _kind = crate::mainutils::coerce::asInteger(kind);
    if TYPEOF(iv) != SEXPTYPE::INTSXP || LENGTH(iv) < 1 {
        error("'iv' must be a nonempty integer vector");
    }
    if TYPEOF(v) != SEXPTYPE::REALSXP || LENGTH(v) < 1 {
        error("'v' must be a nonempty numeric vector");
    }
    SET_INTEGER_ELT(iv, 0, 1);
    R_NilValue()
}

// ---------------------------------------------------------------------------
// port_nlminb — run the minimization
// ---------------------------------------------------------------------------

struct PortCall {
    expr: SEXP,
    rho: SEXP,
    names: SEXP,
    function_count: c_int,
    gradient_count: c_int,
}

impl PortCall {
    /// R callbacks see a fresh named vector and cannot alias the solver's input.
    fn publish(&self, x: &[c_double], sym: SEXP) {
        let par = Rf_allocVector3(SEXPTYPE::REALSXP, x.len() as i64);
        let _par = protect(par);
        crate::library::stats::checked::set_reals(par, x);
        if !is_null(self.names) {
            setAttrib(par, R_NamesSymbol(), self.names);
        }
        crate::library::stats::checked::define_var(sym, par, self.rho);
    }

    fn value(&mut self, x: &[c_double], sym: SEXP) -> c_double {
        self.publish(x, sym);
        let value = crate::eval::eval::eval_inner(self.expr, self.rho);
        let _value = protect(value);
        self.function_count = self.function_count.saturating_add(1);
        let f = crate::mainutils::coerce::asReal(value);
        if f.is_nan() { f64::INFINITY } else { f }
    }

    fn gradient(&mut self, expr: SEXP, x: &[c_double], g: &mut [c_double], sym: SEXP) {
        self.publish(x, sym);
        let result = crate::eval::eval::eval_inner(expr, self.rho);
        let _result = protect(result);
        let value = crate::mainutils::coerce::coerceVector(result, SEXPTYPE::REALSXP.as_c_int());
        let _value = protect(value);
        if LENGTH(value) as usize != g.len() {
            error(
                "gradient function must return a numeric vector of length equal to the length of 'start'",
            );
        }
        g.copy_from_slice(&crate::library::stats::checked::real_vec(value));
        if g.iter().any(|gradient| !gradient.is_finite()) {
            error("NA/NaN gradient evaluation");
        }
        self.gradient_count = self.gradient_count.saturating_add(1);
    }
}

/// `C_port_nlminb(fn, gr, hs, rho, lower, upper, d, iv, v)`.
#[allow(clippy::too_many_arguments)]
pub fn port_nlminb(
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
    if is_null(rho) {
        error("use of NULL environment is defunct");
    }
    if TYPEOF(rho) != SEXPTYPE::ENVSXP {
        error("'rho' must be an environment");
    }
    let n = LENGTH(d);
    if TYPEOF(d) != SEXPTYPE::REALSXP || n < 1 {
        error("'d' must be a nonempty numeric (double) vector");
    }
    if TYPEOF(iv) != SEXPTYPE::INTSXP || LENGTH(iv) as usize <= ITERATIONS {
        error("'iv' must be an integer vector with enough PORT control slots");
    }
    if TYPEOF(v) != SEXPTYPE::REALSXP || LENGTH(v) as usize <= OBJECTIVE_VALUE {
        error("'v' must be a double vector with enough PORT result slots");
    }
    if !is_null(hs_expr) && is_null(gr_expr) {
        error("When Hessian defined must also have gradient defined");
    }
    let sym = crate::sexp::symbol::install_str(".par");
    let xpt = crate::library::stats::checked::find_var(sym, rho);
    if xpt.is_null() || xpt == R_UnboundValue() {
        error("environment 'rho' must contain a numeric (double) vector '.par'");
    }
    if TYPEOF(xpt) != SEXPTYPE::REALSXP || LENGTH(xpt) != n {
        error(&format!(
            "environment 'rho' must contain a numeric (double) vector '.par' of length {n}"
        ));
    }
    let _start = protect(xpt);
    let names = getAttrib(xpt, R_NamesSymbol());
    let _names = protect(names);
    let history = 10;
    let (wa_len, iwa_len) = workspace_lengths(n, history)
        .unwrap_or_else(|| error("nlminb workspace exceeds supported dimensions"));
    let n = n as usize;
    let bytes = n
        .checked_mul(6)
        .and_then(|words| words.checked_add(wa_len as usize))
        .and_then(|words| words.checked_mul(size_of::<c_double>()))
        .and_then(|bytes| {
            n.checked_add(iwa_len as usize)?
                .checked_mul(size_of::<c_int>())?
                .checked_add(bytes)
        })
        .unwrap_or_else(|| error(WORKSPACE_LIMIT));
    let owner = crate::library::stats::checked::current_owner();
    let _workspace = owner
        .with_arena(|arena| arena.reserve_workspace(bytes))
        .unwrap_or_else(|failure| error(&failure.to_string()))
        .unwrap_or_else(|| error(WORKSPACE_LIMIT));
    let mut x = workspace_vec(n, 0.0);
    x.copy_from_slice(&crate::library::stats::checked::real_vec(xpt));
    let mut best = workspace_vec(n, 0.0);
    let mut best_value = f64::INFINITY;

    let bounded = LENGTH(lowerb) != 0 || LENGTH(upperb) != 0;
    let mut lower = workspace_vec(n, f64::NEG_INFINITY);
    let mut upper = workspace_vec(n, f64::INFINITY);
    let mut nbd = workspace_vec(n, 0 as c_int);
    if bounded {
        if TYPEOF(lowerb) != SEXPTYPE::REALSXP
            || TYPEOF(upperb) != SEXPTYPE::REALSXP
            || LENGTH(lowerb) as usize != n
            || LENGTH(upperb) as usize != n
        {
            error("'lower' and 'upper' must be double vectors matching 'start'");
        }
        for i in 0..n {
            lower[i] = REAL_ELT(lowerb, i as i32);
            upper[i] = REAL_ELT(upperb, i as i32);
            nbd[i] = match (lower[i].is_finite(), upper[i].is_finite()) {
                (false, false) => 0,
                (false, true) => 3,
                (true, false) => 1,
                (true, true) => 2,
            };
        }
    }

    let eval_max = {
        let raw = INTEGER_ELT(iv, EVAL_MAX as i32);
        if raw > 0 { raw } else { 200 }
    };
    let iter_max = {
        let raw = INTEGER_ELT(iv, ITER_MAX as i32);
        if raw > 0 { raw } else { 150 }
    };
    let trace = INTEGER_ELT(iv, TRACE as i32);

    let mut call = PortCall {
        expr: fn_expr,
        rho,
        names,
        function_count: 0,
        gradient_count: 0,
    };
    let mut f = 0.0 as c_double;
    let mut g = workspace_vec(n, 0.0);
    let mut trial = workspace_vec(n, 0.0);
    let mut wa = workspace_vec(wa_len as usize, 0.0);
    let mut iwa = workspace_vec(iwa_len as usize, 0 as c_int);
    let mut task: [c_char; 60] = [0; 60];
    for (slot, byte) in b"START\0".iter().enumerate() {
        task[slot] = *byte as c_char;
    }
    let mut pgtol = 1e-8 as c_double;
    let factr = 1e7 as c_double;
    let mut isave: [c_int; 44] = [0; 44];
    // lbfgsb only prints when iprint >= 0; PORT/optim default to silent.
    let iprint = if trace != 0 { trace } else { -1 };
    let mut state = LbfgsbState::default();
    let code = 'solver: loop {
        lbfgsb(
            &mut state,
            n as c_int,
            history,
            &mut x,
            &lower,
            &upper,
            &nbd,
            &mut f,
            &mut g,
            factr,
            &pgtol,
            &mut wa,
            &mut iwa,
            &mut task,
            iprint,
            &mut isave,
        );
        let task_len = task.iter().position(|&c| c == 0).unwrap_or(60);
        let task_bytes: Vec<u8> = task[..task_len].iter().map(|&c| c as u8).collect();
        if task_bytes.starts_with(b"CONVERGENCE: NORM") {
            break 3;
        }
        if task_bytes.starts_with(b"CONVERGENCE") {
            break 4;
        }
        if task_bytes.starts_with(b"NEW_X") {
            if state.iter >= iter_max {
                break 10;
            }
            continue;
        }
        if !task_bytes.starts_with(b"FG") {
            break if task_bytes.starts_with(b"ERROR") {
                82
            } else {
                8
            };
        }
        if call.function_count >= eval_max {
            x.copy_from_slice(&best);
            f = best_value;
            break 9;
        }
        f = call.value(&x, sym);
        if f < best_value || call.function_count == 1 {
            best.copy_from_slice(&x);
            best_value = f;
        }
        if call.function_count == 1 && !f.is_finite() {
            break 63;
        }
        if !is_null(gr_expr) {
            call.gradient(gr_expr, &x, &mut g, sym);
            continue;
        }
        trial.copy_from_slice(&x);
        for i in 0..n {
            let step = 1e-5 * (x[i].abs() + 1e-5);
            let forward = (x[i] + step).min(upper[i]);
            let next = if forward > x[i] {
                forward
            } else {
                (x[i] - step).max(lower[i])
            };
            if next == x[i] {
                g[i] = 0.0;
                continue;
            }
            if call.function_count >= eval_max {
                x.copy_from_slice(&best);
                f = best_value;
                break 'solver 9;
            }
            trial[i] = next;
            let perturbed = call.value(&trial, sym);
            g[i] = (perturbed - f) / (next - x[i]);
            trial[i] = x[i];
        }
        call.gradient_count = call.gradient_count.saturating_add(1);
    };

    call.publish(&x, sym);
    SET_INTEGER_ELT(iv, RETURN_CODE as i32, code);
    SET_INTEGER_ELT(iv, ITERATIONS as i32, state.iter);
    SET_INTEGER_ELT(iv, FUNCTION_COUNT as i32, call.function_count);
    SET_INTEGER_ELT(iv, GRADIENT_COUNT as i32, call.gradient_count);
    SET_REAL_ELT(v, OBJECTIVE_VALUE as i32, f);
    R_NilValue()
}
