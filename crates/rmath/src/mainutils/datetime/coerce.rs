use super::*;

unsafe fn posixlt_seconds_digits(sec: SEXP, digits: i32) -> i32 {
    unsafe {
        if TYPEOF(sec) != SEXPTYPE::REALSXP {
            return use_dig_secs(&[], digits);
        }
        let length = XLENGTH(sec).max(0) as usize;
        if length == 0 {
            return use_dig_secs(&[], digits);
        }

        // REAL calls DATAPTR, which materializes compact ALTREP storage before
        // this span is formed. The caller keeps `sec` rooted while this pure
        // scan runs; no R allocation or callback occurs until it returns.
        let values = std::slice::from_raw_parts(REAL(sec), length);
        use_dig_secs(values, digits)
    }
}

/// GNU `as.POSIXlt(x, tz="")`.
pub unsafe fn do_as_POSIXlt(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        if crate::mainutils::objects::inherits2(x, c"POSIXlt".as_ptr()) != 0 {
            if let Some(tz) = posixlt_supplied_tz(args) {
                if !tz.is_empty() {
                    let out = crate::mainutils::duplicate::Rf_duplicate(x);
                    let _o = protect(out);
                    setAttrib(
                        out,
                        Rf_install(c"tzone".as_ptr()),
                        Rf_mkString(CString::new(tz).unwrap_or_default().as_ptr()),
                    );
                    return out;
                }
            }
            return x;
        }

        let mut tz = Rf_mkString(c"".as_ptr());
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let t = CAR(rest);
            if TYPEOF(t) == SEXPTYPE::STRSXP && XLENGTH(t) > 0 {
                tz = t;
            }
        }
        let _tz = protect(tz);
        let mut tz_s = if TYPEOF(tz) == SEXPTYPE::STRSXP && XLENGTH(tz) > 0 {
            charsxp_text(STRING_ELT(tz, 0), "tz")
        } else {
            String::new()
        };
        if crate::mainutils::objects::inherits2(x, c"Date".as_ptr()) != 0 {
            // GNU as.POSIXlt.Date(x, tz="UTC"): Date2POSIXlt(x, tz) then
            // .POSIXlt(x, tz) — zone strings and tzone both carry tz.
            let tz_use = if tz_s.is_empty() {
                "UTC".to_string()
            } else {
                tz_s.clone()
            };
            let tz_arg = Rf_mkString(CString::new(tz_use.as_str()).unwrap_or_default().as_ptr());
            let _ta = protect(tz_arg);
            return do_D2POSIXlt(call, op, Rf_cons(x, Rf_cons(tz_arg, R_NilValue())), env);
        }

        if crate::mainutils::objects::inherits2(x, c"POSIXct".as_ptr()) != 0
            || TYPEOF(x) == SEXPTYPE::REALSXP
            || TYPEOF(x) == SEXPTYPE::INTSXP
            || TYPEOF(x) == SEXPTYPE::LGLSXP
        {
            if tz_s.is_empty() {
                let attr = getAttrib(x, Rf_install(c"tzone".as_ptr()));
                if !attr.is_null()
                    && attr != R_NilValue()
                    && TYPEOF(attr) == SEXPTYPE::STRSXP
                    && XLENGTH(attr) > 0
                {
                    tz_s = charsxp_text(STRING_ELT(attr, 0), "tz");
                }
            }
            return convert_posixct_to_posixlt(x, &tz_s);
        }

        if TYPEOF(x) == SEXPTYPE::STRSXP {
            let sample = if XLENGTH(x) > 0 {
                let ch = STRING_ELT(x, 0);
                if ch.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(CHAR(ch)).to_string_lossy().into_owned()
                }
            } else {
                String::new()
            };
            if sample == "Inf"
                || sample == "+Inf"
                || sample == "-Inf"
                || sample.eq_ignore_ascii_case("NaN")
            {
                let n = XLENGTH(x);
                let ct = Rf_allocVector3(SEXPTYPE::REALSXP, n);
                let _ct = protect(ct);
                for i in 0..n {
                    let text = charsxp_text(STRING_ELT(x, i), "x");
                    *REAL(ct).add(i as usize) =
                        crate::mainutils::essentials::parse_iso_datetime_seconds(&text)
                            .unwrap_or(NA_REAL);
                }
                return convert_posixct_to_posixlt(ct, &tz_s);
            }
            let try_fmts: &[&str] = &[
                "%Y-%m-%d %H:%M:%OS",
                "%Y/%m/%d %H:%M:%OS",
                "%Y-%m-%d %H:%M",
                "%Y/%m/%d %H:%M",
                "%Y-%m-%d",
                "%Y/%m/%d",
            ];
            let mut last = R_NilValue();
            for fmt in try_fmts {
                let fmt_s = Rf_mkString(CString::new(*fmt).unwrap_or_default().as_ptr());
                let _fmt = protect(fmt_s);
                last = do_strptime(
                    call,
                    op,
                    Rf_cons(x, Rf_cons(fmt_s, Rf_cons(tz, R_NilValue()))),
                    env,
                );
                let _last = protect(last);
                if posixlt_parses_every_string(last, x) {
                    if !tz_s.is_empty() {
                        setAttrib(
                            last,
                            Rf_install(c"tzone".as_ptr()),
                            Rf_mkString(CString::new(tz_s.as_str()).unwrap_or_default().as_ptr()),
                        );
                    }
                    return last;
                }
            }
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "character string is not in a standard unambiguous format",
            );
        }
        let (text, fmt) = if crate::mainutils::objects::inherits2(x, c"Date".as_ptr()) != 0 {
            let formatted = crate::mainutils::essentials::do_format_Date(
                call,
                op,
                Rf_cons(x, R_NilValue()),
                env,
            );
            (formatted, "%Y-%m-%d")
        } else {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "do not know how to convert 'x' to class \"POSIXlt\"",
            );
        };
        let _text = protect(text);
        let fmt_s = Rf_mkString(CString::new(fmt).unwrap_or_default().as_ptr());
        let _fmt = protect(fmt_s);
        do_strptime(
            call,
            op,
            Rf_cons(text, Rf_cons(fmt_s, Rf_cons(tz, R_NilValue()))),
            env,
        )
    }
}

/// GNU `format.POSIXlt(x, format, usetz, digits)`.
pub unsafe fn do_format_POSIXlt(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mut format = format_posix_named_arg(args, "format", 0);
        let usetz_arg = format_posix_named_arg(args, "usetz", 1);
        let digits_arg = format_posix_named_arg(args, "digits", 2);

        if format.is_null() || format == R_NilValue() || TYPEOF(format) != SEXPTYPE::STRSXP {
            format = Rf_mkString(c"".as_ptr());
        }
        let _f0 = protect(format);
        let nf = XLENGTH(format);
        let mut filled = false;
        for i in 0..nf {
            let ch = STRING_ELT(format, i);
            let empty = ch.is_null()
                || ch == R_NaString()
                || CStr::from_ptr(CHAR(ch)).to_bytes().is_empty();
            if empty {
                filled = true;
                break;
            }
        }
        let opt_digits = {
            let opt = crate::mainutils::options::GetOption1(Rf_install(c"digits.secs".as_ptr()));
            if opt.is_null() || opt == R_NilValue() {
                0
            } else {
                crate::mainutils::coerce::asInteger(opt)
            }
        };
        let explicit_digits = !digits_arg.is_null() && digits_arg != R_NilValue();
        let mut digits = if explicit_digits {
            crate::mainutils::coerce::asInteger(digits_arg)
        } else {
            opt_digits
        };
        if explicit_digits
            && digits != opt_digits
            && TYPEOF(x) == SEXPTYPE::VECSXP
            && XLENGTH(x) >= 1
        {
            let mut bare_os = false;
            for i in 0..nf {
                let ch = STRING_ELT(format, i);
                if ch.is_null() || ch == R_NaString() {
                    continue;
                }
                let s = CStr::from_ptr(CHAR(ch)).to_string_lossy();
                if let Some(pos) = s.find("%OS") {
                    match s.as_bytes().get(pos + 3) {
                        None => bare_os = true,
                        Some(b) if !b.is_ascii_digit() => bare_os = true,
                        _ => {}
                    }
                }
            }
            if bare_os {
                let sec = VECTOR_ELT(x, 0);
                // `x` is rooted by the call arguments, and this scan is pure:
                // finish the borrowed read before any later R allocation.
                digits = posixlt_seconds_digits(sec, digits);
            }
        }

        if filled && TYPEOF(x) == SEXPTYPE::VECSXP && XLENGTH(x) >= 3 {
            // Keep the non-owning span strictly inside this pure calculation;
            // the formatting allocations below begin only after it ends.
            let np = posixlt_seconds_digits(VECTOR_ELT(x, 0), digits);
            let times_zero = {
                let mut z = true;
                for comp in 0..3 {
                    let v = VECTOR_ELT(x, comp);
                    let n = XLENGTH(v);
                    for i in 0..n {
                        let val = if TYPEOF(v) == SEXPTYPE::REALSXP {
                            *REAL(v).add(i as usize)
                        } else if TYPEOF(v) == SEXPTYPE::INTSXP {
                            let iv = *INTEGER(v).add(i as usize);
                            if iv == NA_INTEGER {
                                continue;
                            }
                            iv as f64
                        } else {
                            0.0
                        };
                        if val.is_finite() && val != 0.0 {
                            z = false;
                        }
                    }
                }
                z
            };
            let repl = if times_zero {
                "%Y-%m-%d"
            } else if np == 0 {
                "%Y-%m-%d %H:%M:%S"
            } else {
                // filled below per-element
                ""
            };
            let format2 = Rf_allocVector3(SEXPTYPE::STRSXP, nf);
            let _f2 = protect(format2);
            for i in 0..nf {
                let ch = STRING_ELT(format, i);
                let empty = ch.is_null()
                    || ch == R_NaString()
                    || CStr::from_ptr(CHAR(ch)).to_bytes().is_empty();
                if empty {
                    let s = if times_zero {
                        "%Y-%m-%d".to_string()
                    } else if np == 0 {
                        "%Y-%m-%d %H:%M:%S".to_string()
                    } else {
                        format!("%Y-%m-%d %H:%M:%OS{np}")
                    };
                    let cs = CString::new(s).unwrap_or_default();
                    SET_STRING_ELT(format2, i, Rf_mkChar(cs.as_ptr()));
                } else {
                    SET_STRING_ELT(format2, i, ch);
                }
            }
            format = format2;
            let _ = repl;
        }
        let usetz = if usetz_arg.is_null() || usetz_arg == R_NilValue() {
            Rf_ScalarLogical(0)
        } else {
            usetz_arg
        };
        let _u = protect(usetz);

        let digs = Rf_ScalarInteger(digits);
        let _d = protect(digs);
        let out = do_formatPOSIXlt(
            call,
            op,
            Rf_cons(
                x,
                Rf_cons(format, Rf_cons(usetz, Rf_cons(digs, R_NilValue()))),
            ),
            env,
        );
        let _out = protect(out);
        // Observation names live on POSIXlt components (year), never on
        // the list itself — list names are sec/min/hour/...
        let mut names = R_NilValue();
        if TYPEOF(x) == SEXPTYPE::VECSXP && XLENGTH(x) >= 6 {
            names = getAttrib(VECTOR_ELT(x, 5), R_NamesSymbol());
        } else {
            names = getAttrib(x, R_NamesSymbol());
        }
        if !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && XLENGTH(names) == XLENGTH(out)
        {
            setAttrib(out, R_NamesSymbol(), names);
        }

        out
    }
}

/// GNU `format.POSIXct(x, format, tz, usetz, digits)`.
pub unsafe fn do_format_POSIXct(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let format = format_posix_named_arg(args, "format", 0);
        let tz = format_posix_named_arg(args, "tz", 1);
        let usetz = format_posix_named_arg(args, "usetz", 2);
        let digits = format_posix_named_arg(args, "digits", 3);
        let mut tz_s = tz;
        if tz_s.is_null() || tz_s == R_NilValue() {
            let attr = getAttrib(x, Rf_install(c"tzone".as_ptr()));
            if !attr.is_null() && attr != R_NilValue() {
                tz_s = attr;
            }
        }
        let lt_args = if tz_s.is_null() || tz_s == R_NilValue() {
            Rf_cons(x, R_NilValue())
        } else {
            Rf_cons(x, Rf_cons(tz_s, R_NilValue()))
        };
        let _la = protect(lt_args);
        let lt = do_as_POSIXlt(call, op, lt_args, env);
        let _lt = protect(lt);
        let mut rest = R_NilValue();
        if !digits.is_null() && digits != R_NilValue() {
            let cell = Rf_cons(digits, rest);
            SETTAG(cell, Rf_install(c"digits".as_ptr()));
            rest = cell;
        }
        if !usetz.is_null() && usetz != R_NilValue() {
            let cell = Rf_cons(usetz, rest);
            SETTAG(cell, Rf_install(c"usetz".as_ptr()));
            rest = cell;
        }
        if !format.is_null() && format != R_NilValue() {
            let cell = Rf_cons(format, rest);
            SETTAG(cell, Rf_install(c"format".as_ptr()));
            rest = cell;
        }
        let call_args = Rf_cons(lt, rest);
        let _ca = protect(call_args);
        let out = do_format_POSIXlt(call, op, call_args, env);
        let _o = protect(out);
        let names = getAttrib(x, R_NamesSymbol());
        if !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && XLENGTH(names) == XLENGTH(out)
        {
            setAttrib(out, R_NamesSymbol(), names);
        }
        out
    }
}

#[cfg(test)]
mod seconds_slice_tests {
    use super::posixlt_seconds_digits;
    use crate::sexp::{
        accessors::REAL, altrep::is_materialized, constructors::Rf_allocVector, ffi::SEXPTYPE,
        session::RSession,
    };

    #[test]
    fn posixlt_seconds_scan_materializes_compact_altrep_and_keeps_its_root() {
        let session = RSession::new_for_gc_tests();
        let compact_raw =
            session.with_active(|| unsafe { crate::sexp::altseq::compact_real_seq(12.0, 0.25, 4) });
        let compact = session.sexp(compact_raw).unwrap();
        assert!(!is_materialized(&compact));

        let digits = session.with_active(|| unsafe { posixlt_seconds_digits(compact.as_raw(), 3) });
        assert_eq!(digits, 2);
        assert!(is_materialized(&compact));

        session.gc();
        assert_eq!(compact.try_real_elt(0).unwrap(), 12.0);
        assert_eq!(compact.try_real_elt(3).unwrap(), 12.75);

        let ordinary_raw = session.with_active(|| unsafe {
            let values = Rf_allocVector(SEXPTYPE::REALSXP, 3);
            *REAL(values) = 3.0;
            *REAL(values).add(1) = 3.1;
            *REAL(values).add(2) = 3.125;
            values
        });
        let ordinary = session.sexp(ordinary_raw).unwrap();
        assert_eq!(
            session.with_active(|| unsafe { posixlt_seconds_digits(ordinary.as_raw(), 3) }),
            3
        );
    }
}

unsafe fn posixlt_as_date(call: SEXP, op: SEXP, x: SEXP, env: SEXP) -> SEXP {
    unsafe {
        if crate::mainutils::objects::inherits2(x, c"POSIXlt".as_ptr()) != 0 {
            crate::mainutils::essentials::do_as_Date(call, op, Rf_cons(x, R_NilValue()), env)
        } else {
            x
        }
    }
}

/// GNU `weekdays.POSIXt(x)`.
pub unsafe fn do_weekdays_POSIXt(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let x = posixlt_as_date(call, op, CAR(args), env);
        crate::mainutils::essentials::do_weekdays(call, op, Rf_cons(x, CDR(args)), env)
    }
}

/// GNU `months.POSIXt(x)`.
pub unsafe fn do_months_POSIXt(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let x = posixlt_as_date(call, op, CAR(args), env);
        crate::mainutils::essentials::do_months(call, op, Rf_cons(x, CDR(args)), env)
    }
}

/// GNU `quarters.POSIXt(x)`.
pub unsafe fn do_quarters_POSIXt(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let x = posixlt_as_date(call, op, CAR(args), env);
        crate::mainutils::essentials::do_quarters(call, op, Rf_cons(x, CDR(args)), env)
    }
}

/// GNU `as.character.POSIXt(x, digits, OutDec)`.
pub unsafe fn do_as_character_POSIXt(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let matched = match_named_then_positional(call, args, &["x", "digits", "OutDec", "..."]);

        let x = matched[0];
        let digits_arg = matched[1];
        let outdec_arg = matched[2];
        let is_lt = crate::mainutils::objects::inherits2(x, c"POSIXlt".as_ptr()) != 0;
        let digits = if !digits_arg.is_null()
            && digits_arg != R_NilValue()
            && digits_arg != R_MissingArg()
            && (TYPEOF(digits_arg) == SEXPTYPE::INTSXP || TYPEOF(digits_arg) == SEXPTYPE::REALSXP)
        {
            crate::mainutils::coerce::asInteger(digits_arg)
        } else if is_lt {
            14
        } else {
            6
        };
        let outdec = if !outdec_arg.is_null()
            && outdec_arg != R_NilValue()
            && TYPEOF(outdec_arg) == SEXPTYPE::STRSXP
            && XLENGTH(outdec_arg) > 0
        {
            charsxp_text(STRING_ELT(outdec_arg, 0), "OutDec")
        } else {
            ".".to_string()
        };

        let lt = if is_lt {
            x
        } else {
            do_as_POSIXlt(call, op, Rf_cons(x, R_NilValue()), env)
        };
        let _lt0 = protect(lt);
        let lt = do_balancePOSIXlt(call, op, Rf_cons(lt, R_NilValue()), env);
        let _lt = protect(lt);
        if TYPEOF(lt) != SEXPTYPE::VECSXP || XLENGTH(lt) < 6 {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        let n = XLENGTH(VECTOR_ELT(lt, 0)).max(0);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        // GNU as.character.POSIXt: options(scipen = max(digits)+1) so
        // as.character(sec) does not go scientific for tiny fractions.
        let scipen_sym = Rf_install(c"scipen".as_ptr());
        let old_scipen = crate::mainutils::options::GetOption1(scipen_sym);
        let _old_s = protect(old_scipen);
        let want = digits.max(0) + 1;
        let cur = if !old_scipen.is_null() && old_scipen != R_NilValue() {
            crate::mainutils::coerce::asInteger(old_scipen)
        } else {
            0
        };
        let _scipen = ScipenGuard {
            old: old_scipen,
            active: cur <= digits,
        };
        if cur <= digits {
            let nv = Rf_ScalarInteger(want);
            let _nv = protect(nv);
            crate::mainutils::options::SetOptionByName("scipen", nv);
        }
        let sec = VECTOR_ELT(lt, 0);

        for i in 0..n {
            let iu = i as usize;
            let s = if TYPEOF(sec) == SEXPTYPE::REALSXP {
                *REAL(sec).add(iu)
            } else {
                NA_REAL
            };
            let hour = posixlt_int_elt(VECTOR_ELT(lt, 2), iu);
            let minu = posixlt_int_elt(VECTOR_ELT(lt, 1), iu);
            let mday = posixlt_int_elt(VECTOR_ELT(lt, 3), iu);
            let mon = posixlt_int_elt(VECTOR_ELT(lt, 4), iu);
            let year = posixlt_int_elt(VECTOR_ELT(lt, 5), iu);
            let time = f64::from(hour) + f64::from(minu) + s;
            let is_na = s.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN;
            let ok = time.is_finite()
                && hour != NA_INTEGER
                && minu != NA_INTEGER
                && mday != NA_INTEGER
                && mon != NA_INTEGER
                && year != NA_INTEGER;
            let text = if !ok && is_na {
                None
            } else if !ok {
                Some(as_character_real(call, op, env, s, &outdec))
            } else {
                let date = format!("{}-{:02}-{:02}", 1900 + year, mon + 1, mday);
                if time == 0.0 {
                    Some(date)
                } else {
                    let rounded = {
                        let sx = Rf_ScalarReal(s);
                        let _sx = protect(sx);
                        let dg = Rf_ScalarInteger(digits);
                        let _dg = protect(dg);
                        let rnd = crate::mainutils::essentials::do_round(
                            call,
                            op,
                            Rf_cons(sx, Rf_cons(dg, R_NilValue())),
                            env,
                        );
                        let _r = protect(rnd);
                        if TYPEOF(rnd) == SEXPTYPE::REALSXP && XLENGTH(rnd) > 0 {
                            *REAL(rnd)
                        } else {
                            s
                        }
                    };

                    let mut sch = as_character_real(call, op, env, rounded, &outdec);
                    if rounded < 10.0 && rounded >= 0.0 {
                        sch.insert(0, '0');
                    }
                    Some(format!("{date} {hour:02}:{minu:02}:{sch}"))
                }
            };
            match text {
                None => SET_STRING_ELT(out, i, R_NaString()),
                Some(s) => {
                    let cs = CString::new(s).unwrap_or_default();
                    SET_STRING_ELT(out, i, Rf_mkChar(cs.as_ptr()));
                }
            }
        }
        out
    }
}
