use super::*;

/// R's `as.POSIXct(x, tz, origin)` — coerce simple UTC inputs to POSIXct.
pub unsafe fn do_as_POSIXct(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            // GNU as.POSIXct.default: NULL -> .POSIXct(integer(), tz).
            let out = Rf_allocVector3(SEXPTYPE::INTSXP, 0);
            let _o = protect(out);
            set_posixct_class(out, &posixct_tz_arg(args));
            return out;
        }
        if sexp_has_class(x, "POSIXct")
            && (TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(x) == SEXPTYPE::INTSXP)
        {
            let tz_arg = arg_by_name_or_position(args, &["tz"], 1);
            if !tz_arg.is_null()
                && tz_arg != R_NilValue()
                && tz_arg != R_MissingArg()
                && TYPEOF(tz_arg) == SEXPTYPE::STRSXP
                && XLENGTH(tz_arg) > 0
            {
                let tz = elt_to_string(tz_arg, 0);
                if !tz.is_empty() {
                    let out = crate::mainutils::duplicate::Rf_duplicate(x);
                    let _o = protect(out);
                    set_posixct_class(out, &tz);
                    return out;
                }
            }
            return x;
        }

        if sexp_has_class(x, "POSIXlt") && TYPEOF(x) == SEXPTYPE::VECSXP && XLENGTH(x) >= 6 {
            let tz_arg = arg_by_name_or_position(args, &["tz"], 1);
            let mut tz = if tz_arg.is_null() || tz_arg == R_NilValue() || XLENGTH(tz_arg) == 0 {
                String::new()
            } else {
                elt_to_string(tz_arg, 0)
            };
            // GNU as.POSIXct.POSIXlt: the tzone attribute only fills in a
            // MISSING tz; an explicit tz = "" means "local zone". The
            // builtin sees evaluated args, so inspect the call's tags.
            let tz_supplied = {
                let mut cell = _call;
                let mut found = false;
                while !cell.is_null() && cell != R_NilValue() {
                    if TYPEOF(cell) == SEXPTYPE::LANGSXP || TYPEOF(cell) == SEXPTYPE::LISTSXP {
                        let tag = TAG(cell);
                        if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP
                        {
                            let name =
                                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag))).to_string_lossy();
                            if name == "tz" {
                                found = true;
                                break;
                            }
                        }
                        cell = CDR(cell);
                    } else {
                        break;
                    }
                }
                found
            };
            if tz.is_empty() && !tz_supplied {
                let attr = crate::sexp::attrib_core::getAttrib(
                    x,
                    crate::sexp::symbol::Rf_install(c"tzone".as_ptr()),
                );
                if !attr.is_null()
                    && attr != R_NilValue()
                    && TYPEOF(attr) == SEXPTYPE::STRSXP
                    && XLENGTH(attr) > 0
                {
                    tz = elt_to_string(attr, 0);
                }
            }
            let result = crate::mainutils::datetime::convert_posixlt_to_posixct(x, &tz);
            let _r = protect(result);
            // GNU .POSIXct(y, tz): an empty tz means "local zone", not UTC.
            set_posixct_class(result, &tz);
            // List names are sec/min/hour. Observation names sit on year.
            let names = if XLENGTH(x) >= 6 {
                crate::sexp::attrib_core::getAttrib(
                    VECTOR_ELT(x, 5),
                    crate::sexp::attrib_core::R_NamesSymbol(),
                )
            } else {
                R_NilValue()
            };
            if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && XLENGTH(names) > 0
            {
                // Pad with NA to the object length; the result names then
                // recycle like GNU's.
                let n = XLENGTH(result);
                let padded = Rf_allocVector3(SEXPTYPE::STRSXP, n);
                let _p = protect(padded);
                for j in 0..n {
                    let s = if j < XLENGTH(names) {
                        STRING_ELT(names, j)
                    } else {
                        crate::sexp::globals::R_NaString()
                    };
                    SET_STRING_ELT(padded, j, s);
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    padded,
                );
            }

            return result;
        }
        if sexp_has_class(x, "Date")
            && (TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(x) == SEXPTYPE::INTSXP)
        {
            // GNU as.POSIXct.Date <- function(x, tz = "UTC", ...)
            // .POSIXct(unclass(x)*86400, tz=tz)
            // Missing tz is UTC even when TZ is set; explicit tz="" stays "".
            let tz_arg = arg_by_name_or_position(args, &["tz"], 1);
            let tz_missing = tz_arg.is_null()
                || tz_arg == R_NilValue()
                || tz_arg == R_MissingArg()
                || XLENGTH(tz_arg) == 0;
            let tz = if tz_missing {
                "UTC".to_string()
            } else {
                elt_to_string(tz_arg, 0)
            };
            let n = XLENGTH(x);
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
            let _guard = protect(result);
            let out = REAL(result);
            for i in 0..n {
                let days = date_days_elt(x, i);
                *out.add(i as usize) = if days.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    NA_REAL
                } else {
                    days.floor() * 86_400.0
                };
            }
            set_posixct_class(result, &tz);
            let names =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            if !names.is_null() && names != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
            return result;
        }

        let tz_arg = arg_by_name_or_position(args, &["tz"], 1);
        let tz_missing = tz_arg.is_null()
            || tz_arg == R_NilValue()
            || tz_arg == R_MissingArg()
            || XLENGTH(tz_arg) == 0;
        let tz = if tz_missing {
            crate::tzone::timezone_override()
                .or_else(|| std::env::var("TZ").ok())
                .unwrap_or_default()
        } else {
            elt_to_string(tz_arg, 0)
        };

        if TYPEOF(x) == SEXPTYPE::STRSXP {
            let tz_s = Rf_mkString(CString::new(tz.as_str()).unwrap_or_default().as_ptr());
            let _tz = protect(tz_s);
            let lt = crate::mainutils::datetime::do_as_POSIXlt(
                _call,
                _op,
                Rf_cons(x, Rf_cons(tz_s, R_NilValue())),
                _rho,
            );
            let _lt = protect(lt);
            let result = crate::mainutils::datetime::convert_posixlt_to_posixct(lt, &tz);
            let _r = protect(result);
            set_posixct_class(result, &tz);
            let names =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            if !names.is_null() && names != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
            return result;
        }

        let origin = arg_by_name_or_position(args, &["origin"], 2);
        let origin_missing = origin.is_null() || origin == R_NilValue() || origin == R_MissingArg();
        if !sexp_has_class(x, "Date")
            && (TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(x) == SEXPTYPE::INTSXP)
            && origin_missing
        {
            // GNU as.POSIXct.numeric: missing origin keeps x's type.
            let out = crate::mainutils::duplicate::Rf_duplicate(x);
            let _o = protect(out);
            set_posixct_class(out, &posixct_tz_arg(args));
            return out;
        }

        let tz = if tz.is_empty() { "UTC".to_string() } else { tz };

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        let out = REAL(result);

        if TYPEOF(x) == SEXPTYPE::LGLSXP {
            // GNU as.POSIXct.default: logical all-NA -> numeric POSIXct.
            for i in 0..n {
                if *INTEGER(x).add(i as usize) != NA_INTEGER {
                    base_error("do not know how to convert 'x' to class \"POSIXct\"");
                }
                *out.add(i as usize) = NA_REAL;
            }
        } else if TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(x) == SEXPTYPE::INTSXP {
            let origin_seconds = if origin_missing {
                0.0
            } else if sexp_has_class(origin, "POSIXct") && TYPEOF(origin) == SEXPTYPE::REALSXP {
                let v = *REAL(origin);
                if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    NA_REAL
                } else {
                    v
                }
            } else {
                parse_iso_datetime_seconds(&elt_to_string(origin, 0))
                    .or_else(|| {
                        parse_iso_date_days(&elt_to_string(origin, 0)).map(|days| days * 86_400.0)
                    })
                    .unwrap_or_else(|| base_error("'origin' must be a character string"))
            };

            for i in 0..n {
                let seconds = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    let v = *REAL(x).add(i as usize);
                    if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        NA_REAL
                    } else {
                        origin_seconds + v
                    }
                } else {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER {
                        NA_REAL
                    } else {
                        origin_seconds + f64::from(v)
                    }
                };
                *out.add(i as usize) = seconds;
            }
        } else {
            base_error("do not know how to convert 'x' to class \"POSIXct\"");
        }

        set_posixct_class(result, &tz);
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

unsafe fn posixlt_as_posixct(call: SEXP, op: SEXP, x: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_as_POSIXct(call, op, Rf_cons(x, R_NilValue()), rho) }
}

/// GNU `is.na.POSIXlt <- function(x) is.na(as.POSIXct(x))`
pub unsafe fn do_is_na_POSIXlt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let ct = posixlt_as_posixct(call, op, CAR(args), rho);
        let _ct = protect(ct);
        crate::mainutils::essentials::do_is_na(call, op, Rf_cons(ct, R_NilValue()), rho)
    }
}

/// GNU `is.nan.POSIXlt <- function(x) is.nan(as.POSIXct(x))`
pub unsafe fn do_is_nan_POSIXlt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let ct = posixlt_as_posixct(call, op, CAR(args), rho);
        let _ct = protect(ct);
        crate::mainutils::essentials::do_is_nan(call, op, Rf_cons(ct, R_NilValue()), rho)
    }
}

/// GNU `is.finite.POSIXlt <- function(x) is.finite(as.POSIXct(x))`
pub unsafe fn do_is_finite_POSIXlt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let ct = posixlt_as_posixct(call, op, CAR(args), rho);
        let _ct = protect(ct);
        crate::mainutils::essentials::do_is_finite(call, op, Rf_cons(ct, R_NilValue()), rho)
    }
}

/// GNU `is.infinite.POSIXlt <- function(x) is.infinite(as.POSIXct(x))`
pub unsafe fn do_is_infinite_POSIXlt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let ct = posixlt_as_posixct(call, op, CAR(args), rho);
        let _ct = protect(ct);
        crate::mainutils::essentials::do_is_infinite(call, op, Rf_cons(ct, R_NilValue()), rho)
    }
}
