use super::*;

/// Set mon and mday which the parser does not always set.
/// Use current year/... if none has been specified.
///
/// Specifying mon but not mday nor yday is invalid.
///
/// Ported from `glibc_fix()` in datetime.c.
fn glibc_fix(tm: &mut stm, invalid: &mut bool) {
    unsafe {
        // SystemTime works on every target (wasm yields the epoch,
        // matching the previous wasm-libc facade behavior).
        let t: time_t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut tm0: tz_tm = std::mem::zeroed();
        if unsafe { R_localtime_r(&t, &mut tm0) }.is_null() {
            return;
        }
        if tm.tm_year == NA_INTEGER {
            tm.tm_year = tm0.tm_year;
        }
        if tm.tm_mon != NA_INTEGER && tm.tm_mday != NA_INTEGER {
            return;
        }
        // At least one of the month and the day of the month is missing.
        if tm.tm_yday != NA_INTEGER {
            // Since we have yday, let that take precedence over mon/mday.
            let mut yday = tm.tm_yday;
            let mut mon = 0;
            while mon < 12 {
                let tmp = days_in_month(mon, tm.tm_year);
                if yday < tmp {
                    break;
                }
                yday -= tmp;
                mon += 1;
            }
            tm.tm_mon = mon;
            tm.tm_mday = yday + 1;
        } else {
            if tm.tm_mday == NA_INTEGER {
                if tm.tm_mon != NA_INTEGER {
                    *invalid = true;
                    return;
                }
                tm.tm_mday = tm0.tm_mday;
            }
            if tm.tm_mon == NA_INTEGER {
                tm.tm_mon = tm0.tm_mon;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TZ switch setup -- set_tz / reset_tz / prepare_reset_tz in datetime.c
// ---------------------------------------------------------------------------

/// Temporarily select a timezone in the owning session's ported timezone engine.
/// This guard never changes the host process environment or libc timezone.
pub(crate) struct TzSetup {
    pub(crate) old: Option<String>,
    pub(crate) owner: *mut crate::sexp::instance::RInstance,
}

impl TzSetup {
    /// Port of `prepare_reset_tz()`: snapshot the current TZ.
    pub(crate) fn prepare() -> Self {
        let owner = crate::sexp::instance::with_required_current_instance(|inst| inst);
        TzSetup {
            old: crate::tzone::timezone_override(),
            owner,
        }
    }

    /// Port of `set_tz()`.
    pub(crate) fn set(&self, tz: &str) {
        crate::tzone::set_timezone_override(Some(tz.to_owned()));
    }
}

impl Drop for TzSetup {
    fn drop(&mut self) {
        // The guard is private to a synchronous evaluation and cannot outlive
        // its session. Restore that owner even if a nested session is active;
        // accessing ambient state here could restore the wrong timezone.
        unsafe { (*self.owner).tzone_state.set_override(self.old.take()) };
    }
}

/// Decode a CHARSXP as UTF-8 text, mirroring trunk's multibyte-path
/// validation in `R_strptime()` (mbstowcs failing is an error).
pub(crate) unsafe fn charsxp_text(s: SEXP, what: &str) -> String {
    unsafe {
        let p = CHAR(s);
        let bytes: &[u8] = if p.is_null() {
            &[]
        } else {
            CStr::from_ptr(p).to_bytes()
        };
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_owned(),
            Err(_) => std::panic::panic_any(RError {
                message: format!("invalid multibyte {} string", what),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// do_strptime -- .Internal(strptime(x, format, tz))
// ---------------------------------------------------------------------------

/// Parse a date/time string according to a format, producing a POSIXlt
/// object.
///
/// Emulates R's base closure
/// `strptime <- function(x, format = "", tz = "")`
///   `.Internal(strptime(as.character(x), format, tz))` together with the
/// `.Internal` handler `do_strptime()` in datetime.c (trunk r90447).
pub unsafe fn do_strptime(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let mut x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::STRSXP {
            // The closure applies as.character() before calling .Internal
            // (`storage.mode<-(x, "character")` for plain vectors).
            let arglist = Rf_cons(x, R_NilValue());
            let _arglist_guard = protect(arglist);
            x = crate::mainutils::essentials_basic::do_as_character(_call, _op, arglist, _env);
        }
        let _x_guard = protect(x);

        let sformat = CADR(args);
        if TYPEOF(sformat) != SEXPTYPE::STRSXP || XLENGTH(sformat) == 0 {
            std::panic::panic_any(RError {
                message: "invalid 'format' argument".to_string(),
            });
        }

        // GET_tz_n_CHECK: 'tz' must be a length-1 string; the closure
        // supplies the default tz = "" when the argument is missing.
        let mut stz = CADDR(args);
        if stz.is_null() || stz == R_NilValue() || stz == R_MissingArg() {
            stz = Rf_mkString(c"".as_ptr());
        } else if TYPEOF(stz) != SEXPTYPE::STRSXP || XLENGTH(stz) != 1 {
            std::panic::panic_any(RError {
                message: "invalid 'tz' value".to_string(),
            });
        }
        let _stz_guard = protect(stz);

        let mut tz = charsxp_text(STRING_ELT(stz, 0), "tz");
        let mut new_tz = tz.is_empty(); // tz = ""
        if new_tz {
            // Do a direct look up here as this does not otherwise work on
            // Windows.
            match std::env::var_os("TZ") {
                Some(v) => {
                    tz = v.to_string_lossy().into_owned();
                }
                None => new_tz = false,
            }
        }
        // isUTC here means that the timezone has been set to UTC either by
        // default, via TZ="UTC", or via a 'tz' argument. It controls setting
        // TZ, the use of gmtime vs localtime, forcing isdst = 0 and how the
        // "tzone" attribute is set.
        let isUTC = tz == "GMT" || tz == "UTC";
        let tzsi = TzSetup::prepare();
        if !isUTC {
            tzsi.set(&tz);
        }

        let n = XLENGTH(x);
        let m = XLENGTH(sformat);
        let N = if n > 0 { std::cmp::max(m, n) } else { 0 };

        let (ans, ansnames) = make_posixlt_skeleton(N);
        let _ans_guard = protect(ans);
        let _ansnames_guard = protect(ansnames);

        // SET_TZONE: set now in case this gets changed by conversions.
        let tzone = if isUTC {
            Rf_mkString(mk_char_str(&tz).as_ptr())
        } else {
            let tzone = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
            for (j, s) in [tz.clone(), tzname_str(0), tzname_str(1)]
                .iter()
                .enumerate()
            {
                let cs = CString::new(s.as_str()).unwrap_or_default();
                SET_STRING_ELT(tzone, j as R_xlen_t, Rf_mkChar(cs.as_ptr()));
            }
            tzone
        };
        let _tzone_guard = protect(tzone);

        for i in 0..N {
            let iu = i as usize;
            // For glibc's sake. That only sets some unspecified fields,
            // sometimes.
            let mut tm = stm::new();
            tm.tm_sec = 0;
            tm.tm_min = 0;
            tm.tm_hour = 0;
            tm.tm_year = NA_INTEGER;
            tm.tm_mon = NA_INTEGER;
            tm.tm_mday = NA_INTEGER;
            tm.tm_yday = NA_INTEGER;
            tm.tm_wday = NA_INTEGER;
            tm.tm_gmtoff = NA_INTEGER as c_long;
            tm.tm_isdst = -1;
            let mut psecs: c_double = 0.0;
            let mut offset: c_int = NA_INTEGER;

            let xs = STRING_ELT(x, (i % n) as R_xlen_t);
            let mut invalid = xs == R_NaString() || xs.is_null() || xs == R_NilValue();
            if !invalid {
                let input = charsxp_text(xs, "input");
                let fmt = charsxp_text(STRING_ELT(sformat, (i % m) as R_xlen_t), "format");
                invalid = !R_strptime(&input, &fmt, &mut tm, &mut psecs, &mut offset);
            }

            let mut tm2 = tm;
            let mut use_tm2 = false;
            if !invalid {
                // Solaris sets missing fields to 0.
                if tm.tm_mday == 0 {
                    tm.tm_mday = NA_INTEGER;
                }
                if tm.tm_mon == NA_INTEGER || tm.tm_mday == NA_INTEGER || tm.tm_year == NA_INTEGER {
                    glibc_fix(&mut tm, &mut invalid);
                }
                tm.tm_isdst = -1;
                if offset != NA_INTEGER {
                    tm.tm_gmtoff = offset as c_long; // not always correct; better than always NA
                }
                tm2 = tm;
                if offset != NA_INTEGER {
                    // We know the offset, but not the timezone; so all we
                    // can do is to convert to time_t, adjust and convert
                    // back.
                    let t0 = mktime0(&mut tm2, false);
                    if t0 != -1.0 {
                        let tt0 = t0 - offset as c_double;
                        localtime0(&tt0, !isUTC, &mut tm2);
                        use_tm2 = true;
                    } else {
                        invalid = true;
                    }
                } else {
                    // We do want to set wday, yday, isdst, but not to
                    // adjust the structure at DST boundaries.
                    if isUTC {
                        tm.tm_isdst = 0;
                    }
                    // mktime _may_ result in error e.g. during the
                    // spring-forward gap.
                    if mktime0(&mut tm2, !isUTC) != -1.0 {
                        tm.tm_wday = tm2.tm_wday;
                        tm.tm_yday = tm2.tm_yday;
                        tm.tm_zone = tm2.tm_zone;
                        if !isUTC && tm.tm_hour == tm2.tm_hour && tm.tm_min == tm2.tm_min {
                            tm.tm_isdst = tm2.tm_isdst;
                        }
                    }
                }
                invalid = validate_tm(&mut tm) != 0;
            }

            makelt(
                if use_tm2 { &tm2 } else { &tm },
                ans,
                i as R_xlen_t,
                !invalid,
                if invalid {
                    NA_REAL
                } else {
                    psecs - psecs.floor()
                },
            );

            if isUTC {
                let cs = CString::new(tz.as_str()).unwrap_or_default();
                SET_STRING_ELT(VECTOR_ELT(ans, 9), i as R_xlen_t, Rf_mkChar(cs.as_ptr()));
                *INTEGER(VECTOR_ELT(ans, 10)).add(iu) = 0; // gmtoff
            } else {
                let p = if !invalid && tm.tm_isdst >= 0 {
                    let named = tm_zone_string(tm.tm_zone);
                    if named.is_empty() {
                        tzname_str(tm.tm_isdst.clamp(0, 1) as usize)
                    } else {
                        named
                    }
                } else {
                    String::new()
                };
                let cs = CString::new(p).unwrap_or_default();
                SET_STRING_ELT(VECTOR_ELT(ans, 9), i as R_xlen_t, Rf_mkChar(cs.as_ptr()));
                *INTEGER(VECTOR_ELT(ans, 10)).add(iu) = if invalid {
                    NA_INTEGER
                } else {
                    tm.tm_gmtoff as c_int
                };
            }
        } // for(i ..)

        // END_MAKElt
        setAttrib(ans, R_NamesSymbol(), ansnames);
        let klass = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _klass_guard = protect(klass);
        SET_STRING_ELT(klass, 0, Rf_mkChar(c"POSIXlt".as_ptr()));
        SET_STRING_ELT(klass, 1, Rf_mkChar(c"POSIXt".as_ptr()));
        R_classgets(ans, klass);
        if TYPEOF(tzone) == SEXPTYPE::STRSXP {
            setAttrib(ans, Rf_install(c"tzone".as_ptr()), tzone);
        }
        set_posixlt_balanced(ans);

        // The base closure post-processes non-finite inputs: elements of
        // 'x' equal to "Inf" / "-Inf" are replaced by
        // as.POSIXlt.POSIXct(.POSIXct(+-Inf)), i.e. sec = +-Inf with all
        // other components NA and isdst = -1.
        for i in 0..N {
            let xi = STRING_ELT(x, (i % n) as R_xlen_t);
            if xi == R_NaString() || xi.is_null() {
                continue;
            }
            let s = charsxp_text(xi, "input");
            let v = match s.as_str() {
                "Inf" => Some(c_double::INFINITY),
                "-Inf" => Some(c_double::NEG_INFINITY),
                _ => None,
            };
            if let Some(v) = v {
                *REAL(VECTOR_ELT(ans, 0)).add(i as usize) = v;
                for j in 1..8 {
                    *INTEGER(VECTOR_ELT(ans, j)).add(i as usize) = NA_INTEGER;
                }
                *INTEGER(VECTOR_ELT(ans, 8)).add(i as usize) = -1;
                if isUTC {
                    let cs = CString::new(tz.as_str()).unwrap_or_default();
                    SET_STRING_ELT(VECTOR_ELT(ans, 9), i as R_xlen_t, Rf_mkChar(cs.as_ptr()));
                    *INTEGER(VECTOR_ELT(ans, 10)).add(i as usize) = 0;
                } else {
                    SET_STRING_ELT(VECTOR_ELT(ans, 9), i as R_xlen_t, Rf_mkChar(c"".as_ptr()));
                    *INTEGER(VECTOR_ELT(ans, 10)).add(i as usize) = NA_INTEGER;
                }
            }
        }
        let nm = getAttrib(x, R_NamesSymbol());
        if nm != R_NilValue() && TYPEOF(nm) == SEXPTYPE::STRSXP && XLENGTH(nm) > 0 {
            if N == n {
                setAttrib(VECTOR_ELT(ans, 5), R_NamesSymbol(), nm);
            } else if N > n {
                let nm3 = Rf_allocVector3(SEXPTYPE::STRSXP, N);
                let _nm3_guard = protect(nm3);
                for j in 0..N {
                    SET_STRING_ELT(nm3, j as R_xlen_t, STRING_ELT(nm, (j % n) as R_xlen_t));
                }
                setAttrib(VECTOR_ELT(ans, 5), R_NamesSymbol(), nm3);
            }
        }

        ans
    }
}
