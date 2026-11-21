use super::*;

/// GNU `julian.Date(x, origin=as.Date("1970-01-01"))`.
pub fn do_julian(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let x = if crate::mainutils::objects::inherits2(x, c"POSIXlt") != 0 {
        do_as_POSIXct(_call, _op, Rf_cons(x, R_NilValue()), _rho)
    } else {
        x
    };
    let _x = protect(x);
    let posix = sexp_has_class(x, "POSIXct") || sexp_has_class(x, "POSIXt");
    let mut origin_val = 0.0;
    let rest = CDR(args);
    if !rest.is_null() && rest != R_NilValue() {
        let origin = CAR(rest);
        if !origin.is_null()
            && origin != R_NilValue()
            && TYPEOF(origin) == SEXPTYPE::REALSXP
            && XLENGTH(origin) > 0
        {
            origin_val = real_elt_checked(origin, 0);
        }
    }
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    let _r = protect(result);
    for i in 0..n {
        let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
            real_elt_checked(x, (i as usize) as R_xlen_t)
        } else if TYPEOF(x) == SEXPTYPE::INTSXP {
            let iv = integer_elt_checked(x, (i as usize) as R_xlen_t);
            if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
        } else {
            NA_REAL
        };
        set_real_checked(result, (i as usize) as R_xlen_t, if posix {
            (v - origin_val) / 86_400.0
        } else {
            v - origin_val
        });
    }
    let origin = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
    let _o = protect(origin);
    set_real_checked(origin, 0, origin_val);
    if posix {
        set_posixct_class(origin, "GMT");
        set_single_class(result, "difftime");
        crate::sexp::attrib_core::setAttrib(
            result,
            install_str("units"),
            mk_string_cstr(c"days"),
        );
    } else {
        set_single_class(origin, "Date");
    }
    crate::sexp::attrib_core::setAttrib(
        result,
        install_str("origin"),
        origin,
    );
    result
}

fn difftime_seconds(x: SEXP) -> f64 {
    
        if x.is_null() || x == R_NilValue() {
            return f64::NAN;
        }
        let v = if TYPEOF(x) == SEXPTYPE::REALSXP && XLENGTH(x) > 0 {
            real_elt_checked(x, 0)
        } else if TYPEOF(x) == SEXPTYPE::INTSXP && XLENGTH(x) > 0 {
            let iv = integer_elt_checked(x, 0);
            if iv == NA_INTEGER {
                return f64::NAN;
            }
            iv as f64
        } else {
            return f64::NAN;
        };
        if crate::mainutils::objects::inherits2(x, c"Date") != 0 {
            v * 86_400.0
        } else {
            v
        }
    
}

fn difftime_seconds_at(x: SEXP, i: i64) -> f64 {
    
        if x.is_null() || x == R_NilValue() || XLENGTH(x) == 0 {
            return f64::NAN;
        }
        let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
            real_elt_checked(x, (i as usize) as R_xlen_t)
        } else if TYPEOF(x) == SEXPTYPE::INTSXP {
            let iv = integer_elt_checked(x, (i as usize) as R_xlen_t);
            if iv == NA_INTEGER {
                return f64::NAN;
            }
            iv as f64
        } else {
            return f64::NAN;
        };
        if crate::mainutils::objects::inherits2(x, c"Date") != 0 {
            v * 86_400.0
        } else {
            v
        }
    
}

/// GNU `difftime(time1, time2, units="auto")`.
pub fn do_difftime(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let time1 = CAR(args);
    let time2 = CAR(CDR(args));
    let mut units = "auto".to_string();
    let mut cell = CDR(CDR(args));
    while !cell.is_null() && cell != R_NilValue() {
        let value = CAR(cell);
        let tag = TAG(cell);
        let named = if !tag.is_null() && tag != R_NilValue() {
            charsxp_text(PRINTNAME(tag)).unwrap_or_default()
        } else {
            String::new()
        };
        if named == "units" && TYPEOF(value) == SEXPTYPE::STRSXP && XLENGTH(value) > 0 {
            let ch = STRING_ELT(value, 0);
            if !ch.is_null() {
                units = charsxp_text(ch).unwrap_or_default();
            }
        }
        cell = CDR(cell);
    }
    let n1 = XLENGTH(time1).max(0);
    let n2 = XLENGTH(time2).max(0);
    let n = n1.max(n2);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    let _r = protect(result);
    let scale = match units.as_str() {
        "auto" => 0.0,
        "mins" => 60.0,
        "hours" => 3600.0,
        "days" => 86_400.0,
        "weeks" => 7.0 * 86_400.0,
        _ => 1.0,
    };
    // GNU difftime(units="auto") picks the unit from the SMALLEST
    // finite |z| (all-NA => secs): zz <- min(abs(z), na.rm = TRUE).
    let mut min_abs: Option<f64> = None;
    for i in 0..n {
        let a = difftime_seconds_at(time1, if n1 == 0 { 0 } else { i % n1 });
        let b = difftime_seconds_at(time2, if n2 == 0 { 0 } else { i % n2 });
        let z = a - b;
        if z.is_finite() {
            min_abs = Some(match min_abs {
                Some(current) => current.min(z.abs()),
                None => z.abs(),
            });
        }
        set_real_checked(result, (i as usize) as R_xlen_t, z);
    }
    let units = if units == "auto" {
        match min_abs {
            Some(zz) if zz.is_finite() && zz >= 60.0 => {
                if zz < 3600.0 {
                    "mins".to_string()
                } else if zz < 86400.0 {
                    "hours".to_string()
                } else {
                    "days".to_string()
                }
            }
            _ => "secs".to_string(),
        }
    } else {
        units
    };
    let div = if scale == 0.0 {
        match units.as_str() {
            "mins" => 60.0,
            "hours" => 3600.0,
            "days" => 86_400.0,
            "weeks" => 7.0 * 86_400.0,
            _ => 1.0,
        }
    } else {
        scale
    };
    if div != 1.0 {
        for i in 0..n {
            set_real_checked(result, (i as usize) as R_xlen_t, real_elt_checked(result, (i as usize) as R_xlen_t) / div);
        }
    }

    set_single_class(result, "difftime");
    let u = mk_string_cstr(&std::ffi::CString::new(units.as_str())
            .unwrap_or_default()
            );
    crate::sexp::attrib_core::setAttrib(
        result,
        install_str("units"),
        u,
    );
    result
}

/// GNU `as.difftime(tim, units)`.
pub fn do_as_difftime(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let tim = CAR(args);
    if crate::mainutils::objects::inherits2(tim, c"difftime") != 0 {
        return tim;
    }
    if TYPEOF(tim) == SEXPTYPE::STRSXP {
        let n = XLENGTH(tim);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let ch = STRING_ELT(tim, i as i64);
            let text = if ch.is_null() {
                String::new()
            } else {
                charsxp_text(ch).unwrap_or_default()
            };
            let parts: Vec<f64> = text
                .split(':')
                .filter_map(|p| p.parse::<f64>().ok())
                .collect();
            let secs = match parts.as_slice() {
                [h, m, s] => h * 3600.0 + m * 60.0 + s,
                [m, s] => m * 60.0 + s,
                [s] => *s,
                _ => NA_REAL,
            };
            set_real_checked(result, (i as usize) as R_xlen_t, secs);
        }
        // GNU as.difftime(<character>) routes through difftime(...,
        // units = "auto"): the unit is chosen from the SMALLEST finite
        // |z| (all-NA => secs), so e.g. c("0:3:20","11:23:15") prints
        // in mins, not secs.
        let mut min_abs: Option<f64> = None;
        for i in 0..n as usize {
            let z = real_elt_checked(result, (i) as R_xlen_t);
            if z.is_finite() {
                min_abs = Some(match min_abs {
                    Some(current) => current.min(z.abs()),
                    None => z.abs(),
                });
            }
        }
        let units = match min_abs {
            Some(zz) if zz.is_finite() && zz >= 60.0 => {
                if zz < 3600.0 {
                    "mins"
                } else if zz < 86400.0 {
                    "hours"
                } else {
                    "days"
                }
            }
            _ => "secs",
        };
        if units != "secs" {
            let div = match units {
                "mins" => 60.0,
                "hours" => 3600.0,
                "days" => 86_400.0,
                _ => 1.0,
            };
            for i in 0..n as usize {
                set_real_checked(result, (i) as R_xlen_t, real_elt_checked(result, (i) as R_xlen_t) / div);
            }
        }
        let u = mk_string_cstr(&std::ffi::CString::new(units).unwrap_or_default());
        crate::sexp::attrib_core::setAttrib(
            result,
            install_str("units"),
            u,
        );
        let class = mk_string_cstr(c"difftime");
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        return result;
    }
    let mut units = String::new();
    let mut cell = CDR(args);
    while !cell.is_null() && cell != R_NilValue() {
        let value = CAR(cell);
        let tag = TAG(cell);
        let named = if !tag.is_null() && tag != R_NilValue() {
            charsxp_text(PRINTNAME(tag)).unwrap_or_default()
        } else {
            String::new()
        };
        if named == "units" && TYPEOF(value) == SEXPTYPE::STRSXP && XLENGTH(value) > 0 {
            let ch = STRING_ELT(value, 0);
            if !ch.is_null() {
                units = charsxp_text(ch).unwrap_or_default();
            }
        }
        cell = CDR(cell);
    }
    if units.is_empty() || units == "auto" {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "need explicit units for numeric conversion",
        );
    }
    let n = XLENGTH(tim);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    let _r = protect(result);
    for i in 0..n {
        let v = if TYPEOF(tim) == SEXPTYPE::REALSXP {
            real_elt_checked(tim, (i as usize) as R_xlen_t)
        } else if TYPEOF(tim) == SEXPTYPE::INTSXP {
            let iv = integer_elt_checked(tim, (i as usize) as R_xlen_t);
            if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
        } else {
            NA_REAL
        };
        set_real_checked(result, (i as usize) as R_xlen_t, v);
    }
    let names =
        crate::sexp::attrib_core::getAttrib(tim, crate::sexp::attrib_core::R_NamesSymbol());
    if !names.is_null() && names != R_NilValue() {
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
    }
    set_single_class(result, "difftime");
    let u = mk_string_cstr(&CString::new(units.as_str()).unwrap_or_default());
    crate::sexp::attrib_core::setAttrib(
        result,
        install_str("units"),
        u,
    );
    result
}

/// GNU `units(x)` is attr(x, "units").
pub fn do_units(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::attrib_core::getAttrib(
        CAR(args),
        install_str("units"),
    )
}
