//! plot/axis/graphics builtins (stubs and generic drivers).

use super::*;

/// R's plot generic delegates through the ordinary S3 machinery.
pub unsafe fn do_plot(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "plot",
            "function(x, y, ...) UseMethod('plot')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `preplot(object, ...)` — `UseMethod("preplot")`.
pub unsafe fn do_preplot(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "preplot",
            "function(object, ...) UseMethod('preplot')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `profile(fitted, ...)` — `UseMethod("profile")`.
pub unsafe fn do_profile(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "profile",
            "function(fitted, ...) UseMethod('profile')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `tsdiag(object, gof.lag, ...)` — `UseMethod("tsdiag")`.
pub unsafe fn do_tsdiag(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "tsdiag",
            "function(object, gof.lag, ...) UseMethod('tsdiag')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `free1way(y, ...)` — `UseMethod("free1way")`.
pub unsafe fn do_free1way(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "free1way",
            "function(y, ...) UseMethod('free1way')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `power.free1way.test` — exactly one of n/delta/power/sig.level is NULL.
pub unsafe fn do_power_free1way_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut n_null = true;
        let mut delta_null = true;
        let mut power_null = true;
        let mut sig_null = false;
        let mut cell = args;
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let val = CAR(cell);
            let is_null = val.is_null() || val == R_NilValue();
            if name == "n" || (name.is_empty() && pos == 0) {
                n_null = is_null;
            } else if name == "delta" {
                delta_null = is_null;
            } else if name == "power" {
                power_null = is_null;
            } else if name == "sig.level" {
                sig_null = is_null;
            }
            if name.is_empty() {
                pos += 1;
            }
            cell = CDR(cell);
        }
        let nnull = [n_null, delta_null, power_null, sig_null]
            .iter()
            .filter(|b| **b)
            .count();
        if nnull != 1 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "exactly one of 'n', 'delta', 'power', and 'sig.level' must be NULL",
            );
        }
        R_NilValue()
    }
}

/// GNU `monthplot(x, ...)` — `UseMethod("monthplot")`.
pub unsafe fn do_monthplot(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "monthplot",
            "function(x, ...) UseMethod('monthplot')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `monthplot.default(x)` — `range(x)` becomes `ylim`.
pub unsafe fn do_monthplot_default(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = if x.is_null() || x == R_NilValue() {
            0
        } else {
            XLENGTH(x)
        };
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let mut any = false;
        for i in 0..n {
            let v = elt_real_safe(x, i);
            if v.is_finite() {
                any = true;
                if v < lo {
                    lo = v;
                }
                if v > hi {
                    hi = v;
                }
            }
        }
        if !any || !lo.is_finite() || !hi.is_finite() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid 'ylim' value",
            );
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// GNU `scatter.smooth(x, y)` — `seq(min(x), max(x), length.out=)`.
pub unsafe fn do_scatter_smooth(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = if x.is_null() || x == R_NilValue() {
            0
        } else {
            XLENGTH(x)
        };
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let mut any = false;
        for i in 0..n {
            let v = elt_real_safe(x, i);
            if v.is_finite() {
                any = true;
                if v < lo {
                    lo = v;
                }
                if v > hi {
                    hi = v;
                }
            }
        }
        if !any || !lo.is_finite() || !hi.is_finite() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'from' must be a finite number",
            );
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// GNU `interaction.plot(x.factor, trace.factor, response)` — invisible.
pub unsafe fn do_interaction_plot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let rest = CDR(args);
        let trace = if rest.is_null() || rest == R_NilValue() {
            R_NilValue()
        } else {
            CAR(rest)
        };
        let resp = if rest.is_null() || rest == R_NilValue() {
            R_NilValue()
        } else {
            let r2 = CDR(rest);
            if r2.is_null() || r2 == R_NilValue() {
                R_NilValue()
            } else {
                CAR(r2)
            }
        };
        if x.is_null()
            || x == R_NilValue()
            || XLENGTH(x) == 0
            || resp.is_null()
            || resp == R_NilValue()
            || XLENGTH(resp) == 0
        {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid 'ylim' value",
            );
        }
        let _ = trace;
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// GNU `lag.plot(x)` — `as.ts(as.matrix(x))` then plot.
pub unsafe fn do_lag_plot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || XLENGTH(x) == 0 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid 'xlim' value",
            );
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// GNU `eff.aovlist(aovlist)` — `$qr` on each stratum.
pub unsafe fn do_eff_aovlist(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let aovlist = CAR(args);
        if aovlist.is_null() || aovlist == R_NilValue() || TYPEOF(aovlist) != SEXPTYPE::VECSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "$ operator is invalid for atomic vectors",
            );
        }
        if XLENGTH(aovlist) == 0 {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return R_NilValue();
        }
        let dollar = Rf_lang3(
            crate::sexp::symbol::R_DollarSymbol(),
            VECTOR_ELT(aovlist, 0),
            Rf_install(c"qr".as_ptr()),
        );
        let _ = crate::eval::eval::Rf_eval(dollar, rho);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// GNU `biplot(x, ...)` — `UseMethod("biplot")`.
pub unsafe fn do_biplot(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "biplot",
            "function(x, ...) UseMethod('biplot')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `screeplot(x, ...)` — `UseMethod("screeplot")`.
pub unsafe fn do_screeplot(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "screeplot",
            "function(x, ...) UseMethod('screeplot')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `biplot.default(x, y, ...)` — requires `y`.
pub unsafe fn do_biplot_default(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let y = CAR(CDR(args));
        if y.is_null() || y == R_NilValue() || y == crate::sexp::globals::R_MissingArg() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "argument \"y\" is missing, with no default",
            );
        }
        R_NilValue()
    }
}

/// GNU `screeplot.default(x)` — reads `x$sdev`.
pub unsafe fn do_screeplot_default(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dollar = crate::sexp::constructors::Rf_lang3(
            crate::sexp::symbol::Rf_install(c"$".as_ptr()),
            x,
            crate::sexp::symbol::Rf_install(c"sdev".as_ptr()),
        );
        let _d = protect(dollar);
        let _ = crate::eval::eval::Rf_eval(dollar, rho);
        R_NilValue()
    }
}

/// GNU `plot.spec.coherency(x)` — reads `x$spec`.
pub unsafe fn do_plot_spec_coherency(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { spec_plot_read_spec(args, rho) }
}

/// GNU `plot.spec.phase(x)` — reads `x$spec`.
pub unsafe fn do_plot_spec_phase(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { spec_plot_read_spec(args, rho) }
}

pub(super) unsafe fn spec_plot_read_spec(args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dollar = Rf_lang3(Rf_install(c"$".as_ptr()), x, Rf_install(c"spec".as_ptr()));
        let _d = protect(dollar);
        let _ = crate::eval::eval::Rf_eval(dollar, rho);
        R_NilValue()
    }
}

pub unsafe fn do_plot_default(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    #[cfg(feature = "renderplot-device")]
    unsafe {
        crate::mainutils::portable_plot::plot_default(call, op, args, rho)
    }
    #[cfg(not(feature = "renderplot-device"))]
    {
        let _ = (call, op, rho);
        // GNU plot.default establishes the plot frame via plot.new() first.
        crate::library::graphics::graphics::set_plot_new_state();
        unsafe { record_plot_window(args) };
        unsafe { crate::sexp::globals::R_NilValue() }
    }
}

#[cfg(not(feature = "renderplot-device"))]
pub(super) unsafe fn record_plot_window(args: SEXP) {
    unsafe {
        let x = crate::mainutils::essentials::arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == crate::sexp::globals::R_NilValue() {
            return;
        }
        let y = crate::mainutils::essentials::arg_by_name_or_position(args, &["y"], 1);
        let y_missing = y.is_null()
            || y == crate::sexp::globals::R_NilValue()
            || y == crate::sexp::globals::R_MissingArg();
        let mut yv = if y_missing {
            numeric_plot_values(x)
        } else {
            numeric_plot_values(y)
        };
        let mut xv = if y_missing {
            (1..=yv.len()).map(|i| i as f64).collect()
        } else {
            numeric_plot_values(x)
        };
        if xv.is_empty() || yv.is_empty() {
            return;
        }
        if xv.len() != yv.len() {
            let n = xv.len().min(yv.len());
            xv.truncate(n);
            yv.truncate(n);
        }
        let log_arg = crate::mainutils::essentials::arg_by_name_or_position(args, &["log"], 10);
        let log =
            if log_arg.is_null() || TYPEOF(log_arg) != SEXPTYPE::STRSXP || XLENGTH(log_arg) == 0 {
                String::new()
            } else {
                let chars = CHAR(STRING_ELT(log_arg, 0));
                if chars.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(chars)
                        .to_string_lossy()
                        .into_owned()
                }
            };
        let log_x = log.contains('x');
        let log_y = log.contains('y');
        if log_x {
            xv = xv
                .into_iter()
                .filter(|v| *v > 0.0)
                .map(|v| v.log10())
                .collect();
        }
        if log_y {
            yv = yv
                .into_iter()
                .filter(|v| *v > 0.0)
                .map(|v| v.log10())
                .collect();
        }
        let xaxs_i = matches!(crate::library::graphics::par::parameter("xaxs"), crate::library::graphics::par::ParValue::String(s) if s == "i");
        let yaxs_i = matches!(crate::library::graphics::par::parameter("yaxs"), crate::library::graphics::par::ParValue::String(s) if s == "i");
        let tight = |v: &[f64]| {
            let lo = v
                .iter()
                .copied()
                .filter(|x| x.is_finite())
                .fold(f64::INFINITY, f64::min);
            let hi = v
                .iter()
                .copied()
                .filter(|x| x.is_finite())
                .fold(f64::NEG_INFINITY, f64::max);
            if lo.is_finite() { (lo, hi) } else { (0.0, 1.0) }
        };
        let (x0, x1) = if xaxs_i {
            tight(&xv)
        } else {
            padded_range(&xv)
        };
        let (y0, y1) = if yaxs_i {
            tight(&yv)
        } else {
            padded_range(&yv)
        };
        let apply_limit = |current: (f64, f64), name: &str, logged: bool| {
            let arg =
                crate::mainutils::essentials::arg_by_name_or_position(args, &[name], usize::MAX);
            let v = numeric_plot_values(arg);
            if v.len() < 2 || !v[0].is_finite() || !v[1].is_finite() {
                return current;
            }
            let (mut a, mut b) = (v[0], v[1]);
            if logged {
                a = a.log10();
                b = b.log10();
                if !a.is_finite() || !b.is_finite() {
                    let fmt = std::ffi::CString::new(format!(
                        "nonfinite axis={} limits [GScale({a},{b},..); log=TRUE] -- corrected now",
                        if name == "xlim" { 1 } else { 2 }
                    ))
                    .unwrap_or_default();
                    crate::mainutils::errors::warningcall(
                        crate::sexp::globals::R_NilValue(),
                        fmt.as_ptr(),
                    );
                    let fix = |z: f64| {
                        if z.is_finite() {
                            z
                        } else if z < 0. {
                            -320.
                        } else {
                            308.254715559
                        }
                    };
                    a = fix(a);
                    b = fix(b);
                }
            }
            let extra = (b - a).abs() * 0.04;
            if a <= b {
                (a - extra, b + extra)
            } else {
                (a + extra, b - extra)
            }
        };
        let (mut x0, mut x1) = apply_limit(
            (x0, x1),
            "xlim",
            log_x,
        );
        let (mut y0, mut y1) = apply_limit(
            (y0, y1),
            "ylim",
            log_y,
        );
        let clamp_plot_log = |v: &mut f64| {
            if *v == f64::INFINITY || *v >= 308.25035 {
                *v = (0.99 * f64::MAX).log10();
            } else if !v.is_finite() || *v < -1074.0 * std::f64::consts::LOG10_2 {
                *v = (1.01 * f64::MIN_POSITIVE).log10();
            }
        };
        if log_x {
            clamp_plot_log(&mut x0);
            clamp_plot_log(&mut x1);
        }
        if log_y {
            clamp_plot_log(&mut y0);
            clamp_plot_log(&mut y1);
        }
        let (xa0, xa1, xn) = pretty_axp(x0, x1);
        let (ya0, ya1, yn) = pretty_axp(y0, y1);
        use crate::library::graphics::par::{ParValue, set_plot_parameter};
        set_plot_parameter("usr", ParValue::Real(vec![x0, x1, y0, y1]));
        set_plot_parameter(
            "xlog",
            ParValue::Logical(vec![if log_x {
                1
            } else {
                0
            }]),
        );
        set_plot_parameter(
            "ylog",
            ParValue::Logical(vec![if log_y {
                1
            } else {
                0
            }]),
        );
        if log_x {
            let (a, b) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
            let lo = a.ceil().clamp(-307.0, 308.0);
            let hi = b.floor().clamp(lo, 308.0);
            let n = if hi - lo <= 2.0 {
                3.0
            } else if hi - lo <= 3.0 {
                2.0
            } else {
                1.0
            };
            let (p0, p1) = if x0 <= x1 {
                (10f64.powf(lo), 10f64.powf(hi))
            } else {
                (10f64.powf(hi), 10f64.powf(lo))
            };
            set_plot_parameter("xaxp", ParValue::Real(vec![p0, p1, n]));
        } else {
            set_plot_parameter("xaxp", ParValue::Real(vec![xa0, xa1, xn]));
        }
        if log_y {
            let (a, b) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
            let lo = a.ceil().clamp(-307.0, 308.0);
            let hi = b.floor().clamp(lo, 308.0);
            let n = if hi - lo <= 2.0 {
                3.0
            } else if hi - lo <= 3.0 {
                2.0
            } else {
                1.0
            };
            let (p0, p1) = if y0 <= y1 {
                (10f64.powf(lo), 10f64.powf(hi))
            } else {
                (10f64.powf(hi), 10f64.powf(lo))
            };
            set_plot_parameter("yaxp", ParValue::Real(vec![p0, p1, n]));
        } else {
            set_plot_parameter("yaxp", ParValue::Real(vec![ya0, ya1, yn]));
        }
    }
}

#[cfg(not(feature = "renderplot-device"))]
pub(super) fn numeric_plot_values(x: SEXP) -> Vec<f64> {
    unsafe {
        let n = crate::sexp::accessors::XLENGTH(x) as usize;
        let t = crate::sexp::accessors::TYPEOF(x);
        if t == crate::sexp::ffi::SEXPTYPE::REALSXP {
            (0..n)
                .map(|i| crate::sexp::accessors::REAL(x).add(i).read())
                .collect()
        } else if t == crate::sexp::ffi::SEXPTYPE::INTSXP {
            (0..n)
                .map(|i| crate::sexp::accessors::INTEGER(x).add(i).read() as f64)
                .collect()
        } else {
            Vec::new()
        }
    }
}

#[cfg(not(feature = "renderplot-device"))]
pub(super) fn padded_range(v: &[f64]) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for x in v {
        if x.is_finite() {
            lo = lo.min(*x);
            hi = hi.max(*x);
        }
    }
    if !lo.is_finite() {
        return (0.0, 1.0);
    }
    if lo == hi {
        return (lo - 1.0, hi + 1.0);
    }
    let extra = if hi.abs() > 100.0 || lo.abs() > 100.0 {
        0.04 * hi - 0.04 * lo
    } else {
        (hi - lo) * 0.04
    };
    let a = lo - extra;
    let b = hi + extra;
    (
        if a.is_finite() { a } else { lo },
        if b.is_finite() { b } else { hi },
    )
}

pub(super) fn pretty_axp(lo0: f64, hi0: f64) -> (f64, f64, f64) {
    let mut lo = lo0;
    let mut hi = hi0;
    let mut ndiv = 5i32;
    let high = [0.8f64, 1.7, 1.125];
    let unit = unsafe {
        crate::appl::pretty::R_pretty(&mut lo, &mut hi, &mut ndiv, 1, 0.25, high.as_ptr(), 2, 0)
    };
    if lo * unit < lo0 - 1e-10 * unit {
        lo += 1.0;
    }
    if hi * unit > hi0 + 1e-10 * unit {
        hi -= 1.0;
    }
    let n = (hi - lo).max(1.0);
    (lo * unit, hi * unit, n)
}

pub unsafe fn do_plot_window(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    #[cfg(feature = "renderplot-device")]
    unsafe {
        crate::mainutils::portable_plot::draw_builtin("plot.window", args)
    }
    #[cfg(not(feature = "renderplot-device"))]
    unsafe {
        record_window_limits(args);
        crate::sexp::globals::R_NilValue()
    }
}

#[cfg(not(feature = "renderplot-device"))]
pub(super) unsafe fn record_window_limits(args: SEXP) {
    unsafe {
        let xlim = crate::mainutils::essentials::arg_by_name_or_position(args, &["xlim"], 0);
        let ylim = crate::mainutils::essentials::arg_by_name_or_position(args, &["ylim"], 1);
        let xv = numeric_plot_values(xlim);
        let yv = numeric_plot_values(ylim);
        if xv.len() < 2 || yv.len() < 2 {
            return;
        }
        let log_arg = crate::mainutils::essentials::arg_by_name_or_position(args, &["log"], 2);
        let log =
            if log_arg.is_null() || TYPEOF(log_arg) != SEXPTYPE::STRSXP || XLENGTH(log_arg) == 0 {
                String::new()
            } else {
                let chars = CHAR(STRING_ELT(log_arg, 0));
                if chars.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(chars)
                        .to_string_lossy()
                        .into_owned()
                }
            };
        let xlog = log.contains('x');
        let ylog = log.contains('y');
        let mut x_lo = xv[0];
        let mut x_hi = xv[1];
        let mut y_lo = yv[0];
        let mut y_hi = yv[1];
        let to_log = |v: f64| {
            if v > 0.0 {
                v.log10()
            } else {
                f64::NEG_INFINITY
            }
        };
        if xlog {
            x_lo = to_log(x_lo);
            x_hi = to_log(x_hi);
        }
        if ylog {
            y_lo = to_log(y_lo);
            y_hi = to_log(y_hi);
        }
        let finite_floor = (1.01 * f64::MIN_POSITIVE).log10();
        let clamp_log_end = |v: &mut f64, pull_subnormal: bool| {
            if *v == f64::INFINITY || *v >= 308.25035 {
                *v = (0.99 * f64::MAX).log10();
            } else if !v.is_finite()
                || 10f64.powf(*v) == 0.0
                || (pull_subnormal && *v < finite_floor)
            {
                *v = finite_floor;
            }
        };
        if xlog {
            clamp_log_end(&mut x_lo, false);
            clamp_log_end(&mut x_hi, false);
        }
        if ylog {
            clamp_log_end(&mut y_lo, false);
            clamp_log_end(&mut y_hi, false);
        }
        let style_i = |name: &str| {
            matches!(
                crate::library::graphics::par::parameter(name),
                crate::library::graphics::par::ParValue::String(s) if s == "i"
            )
        };
        let x_tight = style_i("xaxs");
        let y_tight = style_i("yaxs");
        if !x_tight {
            let extra = if x_hi.abs() > 100.0 || x_lo.abs() > 100.0 {
                0.04 * x_hi - 0.04 * x_lo
            } else {
                (x_hi - x_lo) * 0.04
            };
            if extra.is_finite() && extra != 0.0 {
                let a = x_lo - extra;
                let b = x_hi + extra;
                if a.is_finite() {
                    x_lo = a;
                }
                if b.is_finite() {
                    x_hi = b;
                }
            }
        }
        if !y_tight {
            let extra = if y_hi.abs() > 100.0 || y_lo.abs() > 100.0 {
                0.04 * y_hi - 0.04 * y_lo
            } else {
                (y_hi - y_lo) * 0.04
            };
            if extra.is_finite() && extra != 0.0 {
                let a = y_lo - extra;
                let b = y_hi + extra;
                if a.is_finite() {
                    y_lo = a;
                }
                if b.is_finite() {
                    y_hi = b;
                }
            }
        }
        if xlog {
            clamp_log_end(&mut x_lo, !x_tight);
            clamp_log_end(&mut x_hi, !x_tight);
        }
        if ylog {
            clamp_log_end(&mut y_lo, !y_tight);
            clamp_log_end(&mut y_hi, !y_tight);
        }
        use crate::library::graphics::par::{ParValue, set_plot_parameter};
        set_plot_parameter("usr", ParValue::Real(vec![x_lo, x_hi, y_lo, y_hi]));
        set_plot_parameter("xlog", ParValue::Logical(vec![if xlog { 1 } else { 0 }]));
        set_plot_parameter("ylog", ParValue::Logical(vec![if ylog { 1 } else { 0 }]));
        let log_axp = |lo0: f64, hi0: f64| {
            let (a, b) = if lo0 <= hi0 { (lo0, hi0) } else { (hi0, lo0) };
            let lo = a.ceil().clamp(-307.0, 308.0);
            let hi = b.floor().clamp(lo, 308.0);
            let n = if hi - lo <= 2.0 {
                3.0
            } else if hi - lo <= 3.0 {
                2.0
            } else {
                1.0
            };
            let (p0, p1) = if lo0 <= hi0 {
                (10f64.powf(lo), 10f64.powf(hi))
            } else {
                (10f64.powf(hi), 10f64.powf(lo))
            };
            (p0, p1, n)
        };
        if xlog {
            let (p0, p1, n) = log_axp(x_lo, x_hi);
            set_plot_parameter("xaxp", ParValue::Real(vec![p0, p1, n]));
        } else {
            let (xa0, xa1, xn) = pretty_axp(x_lo, x_hi);
            set_plot_parameter("xaxp", ParValue::Real(vec![xa0, xa1, xn]));
        }
        if ylog {
            let (p0, p1, n) = log_axp(y_lo, y_hi);
            set_plot_parameter("yaxp", ParValue::Real(vec![p0, p1, n]));
        } else {
            let (ya0, ya1, yn) = pretty_axp(y_lo, y_hi);
            set_plot_parameter("yaxp", ParValue::Real(vec![ya0, ya1, yn]));
        }
    }
}

macro_rules! portable_graphics_handlers {
    ($($handler:ident => $name:literal),* $(,)?) => {$(
        pub unsafe fn $handler(_call:SEXP,_op:SEXP,args:SEXP,_rho:SEXP)->SEXP {
            #[cfg(feature="renderplot-device")]
            unsafe {crate::mainutils::portable_plot::draw_builtin($name,args)}
            #[cfg(not(feature="renderplot-device"))]
            {let _=args; unsafe { crate::sexp::globals::R_NilValue() }}
        }
    )*};
}
portable_graphics_handlers! {
    do_lines_default=>"lines.default",do_points_default=>"points.default",
    do_segments=>"segments",do_arrows=>"arrows",do_polygon=>"polygon",
    do_text_default=>"text.default",do_title=>"title",do_box=>"box",
}

pub unsafe fn do_axis(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    #[cfg(feature = "renderplot-device")]
    unsafe {
        return crate::mainutils::portable_plot::draw_builtin("axis", args);
    }
    #[cfg(not(feature = "renderplot-device"))]
    unsafe {
        // GNU C_axis opens with GCheckState: a plot frame must exist.
        no_device_require_plot_frame();
        let mut side = 1.0;
        let mut cell = args;
        while !cell.is_null() && cell != crate::sexp::globals::R_NilValue() {
            let value = CAR(cell);
            if !value.is_null() && XLENGTH(value) == 1 {
                let v = if TYPEOF(value) == SEXPTYPE::INTSXP || TYPEOF(value) == SEXPTYPE::LGLSXP {
                    *INTEGER(value) as f64
                } else if TYPEOF(value) == SEXPTYPE::REALSXP {
                    *REAL(value)
                } else {
                    f64::NAN
                };
                if (1.0..=4.0).contains(&v) {
                    side = v;
                    break;
                }
            }
            cell = CDR(cell);
        }
        let name = if side == 1.0 || side == 3.0 {
            "xaxp"
        } else {
            "yaxp"
        };
        let axp = crate::library::graphics::par::parameter(name);
        let crate::library::graphics::par::ParValue::Real(a) = axp else {
            return crate::sexp::globals::R_NilValue();
        };
        if a.len() < 3 || !a[0].is_finite() || !a[1].is_finite() {
            return crate::sexp::globals::R_NilValue();
        }
        let steps = a[2].round().max(1.0).min(100.0) as usize;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, (steps + 1) as R_xlen_t);
        if result.is_null() {
            return crate::sexp::globals::R_NilValue();
        }
        let log_name = if side == 1.0 || side == 3.0 {
            "xlog"
        } else {
            "ylog"
        };
        let logged = matches!(
            crate::library::graphics::par::parameter(log_name),
            crate::library::graphics::par::ParValue::Logical(v) if v.first().copied() == Some(1)
        );
        if logged {
            let usr = crate::library::graphics::par::parameter("usr");
            let crate::library::graphics::par::ParValue::Real(u) = usr else {
                return crate::sexp::globals::R_NilValue();
            };
            let (lo0, hi0) = if side == 1.0 || side == 3.0 {
                (u[0], u[1])
            } else {
                (u[2], u[3])
            };
            let lo = (lo0.ceil() as i32).clamp(-307, 308);
            let hi = (hi0.floor() as i32).clamp(lo, 308);
            if hi >= lo {
                let n = (hi - lo + 1) as R_xlen_t;
                let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
                for i in 0..n {
                    *REAL(result).add(i as usize) = 10f64.powi(lo + i as i32);
                }
                return result;
            }
        }
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            *REAL(result).add(i) = a[0] * (1.0 - t) + a[1] * t;
        }
        result
    }
}

pub unsafe fn do_plot_new(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // No active device: GEcurrentDevice evaluates options("device").
    // A closure that opens nothing is an error (PR#15883).
    #[cfg(feature = "renderplot-device")]
    unsafe {
        crate::mainutils::portable_plot::draw_builtin("plot.new", args)
    }
    #[cfg(not(feature = "renderplot-device"))]
    {
        let _ = args;
        // GNU plot.new opens options("device") when nothing is active.
        // PR#15883: a closure that opens no device is an error, not success.
        unsafe {
            let _dev = crate::library::grdevices::device_registry::GEcurrentDevice();
        }
        crate::library::graphics::graphics::set_plot_new_state();
        unsafe { crate::sexp::globals::R_NilValue() }
    }
}

/// GNU `rect(...)` — no device: `plot.new has not been called yet`.
pub unsafe fn do_rect(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    #[cfg(feature = "renderplot-device")]
    unsafe {
        crate::mainutils::portable_plot::draw_builtin("rect", args)
    }
    #[cfg(not(feature = "renderplot-device"))]
    {
        let _ = args;
        no_device_require_plot_frame();
        unsafe { crate::sexp::globals::R_NilValue() }
    }
}

/// GNU `abline(...)` — no device: `plot.new has not been called yet`.
pub unsafe fn do_abline(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    #[cfg(feature = "renderplot-device")]
    unsafe {
        crate::mainutils::portable_plot::draw_builtin("abline", args)
    }
    #[cfg(not(feature = "renderplot-device"))]
    {
        let _ = args;
        no_device_require_plot_frame();
        unsafe { R_NilValue() }
    }
}

/// GNU's drawing builtins share one contract: `GCheckState` demands a plot
/// frame established by `plot.new` (the device itself auto-opens).
#[cfg(not(feature = "renderplot-device"))]
pub(super) fn no_device_require_plot_frame() {
    if !crate::library::graphics::graphics::plot_new_called() {
        crate::mainutils::errors::errorcall_str(
            unsafe { crate::mainutils::errors::R_getCurrentCall() },
            "plot.new has not been called yet",
        );
    }
}

macro_rules! graphics_generics {
    ($($handler:ident => $name:literal),* $(,)?) => {$(
        pub unsafe fn $handler(_call:SEXP,_op:SEXP,args:SEXP,rho:SEXP)->SEXP {
            unsafe {
                crate::mainutils::base_wrappers::apply($name,concat!("function(x,...) UseMethod('",$name,"')"),args,rho,false)
            }
        }
    )*};
}
graphics_generics! {do_lines=>"lines",do_points=>"points",do_text=>"text"}

#[cfg(all(test, not(feature = "renderplot-device")))]
mod log_axis_isolation_tests {
    use crate::sexp::session::RSession;

    fn run(session: &mut RSession, log: &str) -> String {
        let code = format!("invisible(plot(1:10, log='{log}')); cat(par('xlog'), par('ylog'), '\\n')");
        let (result, output, _) = session.eval_script_with_output_capture(&code);
        assert!(result.is_ok(), "plot failed: {output:?}");
        output.stdout
    }

    #[test]
    fn log_axis_choices_are_local_to_each_plot_and_session() {
        let mut left = RSession::new_for_gc_tests();
        let mut right = RSession::new_for_gc_tests();
        assert_eq!(run(&mut left, "x"), "TRUE FALSE \n");
        assert_eq!(run(&mut right, "y"), "FALSE TRUE \n");
        assert_eq!(run(&mut left, ""), "FALSE FALSE \n");
        assert_eq!(run(&mut right, "xy"), "TRUE TRUE \n");
    }
}
