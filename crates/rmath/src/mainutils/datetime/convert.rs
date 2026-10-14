use super::*;

/// Convert a `stm` to seconds since the Unix epoch.
///
/// If `local` is true, uses `mktime` (local timezone); otherwise uses
/// `timegm00` (UTC). Returns -1.0 on error.
///
/// Ported from `mktime0()` in datetime.c.
pub(crate) fn mktime0(tm: &mut stm, local: bool) -> c_double {
    if validate_tm(tm) < 0 {
        return -1.0;
    }
    if !local {
        return timegm00(tm);
    }

    let mut ctm: tz_tm = unsafe { std::mem::zeroed() };
    ctm.tm_sec = tm.tm_sec;
    ctm.tm_min = tm.tm_min;
    ctm.tm_hour = tm.tm_hour;
    ctm.tm_mday = tm.tm_mday;
    ctm.tm_mon = tm.tm_mon;
    ctm.tm_year = tm.tm_year;
    ctm.tm_isdst = tm.tm_isdst;
    let result = unsafe { R_mktime(&mut ctm) };
    let Some(gmtoff) = c_long::try_from(ctm.tm_gmtoff).ok() else {
        return -1.0;
    };
    tm.tm_sec = ctm.tm_sec;
    tm.tm_min = ctm.tm_min;
    tm.tm_hour = ctm.tm_hour;
    tm.tm_mday = ctm.tm_mday;
    tm.tm_mon = ctm.tm_mon;
    tm.tm_year = ctm.tm_year;
    tm.tm_isdst = ctm.tm_isdst;
    tm.tm_wday = ctm.tm_wday;
    tm.tm_yday = ctm.tm_yday;
    tm.tm_gmtoff = gmtoff;
    tm.tm_zone = ctm.tm_zone;

    if result == -1 {
        return -1.0;
    }

    result as c_double
}

// ---------------------------------------------------------------------------
// localtime0 -- convert seconds since epoch to stm (UTC or local)
// ---------------------------------------------------------------------------

/// Convert a timestamp (seconds since epoch) to a `stm`.
///
/// If `local` is true, uses `localtime_r` (local timezone); otherwise uses
/// UTC conversion via the internal algorithm.
///
/// Ported from `localtime0()` in datetime.c.
pub(crate) fn localtime0(tp: *const c_double, local: bool, ltm: &mut stm) -> bool {
    let d = unsafe { *tp };
    if !d.is_finite() {
        ltm.tm_year = NA_INTEGER;
        ltm.tm_mon = NA_INTEGER;
        ltm.tm_mday = NA_INTEGER;
        ltm.tm_yday = NA_INTEGER;
        ltm.tm_wday = NA_INTEGER;
        ltm.tm_hour = NA_INTEGER;
        ltm.tm_min = NA_INTEGER;
        ltm.tm_sec = NA_INTEGER;
        ltm.tm_isdst = -1;
        return false;
    }

    // Bail out for unreasonable values
    let dday = (d / 86400.0).floor();
    if dday.abs() > 784368402400.0 {
        ltm.tm_year = NA_INTEGER;
        ltm.tm_mon = NA_INTEGER;
        ltm.tm_mday = NA_INTEGER;
        ltm.tm_yday = NA_INTEGER;
        ltm.tm_wday = NA_INTEGER;
        ltm.tm_hour = NA_INTEGER;
        ltm.tm_min = NA_INTEGER;
        ltm.tm_sec = NA_INTEGER;
        ltm.tm_isdst = -1;
        return false;
    }

    // Convert double to time_t (handle negative values correctly)
    let mut t = d as time_t;
    if d < 0.0 && d != (t as c_double) {
        t -= 1;
    }

    let mut ctm: tz_tm = unsafe { std::mem::zeroed() };
    let res = unsafe {
        if local {
            R_localtime_r(&t, &mut ctm)
        } else {
            R_gmtime_r(&t, &mut ctm)
        }
    };

    if res.is_null() {
        return false;
    }
    let Some(gmtoff) = c_long::try_from(ctm.tm_gmtoff).ok() else {
        return false;
    };

    ltm.tm_sec = ctm.tm_sec;
    ltm.tm_min = ctm.tm_min;
    ltm.tm_hour = ctm.tm_hour;
    ltm.tm_mday = ctm.tm_mday;
    ltm.tm_mon = ctm.tm_mon;
    ltm.tm_year = ctm.tm_year;
    ltm.tm_wday = ctm.tm_wday;
    ltm.tm_yday = ctm.tm_yday;
    ltm.tm_isdst = ctm.tm_isdst;
    ltm.tm_gmtoff = gmtoff;
    ltm.tm_zone = ctm.tm_zone;

    true
}

// ---------------------------------------------------------------------------
// makelt -- populate POSIXlt vector elements from stm
// ---------------------------------------------------------------------------

/// Populate the POSIXlt list elements for index `i`.
///
/// `ans` is a VECSXP of 11 elements (sec, min, hour, mday, mon, year, wday,
/// yday, isdst, zone, gmtoff). Sets elements 0-8 based on `tm`.
pub(crate) fn makelt(tm: &stm, ans: SEXP, i: R_xlen_t, valid: bool, frac_secs: c_double) {
    if valid {
        unsafe {
            *REAL(VECTOR_ELT(ans, 0)).add(i as usize) = tm.tm_sec as c_double + frac_secs;
            *INTEGER(VECTOR_ELT(ans, 1)).add(i as usize) = tm.tm_min;
            *INTEGER(VECTOR_ELT(ans, 2)).add(i as usize) = tm.tm_hour;
            *INTEGER(VECTOR_ELT(ans, 3)).add(i as usize) = tm.tm_mday;
            *INTEGER(VECTOR_ELT(ans, 4)).add(i as usize) = tm.tm_mon;
            *INTEGER(VECTOR_ELT(ans, 5)).add(i as usize) = tm.tm_year;
            *INTEGER(VECTOR_ELT(ans, 6)).add(i as usize) = tm.tm_wday;
            *INTEGER(VECTOR_ELT(ans, 7)).add(i as usize) = tm.tm_yday;
            *INTEGER(VECTOR_ELT(ans, 8)).add(i as usize) = tm.tm_isdst;
        }
    } else {
        unsafe {
            *REAL(VECTOR_ELT(ans, 0)).add(i as usize) = frac_secs;
            for j in 1..8 {
                *INTEGER(VECTOR_ELT(ans, j)).add(i as usize) = NA_INTEGER;
            }
            *INTEGER(VECTOR_ELT(ans, 8)).add(i as usize) = -1; // isdst
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: create a POSIXlt result vector with names and class
// ---------------------------------------------------------------------------

/// Build the standard POSIXlt VECSXP with 11 named components.
/// Returns (ans, ansnames) both protected.
pub(crate) unsafe fn make_posixlt_skeleton(n: R_xlen_t) -> (SEXP, SEXP) {
    unsafe {
        let nans: c_int = 11;
        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, nans as R_xlen_t);
        let _ans_guard = protect(ans);
        for i in 0..9 {
            let sexp: c_int = if i > 0 {
                SEXPTYPE::INTSXP.into()
            } else {
                SEXPTYPE::REALSXP.into()
            };
            SET_VECTOR_ELT(ans, i as R_xlen_t, Rf_allocVector3(sexp, n));
        }
        SET_VECTOR_ELT(ans, 9, Rf_allocVector3(SEXPTYPE::STRSXP, n)); // zone
        SET_VECTOR_ELT(ans, 10, Rf_allocVector3(SEXPTYPE::INTSXP, n)); // gmtoff

        let ansnames = Rf_allocVector3(SEXPTYPE::STRSXP, nans as R_xlen_t);
        let _ansnames_guard = protect(ansnames);
        for i in 0..nans {
            let cstr = CString::new(ltnames[i as usize]).unwrap_or_default();
            SET_STRING_ELT(ansnames, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }

        (ans, ansnames)
    }
}

pub(crate) fn tm_zone_string(p: *const std::os::raw::c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() }
    }
}

fn tz_is_utc(tz: &str) -> bool {
    matches!(
        tz,
        "GMT"
            | "UTC"
            | "Etc/UTC"
            | "Etc/GMT"
            | "UTC0"
            | "GMT0"
            | "UTC+0"
            | "UTC-0"
            | "GMT+0"
            | "GMT-0"
    )
}

pub(crate) unsafe fn posixlt_parses_every_string(lt: SEXP, x: SEXP) -> bool {
    unsafe {
        if lt.is_null() || lt == R_NilValue() || TYPEOF(lt) != SEXPTYPE::VECSXP {
            return false;
        }
        let sec = VECTOR_ELT(lt, 0);
        if sec.is_null() || TYPEOF(sec) != SEXPTYPE::REALSXP {
            return false;
        }
        let n = XLENGTH(x);
        if XLENGTH(sec) < n {
            return false;
        }
        // GNU as.POSIXlt.character (2.13.0+): xx <- x[!is.na(x)] and a
        // format is accepted only when `all(!is.na(strptime(xx, f)))`.
        // Every non-NA string — including empty ones, which cannot parse —
        // must yield a non-NA result.
        for i in 0..n {
            let s = STRING_ELT(x, i);
            if s.is_null() || s == R_NaString() {
                continue;
            }
            let v = *REAL(sec).add(i as usize);
            if !R_FINITE(v) {
                return false;
            }
        }
        true
    }
}

unsafe fn posixlt_has_valid_time(x: SEXP) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP || XLENGTH(x) < 1 {
            return false;
        }
        let sec = VECTOR_ELT(x, 0);
        if sec.is_null() || XLENGTH(sec) < 1 {
            return false;
        }
        if TYPEOF(sec) == SEXPTYPE::REALSXP {
            let v = *REAL(sec);
            R_FINITE(v) || (v.is_infinite() && !R_IsNA(v))
        } else if TYPEOF(sec) == SEXPTYPE::INTSXP {
            *INTEGER(sec) != NA_INTEGER
        } else {
            false
        }
    }
}

/// GNU `as.POSIXlt.default` uses `missing(tz)`. A supplied non-empty tz
/// only relabels `tzone` on an existing POSIXlt.
pub(crate) unsafe fn posixlt_supplied_tz(args: SEXP) -> Option<String> {
    unsafe {
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let named_tz =
                !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP && {
                    let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag))).to_string_lossy();
                    name == "tz"
                };
            let positional = tag.is_null() || tag == R_NilValue();
            if named_tz || positional {
                let t = CAR(cell);
                if t.is_null() || t == R_NilValue() || t == R_MissingArg() {
                    return None;
                }
                if TYPEOF(t) == SEXPTYPE::STRSXP && XLENGTH(t) > 0 {
                    return Some(charsxp_text(STRING_ELT(t, 0), "tz"));
                }
                return None;
            }
            cell = CDR(cell);
        }
        None
    }
}

pub(crate) unsafe fn set_posixlt_balanced(ans: SEXP) {
    unsafe {
        setAttrib(
            ans,
            Rf_install(c"balanced".as_ptr()),
            Rf_ScalarLogical(TRUE),
        );
    }
}

unsafe fn posixlt_tzone_sexp(tz: &str, is_utc: bool) -> SEXP {
    unsafe {
        if is_utc {
            let name = if tz.is_empty() { "UTC" } else { tz };
            Rf_mkString(CString::new(name).unwrap_or_default().as_ptr())
        } else {
            let label = if tz.is_empty() {
                crate::tzone::timezone_override()
                    .or_else(|| std::env::var("TZ").ok())
                    .unwrap_or_default()
            } else {
                tz.to_string()
            };

            let tzone = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
            for (j, s) in [label, tzname_str(0), tzname_str(1)].iter().enumerate() {
                let cs = CString::new(s.as_str()).unwrap_or_default();
                SET_STRING_ELT(tzone, j as R_xlen_t, Rf_mkChar(cs.as_ptr()));
            }
            tzone
        }
    }
}

pub(crate) unsafe fn finish_posixlt(ans: SEXP, ansnames: SEXP, tzone: SEXP) {
    unsafe {
        setAttrib(ans, R_NamesSymbol(), ansnames);
        let klass = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _klass_guard = protect(klass);
        SET_STRING_ELT(klass, 0, Rf_mkChar(c"POSIXlt".as_ptr()));
        SET_STRING_ELT(klass, 1, Rf_mkChar(c"POSIXt".as_ptr()));
        R_classgets(ans, klass);
        if !tzone.is_null() && tzone != R_NilValue() && TYPEOF(tzone) == SEXPTYPE::STRSXP {
            setAttrib(ans, Rf_install(c"tzone".as_ptr()), tzone);
        }
        set_posixlt_balanced(ans);
    }
}

/// GNU `.Internal(as.POSIXlt(x, tz))` for numeric POSIXct seconds.
pub unsafe fn convert_posixct_to_posixlt(x: SEXP, tz: &str) -> SEXP {
    unsafe {
        let is_utc = tz_is_utc(tz);
        let tzsi = TzSetup::prepare();
        if !is_utc {
            if tz.is_empty() {
                if let Some(env_tz) =
                    crate::tzone::timezone_override().or_else(|| std::env::var("TZ").ok())
                {
                    tzsi.set(&env_tz);
                }
            } else {
                tzsi.set(tz);
            }
        }

        let n = XLENGTH(x);
        let (ans, ansnames) = make_posixlt_skeleton(n);
        let _ans_guard = protect(ans);
        let _ansnames_guard = protect(ansnames);
        let tzone = posixlt_tzone_sexp(tz, is_utc);
        let _tz_guard = protect(tzone);
        for i in 0..n {
            let mut dummy = stm::new();
            let d = if TYPEOF(x) == SEXPTYPE::INTSXP || TYPEOF(x) == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER {
                    NA_REAL
                } else {
                    v as c_double
                }
            } else {
                *REAL(x).add(i as usize)
            };

            let valid = if R_FINITE(d) {
                localtime0(&d as *const c_double, !is_utc, &mut dummy)
            } else {
                false
            };
            makelt(&dummy, ans, i, valid, if valid { d - d.floor() } else { d });
            // GNU datetime.c asPOSIXlt: when isUTC, zone is the tz string
            // itself and gmtoff is 0 (localtime0's gmtime path leaves
            // tm_zone unset in this port).
            let (zone, gmtoff) = if is_utc {
                (tz.to_string(), 0)
            } else {
                let named = if valid && dummy.tm_isdst >= 0 {
                    let named = tm_zone_string(dummy.tm_zone);
                    if named.is_empty() {
                        tzname_str(dummy.tm_isdst.clamp(0, 1) as usize)
                    } else {
                        named
                    }
                } else {
                    String::new()
                };
                (
                    named,
                    if valid {
                        dummy.tm_gmtoff as c_int
                    } else {
                        NA_INTEGER
                    },
                )
            };
            SET_STRING_ELT(
                VECTOR_ELT(ans, 9),
                i,
                Rf_mkChar(CString::new(zone).unwrap_or_default().as_ptr()),
            );
            *INTEGER(VECTOR_ELT(ans, 10)).add(i as usize) = gmtoff;
        }
        finish_posixlt(ans, ansnames, tzone);
        let names = getAttrib(x, R_NamesSymbol());
        if !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && XLENGTH(names) == n
        {
            setAttrib(VECTOR_ELT(ans, 5), R_NamesSymbol(), names);
        }
        ans
    }
}

/// GNU `.Internal(as.POSIXct(x, tz))` for a POSIXlt list.
pub unsafe fn convert_posixlt_to_posixct(x: SEXP, tz: &str) -> SEXP {
    unsafe {
        let is_utc = tz_is_utc(tz);
        let tzsi = TzSetup::prepare();
        if !is_utc && !tz.is_empty() {
            tzsi.set(tz);
        }
        do_asPOSIXct(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            Rf_cons(
                x,
                Rf_cons(
                    Rf_mkString(CString::new(tz).unwrap_or_default().as_ptr()),
                    R_NilValue(),
                ),
            ),
            std::ptr::null_mut(),
        )
    }
}

// ---------------------------------------------------------------------------
// do_asPOSIXlt -- .Internal(as.POSIXlt(x, tz))
// ---------------------------------------------------------------------------

/// Convert a POSIXct numeric vector to a POSIXlt list.
///
/// Ported from `do_asPOSIXlt()` in datetime.c.
pub unsafe fn do_asPOSIXlt(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::REALSXP && TYPEOF(x) != SEXPTYPE::INTSXP {
            std::panic::panic_any(RError {
                message: "invalid 'x' value: not numeric".to_string(),
            });
        }
        let stz = CADR(args);
        let tz = if stz.is_null() || stz == R_NilValue() || stz == R_MissingArg() {
            String::new()
        } else if TYPEOF(stz) == SEXPTYPE::STRSXP && XLENGTH(stz) > 0 {
            charsxp_text(STRING_ELT(stz, 0), "tz")
        } else {
            String::new()
        };
        convert_posixct_to_posixlt(x, &tz)
    }
}

// ---------------------------------------------------------------------------
// do_asPOSIXct -- .Internal(as.POSIXct(x, tz))
// ---------------------------------------------------------------------------

/// Convert a POSIXlt list to a POSIXct numeric vector.
///
/// Ported from `do_asPOSIXct()` in datetime.c.
pub unsafe fn do_asPOSIXct(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let _x_guard = protect(x);
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            std::panic::panic_any(RError {
                message: "a valid \"POSIXlt\" object is a list of at least 9 elements".to_string(),
            });
        }
        let stz = CADR(args);
        let tz = if stz.is_null() || stz == R_NilValue() || stz == R_MissingArg() {
            String::new()
        } else if TYPEOF(stz) == SEXPTYPE::STRSXP && XLENGTH(stz) > 0 {
            charsxp_text(STRING_ELT(stz, 0), "tz")
        } else {
            String::new()
        };
        let is_utc = tz_is_utc(&tz);

        let mut n: R_xlen_t = 0;
        let mut nlen = [0i64; 9];
        for i in 0..6 {
            let len = XLENGTH(VECTOR_ELT(x, i as R_xlen_t));
            nlen[i as usize] = len;
            if len > n {
                n = len;
            }
        }
        let len8 = XLENGTH(VECTOR_ELT(x, 8));
        nlen[8] = len8;
        if len8 > n {
            n = len8;
        }

        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _ans_guard = protect(ans);

        for i in 0..n {
            let iu = i as usize;
            let secs = posixlt_real_elt(VECTOR_ELT(x, 0), iu);

            let fsecs = secs.floor();

            let mut tm = stm::new();
            tm.tm_sec = if R_FINITE(secs) {
                fsecs as c_int
            } else {
                NA_INTEGER
            };
            tm.tm_min = posixlt_int_elt(VECTOR_ELT(x, 1), iu);
            tm.tm_hour = posixlt_int_elt(VECTOR_ELT(x, 2), iu);
            tm.tm_mday = posixlt_int_elt(VECTOR_ELT(x, 3), iu);
            tm.tm_mon = posixlt_int_elt(VECTOR_ELT(x, 4), iu);
            tm.tm_year = posixlt_int_elt(VECTOR_ELT(x, 5), iu);
            tm.tm_isdst = if is_utc {
                0
            } else {
                posixlt_int_elt(VECTOR_ELT(x, 8), iu)
            };

            if !R_FINITE(secs) {
                *REAL(ans).add(iu) = secs;
            } else if tm.tm_min == NA_INTEGER
                || tm.tm_hour == NA_INTEGER
                || tm.tm_mday == NA_INTEGER
                || tm.tm_mon == NA_INTEGER
                || tm.tm_year == NA_INTEGER
            {
                *REAL(ans).add(iu) = NA_REAL;
            } else {
                let tmp = mktime0(&mut tm, !is_utc);
                // GNU datetime.c:1240-1259 (no errno): -1 is NA unless
                // this is the epoch-minus-one gotcha (sec==59) or the
                // sec=58 probe returns -2.
                let failed = tmp == -1.0 && tm.tm_sec != 59 && {
                    let mut probe = tm;
                    probe.tm_sec = 58;
                    mktime0(&mut probe, !is_utc) != -2.0
                };
                *REAL(ans).add(iu) = if failed {
                    NA_REAL
                } else {
                    tmp + (secs - fsecs)
                };
            }
        }

        let names = if XLENGTH(x) >= 6 {
            getAttrib(VECTOR_ELT(x, 5), R_NamesSymbol())
        } else {
            R_NilValue()
        };
        if !names.is_null() && names != R_NilValue() && XLENGTH(names) == n {
            setAttrib(ans, R_NamesSymbol(), names);
        }
        ans
    }
}
