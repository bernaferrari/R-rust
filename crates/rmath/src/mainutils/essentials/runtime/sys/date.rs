use super::*;

pub(crate) unsafe fn set_single_class(x: SEXP, class_name: &str) {
    unsafe {
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if class.is_null() {
            return;
        }
        let _guard = protect(class);
        let cstr = CString::new(class_name).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            SET_STRING_ELT(class, 0, charsxp);
        }
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_ClassSymbol(), class);
    }
}

pub(crate) unsafe fn set_posixct_class(x: SEXP, tz: &str) {
    unsafe {
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !class.is_null() {
            let _guard = protect(class);
            SET_STRING_ELT(class, 0, Rf_mkChar(c"POSIXct".as_ptr()));
            SET_STRING_ELT(class, 1, Rf_mkChar(c"POSIXt".as_ptr()));
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }

        let tz_cstr = CString::new(tz).unwrap_or_default();
        let tzone = Rf_mkString(tz_cstr.as_ptr());
        if !tzone.is_null() {
            crate::sexp::attrib_core::setAttrib(x, Rf_install(c"tzone".as_ptr()), tzone);
        }
    }
}

/// GNU `as.POSIXct(*, tz=)` stored tzone: missing/empty stays `""`.
pub(crate) fn posixct_tz_arg(args: SEXP) -> String {
    unsafe {
        let tz_arg = arg_by_name_or_position(args, &["tz"], 1);
        if tz_arg.is_null()
            || tz_arg == R_NilValue()
            || tz_arg == R_MissingArg()
            || XLENGTH(tz_arg) == 0
        {
            String::new()
        } else {
            elt_to_string(tz_arg, 0)
        }
    }
}

/// R's `as.Date(x, origin)` — coerce ISO date strings or day counts to Date.
pub unsafe fn do_as_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // GNU as.Date.default: inherits(x, "Date") returns x unchanged,
        // including integer-backed `.Date(nL)` objects.
        if sexp_has_class(x, "Date")
            && (TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(x) == SEXPTYPE::INTSXP)
        {
            return x;
        }

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        let out = REAL(result);

        let format_arg = arg_by_name_or_position(args, &["format"], 1);
        let format = if TYPEOF(format_arg) == SEXPTYPE::STRSXP && XLENGTH(format_arg) > 0 {
            let ch = STRING_ELT(format_arg, 0);
            if ch.is_null() {
                String::new()
            } else {
                CStr::from_ptr(CHAR(ch)).to_string_lossy().into_owned()
            }
        } else {
            String::new()
        };
        if TYPEOF(x) == SEXPTYPE::STRSXP {
            for i in 0..n {
                let value = STRING_ELT(x, i);
                let days = if value == crate::sexp::globals::R_NaString() {
                    NA_REAL
                } else {
                    let text = CStr::from_ptr(CHAR(value)).to_str().unwrap_or("");
                    if text.is_empty() {
                        NA_REAL
                    } else if text == "Inf" || text == "+Inf" {
                        f64::INFINITY
                    } else if text == "-Inf" {
                        f64::NEG_INFINITY
                    } else if text.eq_ignore_ascii_case("NaN") {
                        f64::NAN
                    } else if format == "%Y"
                        && text.len() == 4
                        && text.chars().all(|c| c.is_ascii_digit())
                    {
                        parse_iso_date_days(&format!("{text}-01-01")).unwrap_or(NA_REAL)
                    } else {
                        parse_iso_date_days(text).unwrap_or_else(|| {
                            base_error("character string is not in a standard unambiguous format")
                        })
                    }
                };
                *out.add(i as usize) = days;
            }
        } else if sexp_has_class(x, "POSIXct")
            && (TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(x) == SEXPTYPE::INTSXP)
        {
            for i in 0..n {
                let seconds = if TYPEOF(x) == SEXPTYPE::INTSXP {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    *REAL(x).add(i as usize)
                };
                // GNU as.Date.POSIXct (UTC fast path): floor(x/86400);
                // NaN stays NaN, only the NA bit pattern maps to NA.
                *out.add(i as usize) = if seconds.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    NA_REAL
                } else {
                    (seconds / 86_400.0).floor()
                };
            }
        } else if sexp_has_class(x, "POSIXlt") && TYPEOF(x) == SEXPTYPE::VECSXP && XLENGTH(x) >= 6 {
            let days = crate::mainutils::datetime::do_POSIXlt2D(
                _call,
                _op,
                Rf_cons(x, R_NilValue()),
                _rho,
            );
            let _d = protect(days);
            set_single_class(days, "Date");
            let year = VECTOR_ELT(x, 5);
            let year_names = crate::sexp::attrib_core::getAttrib(
                year,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            if !year_names.is_null() && year_names != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    days,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    year_names,
                );
            }
            return days;
        } else if TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(x) == SEXPTYPE::INTSXP {
            let origin = arg_by_name_or_position(args, &["origin"], 1);
            let origin_days = if origin.is_null() || origin == R_NilValue() {
                0.0
            } else {
                parse_iso_date_days(&elt_to_string(origin, 0))
                    .unwrap_or_else(|| base_error("'origin' must be a character string"))
            };
            for i in 0..n {
                let days = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    let v = *REAL(x).add(i as usize);
                    if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        NA_REAL
                    } else {
                        origin_days + v.floor()
                    }
                } else {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER {
                        NA_REAL
                    } else {
                        origin_days + f64::from(v)
                    }
                };
                *out.add(i as usize) = days;
            }
        } else if TYPEOF(x) == SEXPTYPE::LGLSXP {
            let mut all_na = true;
            for i in 0..n {
                if *INTEGER(x).add(i as usize) != NA_INTEGER {
                    all_na = false;
                    break;
                }
            }
            if !all_na {
                base_error("do not know how to convert 'x' to class \"Date\"");
            }
            for i in 0..n {
                *out.add(i as usize) = NA_REAL;
            }
        } else {
            base_error("do not know how to convert 'x' to class \"Date\"");
        }
        set_single_class(result, "Date");
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        result
    }
}

/// GNU `trunc.Date(x, units)`.
pub unsafe fn do_trunc_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let units = date_units_arg(args);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let days = date_days_elt(x, i);
            let out = if !days.is_finite() {
                days
            } else if units.starts_with("month") {
                date_first_of_month(days)
            } else if units.starts_with("year") {
                date_first_of_year(days)
            } else {
                days.floor()
            };
            *REAL(result).add(i as usize) = out;
        }
        set_single_class(result, "Date");
        result
    }
}

/// GNU `round.Date(x, units)`.
pub unsafe fn do_round_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let units = date_units_arg(args);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let days = date_days_elt(x, i);
            let out = if !days.is_finite() {
                days
            } else if units.starts_with("month") {
                let lo = date_first_of_month(days);
                let hi = date_add_months(lo, 1);
                if (hi - days) <= (days - lo) { hi } else { lo }
            } else if units.starts_with("year") {
                let lo = date_first_of_year(days);
                let hi = date_first_of_year(lo + 370.0);
                if (hi - days) <= (days - lo) { hi } else { lo }
            } else {
                days.round()
            };
            *REAL(result).add(i as usize) = out;
        }
        set_single_class(result, "Date");
        result
    }
}
