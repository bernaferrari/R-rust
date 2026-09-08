//! Public `pretty`/`pretty.default` frontend backed by R's `R_pretty` kernel.
//!
//! The axis and histogram code already uses the portable `R_pretty` port.  A
//! public frontend is kept here so callers of `pretty()` receive the same
//! bounds and sequence contract as base R instead of a second approximation.

use crate::appl::pretty::R_pretty;
use crate::mainutils::coerce::coerceVector;
use crate::mainutils::essentials::{arg_by_name_or_position, base_error};
use crate::sexp::accessors::{REAL, XLENGTH};
use crate::sexp::constructors::Rf_allocVector3;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

unsafe fn scalar_real(value: SEXP, name: &str, default: f64) -> f64 {
    unsafe {
        if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
            return default;
        }
        let real = coerceVector(value, SEXPTYPE::REALSXP.as_c_int());
        if real.is_null() || XLENGTH(real) == 0 {
            base_error(format!("invalid '{name}' argument"));
        }
        let _guard = protect(real);
        let result = *REAL(real);
        if result.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || result.is_nan() {
            base_error(format!("invalid '{name}' argument"));
        }
        result
    }
}

unsafe fn scalar_bool(value: SEXP, name: &str, default: bool) -> bool {
    unsafe {
        if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
            return default;
        }
        let real = coerceVector(value, SEXPTYPE::REALSXP.as_c_int());
        if real.is_null() || XLENGTH(real) == 0 {
            base_error(format!("invalid '{name}' argument"));
        }
        let _guard = protect(real);
        let result = *REAL(real);
        if result.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || result.is_nan() {
            base_error(format!("invalid '{name}' argument"));
        }
        result != 0.0
    }
}

/// `pretty.default`, using the same `R_pretty` kernel as graphics axes.
pub unsafe fn pretty_values(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_expr = arg_by_name_or_position(args, &["x"], 0);
        if x_expr.is_null() || x_expr == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let x_real = coerceVector(x_expr, SEXPTYPE::REALSXP.as_c_int());
        if x_real.is_null() {
            base_error("invalid 'x' argument");
        }
        let _x_guard = protect(x_real);
        let mut finite = Vec::new();
        for i in 0..XLENGTH(x_real) as usize {
            let value = *REAL(x_real).add(i);
            if value.is_finite() {
                finite.push(value);
            }
        }
        if finite.is_empty() {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }

        let n_value = arg_by_name_or_position(args, &["n"], 1);
        let n = scalar_real(n_value, "n", 5.0);
        if !n.is_finite() || n < 0.0 || n > 1_000_000.0 {
            base_error("invalid 'n' argument");
        }
        let n = n as i32;
        let min_n = scalar_real(
            arg_by_name_or_position(args, &["min.n"], 2),
            "min.n",
            (n / 3) as f64,
        );
        if !min_n.is_finite() || min_n < 0.0 || min_n > 1_000_000.0 {
            base_error("invalid 'min.n' argument");
        }
        let min_n = min_n as i32;
        let shrink = scalar_real(
            arg_by_name_or_position(args, &["shrink.sml"], 3),
            "shrink.sml",
            0.75,
        );
        let high_u_bias = scalar_real(
            arg_by_name_or_position(args, &["high.u.bias"], 4),
            "high.u.bias",
            1.5,
        );
        let u5_bias = scalar_real(
            arg_by_name_or_position(args, &["u5.bias"], 5),
            "u5.bias",
            0.5 + 1.5 * high_u_bias,
        );
        let eps = scalar_real(
            arg_by_name_or_position(args, &["eps.correct"], 6),
            "eps.correct",
            0.0,
        );
        if !eps.is_finite() || !(0.0..=2.0).contains(&eps) {
            base_error("invalid 'eps.correct' argument");
        }
        let eps = eps as i32;
        let f_min = scalar_real(
            arg_by_name_or_position(args, &["f.min"], 7),
            "f.min",
            2.0_f64.powi(-20),
        );
        let bounds = scalar_bool(
            arg_by_name_or_position(args, &["bounds"], 8),
            "bounds",
            true,
        );
        if !shrink.is_finite()
            || shrink <= 0.0
            || !high_u_bias.is_finite()
            || high_u_bias <= 0.0
            || !u5_bias.is_finite()
            || u5_bias <= 0.0
            || !f_min.is_finite()
            || f_min <= 0.0
        {
            base_error("invalid pretty scale parameters");
        }

        let mut lo = finite.iter().copied().fold(f64::INFINITY, f64::min);
        let mut up = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mut ndiv = n;
        let high_u_fact = [high_u_bias, u5_bias, f_min];
        let unit = R_pretty(
            &mut lo,
            &mut up,
            &mut ndiv,
            min_n,
            shrink,
            high_u_fact.as_ptr(),
            eps,
            if bounds { 1 } else { 0 },
        );
        if ndiv < 0 || ndiv > 1_000_000 {
            base_error("invalid 'n' argument");
        }
        let length = ndiv as usize + 1;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, length as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let (start, end) = if bounds {
            (lo, up)
        } else {
            (lo * unit, up * unit)
        };
        if ndiv == 0 {
            *REAL(result) = start;
        } else {
            for i in 0..=ndiv as usize {
                *REAL(result).add(i) = if i == 0 {
                    start
                } else if i == ndiv as usize {
                    end
                } else {
                    let t = i as f64 / ndiv as f64;
                    start * (1.0 - t) + end * t
                };
            }
        }
        if eps == 0 && ndiv > 0 {
            // Match R's `diff(range(z$l, z$u) / n)` ordering.  Dividing
            // endpoints before subtracting avoids overflowing for a valid
            // range such as [-1e308, 1e308].
            let delta = (end / ndiv as f64 - start / ndiv as f64).abs();
            for i in 0..length {
                let value = *REAL(result).add(i);
                if value.abs() < 1e-14 * delta {
                    *REAL(result).add(i) = 0.0;
                }
            }
        }
        result
    }
}

/// The public `pretty` generic.
pub unsafe fn do_pretty(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "pretty",
            "function(x, ...) UseMethod(\"pretty\")",
            args,
            rho,
            false,
        )
    }
}

/// Ordinary R argument matching and dependent defaults precede the rooted builtin.
pub unsafe fn do_pretty_default(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "pretty.default",
            "function(x,n=5L,min.n=n%/%3L,shrink.sml=.75,high.u.bias=1.5,u5.bias=.5+1.5*high.u.bias,eps.correct=0L,f.min=2^-20,bounds=TRUE,...) .rport_pretty(x=x,n=n,min.n=min.n,shrink.sml=shrink.sml,high.u.bias=high.u.bias,u5.bias=u5.bias,eps.correct=eps.correct,f.min=f.min,bounds=bounds)",
            args,
            rho,
            false,
        )
    }
}
