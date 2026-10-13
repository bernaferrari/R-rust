use super::*;

fn unix_secs_to_utc(secs: i64) -> crate::tzone_strftime::stm {
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let yday = if m > 2 { doy - 59 } else { doy + 306 };
    crate::tzone_strftime::stm {
        tm_sec: (sod % 60) as i32,
        tm_min: ((sod / 60) % 60) as i32,
        tm_hour: (sod / 3600) as i32,
        tm_mday: d as i32,
        tm_mon: (m as i32) - 1,
        tm_year: y as i32 - 1900,
        tm_wday: ((days + 4).rem_euclid(7)) as i32,
        tm_yday: yday as i32,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: b"GMT\0".as_ptr() as *const std::os::raw::c_char,
    }
}

/// GNU `strftime(x, format, tz)` is `format(as.POSIXlt(x, tz), format)`.
pub unsafe fn do_strftime(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let mut format = R_NilValue();
        let mut tz = R_NilValue();
        let mut cell = CDR(args);
        let mut pos = 0usize;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "format" || (name.is_empty() && pos == 0) {
                format = CAR(cell);
            } else if name == "tz" || (name.is_empty() && pos == 1) {
                tz = CAR(cell);
            }
            if name.is_empty() {
                pos += 1;
            }
            cell = CDR(cell);
        }
        let lt_args = if tz.is_null() || tz == R_NilValue() {
            Rf_cons(x, R_NilValue())
        } else {
            Rf_cons(x, Rf_cons(tz, R_NilValue()))
        };
        let _la = protect(lt_args);
        let lt = crate::mainutils::datetime::do_as_POSIXlt(call, op, lt_args, rho);
        let _lt = protect(lt);
        let fmt_args = if format.is_null() || format == R_NilValue() {
            Rf_cons(lt, R_NilValue())
        } else {
            let cell = Rf_cons(format, R_NilValue());
            SETTAG(cell, Rf_install(c"format".as_ptr()));
            Rf_cons(lt, cell)
        };
        let _fa = protect(fmt_args);
        crate::mainutils::datetime::do_format_POSIXlt(call, op, fmt_args, rho)
    }
}

/// GNU `format.Date(x, format="%Y-%m-%d")`.
pub unsafe fn do_format_Date(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mut fmt = "%Y-%m-%d".to_string();
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let f = CAR(rest);
            if TYPEOF(f) == SEXPTYPE::STRSXP && XLENGTH(f) > 0 {
                let ch = STRING_ELT(f, 0);
                if !ch.is_null() {
                    let s = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                    if !s.is_empty() {
                        fmt = s;
                    }
                }
            }
        }
        let fmt_s = Rf_mkString(CString::new(fmt.as_str()).unwrap_or_default().as_ptr());
        let _f = protect(fmt_s);
        do_strftime(call, op, Rf_cons(x, Rf_cons(fmt_s, R_NilValue())), rho)
    }
}

/// GNU `as.character.Date` is `as.character(as.POSIXlt(x))` — no names.
pub unsafe fn do_as_character_Date(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let out = do_format_Date(call, op, args, rho);
        let _o = protect(out);
        crate::sexp::attrib_core::setAttrib(
            out,
            crate::sexp::attrib_core::R_NamesSymbol(),
            R_NilValue(),
        );
        out
    }
}

pub(crate) fn date_level_string(days: f64) -> String {
    let tm = unix_secs_to_utc((days * 86_400.0) as i64);
    crate::tzone_strftime::strftime_safe("%Y-%m-%d", &tm).unwrap_or_default()
}
