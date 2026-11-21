use super::*;

fn iso_arg_num(x: SEXP, default: f64) -> f64 {
    
        if x.is_null() || x == R_NilValue() {
            return default;
        }
        if TYPEOF(x) == SEXPTYPE::REALSXP && XLENGTH(x) > 0 {
            real_elt_checked(x, 0)
        } else if TYPEOF(x) == SEXPTYPE::INTSXP && XLENGTH(x) > 0 {
            let v = integer_elt_checked(x, 0);
            if v == NA_INTEGER { default } else { v as f64 }
        } else {
            default
        }
    
}

fn iso_posixct(year: f64, month: f64, day: f64, hour: f64, min: f64, sec: f64, tz: &str) -> SEXP {
    unsafe {
        if !(1.0..=12.0).contains(&month) {
            let na = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
            set_real_checked(na, 0, f64::NAN);
            let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
            SET_STRING_ELT(class, 0, mk_char_cstr(c"POSIXct"));
            SET_STRING_ELT(class, 1, mk_char_cstr(c"POSIXt"));
            crate::sexp::attrib_core::setAttrib(
                na,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
            let tzone = Rf_mkString(CString::new(tz).unwrap_or_default().as_ptr());
            crate::sexp::attrib_core::setAttrib(
                na,
                install_str("tzone"),
                tzone,
            );
            return na;
        }
        let stamp = format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:09.6}",
            year as i32, month as i32, day as i32, hour as i32, min as i32, sec
        );
        let text = Rf_mkString(CString::new(stamp.as_str()).unwrap_or_default().as_ptr());
        let _t = protect(text);
        let tz_s = Rf_mkString(CString::new(tz).unwrap_or_default().as_ptr());
        let _z = protect(tz_s);
        let args = Rf_cons(text, Rf_cons(tz_s, R_NilValue()));
        let _a = protect(args);
        SETTAG(CDR(args), install_str("tz"));
        do_as_POSIXct(R_NilValue(), R_NilValue(), args, R_NilValue())
    }
}

/// GNU `ISOdatetime(year, month, day, hour, min, sec, tz)`.
pub fn do_ISOdatetime(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    
        let year = iso_arg_num(CAR(args), 1970.0);
        let month = iso_arg_num(CAR(CDR(args)), 1.0);
        let day = iso_arg_num(CAR(CDR(CDR(args))), 1.0);
        let hour = iso_arg_num(CAR(CDR(CDR(CDR(args)))), 0.0);
        let min = iso_arg_num(CAR(CDR(CDR(CDR(CDR(args))))), 0.0);
        let sec = iso_arg_num(CAR(CDR(CDR(CDR(CDR(CDR(args)))))), 0.0);
        let mut tz = String::new();
        let cell = CDR(CDR(CDR(CDR(CDR(CDR(args))))));
        if !cell.is_null() && cell != R_NilValue() {
            let t = CAR(cell);
            if TYPEOF(t) == SEXPTYPE::STRSXP && XLENGTH(t) > 0 {
                let ch = STRING_ELT(t, 0);
                if !ch.is_null() {
                    tz = charsxp_text(ch).unwrap_or_default();
                }
            }
        }
        iso_posixct(year, month, day, hour, min, sec, &tz)
    
}

/// GNU `ISOdate(year, month, day)` is noon GMT.
pub fn do_ISOdate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    
        let year = iso_arg_num(CAR(args), 1970.0);
        let month = iso_arg_num(CAR(CDR(args)), 1.0);
        let day = iso_arg_num(CAR(CDR(CDR(args))), 1.0);
        let mut hour = 12.0;
        let mut min = 0.0;
        let mut sec = 0.0;
        let mut tz = "GMT".to_string();
        let mut cell = CDR(CDR(CDR(args)));
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let named = if !tag.is_null() && tag != R_NilValue() {
                charsxp_text(PRINTNAME(tag)).unwrap_or_default()
            } else {
                String::new()
            };
            if named == "hour" || (named.is_empty() && pos == 0) {
                hour = iso_arg_num(value, 12.0);
            } else if named == "min" || (named.is_empty() && pos == 1) {
                min = iso_arg_num(value, 0.0);
            } else if named == "sec" || (named.is_empty() && pos == 2) {
                sec = iso_arg_num(value, 0.0);
            } else if named == "tz" || (named.is_empty() && pos == 3) {
                if TYPEOF(value) == SEXPTYPE::STRSXP && XLENGTH(value) > 0 {
                    let ch = STRING_ELT(value, 0);
                    if !ch.is_null() {
                        tz = charsxp_text(ch).unwrap_or_default();
                    }
                }
            }
            if named.is_empty() {
                pos += 1;
            }
            cell = CDR(cell);
        }
        iso_posixct(year, month, day, hour, min, sec, &tz)
    
}
