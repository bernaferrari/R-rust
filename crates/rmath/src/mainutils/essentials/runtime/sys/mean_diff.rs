use super::*;

/// GNU `mean.Date(x)`.
pub fn do_mean_Date(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let m = crate::eval::arithmetic::do_mean(call, op, args, rho);
        let _m = protect(m);
        set_single_class(m, "Date");
        m
    }
}

/// GNU `mean.POSIXct(x)`.
pub fn do_mean_POSIXct(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let tz = crate::sexp::attrib_core::getAttrib(
            x,
            install_str("tzone"),
        );
        let mut tz_s = "UTC".to_string();
        if !tz.is_null() && tz != R_NilValue() && TYPEOF(tz) == SEXPTYPE::STRSXP && XLENGTH(tz) > 0
        {
            let ch = STRING_ELT(tz, 0);
            if !ch.is_null() {
                tz_s = charsxp_text(ch).unwrap_or_default();
            }
        }
        let m = crate::eval::arithmetic::do_mean(call, op, args, rho);
        let _m = protect(m);
        set_posixct_class(m, &tz_s);
        m
    }
}

/// GNU `mean.POSIXlt(x)`.
pub fn do_mean_POSIXlt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let x = CAR(args);
    let ct = do_as_POSIXct(call, op, Rf_cons(x, R_NilValue()), rho);
    let _ct = protect(ct);
    let m = do_mean_POSIXct(call, op, Rf_cons(ct, R_NilValue()), rho);
    let _m = protect(m);
    crate::mainutils::datetime::do_as_POSIXlt(call, op, Rf_cons(m, R_NilValue()), rho)
}

/// GNU `diff.POSIXt(x)`.
pub fn do_diff_POSIXt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
    }
    let x = if crate::mainutils::objects::inherits2(x, c"POSIXlt") != 0 {
        do_as_POSIXct(_call, _op, Rf_cons(x, R_NilValue()), _rho)
    } else {
        x
    };
    let _x = protect(x);
    let lag = {
        let arg = crate::mainutils::essentials::shared::arg_by_name_or_position(args, &["lag"], 1);
        let v = if arg.is_null()
            || arg == R_NilValue()
            || arg == crate::sexp::globals::R_MissingArg()
        {
            1.0
        } else {
            crate::mainutils::essentials::shared::real_or_default(arg, 1.0)
        };
        if v < 1.0 { 1 } else { v as usize }
    };
    let differences = {
        let arg = crate::mainutils::essentials::shared::arg_by_name_or_position(
            args,
            &["differences"],
            2,
        );
        let v = if arg.is_null()
            || arg == R_NilValue()
            || arg == crate::sexp::globals::R_MissingArg()
        {
            1.0
        } else {
            crate::mainutils::essentials::shared::real_or_default(arg, 1.0)
        };
        if v < 1.0 { 1 } else { v as usize }
    };
    let n = XLENGTH(x);
    if n < 2 || n <= lag as R_xlen_t {
        let empty = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        let _e = protect(empty);
        set_single_class(empty, "difftime");
        crate::sexp::attrib_core::setAttrib(empty, install_str("units"), mk_string_cstr(c"secs"));
        return empty;
    }
    let seconds_at = |x: SEXP, i: usize| -> f64 {
        if TYPEOF(x) == SEXPTYPE::REALSXP {
            real_elt_checked(x, (i) as R_xlen_t)
        } else if TYPEOF(x) == SEXPTYPE::INTSXP {
            let v = integer_elt_checked(x, i as R_xlen_t);
            if v == crate::sexp::ffi::NA_INTEGER {
                crate::sexp::ffi::NA_REAL
            } else {
                v as f64
            }
        } else {
            0.0
        }
    };
    let mut z = Vec::with_capacity((n as usize).saturating_sub(lag));
    for i in 0..(n as usize - lag) {
        z.push(seconds_at(x, i + lag) - seconds_at(x, i));
    }
    let zz = z
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .map(|v| v.abs())
        .fold(f64::INFINITY, f64::min);
    let units = if !zz.is_finite() || zz < 60.0 {
        "secs"
    } else if zz < 3600.0 {
        "mins"
    } else if zz < 86400.0 {
        "hours"
    } else {
        "days"
    };
    let scale = match units {
        "mins" => 60.0,
        "hours" => 3600.0,
        "days" => 86_400.0,
        _ => 1.0,
    };
    for v in &mut z {
        *v /= scale;
    }
    for _ in 1..differences {
        if z.len() <= lag {
            z.clear();
            break;
        }
        let next: Vec<f64> = (0..z.len() - lag).map(|i| z[i + lag] - z[i]).collect();
        z = next;
    }
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, z.len() as i64);
    let _r = protect(result);
    for (i, v) in z.iter().enumerate() {
        set_real_checked(result, (i) as R_xlen_t, *v);
    }
    set_single_class(result, "difftime");
    crate::sexp::attrib_core::setAttrib(
        result,
        install_str("units"),
        mk_string_cstr(&CString::new(units).unwrap_or_default()),
    );
    result
}

/// GNU `trunc.POSIXt(x, units)`.
pub fn do_trunc_POSIXt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let x = if crate::mainutils::objects::inherits2(x, c"POSIXlt") != 0 {
            do_as_POSIXct(_call, _op, Rf_cons(x, R_NilValue()), _rho)
        } else {
            x
        };
        let _x = protect(x);
        let mut units = "secs".to_string();
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let u = CAR(rest);
            if TYPEOF(u) == SEXPTYPE::STRSXP && XLENGTH(u) > 0 {
                let ch = STRING_ELT(u, 0);
                if !ch.is_null() {
                    units = charsxp_text(ch).unwrap_or_default();
                }
            }
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let secs = if TYPEOF(x) == SEXPTYPE::REALSXP {
                real_elt_checked(x, (i as usize) as R_xlen_t)
            } else {
                0.0
            };
            let out = if !secs.is_finite() {
                secs
            } else if units.starts_with("min") {
                (secs / 60.0).floor() * 60.0
            } else if units.starts_with("hour") {
                (secs / 3600.0).floor() * 3600.0
            } else if units.starts_with("day") {
                (secs / 86_400.0).floor() * 86_400.0
            } else if units.starts_with("month") {
                date_first_of_month((secs / 86_400.0).floor()) * 86_400.0
            } else if units.starts_with("year") {
                date_first_of_year((secs / 86_400.0).floor()) * 86_400.0
            } else {
                secs.floor()
            };
            set_real_checked(result, (i as usize) as R_xlen_t, out);
        }
        set_posixct_class(result, "GMT");
        result
    
}

/// GNU `round.POSIXt(x, units)`.
pub fn do_round_POSIXt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return x;
    }
    let mut units = "secs".to_string();
    let rest = CDR(args);
    if !rest.is_null() && rest != R_NilValue() {
        let u = CAR(rest);
        if TYPEOF(u) == SEXPTYPE::STRSXP && XLENGTH(u) > 0 {
            let ch = STRING_ELT(u, 0);
            if !ch.is_null() {
                units = charsxp_text(ch).unwrap_or_default();
            }
        }
    }
    let x = if crate::mainutils::objects::inherits2(x, c"POSIXlt") != 0 {
        do_as_POSIXct(call, op, Rf_cons(x, R_NilValue()), rho)
    } else {
        x
    };
    let _x = protect(x);
    let half = if units.starts_with("min") {
        30.0
    } else if units.starts_with("hour") {
        1800.0
    } else if units.starts_with("day") {
        43200.0
    } else if units.starts_with("month") || units.starts_with("year") {
        0.0
    } else {
        0.5
    };
    let n = XLENGTH(x);
    let shifted = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    let _s = protect(shifted);
    for i in 0..n {
        let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
            real_elt_checked(x, (i as usize) as R_xlen_t)
        } else {
            0.0
        };
        set_real_checked(shifted, (i as usize) as R_xlen_t, v + half);
    }
    set_posixct_class(shifted, "GMT");
    let u = mk_string_cstr(&CString::new(units.as_str()).unwrap_or_default());
    let _u = protect(u);
    let truncated = do_trunc_POSIXt(call, op, Rf_cons(shifted, Rf_cons(u, R_NilValue())), rho);
    let _t = protect(truncated);
    crate::mainutils::datetime::do_as_POSIXlt(call, op, Rf_cons(truncated, R_NilValue()), rho)
}
