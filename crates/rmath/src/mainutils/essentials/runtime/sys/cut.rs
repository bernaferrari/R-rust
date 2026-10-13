use super::*;

/// GNU `cut.Date(x, breaks)` for week/month/year.
pub unsafe fn do_cut_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let breaks = CAR(CDR(args));
        let numeric_breaks = !breaks.is_null()
            && breaks != R_NilValue()
            && (TYPEOF(breaks) == SEXPTYPE::REALSXP || TYPEOF(breaks) == SEXPTYPE::INTSXP)
            && XLENGTH(breaks) == 1;
        if numeric_breaks {
            let days = crate::mainutils::duplicate::duplicate(x);
            let _d = protect(days);
            crate::sexp::attrib_core::setAttrib(
                days,
                crate::sexp::attrib_core::R_ClassSymbol(),
                R_NilValue(),
            );
            let right_val = Rf_ScalarLogical(0);
            let _r = protect(right_val);
            let call_args = Rf_cons(
                days,
                Rf_cons(
                    breaks,
                    Rf_cons(R_NilValue(), Rf_cons(right_val, R_NilValue())),
                ),
            );
            let res = crate::mainutils::essentials::sets::do_cut(_call, _op, call_args, _rho);
            let _res = protect(res);
            let codes = INTEGER(res);
            let nlev = {
                let lev = crate::sexp::attrib_core::getAttrib(
                    res,
                    crate::sexp::attrib_core::R_LevelsSymbol(),
                );
                if lev.is_null() || lev == R_NilValue() {
                    0
                } else {
                    XLENGTH(lev)
                }
            };
            let labels = Rf_allocVector3(SEXPTYPE::STRSXP, nlev);
            let _lab = protect(labels);
            for lev in 0..nlev {
                let mut label = String::from("NA");
                for i in 0..n {
                    if *codes.add(i as usize) == (lev as i32) + 1 {
                        label = crate::mainutils::essentials::shared::date_days_to_iso(
                            date_days_elt(x, i),
                        )
                        .unwrap_or_else(|| "NA".to_string());
                        break;
                    }
                }
                SET_STRING_ELT(
                    labels,
                    lev,
                    Rf_mkChar(CString::new(label).unwrap().as_ptr()),
                );
            }
            crate::sexp::attrib_core::setAttrib(
                res,
                crate::sexp::attrib_core::R_LevelsSymbol(),
                labels,
            );
            return res;
        }
        let mut days: Vec<f64> = Vec::with_capacity(n as usize);
        for i in 0..n {
            days.push(date_days_elt(x, i));
        }
        let mut units = "days".to_string();
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let b = CAR(rest);
            if TYPEOF(b) == SEXPTYPE::STRSXP && XLENGTH(b) > 0 {
                let ch = STRING_ELT(b, 0);
                if !ch.is_null() {
                    units = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
        // GNU: "3 months" is step 3 of unit months.
        let (step, units) = if let Some((n, rest)) = units.split_once(' ') {
            match n.parse::<i32>() {
                Ok(k) if k > 0 => (k, rest.to_string()),
                _ => (1, units),
            }
        } else {
            (1, units)
        };
        let finite: Vec<f64> = days.iter().copied().filter(|d| d.is_finite()).collect();
        if finite.is_empty() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let min_d = finite.iter().copied().fold(f64::INFINITY, f64::min);
        let max_d = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mut breaks: Vec<f64> = Vec::new();
        if units.starts_with("week") {
            let w = ((min_d as i64 + 4).rem_euclid(7)) as i32;
            let off = if w == 0 { 6 } else { w - 1 };
            let mut b = min_d as i64 - off as i64;
            while (b as f64) <= max_d {
                breaks.push(b as f64);
                b += 7 * step as i64;
            }
            breaks.push(b as f64);
        } else if units.starts_with("month") {
            let mut b = date_first_of_month(min_d);
            while b <= max_d {
                breaks.push(b);
                b = date_add_months(b, step);
            }
            breaks.push(b);
        } else if units.starts_with("year") {
            let mut b = date_first_of_year(min_d);
            while b <= max_d {
                breaks.push(b);
                b = date_add_months(b, 12 * step);
            }
            breaks.push(b);
        } else if units.starts_with("quarter") {
            let mut b = date_first_of_quarter(min_d);
            while b <= max_d {
                breaks.push(b);
                b = date_add_months(b, 3 * step);
            }
            breaks.push(b);
        } else if units.starts_with("day") {
            let mut b = min_d.floor();
            while b <= max_d {
                breaks.push(b);
                b += step as f64;
            }
            breaks.push(b);
        } else {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid specification of 'breaks'",
            );
        }
        if breaks.len() < 2 {
            breaks.push(max_d + 1.0);
        }
        let nlev = breaks.len() - 1;
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        let _r = protect(result);
        for i in 0..n as usize {
            let d = days[i];
            let code = if !d.is_finite() {
                NA_INTEGER
            } else {
                let mut c = NA_INTEGER;
                for k in 0..nlev {
                    if d >= breaks[k] && d < breaks[k + 1] {
                        c = (k as i32) + 1;
                        break;
                    }
                }
                c
            };
            *INTEGER(result).add(i) = code;
        }
        let levels_vec = Rf_allocVector3(SEXPTYPE::STRSXP, nlev as i64);
        let _l = protect(levels_vec);
        for k in 0..nlev {
            let s = date_level_string(breaks[k]);
            let c = CString::new(s).unwrap_or_default();
            SET_STRING_ELT(levels_vec, k as i64, Rf_mkChar(c.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_LevelsSymbol(),
            levels_vec,
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"factor".as_ptr()),
        );
        result
    }
}

/// GNU `cut.POSIXt(x, breaks)` for day/week/month/year/quarter.
pub unsafe fn do_cut_POSIXt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let x = if crate::mainutils::objects::inherits2(x, c"POSIXlt".as_ptr()) != 0 {
            do_as_POSIXct(call, op, Rf_cons(x, R_NilValue()), rho)
        } else {
            x
        };
        let _x = protect(x);
        let n = XLENGTH(x);
        let breaks = CAR(CDR(args));
        let numeric_breaks = !breaks.is_null()
            && breaks != R_NilValue()
            && (TYPEOF(breaks) == SEXPTYPE::REALSXP || TYPEOF(breaks) == SEXPTYPE::INTSXP)
            && XLENGTH(breaks) == 1;
        if numeric_breaks {
            let secs = crate::mainutils::duplicate::duplicate(x);
            let _s = protect(secs);
            crate::sexp::attrib_core::setAttrib(
                secs,
                crate::sexp::attrib_core::R_ClassSymbol(),
                R_NilValue(),
            );
            let mut right = false;
            let mut cell = CDR(CDR(args));
            while !cell.is_null() && cell != R_NilValue() {
                let tag = TAG(cell);
                if !tag.is_null() && tag != R_NilValue() {
                    let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag))).to_string_lossy();
                    if name == "right" {
                        right = crate::mainutils::coerce::asLogical(CAR(cell)) == 1;
                    }
                }
                cell = CDR(cell);
            }
            let right_val = Rf_ScalarLogical(if right { 1 } else { 0 });
            let _r = protect(right_val);
            let call_args = Rf_cons(
                secs,
                Rf_cons(
                    breaks,
                    Rf_cons(R_NilValue(), Rf_cons(right_val, R_NilValue())),
                ),
            );
            let res = crate::mainutils::essentials::sets::do_cut(call, op, call_args, rho);
            let _res = protect(res);
            let codes = INTEGER(res);
            let nlev = {
                let lev = crate::sexp::attrib_core::getAttrib(
                    res,
                    crate::sexp::attrib_core::R_LevelsSymbol(),
                );
                if lev.is_null() || lev == R_NilValue() {
                    0
                } else {
                    XLENGTH(lev)
                }
            };
            let labels = Rf_allocVector3(SEXPTYPE::STRSXP, nlev);
            let _lab = protect(labels);
            for lev in 0..nlev {
                let mut label = String::from("NA");
                for i in 0..n {
                    if *codes.add(i as usize) == (lev as i32) + 1 {
                        let secs = if TYPEOF(x) == SEXPTYPE::INTSXP {
                            let v = *INTEGER(x).add(i as usize);
                            if v == NA_INTEGER { NA_REAL } else { v as f64 }
                        } else {
                            *REAL(x).add(i as usize)
                        };
                        let day = (secs / 86_400.0).floor();
                        label = crate::mainutils::essentials::shared::date_days_to_iso(day)
                            .unwrap_or_else(|| "NA".to_string());
                        break;
                    }
                }
                SET_STRING_ELT(
                    labels,
                    lev,
                    Rf_mkChar(std::ffi::CString::new(label).unwrap().as_ptr()),
                );
            }
            crate::sexp::attrib_core::setAttrib(
                res,
                crate::sexp::attrib_core::R_LevelsSymbol(),
                labels,
            );
            return res;
        }
        if TYPEOF(breaks) == SEXPTYPE::STRSXP && XLENGTH(breaks) >= 1 {
            let ch = STRING_ELT(breaks, 0);
            let text = if ch.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned()
            };
            let mut parts = text.split_whitespace();
            let first = parts.next().unwrap_or("");
            let (count, unit) = if let Some(unit) = parts.next() {
                (first.parse::<f64>().unwrap_or(0.0), unit)
            } else {
                (1.0, first)
            };
            let mult = if unit.starts_with("hour") {
                3600.0
            } else if unit.starts_with("min") {
                60.0
            } else if unit.starts_with("sec") {
                1.0
            } else {
                0.0
            };
            if mult > 0.0 && count > 0.0 {
                let step = count * mult;
                let mut secs = Vec::with_capacity(n as usize);
                for i in 0..n {
                    secs.push(if TYPEOF(x) == SEXPTYPE::INTSXP {
                        let v = *INTEGER(x).add(i as usize);
                        if v == NA_INTEGER { NA_REAL } else { v as f64 }
                    } else {
                        *REAL(x).add(i as usize)
                    });
                }
                let min = secs
                    .iter()
                    .copied()
                    .filter(|v| v.is_finite())
                    .fold(f64::INFINITY, f64::min);
                let start = if min.is_finite() {
                    (min / step).floor() * step
                } else {
                    0.0
                };
                let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
                let mut nlev = 0i32;
                for i in 0..n as usize {
                    let code = if !secs[i].is_finite() {
                        NA_INTEGER
                    } else {
                        let c = ((secs[i] - start) / step).floor() as i32 + 1;
                        if c > nlev {
                            nlev = c;
                        }
                        c
                    };
                    *INTEGER(result).add(i) = code;
                }
                let labels = Rf_allocVector3(SEXPTYPE::STRSXP, nlev as i64);
                for k in 0..nlev {
                    let label = format!("{}", start + (k as f64) * step);
                    SET_STRING_ELT(
                        labels,
                        k as i64,
                        Rf_mkChar(std::ffi::CString::new(label).unwrap().as_ptr()),
                    );
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_LevelsSymbol(),
                    labels,
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_ClassSymbol(),
                    Rf_mkString(c"factor".as_ptr()),
                );
                return result;
            }
        }
        let n = XLENGTH(x);
        let days = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _d = protect(days);
        for i in 0..n {
            let secs = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else {
                0.0
            };
            *REAL(days).add(i as usize) = (secs / 86_400.0).floor();
        }
        set_single_class(days, "Date");
        do_cut_Date(call, op, Rf_cons(days, CDR(args)), rho)
    }
}

pub(crate) fn date_units_arg(args: SEXP) -> String {
    unsafe {
        let mut units = "days".to_string();
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let named = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if (named == "units" || named.is_empty())
                && TYPEOF(value) == SEXPTYPE::STRSXP
                && XLENGTH(value) > 0
            {
                let ch = STRING_ELT(value, 0);
                if !ch.is_null() {
                    units = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                }
                break;
            }
            cell = CDR(cell);
        }
        units
    }
}

pub(crate) fn date_days_elt(x: SEXP, i: i64) -> f64 {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if TYPEOF(x) == SEXPTYPE::INTSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        }
    }
}

fn date_from_ymd(year: i32, month: i32, day: i32) -> f64 {
    let mut tm = crate::mainutils::datetime::stm::new();
    tm.tm_year = year - 1900;
    tm.tm_mon = month - 1;
    tm.tm_mday = day;
    tm.tm_hour = 0;
    tm.tm_min = 0;
    tm.tm_sec = 0;
    crate::mainutils::datetime::mkdate00(&mut tm)
}

fn date_civil(days: f64) -> Option<(i32, i32, i32)> {
    let mut tm = crate::mainutils::datetime::stm::new();
    if crate::mainutils::datetime::julian2dtime(days, &mut tm) {
        Some((tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday))
    } else {
        None
    }
}

pub(crate) fn date_first_of_month(days: f64) -> f64 {
    match date_civil(days) {
        Some((y, m, _)) => date_from_ymd(y, m, 1),
        None => days,
    }
}

fn date_first_of_quarter(days: f64) -> f64 {
    match date_civil(days) {
        Some((y, m, _)) => date_from_ymd(y, (m - 1) / 3 * 3 + 1, 1),
        None => days,
    }
}

pub(crate) fn date_first_of_year(days: f64) -> f64 {
    match date_civil(days) {
        Some((y, _, _)) => date_from_ymd(y, 1, 1),
        None => days,
    }
}

pub(crate) fn date_add_months(days: f64, add: i32) -> f64 {
    match date_civil(days) {
        Some((y, m, _)) => {
            let mut y = y;
            let mut m = m + add;
            while m > 12 {
                m -= 12;
                y += 1;
            }
            while m < 1 {
                m += 12;
                y -= 1;
            }
            date_from_ymd(y, m, 1)
        }
        None => days,
    }
}
