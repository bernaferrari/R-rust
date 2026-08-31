#![allow(unused_imports)]
use super::*;
use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::sexp::accessors::{
    CADDDR, CADDR, CADR, CAR, CDDDR, CDDR, CDR, CHAR, COMPLEX, INTEGER, LENGTH, LOGICAL, PRINTNAME,
    RAW, REAL, SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT,
    XLENGTH, translateChar,
};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector, Rf_allocVector3, Rf_isInteger, Rf_isNull,
    Rf_isReal, Rf_isVector, Rf_length, Rf_mkChar, Rf_mkString,
};
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_FINITE, R_xlen_t, SEXP};
use crate::sexp::globals::{R_MissingArg, R_NilValue};

// datetime_seq: Date / POSIXct support for seq(), mirroring stock R's S3
// methods seq.Date() and seq.POSIXt() (src/library/base/R/dates.R / dateTime.R).
// Returns Some(result) when either endpoint carries a datetime class,
// None otherwise so the plain numeric path runs.
// ---------------------------------------------------------------------------

pub unsafe fn first_str_elt(x: SEXP) -> Option<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != STRSXP_VAL || LENGTH(x) == 0 {
            return None;
        }
        let s = STRING_ELT(x, 0);
        if s.is_null() || s == crate::sexp::globals::R_NaString() {
            return None;
        }
        Some(
            CStr::from_ptr(translateChar(s))
                .to_string_lossy()
                .into_owned(),
        )
    }
}

pub unsafe fn datetime_class_of(x: SEXP) -> Option<DatetimeKind> {
    unsafe {
        if crate::mainutils::essentials::sexp_has_class(x, "POSIXct") {
            Some(DatetimeKind::Posixct)
        } else if crate::mainutils::essentials::sexp_has_class(x, "Date") {
            Some(DatetimeKind::Date)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum DatetimeKind {
    Date,
    Posixct,
}

/// pmatch(x, table) for a single string: an exact match wins, otherwise a
/// unique prefix match; ambiguity or no match is NA (stock pmatch).
pub fn pmatch_one(x: &str, table: &[&str]) -> Option<usize> {
    if let Some(i) = table.iter().position(|t| *t == x) {
        return Some(i);
    }
    let mut matches = table.iter().enumerate().filter(|(_, t)| t.starts_with(x));
    match (matches.next(), matches.next()) {
        (Some((i, _)), None) => Some(i),
        _ => None,
    }
}

/// as.integer() on a character multiplier: parse as a double and truncate
/// toward zero (as.integer("1.5") == 1L).  Non-numeric or out-of-range
/// strings give NA with stock's coercion warning.
pub unsafe fn as_integer_multiplier(call: SEXP, s: &str) -> Option<i64> {
    unsafe {
        let warn = |msg: &str| {
            let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
            warningcall(call, c_msg.as_ptr());
        };
        let t = s.trim();
        if t.is_empty() {
            warn("NAs introduced by coercion");
            return None;
        }
        match t.parse::<c_double>() {
            Ok(v) if v.is_finite() && v >= INT_MIN_C && v <= INT_MAX_C => Some(v as i64),
            Ok(_) => {
                // Numeric but outside the integer range: stock's distinct
                // "coercion to integer range" warning.
                warn("NAs introduced by coercion to integer range");
                None
            }
            Err(_) => {
                warn("NAs introduced by coercion");
                None
            }
        }
    }
}

/// strsplit(s, " ", fixed=TRUE): split on every single space, dropping the
/// trailing empty strings R's strsplit discards.
pub fn split_by_spaces(s: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = s.split(' ').collect();
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    parts
}

/// Which POSIXlt field a calendar `by` steps (seq.POSIXt months/years/
/// DSTdays handling).
#[derive(Clone, Copy, PartialEq)]
pub enum CalendarField {
    Months,
    Years,
    Dstdays,
}

/// UTC POSIXlt-style fields of an epoch value.  The runtime models
/// Date/POSIXct in UTC, which is also what stock uses for Date endpoints
/// (as.POSIXlt.Date is UTC midnight).
pub fn posixlt_fields(secs: c_double) -> Option<(i64, i64, i64, c_double)> {
    if secs.to_bits() == NA_REAL.to_bits() || !secs.is_finite() {
        return None;
    }
    let frac = secs - secs.floor();
    let whole = secs.floor() as i64;
    let days = whole.div_euclid(86_400);
    let tod = whole.rem_euclid(86_400) as c_double + frac;
    let (y, m, d) = crate::mainutils::essentials::civil_from_days(days);
    Some((y, m - 1, d, tod))
}

/// mktime-style recomposition: month and day overflow normalizes by
/// rolling into later months (linear civil-day arithmetic, like mktime).
pub fn mktime_utc(year: i64, mon0: i64, mday: i64, tod: c_double) -> c_double {
    let y = year + mon0.div_euclid(12);
    let m = mon0.rem_euclid(12) + 1;
    crate::mainutils::essentials::days_from_civil(y, m, mday) as c_double * 86_400.0 + tod
}

/// seq.int(from, to, by) over exact integers: returns the number of values
/// (from, from+by, ... <= to for by > 0), applying stock's error checks.
pub unsafe fn calendar_count(call: SEXP, from: i64, to: i64, by: i64) -> i64 {
    unsafe {
        if by == 0 {
            if from == to {
                return 1;
            }
            errorcall(
                call,
                b"invalid '(to - from)/by'\0".as_ptr() as *const c_char,
            );
        }
        let del = to - from;
        if del != 0 && (del > 0) != (by > 0) {
            errorcall(
                call,
                b"wrong sign in 'by' argument\0".as_ptr() as *const c_char,
            );
        }
        del / by + 1
    }
}

pub unsafe fn attach_datetime_class(ans: SEXP, kind: DatetimeKind, tz_source: SEXP) -> SEXP {
    unsafe {
        match kind {
            DatetimeKind::Date => {
                crate::mainutils::essentials::set_single_class(ans, "Date");
            }
            DatetimeKind::Posixct => {
                let tz = crate::mainutils::essentials::posixct_tzone_string(tz_source);
                crate::mainutils::essentials::set_posixct_class(ans, &tz);
            }
        }
        ans
    }
}

/// Calendar stepping for by = "months"/"quarters"/"years"/"DSTdays"
/// (seq.POSIXt's POSIXlt arithmetic, which seq.Date delegates to).
pub unsafe fn calendar_seq(
    call: SEXP,
    field: CalendarField,
    mult: i64,
    vanchor: c_double,
    vother: c_double,
    miss_to: bool,
    miss_from: bool,
    lout: R_xlen_t,
) -> Vec<c_double> {
    unsafe {
        // Anchor fields (lres <- as.POSIXlt(if from given from else to)).
        let Some((year, mon0, mday, tod)) = posixlt_fields(vanchor) else {
            // NA anchor: the from+to modes filter everything out; the
            // length.out modes propagate NA fields (seq.int on NA).
            return if miss_to || miss_from {
                vec![c_double::NAN; lout.max(0) as usize]
            } else {
                Vec::new()
            };
        };

        // Value at integer step k from the anchor.
        let value_at = |k: i64| -> c_double {
            match field {
                CalendarField::Months | CalendarField::Years => {
                    let mon_step = if field == CalendarField::Months {
                        mult
                    } else {
                        12 * mult
                    };
                    let mon_abs = (year * 12 + mon0) + k * mon_step;
                    mktime_utc(0, mon_abs, mday, tod)
                }
                CalendarField::Dstdays => mktime_utc(year, mon0, mday + k * mult, tod),
            }
        };

        if miss_to || miss_from {
            // length.out mode: exactly lout values anchored at the given
            // endpoint (seq.int(to/from = <field>, by = by, length.out)).
            let n = lout.max(0) as i64;
            return (0..n)
                .map(|i| {
                    if miss_from {
                        value_at(i - (n - 1))
                    } else {
                        value_at(i)
                    }
                })
                .collect();
        }

        // from + to + by: seq.int(<field>, <target field>, by) then keep
        // values not past `to` (seq.POSIXt's res[res <= cto] filter, which
        // drops a final month whose day-overflow passes the endpoint).
        let mut values: Vec<c_double> = if field == CalendarField::Dstdays {
            // "We might have a short day, so need to over-estimate":
            // length.out = 2 + floor((cto - cfrom)/(by * 86400)).
            if mult == 0 {
                errorcall(
                    call,
                    b"invalid '(to - from)/by'\0".as_ptr() as *const c_char,
                );
            }
            let span = (vother - vanchor) / (mult as c_double * 86_400.0);
            let n_est = 2.0 + span.floor();
            let n_est = if n_est.is_finite() && n_est >= 0.0 {
                n_est as i64
            } else {
                0
            };
            (0..n_est).map(value_at).collect()
        } else {
            let Some((to_year, to_mon0, _, _)) = posixlt_fields(vother) else {
                return Vec::new();
            };
            let count = if field == CalendarField::Years {
                calendar_count(call, year, to_year, mult)
            } else {
                calendar_count(call, year * 12 + mon0, to_year * 12 + to_mon0, mult)
            };
            (0..count).map(value_at).collect()
        };
        if mult > 0 {
            values.retain(|v| *v <= vother);
        } else {
            values.retain(|v| *v >= vother);
        }
        values
    }
}

pub unsafe fn datetime_seq(
    call: SEXP,
    from: SEXP,
    to: SEXP,
    by: SEXP,
    lout: R_xlen_t,
    miss_from: bool,
    miss_to: bool,
) -> Option<SEXP> {
    unsafe {
        // Classify endpoints.  The leading endpoint's class wins when the
        // two differ, mirroring UseMethod dispatch on the first argument of
        // the seq.Date / seq.POSIXt S3 methods.
        let kind: DatetimeKind = {
            let kf = if miss_from {
                None
            } else {
                datetime_class_of(from)
            };
            let kt = if miss_to { None } else { datetime_class_of(to) };
            match (kf, kt) {
                (Some(k), _) | (_, Some(k)) => k,
                _ => return None,
            }
        };

        let have_lout = lout != NA_INTEGER as R_xlen_t;
        let by_given = by != R_MissingArg() && by != R_NilValue();

        // seq.POSIXt: "exactly three of 'to', 'from', 'by' and
        // 'length.out' / 'along.with' must be specified", then the class /
        // length-1 checks on the supplied endpoints ('to' first).
        if kind == DatetimeKind::Posixct {
            let missing_count =
                miss_from as u32 + miss_to as u32 + (!have_lout) as u32 + (!by_given) as u32;
            if missing_count != 1 {
                errorcall(
                    call,
                    b"exactly three of 'to', 'from', 'by' and 'length.out' / 'along.with' must be specified\0"
                        .as_ptr() as *const c_char,
                );
            }
            if !miss_to {
                if !crate::mainutils::essentials::sexp_has_class(to, "POSIXct") {
                    errorcall(
                        call,
                        b"'to' must be a \"POSIXt\" object\0".as_ptr() as *const c_char,
                    );
                }
                if LENGTH(to) != 1 {
                    errorcall(
                        call,
                        b"'to' must be of length 1\0".as_ptr() as *const c_char,
                    );
                }
            }
            if !miss_from {
                if !crate::mainutils::essentials::sexp_has_class(from, "POSIXct") {
                    errorcall(
                        call,
                        b"'from' must be a \"POSIXt\" object\0".as_ptr() as *const c_char,
                    );
                }
                if LENGTH(from) != 1 {
                    errorcall(
                        call,
                        b"'from' must be of length 1\0".as_ptr() as *const c_char,
                    );
                }
            }
        } else if !by_given && (miss_from || miss_to) && !have_lout {
            // seq.Date without 'by'.
            errorcall(
                call,
                b"without 'by', when one of 'to', 'from' is missing, 'length.out' / 'along.with' must be specified\0"
                    .as_ptr() as *const c_char,
            );
        }

        // 'by' handling -----------------------------------------------------
        // Linear step in native units (days for Date, seconds for POSIXct).
        let mut rby: c_double = 1.0;
        let mut calendar: Option<(CalendarField, i64)> = None;
        if by_given {
            if LENGTH(by) != 1 {
                errorcall(
                    call,
                    b"'by' must be of length 1\0".as_ptr() as *const c_char,
                );
            }
            if kind == DatetimeKind::Date {
                let missing_count = miss_from as u32 + miss_to as u32 + (!have_lout) as u32;
                if missing_count != 1 {
                    errorcall(
                        call,
                        b"given 'by', exactly two of 'to', 'from' and 'length.out' / 'along.with' must be specified\0"
                            .as_ptr() as *const c_char,
                    );
                }
            }
            if TYPEOF(by) == STRSXP_VAL {
                // strsplit(by, " ", fixed = TRUE); an NA string gives NA
                // fields, so pmatch returns NA ("invalid string for 'by'").
                let text = match first_str_elt(by) {
                    Some(t) => t,
                    None => errorcall_never(call, "invalid string for 'by'"),
                };
                let parts = split_by_spaces(&text);
                if parts.is_empty() || parts.len() > 2 {
                    errorcall(call, b"invalid 'by' string\0".as_ptr() as *const c_char);
                }
                let last = parts[parts.len() - 1];
                let table: &[&str] = if kind == DatetimeKind::Date {
                    &["days", "weeks", "months", "quarters", "years"]
                } else {
                    &[
                        "secs", "mins", "hours", "days", "weeks", "months", "years", "DSTdays",
                        "quarters",
                    ]
                };
                // pmatch: unique prefix or exact; ambiguous (e.g. "m" for
                // POSIXct: mins/months) is NA -> "invalid string for 'by'".
                let valid = match pmatch_one(last, table) {
                    Some(v) => v,
                    None => errorcall_never(call, "invalid string for 'by'"),
                };
                let mult: i64 = if parts.len() == 2 {
                    match as_integer_multiplier(call, parts[0]) {
                        Some(m) => m,
                        None => errorcall_never(call, "'by' is NA"),
                    }
                } else {
                    1
                };

                if kind == DatetimeKind::Date {
                    match valid {
                        0 => rby = mult as c_double,
                        1 => rby = 7.0 * mult as c_double,
                        2 => calendar = Some((CalendarField::Months, mult)),
                        3 => calendar = Some((CalendarField::Months, 3 * mult)),
                        _ => calendar = Some((CalendarField::Years, mult)),
                    }
                } else {
                    match valid {
                        0 => rby = mult as c_double,
                        1 => rby = 60.0 * mult as c_double,
                        2 => rby = 3600.0 * mult as c_double,
                        3 => rby = 86_400.0 * mult as c_double,
                        4 => rby = 7.0 * 86_400.0 * mult as c_double,
                        5 => calendar = Some((CalendarField::Months, mult)),
                        6 => calendar = Some((CalendarField::Years, mult)),
                        7 => calendar = Some((CalendarField::Dstdays, mult)),
                        _ => calendar = Some((CalendarField::Months, 3 * mult)),
                    }
                }
            } else if TYPEOF(by) == REALSXP_VAL || TYPEOF(by) == INTSXP_VAL {
                rby = asReal(by);
                if ISNAN(rby) {
                    errorcall(call, b"'by' is NA\0".as_ptr() as *const c_char);
                }
            } else {
                errorcall(call, b"invalid mode for 'by'\0".as_ptr() as *const c_char);
            }
        }

        // Endpoints as raw numbers (days for Date, seconds for POSIXct).
        let vfrom = if miss_from {
            c_double::NAN
        } else {
            asReal(from)
        };
        let vto = if miss_to { c_double::NAN } else { asReal(to) };

        let build = |first: c_double, step: c_double, n: usize| -> Vec<c_double> {
            (0..n).map(|i| first + i as c_double * step).collect()
        };

        let values: Vec<c_double> = if let Some((field, mult)) = calendar {
            // Calendar arithmetic runs in epoch seconds (seq.POSIXt's
            // POSIXlt path, which seq.Date delegates to at UTC midnight);
            // Date endpoints are day values.
            let (anchor, other) = if miss_from {
                (vto, c_double::NAN)
            } else {
                (vfrom, vto)
            };
            let (anchor, other) = if kind == DatetimeKind::Date {
                (anchor * 86_400.0, other * 86_400.0)
            } else {
                (anchor, other)
            };
            let secs = calendar_seq(call, field, mult, anchor, other, miss_to, miss_from, lout);
            if kind == DatetimeKind::Date {
                secs.into_iter().map(|s| s / 86_400.0).collect()
            } else {
                secs
            }
        } else if miss_to {
            // from + (by|length.out): step forward from `from`.
            build(vfrom, rby, lout.max(0) as usize)
        } else if miss_from {
            // to + (by|length.out): step backward from `to`.
            let n = lout.max(0) as usize;
            let start = vto - (n as c_double - 1.0) * rby;
            build(start, rby, n)
        } else if have_lout {
            // from + to + length.out (or no 'by'): linear interpolation.
            let n = lout.max(0) as usize;
            if n == 0 {
                Vec::new()
            } else if n == 1 {
                vec![vfrom]
            } else {
                let step = (vto - vfrom) / (n as c_double - 1.0);
                build(vfrom, step, n)
            }
        } else if by_given {
            // from + to + by: seq.int(from, to, by) semantics.
            let del = vto - vfrom;
            let n = del / rby;
            if !n.is_finite() {
                errorcall(
                    call,
                    b"invalid '(to - from)/by'\0".as_ptr() as *const c_char,
                );
            }
            if n > 100.0 * INT_MAX_C {
                errorcall(
                    call,
                    b"'by' argument is much too small\0".as_ptr() as *const c_char,
                );
            }
            if n < -FEPS {
                errorcall(
                    call,
                    b"wrong sign in 'by' argument\0".as_ptr() as *const c_char,
                );
            }
            let nn = (n + FEPS) as i64;
            build(vfrom, rby, (nn + 1) as usize)
        } else {
            // Date from:to without 'by' (seq.int(from, to) colon steps by
            // one day in either direction).
            let del = vto - vfrom;
            if del == 0.0 {
                vec![vfrom]
            } else {
                let step = if del > 0.0 { 1.0 } else { -1.0 };
                let n = del.abs() as usize + 1;
                build(vfrom, step, n)
            }
        };

        // Emit the result vector --------------------------------------------
        let ans = Rf_allocVector(REALSXP_VAL, values.len() as c_int);
        let ra = REAL(ans);
        for (i, v) in values.iter().enumerate() {
            *ra.add(i) = *v;
        }
        Some(attach_datetime_class(
            ans,
            kind,
            if miss_from { to } else { from },
        ))
    }
}
