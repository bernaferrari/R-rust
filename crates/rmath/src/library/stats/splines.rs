/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/stats/src/splines.c
 *
 *  Spline Interpolation — natural, periodic, and FMM splines.
 */

use std::os::raw::{c_char, c_double, c_int};
use std::slice::{from_raw_parts, from_raw_parts_mut};

use crate::attrib_core::{R_NamesSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, coerceVector};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::r_error;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::object::{NodeBody, Sexp};
use crate::sexp::owner::OwnerToken;
use crate::sexp::payload::PayloadLease;
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

unsafe fn get_list_element(list: SEXP, name: &[u8]) -> SEXP {
    unsafe {
        if TYPEOF(list) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names = getAttrib(list, R_NamesSymbol());
        let _names_guard = protect(names);
        let len = XLENGTH(list);
        if TYPEOF(names) != SEXPTYPE::STRSXP || XLENGTH(names) != len {
            return R_NilValue();
        }
        for i in 0..len {
            let nm = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i)));
            if nm.to_bytes() == name {
                return VECTOR_ELT(list, i);
            }
        }
        R_NilValue()
    }
}

unsafe fn knot_count(x: SEXP) -> R_xlen_t {
    unsafe {
        if XLENGTH(x) > 0 {
            match SEXPTYPE::from(TYPEOF(x)) {
                SEXPTYPE::INTSXP => {
                    let n = INTEGER_ELT(x, 0);
                    if n >= 0 {
                        return n as R_xlen_t;
                    }
                }
                SEXPTYPE::REALSXP => {
                    let n = REAL_ELT(x, 0);
                    if (0.0..=(1_i64 << 52) as f64).contains(&n) && n.fract() == 0.0 {
                        return n as R_xlen_t;
                    }
                }
                _ => (),
            }
        }
        r_error("invalid spline knot count")
    }
}

fn materialize_real(value: &Sexp<'_>) {
    value
        .materialize_compact_payload_for_raw()
        .unwrap_or_else(|err| r_error(err.to_string()));
}

fn real_payload(value: &Sexp<'_>, length: R_xlen_t) -> PayloadLease {
    let header = value.header();
    if header.type_of() != SEXPTYPE::REALSXP
        || !matches!(header.body, NodeBody::Vector(vector) if vector.length == length)
    {
        r_error("spline vector changed during materialization");
    }
    header
        .payload_lease()
        .cloned()
        .unwrap_or_else(|| r_error("spline vector has no numeric storage"))
}

// ---------------------------------------------------------------------------
// Spline algorithms (using usize for internal indexing)
// ---------------------------------------------------------------------------

/// Natural spline — end-conditions: second derivative = 0 at endpoints.
fn natural_spline(x: &[f64], y: &[f64], b: &mut [f64], c: &mut [f64], d: &mut [f64]) {
    let n = x.len();
    if n < 2 {
        return;
    }
    if n < 3 {
        let t = y[1] - y[0];
        b[0] = t / (x[1] - x[0]);
        b[1] = b[0];
        c[0] = 0.0;
        c[1] = 0.0;
        d[0] = 0.0;
        d[1] = 0.0;
        return;
    }

    let nm1 = n - 1;

    // Set up tridiagonal system
    d[0] = x[1] - x[0];
    c[1] = (y[1] - y[0]) / d[0];
    let mut i = 1usize;
    while i < nm1 {
        d[i] = x[i + 1] - x[i];
        b[i] = 2.0 * (d[i - 1] + d[i]);
        c[i + 1] = (y[i + 1] - y[i]) / d[i];
        c[i] = c[i + 1] - c[i];
        i += 1;
    }

    // Gaussian elimination
    i = 2;
    while i < nm1 {
        let t = d[i - 1] / b[i - 1];
        b[i] = b[i] - t * d[i - 1];
        c[i] = c[i] - t * c[i - 1];
        i += 1;
    }

    // Backward substitution
    c[nm1 - 1] = c[nm1 - 1] / b[nm1 - 1];
    i = nm1 - 2;
    while i >= 1 {
        c[i] = (c[i] - d[i] * c[i + 1]) / b[i];
        i -= 1;
    }

    // End conditions
    c[0] = 0.0;
    c[nm1] = 0.0;

    // Cubic coefficients
    b[0] = (y[1] - y[0]) / d[0] - d[0] * c[1];
    c[0] = 0.0;
    d[0] = c[1] / d[0];
    b[nm1] = (y[nm1] - y[nm1 - 1]) / d[nm1 - 1] + d[nm1 - 1] * c[nm1 - 1];
    i = 1;
    while i < nm1 {
        b[i] = (y[i + 1] - y[i]) / d[i] - d[i] * (c[i + 1] + 2.0 * c[i]);
        d[i] = (c[i + 1] - c[i]) / d[i];
        c[i] = 3.0 * c[i];
        i += 1;
    }
    c[nm1] = 0.0;
    d[nm1] = 0.0;
}

/// FMM spline — end-conditions from Forsythe, Malcolm, and Moler.
fn fmm_spline(x: &[f64], y: &[f64], b: &mut [f64], c: &mut [f64], d: &mut [f64]) {
    let n = x.len();
    if n < 2 {
        return;
    }
    if n < 3 {
        let t = y[1] - y[0];
        b[0] = t / (x[1] - x[0]);
        b[1] = b[0];
        c[0] = 0.0;
        c[1] = 0.0;
        d[0] = 0.0;
        d[1] = 0.0;
        return;
    }

    let nm1 = n - 1;

    // Set up tridiagonal system
    d[0] = x[1] - x[0];
    c[1] = (y[1] - y[0]) / d[0];
    let mut i = 1usize;
    while i < nm1 {
        d[i] = x[i + 1] - x[i];
        b[i] = 2.0 * (d[i - 1] + d[i]);
        c[i + 1] = (y[i + 1] - y[i]) / d[i];
        c[i] = c[i + 1] - c[i];
        i += 1;
    }

    // End conditions
    b[0] = -d[0];
    b[nm1] = -d[nm1 - 1];
    c[0] = 0.0;
    c[nm1] = 0.0;
    if n > 3 {
        c[0] = c[2] / (x[3] - x[1]) - c[1] / (x[2] - x[0]);
        c[nm1] = c[nm1 - 1] / (x[nm1] - x[nm1 - 2]) - c[nm1 - 2] / (x[nm1 - 1] - x[nm1 - 3]);
        c[0] = c[0] * d[0] * d[0] / (x[3] - x[0]);
        c[nm1] = -c[nm1] * d[nm1 - 1] * d[nm1 - 1] / (x[nm1] - x[nm1 - 3]);
    }

    // Gaussian elimination
    i = 1;
    while i < n {
        let t = d[i - 1] / b[i - 1];
        b[i] = b[i] - t * d[i - 1];
        c[i] = c[i] - t * c[i - 1];
        i += 1;
    }

    c[nm1] = c[nm1] / b[nm1];
    let mut i = nm1;
    loop {
        i -= 1;
        c[i] = (c[i] - d[i] * c[i + 1]) / b[i];
        if i == 0 {
            break;
        }
    }

    // Polynomial coefficients
    b[nm1] = (y[nm1] - y[nm1 - 1]) / d[nm1 - 1] + d[nm1 - 1] * (c[nm1 - 1] + 2.0 * c[nm1]);
    i = 0;
    while i < nm1 {
        b[i] = (y[i + 1] - y[i]) / d[i] - d[i] * (c[i + 1] + 2.0 * c[i]);
        d[i] = (c[i + 1] - c[i]) / d[i];
        c[i] = 3.0 * c[i];
        i += 1;
    }
    c[nm1] = 3.0 * c[nm1];
    d[nm1] = d[nm1 - 1];
}

/// Periodic spline — end-conditions match spline at x[1] and x[n].
fn periodic_spline(x: &[f64], y: &[f64], b: &mut [f64], c: &mut [f64], d: &mut [f64]) {
    let n = x.len();
    if n < 2 || y[0] != y[n - 1] {
        return;
    }

    if n == 2 {
        b[0] = 0.0;
        b[1] = 0.0;
        c[0] = 0.0;
        c[1] = 0.0;
        d[0] = 0.0;
        d[1] = 0.0;
        return;
    }

    let nm1 = n - 1;

    if n == 3 {
        let val = -(y[0] - y[1]) * (x[0] - 2.0 * x[1] + x[2]) / (x[2] - x[1]) / (x[1] - x[0]);
        b[0] = val;
        b[1] = val;
        b[2] = val;
        let cv = -3.0 * (y[0] - y[1]) / (x[2] - x[1]) / (x[1] - x[0]);
        c[0] = cv;
        c[1] = -cv;
        c[2] = cv;
        let dv = -2.0 * cv / 3.0 / (x[1] - x[0]);
        d[0] = dv;
        d[1] = -dv * (x[1] - x[0]) / (x[2] - x[1]);
        d[2] = dv;
        return;
    }

    // n >= 4
    let mut e: Vec<c_double> = vec![0.0; n];
    let mut s: c_double;
    let mut i: usize;

    // Set up matrix system: A=b, B=d, C=c
    d[0] = x[1] - x[0];
    d[nm1 - 1] = x[nm1] - x[nm1 - 1];
    b[0] = 2.0 * (d[0] + d[nm1 - 1]);
    c[0] = (y[1] - y[0]) / d[0] - (y[nm1] - y[nm1 - 1]) / d[nm1 - 1];

    i = 1;
    while i < nm1 {
        d[i] = x[i + 1] - x[i];
        b[i] = 2.0 * (d[i] + d[i - 1]);
        c[i] = (y[i + 1] - y[i]) / d[i] - (y[i] - y[i - 1]) / d[i - 1];
        i += 1;
    }

    // Cholesky: L=b, M=d, E=e
    b[0] = (b[0]).sqrt();
    e[0] = (x[nm1] - x[nm1 - 1]) / b[0];
    s = 0.0;
    i = 0;
    while i <= nm1 - 3 {
        d[i] = d[i] / b[i];
        if i != 0 {
            e[i] = -e[i - 1] * d[i - 1] / b[i];
        }
        b[i + 1] = (b[i + 1] - d[i] * d[i]).sqrt();
        s += e[i] * e[i];
        i += 1;
    }
    d[nm1 - 2] = (d[nm1 - 2] - e[nm1 - 3] * d[nm1 - 3]) / b[nm1 - 2];
    b[nm1 - 1] = (b[nm1 - 1] - d[nm1 - 2] * d[nm1 - 2] - s).sqrt();

    // Forward elimination: Y=c
    c[0] = c[0] / b[0];
    s = 0.0;
    i = 1;
    while i <= nm1 - 2 {
        c[i] = (c[i] - d[i - 1] * c[i - 1]) / b[i];
        s += e[i - 1] * c[i - 1];
        i += 1;
    }
    c[nm1 - 1] = (c[nm1 - 1] - d[nm1 - 2] * c[nm1 - 2] - s) / b[nm1 - 1];

    // Back substitution
    c[nm1 - 1] = c[nm1 - 1] / b[nm1 - 1];
    c[nm1 - 2] = (c[nm1 - 2] - d[nm1 - 2] * c[nm1 - 1]) / b[nm1 - 2];
    for i in (0..=nm1 - 3).rev() {
        c[i] = (c[i] - d[i] * c[i + 1] - e[i] * c[nm1 - 1]) / b[i];
    }

    // Wrap around
    c[nm1] = c[0];

    // Polynomial coefficients
    i = 0;
    while i < nm1 {
        s = x[i + 1] - x[i];
        b[i] = (y[i + 1] - y[i]) / s - s * (c[i + 1] + 2.0 * c[i]);
        d[i] = (c[i + 1] - c[i]) / s;
        c[i] = 3.0 * c[i];
        i += 1;
    }
    b[nm1] = b[0];
    c[nm1] = c[0];
    d[nm1] = d[0];
}

fn spline_coef(method: c_int, x: &[f64], y: &[f64], b: &mut [f64], c: &mut [f64], d: &mut [f64]) {
    match method {
        1 => periodic_spline(x, y, b, c, d),
        2 => natural_spline(x, y, b, c, d),
        3 => fmm_spline(x, y, b, c, d),
        _ => {} // intentionally unhandled: unknown spline method
    }
}

fn spline_eval(
    method: c_int,
    u: &[f64],
    v: &mut [f64],
    x: &[f64],
    y: &[f64],
    b: &[f64],
    c: &[f64],
    d: &[f64],
) {
    let n = x.len();
    let n_1 = n - 1;
    let mut i: usize = 0;

    if method == 1 && n > 1 {
        let dx = x[n_1] - x[0];
        for (out, &query) in v.iter_mut().zip(u) {
            *out = (query - x[0]) % dx;
            if *out < 0.0 {
                *out += dx;
            }
            *out += x[0];
        }
    } else {
        v.copy_from_slice(u);
    }

    for out in v {
        let ul = *out;
        if ul < x[i] || (i < n_1 && x[i + 1] < ul) {
            i = 0;
            let mut j = n;
            loop {
                let k = i + (j - i) / 2;
                if ul < x[k] {
                    j = k;
                } else {
                    i = k;
                }
                if j <= i + 1 {
                    break;
                }
            }
        }
        let dx = ul - x[i];
        let tmp = if method == 2 && ul < x[0] { 0.0 } else { d[i] };
        *out = y[i] + dx * (b[i] + dx * (c[i] + dx * tmp));
    }
}

// ---------------------------------------------------------------------------
// Exported SEXP functions
// ---------------------------------------------------------------------------

/// SplineCoef - compute spline coefficients.
pub unsafe fn SplineCoef(method: SEXP, x: SEXP, y: SEXP) -> SEXP {
    unsafe {
        let x = coerceVector(x, SEXPTYPE::REALSXP.into());
        let _x_guard = protect(x);
        let y = coerceVector(y, SEXPTYPE::REALSXP.into());
        let _y_guard = protect(y);
        let n = XLENGTH(x);
        let m = asInteger(method);
        if XLENGTH(y) != n {
            Rf_error(b"inputs of different lengths\0".as_ptr() as *const c_char);
        }

        let b = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _b_guard = protect(b);
        let c = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _c_guard = protect(c);
        let d = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _d_guard = protect(d);

        if n != 0 {
            let len = usize::try_from(n).unwrap_or_else(|_| r_error("invalid spline knot count"));
            let owner = OwnerToken::current().unwrap_or_else(|err| r_error(err.to_string()));
            let values = [x, y, b, c, d].map(|value| {
                owner
                    .sexp(value)
                    .unwrap_or_else(|err| r_error(err.to_string()))
            });
            materialize_real(&values[0]);
            materialize_real(&values[1]);
            let [xp, yp, bp, cp, dp] = values.each_ref().map(|value| real_payload(value, n));
            // All materialization has finished. The retained inputs have the
            // checked length, and the three freshly allocated outputs are
            // distinct. The kernel never reenters R while any slice is live.
            let (b, c, d) = (
                from_raw_parts_mut(bp.native_projection().cast(), len),
                from_raw_parts_mut(cp.native_projection().cast(), len),
                from_raw_parts_mut(dp.native_projection().cast(), len),
            );
            b.fill(0.0);
            c.fill(0.0);
            d.fill(0.0);
            spline_coef(
                m,
                from_raw_parts(xp.native_projection().cast(), len),
                from_raw_parts(yp.native_projection().cast(), len),
                b,
                c,
                d,
            );
        }

        let ans = Rf_allocVector(SEXPTYPE::VECSXP, 7);
        let _ans_guard = protect(ans);
        SET_VECTOR_ELT(ans, 0, Rf_ScalarInteger(m));
        if n > i32::MAX as R_xlen_t {
            SET_VECTOR_ELT(ans, 1, Rf_ScalarReal(n as c_double));
        } else {
            SET_VECTOR_ELT(ans, 1, Rf_ScalarInteger(n as c_int));
        }
        SET_VECTOR_ELT(ans, 2, x);
        SET_VECTOR_ELT(ans, 3, y);
        SET_VECTOR_ELT(ans, 4, b);
        SET_VECTOR_ELT(ans, 5, c);
        SET_VECTOR_ELT(ans, 6, d);

        let nm = Rf_allocVector(SEXPTYPE::STRSXP, 7);
        let _nm_guard = protect(nm);
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"method\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"n\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 2, Rf_mkChar(b"x\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 3, Rf_mkChar(b"y\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 4, Rf_mkChar(b"b\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 5, Rf_mkChar(b"c\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 6, Rf_mkChar(b"d\0".as_ptr() as *const c_char));
        setAttrib(ans, R_NamesSymbol(), nm);
        ans
    }
}

/// SplineEval - evaluate a spline at given points.
pub unsafe fn SplineEval(xout: SEXP, z: SEXP) -> SEXP {
    unsafe {
        let _z_guard = protect(z);
        let xout = coerceVector(xout, SEXPTYPE::REALSXP.into());
        let _xout_guard = protect(xout);
        let nu = XLENGTH(xout);
        let nx = knot_count(get_list_element(z, b"n"));
        let method = asInteger(get_list_element(z, b"method"));
        let knots = [b"x", b"y", b"b", b"c", b"d"].map(|name| {
            let value = get_list_element(z, name);
            (value, protect(value))
        });
        if knots
            .iter()
            .any(|(part, _)| TYPEOF(*part) != SEXPTYPE::REALSXP || XLENGTH(*part) != nx)
        {
            r_error("invalid spline coefficient lengths or types");
        }
        let yout = Rf_allocVector3(SEXPTYPE::REALSXP, nu);
        let _yout_guard = protect(yout);
        if nu == 0 {
            return yout;
        }
        if nx == 0 {
            r_error("spline has no knots");
        }
        let owner = OwnerToken::current().unwrap_or_else(|err| r_error(err.to_string()));
        let query = owner
            .sexp(xout)
            .unwrap_or_else(|err| r_error(err.to_string()));
        let output = owner
            .sexp(yout)
            .unwrap_or_else(|err| r_error(err.to_string()));
        let values = knots.each_ref().map(|(value, _guard)| {
            owner
                .sexp(*value)
                .unwrap_or_else(|err| r_error(err.to_string()))
        });
        materialize_real(&query);
        for value in &values {
            materialize_real(value);
        }
        let query = real_payload(&query, nu);
        let output = real_payload(&output, nu);
        let parts = values.each_ref().map(|value| real_payload(value, nx));
        let nu = usize::try_from(nu).unwrap_or_else(|_| r_error("invalid spline query length"));
        let nx = usize::try_from(nx).unwrap_or_else(|_| r_error("invalid spline knot count"));
        // The parts retain the exact post-materialization allocations and may
        // share storage with one another. yout was allocated in this call and
        // is disjoint. There are no callbacks while any of these slices live.
        let [x, y, b, c, d] = parts
            .each_ref()
            .map(|lease| from_raw_parts(lease.native_projection().cast(), nx));
        spline_eval(
            method,
            from_raw_parts(query.native_projection().cast(), nu),
            from_raw_parts_mut(output.native_projection().cast(), nu),
            x,
            y,
            b,
            c,
            d,
        );
        yout
    }
}

/// GNU `smooth.spline` interpolating path for exact data.
pub unsafe fn do_smooth_spline(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let method = Rf_ScalarInteger(3);
        let _m = protect(method);
        let z = SplineCoef(method, x, y);
        let _z = protect(z);
        let yhat = SplineEval(x, z);
        let _yh = protect(yhat);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, x);
        SET_VECTOR_ELT(result, 1, yhat);
        SET_VECTOR_ELT(result, 2, Rf_ScalarInteger(n as c_int));
        crate::mainutils::essentials::set_string_names(
            result,
            &["x".to_string(), "y".to_string(), "n".to_string()],
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"smooth.spline".as_ptr()),
        );
        result
    }
}

/// GNU `spline(x, y, xout)` FMM cubic.
pub unsafe fn do_spline(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let xout = CAR(CDR(CDR(args)));
        let method = Rf_ScalarInteger(3);
        let _m = protect(method);
        let z = SplineCoef(method, x, y);
        let _z = protect(z);
        let yout = SplineEval(xout, z);
        let _yo = protect(yout);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, xout);
        SET_VECTOR_ELT(result, 1, yout);
        crate::mainutils::essentials::set_string_names(result, &["x".to_string(), "y".to_string()]);
        result
    }
}

/// GNU `splinefun(x, y)` FMM interpolator.
pub unsafe fn do_splinefun(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let method = Rf_ScalarInteger(3);
        let _m = protect(method);
        let z = SplineCoef(method, x, y);
        let _z = protect(z);
        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), rho, R_NilValue());
        let _env = protect(env);
        crate::sexp::envir::defineVar(crate::sexp::symbol::Rf_install(c"z".as_ptr()), z, env);
        let vsym = crate::sexp::symbol::Rf_install(c"v".as_ptr());
        let formals = Rf_cons(R_MissingArg(), R_NilValue());
        SETTAG(formals, vsym);
        let body = Rf_lang2(
            crate::sexp::symbol::Rf_install(c".spline_apply".as_ptr()),
            vsym,
        );
        crate::mainutils::dstruct::mkCLOSXP(formals, body, env)
    }
}

/// Evaluate a `splinefun` closure at `v`.
pub unsafe fn do_spline_apply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let v = CAR(args);
        let env = ENCLOS(rho);
        let z = crate::sexp::envir::R_findVar(crate::sexp::symbol::Rf_install(c"z".as_ptr()), env);
        SplineEval(v, z)
    }
}

pub unsafe extern "C-unwind" fn c_spline_coef(method: SEXP, x: SEXP, y: SEXP) -> SEXP {
    unsafe { SplineCoef(method, x, y) }
}

pub unsafe extern "C-unwind" fn c_spline_eval(xout: SEXP, z: SEXP) -> SEXP {
    unsafe { SplineEval(xout, z) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::ffi::{NA_REAL, R_IsNA};

    #[test]
    fn spline_methods_match_gnu_for_unordered_queries_and_extrapolation() {
        let x = [-1.25, -0.5, 1.25, 2.0, 4.5];
        let y = [2.0, 4.0, -1.0, 0.25, 2.0];
        let queries = [5.25, -3.0, 0.75, 3.25, 1.25, NA_REAL, f64::NAN];
        // Values from GNU R 4.6.2's spline(x, y, method, xout).
        let expected = [
            [
                4.0,
                0.8899241534776687,
                0.123974583963111,
                0.8329439252336448,
                -1.0,
            ],
            [
                1.915737280827824,
                -4.818251173708919,
                0.10951936106434612,
                1.88657839896522,
                -1.0,
            ],
            [
                -2.8365001762580424,
                -36.57990082095572,
                0.004373523717843608,
                3.245009270495082,
                -1.0,
            ],
        ];
        for (method, expected) in (1..=3).zip(expected) {
            let (mut b, mut c, mut d) = ([0.0; 5], [0.0; 5], [0.0; 5]);
            let mut result = [0.0; 7];
            spline_coef(method, &x, &y, &mut b, &mut c, &mut d);
            spline_eval(method, &queries, &mut result, &x, &y, &b, &c, &d);
            for (actual, expected) in result.into_iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 1e-12,
                    "{method}: {actual} != {expected}"
                );
            }
            assert!(R_IsNA(result[5]));
            assert!(result[6].is_nan() && !R_IsNA(result[6]));
        }
    }

    #[test]
    fn spline_ffi_rejects_wrong_lengths_and_handles_empty_vectors() {
        use crate::sexp::{context::take_error_message, session::RSession};

        let _session = RSession::new_for_gc_tests();
        unsafe {
            let method = Rf_ScalarInteger(3);
            let _method = protect(method);
            let empty = Rf_allocVector(SEXPTYPE::REALSXP, 0);
            let _empty = protect(empty);
            let z = SplineCoef(method, empty, empty);
            let _z = protect(z);
            assert_eq!(XLENGTH(SplineEval(empty, z)), 0);

            let query = Rf_ScalarReal(7.0);
            let _query = protect(query);
            let err = std::panic::catch_unwind(|| SplineEval(query, z)).unwrap_err();
            assert_eq!(take_error_message(err).unwrap(), "spline has no knots");

            let knots = SplineCoef(method, query, query);
            let _knots = protect(knots);
            assert_eq!(REAL_ELT(SplineEval(query, knots), 0), 7.0);
            SET_VECTOR_ELT(knots, 4, empty);
            let err = std::panic::catch_unwind(|| SplineEval(query, knots)).unwrap_err();
            assert_eq!(
                take_error_message(err).unwrap(),
                "invalid spline coefficient lengths or types"
            );
        }
    }
}
