use super::*;

pub(crate) fn use_dig_secs(secs: &[f64], digits: i32) -> i32 {
    let np = digits.min(6);
    if np < 1 {
        return 0;
    }
    let finite: Vec<f64> = secs.iter().copied().filter(|s| s.is_finite()).collect();
    if finite.is_empty() {
        return np;
    }
    for i in 0..np {
        let ti = 10f64.powi(i);
        if finite
            .iter()
            .all(|&s| (s - (s * ti).trunc() / ti).abs() < 1e-6)
        {
            return i;
        }
    }
    np
}

fn expand_os_format(
    fmt: &str,
    secs: f64,
    fsecs: f64,
    tm_sec: i32,
    digits: i32,
    ns0: &mut i32,
) -> String {
    let Some(pos) = fmt.find("%OS") else {
        return fmt.to_string();
    };
    let after = pos + 3;
    let next = fmt.as_bytes().get(after).copied();
    let (ns, nused) = match next {
        Some(b) if b.is_ascii_digit() => ((b - b'0') as i32, 4),
        _ => {
            if *ns0 < 0 {
                *ns0 = if digits == NA_INTEGER { 0 } else { digits };
            }
            (*ns0, 3)
        }
    };
    let ns = ns.clamp(0, 6);
    let mut out = String::new();
    out.push_str(&fmt[..pos]);
    if ns > 0 {
        let s = tm_sec as f64 + (secs - fsecs);
        let t = 10f64.powi(ns);
        let s = ((s * t) as i32) as f64 / t;
        out.push_str(&format!(
            "{s:0width$.prec$}",
            width = (ns + 3) as usize,
            prec = ns as usize
        ));
        out.push_str(&fmt[pos + nused..]);
    } else {
        out.push_str("%S");
        out.push_str(&fmt[pos + nused..]);
    }
    out
}

pub(crate) unsafe fn format_posix_named_arg(args: SEXP, name: &str, pos: usize) -> SEXP {
    unsafe {
        let mut cell = CDR(args);
        let mut i = 0usize;
        let mut positional = R_NilValue();
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let tagged = if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP
            {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if tagged == name {
                return CAR(cell);
            }
            if tagged.is_empty() {
                if i == pos {
                    positional = CAR(cell);
                }
                i += 1;
            }
            cell = CDR(cell);
        }
        positional
    }
}

/// GNU-style matching: exact names, unique prefixes, then positionals.
/// Bind `args` to `names` through GNU `matchArgs` (exact, partial, positional).
pub(crate) unsafe fn match_named_then_positional(
    call: SEXP,
    args: SEXP,
    formals: &[&str],
) -> Vec<SEXP> {
    unsafe { crate::mainutils::match_mod::match_formal_slots(call, args, formals) }
}

// ---------------------------------------------------------------------------
// do_formatPOSIXlt -- .Internal(format.POSIXlt(x, format, usetz, ...))
// ---------------------------------------------------------------------------

unsafe fn posixlt_usetz_zone(
    x: SEXP,
    tzone: SEXP,
    i: usize,
    isdst: c_int,
    nn: c_int,
) -> Option<String> {
    unsafe {
        let charsxp_str = |s: SEXP, idx: R_xlen_t| -> Option<String> {
            if s.is_null() || TYPEOF(s) != SEXPTYPE::STRSXP || XLENGTH(s) <= 0 {
                return None;
            }
            let n = XLENGTH(s);
            let ch = STRING_ELT(s, idx % n);
            if ch.is_null() || ch == R_NaString() {
                return None;
            }
            let p = CHAR(ch);
            if p.is_null() {
                return None;
            }
            let t = CStr::from_ptr(p).to_string_lossy();
            if t.is_empty() {
                None
            } else {
                Some(t.into_owned())
            }
        };
        // GNU datetime.c: have_zone = LENGTH(x) >= 10. When the $zone
        // component exists, UseTZ uses only that (empty → no suffix);
        // tzone is the fallback solely when have_zone is false.
        if nn >= 10 {
            return charsxp_str(VECTOR_ELT(x, 9), i as R_xlen_t);
        }
        if tzone.is_null() || tzone == R_NilValue() || TYPEOF(tzone) != SEXPTYPE::STRSXP {
            return None;
        }
        let ntz = XLENGTH(tzone);
        if ntz <= 0 {
            return None;
        }
        let idx = if ntz == 3 {
            if isdst > 0 {
                2
            } else if isdst == 0 {
                1
            } else {
                0
            }
        } else {
            0
        };
        charsxp_str(tzone, idx.min(ntz - 1))
    }
}

/// Format a POSIXlt object as a character string using strftime.
///
/// Ported from `do_formatPOSIXlt()` in datetime.c.
pub unsafe fn do_formatPOSIXlt(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let _x_guard = protect(x);

        // x must be VECSXP with at least 9 components
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            std::panic::panic_any(RError {
                message: "invalid 'x' argument".to_string(),
            });
        }

        // Get format string (CADR(args))
        let sformat = CADR(args);
        if TYPEOF(sformat) != SEXPTYPE::STRSXP || XLENGTH(sformat) == 0 {
            std::panic::panic_any(RError {
                message: "invalid 'format' argument".to_string(),
            });
        }

        // Determine lengths
        let nn = std::cmp::min(LENGTH(x), 11);
        let mut nlen = [0i64; 11];
        let mut n: R_xlen_t = 0;
        for i in 0..nn {
            let len = XLENGTH(VECTOR_ELT(x, i as R_xlen_t));
            nlen[i as usize] = len;
            if len > n {
                n = len;
            }
        }

        let m = XLENGTH(sformat);
        let N = if n > 0 { std::cmp::max(m, n) } else { 0 };
        let digits_arg = CADDDR(args);
        let digits = if digits_arg.is_null() || digits_arg == R_NilValue() {
            NA_INTEGER
        } else {
            crate::mainutils::coerce::asInteger(digits_arg)
        };
        let usetz = {
            let u = CADDR(args);
            !u.is_null() && u != R_NilValue() && crate::mainutils::coerce::asLogical(u) == TRUE
        };
        let tzone = getAttrib(x, Rf_install(c"tzone".as_ptr()));
        // GNU: if the format includes %Z or %z, set TZ from the tzone attr.
        let mut saved_tz: Option<Option<String>> = None;
        {
            let mut tz1: Option<String> = None;
            if !tzone.is_null()
                && tzone != R_NilValue()
                && TYPEOF(tzone) == SEXPTYPE::STRSXP
                && XLENGTH(tzone) > 0
            {
                let ch = STRING_ELT(tzone, 0);
                if !ch.is_null() && ch != R_NaString() {
                    let s = CStr::from_ptr(CHAR(ch)).to_string_lossy().into_owned();
                    if !s.is_empty() {
                        tz1 = Some(s);
                    }
                }
            }
            if let Some(tz) = tz1.as_ref() {
                let need_tz = (0..m).any(|k| {
                    let ch = STRING_ELT(sformat, k);
                    if ch.is_null() || ch == R_NaString() {
                        return false;
                    }
                    let s = CStr::from_ptr(CHAR(ch)).to_string_lossy();
                    s.contains("%Z") || s.contains("%z")
                });
                if need_tz {
                    saved_tz = Some(crate::tzone::timezone_override());
                    crate::tzone::set_timezone_override(Some(tz.clone()));
                }
            }
        }
        let mut ns0 = -1i32;

        let ans = Rf_allocVector3(SEXPTYPE::STRSXP, N);
        let _ans_guard = protect(ans);

        for i in 0..N {
            let iu = i as usize;
            let secs = posixlt_real_elt(VECTOR_ELT(x, 0), iu);
            let fsecs = secs.floor();

            let mut ctm: tz_tm = std::mem::zeroed();

            if R_FINITE(secs) && fsecs >= c_int::MIN as c_double && fsecs <= c_int::MAX as c_double
            {
                ctm.tm_sec = fsecs as c_int;
            } else {
                ctm.tm_sec = 0;
            }
            ctm.tm_min = posixlt_int_elt(VECTOR_ELT(x, 1), iu);
            ctm.tm_hour = posixlt_int_elt(VECTOR_ELT(x, 2), iu);
            ctm.tm_mday = posixlt_int_elt(VECTOR_ELT(x, 3), iu);
            ctm.tm_mon = posixlt_int_elt(VECTOR_ELT(x, 4), iu);
            ctm.tm_year = posixlt_int_elt(VECTOR_ELT(x, 5), iu);
            ctm.tm_wday = posixlt_int_elt(VECTOR_ELT(x, 6), iu);
            ctm.tm_yday = posixlt_int_elt(VECTOR_ELT(x, 7), iu);
            ctm.tm_isdst = posixlt_int_elt(VECTOR_ELT(x, 8), iu);

            if !R_FINITE(secs) {
                // NA, NaN, Inf, -Inf — GNU datetime.c uses NA_STRING for NA.
                if R_IsNA(secs) {
                    SET_STRING_ELT(ans, i as R_xlen_t, R_NaString());
                } else {
                    let s = if ISNAN(secs) {
                        "NaN"
                    } else if secs > 0.0 {
                        "Inf"
                    } else {
                        "-Inf"
                    };
                    let cstr = CString::new(s).unwrap_or_default();
                    SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                }
            } else if ctm.tm_min == NA_INTEGER
                || ctm.tm_hour == NA_INTEGER
                || ctm.tm_mday == NA_INTEGER
                || ctm.tm_mon == NA_INTEGER
                || ctm.tm_year == NA_INTEGER
            {
                SET_STRING_ELT(ans, i as R_xlen_t, R_NaString());
            } else {
                let mut tm_check = stm::new();
                tm_check.tm_sec = ctm.tm_sec;
                tm_check.tm_min = ctm.tm_min;
                tm_check.tm_hour = ctm.tm_hour;
                tm_check.tm_mday = ctm.tm_mday;
                tm_check.tm_mon = ctm.tm_mon;
                tm_check.tm_year = ctm.tm_year;
                tm_check.tm_isdst = ctm.tm_isdst;

                if validate_tm(&mut tm_check) < 0 || likely_strftime_overflow(&tm_check) {
                    SET_STRING_ELT(ans, i as R_xlen_t, R_NaString());
                } else {
                    // Copy validated values back
                    ctm.tm_sec = tm_check.tm_sec;
                    ctm.tm_min = tm_check.tm_min;
                    ctm.tm_hour = tm_check.tm_hour;
                    ctm.tm_mday = tm_check.tm_mday;
                    ctm.tm_mon = tm_check.tm_mon;
                    ctm.tm_year = tm_check.tm_year;

                    // Get format string
                    let fmt_charsxp = STRING_ELT(sformat, (i % m) as R_xlen_t);
                    let fmt_ptr = CHAR(fmt_charsxp);
                    let fmt_raw = if fmt_ptr.is_null() {
                        "%Y-%m-%d %H:%M:%S"
                    } else {
                        CStr::from_ptr(fmt_ptr)
                            .to_str()
                            .unwrap_or("%Y-%m-%d %H:%M:%S")
                    };
                    let fmt_exp =
                        expand_os_format(fmt_raw, secs, fsecs, ctm.tm_sec, digits, &mut ns0);

                    let mut sf_tm_ctm: sf_tm = std::mem::zeroed();
                    sf_tm_ctm.tm_sec = ctm.tm_sec;
                    sf_tm_ctm.tm_min = ctm.tm_min;
                    sf_tm_ctm.tm_hour = ctm.tm_hour;
                    sf_tm_ctm.tm_mday = ctm.tm_mday;
                    sf_tm_ctm.tm_mon = ctm.tm_mon;
                    sf_tm_ctm.tm_year = ctm.tm_year;
                    sf_tm_ctm.tm_wday = ctm.tm_wday;
                    sf_tm_ctm.tm_yday = ctm.tm_yday;
                    sf_tm_ctm.tm_isdst = ctm.tm_isdst;
                    // GNU: populate tm_zone/tm_gmtoff from components 9/10.
                    let mut tm_zone_buf = [0u8; 20];
                    if LENGTH(x) >= 11 {
                        let zv = VECTOR_ELT(x, 9);
                        if TYPEOF(zv) == SEXPTYPE::STRSXP && XLENGTH(zv) > 0 {
                            let ch = STRING_ELT(zv, (i % XLENGTH(zv)) as R_xlen_t);
                            if !ch.is_null() && ch != R_NaString() {
                                let bytes = CStr::from_ptr(CHAR(ch)).to_bytes();
                                let n = bytes.len().min(19);
                                tm_zone_buf[..n].copy_from_slice(&bytes[..n]);
                                sf_tm_ctm.tm_zone =
                                    tm_zone_buf.as_ptr() as *const core::ffi::c_char;
                            }
                        }
                        let gv = VECTOR_ELT(x, 10);
                        let tmp = posixlt_int_elt(gv, iu);
                        if tmp == NA_INTEGER && fmt_exp.contains("%z") {
                            sf_tm_ctm.tm_gmtoff = 0;
                            // mktime corrects tm_gmtoff where supported (glibc).
                            let _ =
                                R_mktime(&mut sf_tm_ctm as *mut sf_tm as *mut crate::tzone::stm);
                        } else {
                            sf_tm_ctm.tm_gmtoff = tmp as i64;
                        }
                    }
                    let mut buf = [0u8; 2049];
                    let res = unsafe {
                        R_strftime(
                            buf.as_mut_ptr(),
                            2048,
                            CString::new(fmt_exp.as_str()).unwrap_or_default().as_ptr()
                                as *const core::ffi::c_char,
                            &sf_tm_ctm,
                        )
                    };

                    if res == 0 {
                        // GNU datetime.c: overflow is an error except formats
                        // that may legitimately produce zero bytes.
                        if fmt_exp != "%Z" && fmt_exp != "%z" && fmt_exp != "%P" && fmt_exp != "%p"
                        {
                            std::panic::panic_any(RError {
                                message: "output string exceeded 2048 bytes".to_string(),
                            });
                        }
                        let mut out = String::new();
                        if usetz {
                            if let Some(zone) = posixlt_usetz_zone(x, tzone, iu, ctm.tm_isdst, nn) {
                                out.push(' ');
                                out.push_str(&zone);
                            }
                        }
                        let cstr = CString::new(out).unwrap_or_default();
                        SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                    } else {
                        let s = std::str::from_utf8(&buf[..res as usize]).unwrap_or("");
                        let mut out = s.to_string();
                        if usetz {
                            if let Some(zone) = posixlt_usetz_zone(x, tzone, iu, ctm.tm_isdst, nn) {
                                out.push(' ');
                                out.push_str(&zone);
                            }
                        }
                        let cstr = CString::new(out).unwrap_or_default();
                        SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                    }
                }
            }
        }
        // GNU: names come from the year component, padded to n, recycled to N.
        let nm = getAttrib(VECTOR_ELT(x, 5), R_NamesSymbol());
        let _nm = protect(nm);
        if !nm.is_null() && nm != R_NilValue() {
            let n_names = XLENGTH(nm);
            let nm2 = if n_names < n {
                let padded = Rf_allocVector3(SEXPTYPE::STRSXP, n);
                let _p2 = protect(padded);
                for j in 0..n {
                    let s = if j < n_names {
                        STRING_ELT(nm, j as R_xlen_t)
                    } else {
                        R_NaString()
                    };
                    SET_STRING_ELT(padded, j as R_xlen_t, s);
                }
                padded
            } else {
                nm
            };
            let _nm2 = protect(nm2);
            let nm3 = Rf_allocVector3(SEXPTYPE::STRSXP, N);
            let _nm3 = protect(nm3);
            for j in 0..N {
                let idx = (j % XLENGTH(nm2)) as R_xlen_t;
                SET_STRING_ELT(nm3, j as R_xlen_t, STRING_ELT(nm2, idx));
            }
            setAttrib(ans, R_NamesSymbol(), nm3);
        }

        if let Some(prev) = saved_tz {
            crate::tzone::set_timezone_override(prev);
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// glibc_fix -- fill in month/day omitted by the parser
// ---------------------------------------------------------------------------
