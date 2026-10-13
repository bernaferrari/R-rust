//! output — extracted verbatim from output.rs.

use super::*;

pub(crate) fn format_date_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_real_elt(i)
        .ok()
        .and_then(crate::mainutils::essentials::date_days_to_iso)
        .map(|value| format!("\"{}\"", escape_printed_string(&value)))
        .unwrap_or_else(|| "NA".to_string())
}

/// GNU `print.Date`: `print(format(x))` so width wrapping matches print.default.
pub(crate) fn format_date_vector(x: Sexp<'_>) -> String {
    format_date_vector_max(x, None)
}

pub(crate) fn format_date_vector_max(x: Sexp<'_>, max_override: Option<i64>) -> String {
    if x.clone().len() == 0 {
        return "Date of length 0".to_string();
    }
    unsafe {
        let n = x.clone().len();
        let opt_max = crate::mainutils::options::GetOptionMaxPrint() as i64;
        let max = max_override.unwrap_or(opt_max).max(0);
        let n_show = n.min(max as R_xlen_t);
        let formatted = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::STRSXP, n_show);
        if formatted.is_null() {
            return String::new();
        }
        let _g = crate::sexp::protect::protect(formatted);
        for i in 0..n_show {
            let days = x.clone().try_real_elt(i).ok().or_else(|| {
                x.clone().try_integer_elt(i).ok().and_then(|v| {
                    if v == crate::sexp::NA_INTEGER {
                        None
                    } else {
                        Some(v as f64)
                    }
                })
            });
            let text = days.and_then(crate::mainutils::essentials::date_days_to_iso);
            if let Some(text) = text {
                if text == "NA" {
                    crate::sexp::accessors::SET_STRING_ELT(
                        formatted,
                        i,
                        crate::sexp::globals::R_NaString(),
                    );
                } else {
                    let c = std::ffi::CString::new(text).unwrap_or_default();
                    crate::sexp::accessors::SET_STRING_ELT(
                        formatted,
                        i,
                        crate::sexp::constructors::Rf_mkChar(c.as_ptr()),
                    );
                }
            } else {
                crate::sexp::accessors::SET_STRING_ELT(
                    formatted,
                    i,
                    crate::sexp::globals::R_NaString(),
                );
            }
        }
        let sexp = Sexp::from_raw_unchecked(formatted);
        // GNU print.Date uses max+1 when truncating so print.default does
        // not emit a second omitted line.
        let print_max = if n_show < n { n_show as i64 + 1 } else { max };
        let mut out = format_vector_stock_n(sexp, true, Some(print_max));
        if n_show < n {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!(
                " [ reached 'max' / getOption(\"max.print\") -- omitted {} entries ]",
                n - n_show
            ));
        }
        out
    }
}

pub(crate) fn format_posixct_element(
    x: Sexp<'_>,
    i: R_xlen_t,
    include_tz: bool,
    force_time: bool,
) -> String {
    x.try_real_elt(i)
        .ok()
        .and_then(|seconds| {
            crate::mainutils::essentials::posix_seconds_to_iso_with_time(
                seconds, include_tz, force_time,
            )
        })
        .map(|value| format!("\"{}\"", escape_printed_string(&value)))
        .unwrap_or_else(|| "NA".to_string())
}

pub(crate) fn posixct_vector_needs_time(x: Sexp<'_>) -> bool {
    (0..x.clone().len()).any(|i| {
        x.clone().try_real_elt(i).ok().is_some_and(|seconds| {
            !R_IsNA(seconds) && seconds.is_finite() && seconds.floor() as i64 % 86_400 != 0
        })
    })
}

pub(crate) fn format_posixct_vector(x: Sexp<'_>, include_tz: bool) -> String {
    format_posixct_vector_max(x, include_tz, None)
}

pub(crate) fn format_posixct_vector_max(
    x: Sexp<'_>,
    include_tz: bool,
    max_override: Option<i64>,
) -> String {
    if x.clone().len() == 0 {
        return "POSIXct of length 0".to_string();
    }
    unsafe {
        use crate::sexp::constructors::{Rf_ScalarLogical, Rf_cons, Rf_mkString};
        use crate::sexp::ffi::TRUE;
        use crate::sexp::protect::protect;
        let n = x.clone().len();
        let opt_max = crate::mainutils::options::GetOptionMaxPrint() as i64;
        let max = max_override.unwrap_or(opt_max).max(0);
        let format = Rf_mkString(c"".as_ptr());
        let _fmt = protect(format);
        let usetz = Rf_ScalarLogical(if include_tz { TRUE } else { 0 });
        let _u = protect(usetz);
        let args = Rf_cons(
            x.as_raw(),
            Rf_cons(format, Rf_cons(R_NilValue(), Rf_cons(usetz, R_NilValue()))),
        );
        let _a = protect(args);
        let formatted = crate::mainutils::datetime::do_format_POSIXct(
            R_NilValue(),
            R_NilValue(),
            args,
            crate::sexp::globals::R_BaseEnv(),
        );
        if formatted.is_null() {
            return String::new();
        }
        let _f = protect(formatted);
        let n_show = XLENGTH(formatted).min(n.min(max as R_xlen_t));
        let sexp = Sexp::from_raw_unchecked(formatted);
        let print_max = if n_show < n { n_show as i64 + 1 } else { max };
        let mut out = format_vector_stock_n(sexp, true, Some(print_max));
        if n_show < n {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!(
                " [ reached 'max' / getOption(\"max.print\") -- omitted {} entries ]",
                n - n_show
            ));
        }
        out
    }
}

pub(crate) fn posixlt_time_length(x: Sexp<'_>) -> R_xlen_t {
    unsafe {
        let raw = x.as_raw();
        let ncomp = XLENGTH(raw);
        let mut n = 0;
        for i in 0..ncomp {
            let col = VECTOR_ELT(raw, i);
            if !col.is_null() {
                n = n.max(XLENGTH(col));
            }
        }
        n
    }
}

pub(crate) fn format_posixlt_vector(x: Sexp<'_>) -> String {
    let n = posixlt_time_length(x.clone());
    if n == 0 {
        return "POSIXlt of length 0".to_string();
    }
    unsafe {
        use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarLogical, Rf_cons, Rf_mkString};
        use crate::sexp::ffi::{NA_INTEGER, TRUE};
        use crate::sexp::protect::protect;
        let format = Rf_mkString(c"".as_ptr());
        let _fmt = protect(format);
        let usetz = Rf_ScalarLogical(TRUE);
        let _u = protect(usetz);
        let digits = Rf_ScalarInteger(NA_INTEGER);
        let _d = protect(digits);
        let args = Rf_cons(
            x.as_raw(),
            Rf_cons(format, Rf_cons(usetz, Rf_cons(digits, R_NilValue()))),
        );
        let _a = protect(args);
        let formatted = crate::mainutils::datetime::do_format_POSIXlt(
            R_NilValue(),
            R_NilValue(),
            args,
            crate::sexp::globals::R_BaseEnv(),
        );
        if formatted.is_null() {
            return String::new();
        }
        let _f = protect(formatted);
        format_vector_stock_n(Sexp::from_raw_unchecked(formatted), true, None)
    }
}

pub(crate) fn difftime_units(x: Sexp<'_>) -> String {
    unsafe {
        let units = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::symbol::Rf_install(c"units".as_ptr()),
        );
        if let Some(units) = Sexp::from_raw(units)
            && units.clone().typeof_() == SEXPTYPE::STRSXP
            && units.clone().len() > 0
            && let Some(Some(value)) = string_element_text(units, 0)
        {
            return value.to_string();
        }
        "secs".to_string()
    }
}

/// GNU `print.difftime` (base/R/datetime.R).
pub(crate) fn format_difftime_vector(x: Sexp<'_>) -> String {
    let units = difftime_units(x.clone());
    if x.clone().len() == 0 {
        return "difftime of length 0".to_string();
    }
    if x.clone().len() > 1 {
        // cat("Time differences in ", units, "\n") + print(unclassed values)
        let body = unsafe {
            let raw = crate::mainutils::duplicate::duplicate(x.as_raw());
            let _r = crate::sexp::protect::protect(raw);
            crate::sexp::attrib_core::setAttrib(
                raw,
                crate::sexp::attrib_core::R_ClassSymbol(),
                R_NilValue(),
            );
            crate::sexp::attrib_core::setAttrib(
                raw,
                crate::sexp::symbol::Rf_install(c"units".as_ptr()),
                R_NilValue(),
            );
            format_vector_stock(Sexp::from_raw_unchecked(raw), true)
        };
        return format!("Time differences in {units}\n{body}");
    }
    let value = x
        .try_real_elt(0)
        .map(format_real_value)
        .unwrap_or_else(format_access_error);
    format!("Time difference of {value} {units}")
}
