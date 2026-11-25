use super::*;

pub(crate) struct ScipenGuard {
    pub(crate) old: SEXP,
    pub(crate) active: bool,
}

impl Drop for ScipenGuard {
    fn drop(&mut self) {
        if self.active {
            crate::mainutils::options::SetOptionByName("scipen", self.old);
        }
    }
}

fn r_round_digits(x: f64, digits: i32) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let digits = digits.clamp(0, 18);
    let p = 10f64.powi(digits);
    (x * p).round() / p
}

pub(crate) fn as_character_real(
    call: SEXP,
    op: SEXP,
    env: SEXP,
    value: f64,
    outdec: &str,
) -> String {
    let scalar = Rf_ScalarReal(value);
    let _s = protect(scalar);
    // GNU: if(getOption("OutDec") != OutDec) options(OutDec = OutDec)
    // for the duration (restored on exit).
    let outdec_sym = crate::sexp::symbol::install_str("OutDec");
    let old_outdec = crate::mainutils::options::GetOption1(outdec_sym);
    let _od = protect(old_outdec);
    let old_text = if !old_outdec.is_null() && old_outdec != R_NilValue() {
        charsxp_text(STRING_ELT(old_outdec, 0), "OutDec")
    } else {
        ".".to_string()
    };
    let changed = old_text != outdec;
    if changed {
        let nv = crate::sexp::constructors::mk_string_bytes(
            CString::new(outdec).unwrap_or_default().to_bytes(),
        );
        let _n = protect(nv);
        crate::mainutils::options::SetOptionByName("OutDec", nv);
    }
    let ch =
        crate::mainutils::essentials::do_as_character(call, op, Rf_cons(scalar, R_NilValue()), env);
    let _c = protect(ch);
    if changed {
        let nv = crate::sexp::constructors::mk_string_bytes(
            CString::new(old_text.as_str())
                .unwrap_or_default()
                .to_bytes(),
        );
        let _n2 = protect(nv);
        crate::mainutils::options::SetOptionByName("OutDec", nv);
    }
    if TYPEOF(ch) == SEXPTYPE::STRSXP && XLENGTH(ch) > 0 {
        charsxp_text(STRING_ELT(ch, 0), "as.character")
    } else {
        String::new()
    }
}

/// GNU `as.double.POSIXlt <- function(x, ...) as.double(as.POSIXct(x))`.
pub fn do_as_double_POSIXt(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let ct = if crate::mainutils::objects::inherits2(x, c"POSIXct") != 0
            && (TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(x) == SEXPTYPE::INTSXP)
        {
            x
        } else {
            crate::mainutils::essentials::do_as_POSIXct(call, op, Rf_cons(x, R_NilValue()), env)
        };
        let _ct = protect(ct);
        let n = XLENGTH(ct);
        let out = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _o = protect(out);
        if TYPEOF(ct) == SEXPTYPE::REALSXP {
            for i in 0..n {
                *REAL(out).add(i as usize) = *REAL(ct).add(i as usize);
            }
        } else if TYPEOF(ct) == SEXPTYPE::INTSXP {
            for i in 0..n {
                let v = *INTEGER(ct).add(i as usize);
                *REAL(out).add(i as usize) = if v == NA_INTEGER {
                    NA_REAL
                } else {
                    f64::from(v)
                };
            }
        }

        let names = getAttrib(ct, R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() {
            setAttrib(out, R_NamesSymbol(), names);
        }
        out
    }
}

/// Build a CString from an owned string (helper for the code above).
pub(crate) fn cstring_or_empty(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

/// Read `tzname[idx]` as an owned string.
pub(crate) fn tzname_str(idx: usize) -> String {
    unsafe {
        let p = *R_tzname().add(idx);
        if p.is_null() {
            String::new()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

// ---------------------------------------------------------------------------
// do_D2POSIXlt -- .Internal(Date2POSIXlt(x))
// ---------------------------------------------------------------------------

/// Convert a Date (numeric days since epoch) to a POSIXlt list in UTC.
///
/// Ported from `do_D2POSIXlt()` in datetime.c.
pub fn do_D2POSIXlt(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        // GNU do_D2POSIXlt: optional second arg tz; empty -> "UTC".
        let tz_name = {
            let t = CDR(args);
            if !t.is_null() && t != R_NilValue() {
                let stz = CAR(t);
                if TYPEOF(stz) == SEXPTYPE::STRSXP && XLENGTH(stz) > 0 {
                    let ch = STRING_ELT(stz, 0);
                    if !ch.is_null() && ch != R_NaString() {
                        let s =
                            String::from_utf8_lossy(&crate::sexp::accessors::char_cstr_bytes(ch))
                                .into_owned();
                        if s.is_empty() { "UTC".to_string() } else { s }
                    } else {
                        "UTC".to_string()
                    }
                } else {
                    std::panic::panic_any(RError {
                        message: "invalid 'tz' value".to_string(),
                    });
                }
            } else {
                "UTC".to_string()
            }
        };
        let _x_guard = protect(x);
        if TYPEOF(x) != SEXPTYPE::REALSXP && TYPEOF(x) != SEXPTYPE::INTSXP {
            std::panic::panic_any(RError {
                message: "invalid 'x' value: not numeric".to_string(),
            });
        }

        let n = XLENGTH(x);
        let (ans, ansnames) = make_posixlt_skeleton(n);
        let _ans_guard = protect(ans);
        let _ansnames_guard = protect(ansnames);

        for i in 0..n {
            let iu = i as usize;
            let x_i = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(iu)
            } else {
                let v = *INTEGER(x).add(iu);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            };
            let mut tm = stm::new();
            let valid = julian2dtime(x_i, &mut tm);

            makelt(
                &tm,
                ans,
                i as R_xlen_t,
                valid,
                if valid { 0.0 } else { x_i },
            );

            // zone = tz, gmtoff = 0
            let zone_cstr = CString::new(tz_name.as_str()).unwrap_or_default();
            SET_STRING_ELT(
                VECTOR_ELT(ans, 9),
                i as R_xlen_t,
                crate::sexp::constructors::mk_char_bytes(zone_cstr.to_bytes()),
            );
            *INTEGER(VECTOR_ELT(ans, 10)).add(iu) = 0;
        }
        let tzone = crate::sexp::constructors::mk_string_bytes(
            CString::new(tz_name.as_str())
                .unwrap_or_default()
                .to_bytes(),
        );
        let _tz = protect(tzone);
        finish_posixlt(ans, ansnames, tzone);
        let names = getAttrib(x, R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() {
            // GNU POSIXlt keeps observation names on the year component only.
            let elt = VECTOR_ELT(ans, 5);
            if !elt.is_null() && elt != R_NilValue() {
                setAttrib(elt, R_NamesSymbol(), names);
            }
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// do_POSIXlt2D -- .Internal(POSIXlt2Date(x))
// ---------------------------------------------------------------------------

/// Convert a POSIXlt list to a Date (numeric days since epoch).
///
/// Ported from `do_POSIXlt2D()` in datetime.c.
#[allow(clippy::if_same_then_else)]
pub fn do_POSIXlt2D(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let _x_guard = protect(x);
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            std::panic::panic_any(RError {
                message: "a valid \"POSIXlt\" object is a list of at least 9 elements".to_string(),
            });
        }

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

            tm.tm_isdst = 0; // always UTC for Date conversion

            if !R_FINITE(secs) {
                *REAL(ans).add(iu) = secs;
            } else if tm.tm_min == NA_INTEGER
                || tm.tm_hour == NA_INTEGER
                || tm.tm_mday == NA_INTEGER
                || tm.tm_mon == NA_INTEGER
                || tm.tm_year == NA_INTEGER
            {
                *REAL(ans).add(iu) = NA_REAL;
            } else if validate_tm(&mut tm) < 0 {
                *REAL(ans).add(iu) = NA_REAL;
            } else {
                *REAL(ans).add(iu) = mkdate00(&mut tm);
            }
        }

        // GNU tail: propagate names from the year component and attach the
        // "Date" class (do_POSIXlt2D in datetime.c).
        let nm = getAttrib(VECTOR_ELT(x, 5), R_NamesSymbol());
        if !nm.is_null() && nm != R_NilValue() {
            setAttrib(ans, R_NamesSymbol(), nm);
        }
        let klass = crate::sexp::constructors::mk_string_str("Date");
        let _klass_guard = protect(klass);
        setAttrib(ans, R_ClassSymbol(), klass);
        ans
    }
}

fn logical_arg_true(arg: SEXP, default: bool) -> bool {
    if arg.is_null() || arg == R_NilValue() || arg == R_MissingArg() {
        return default;
    }
    let a = wrapped(arg);
    if TYPEOF(arg) == SEXPTYPE::LGLSXP && XLENGTH(arg) > 0 {
        return a.try_logical_elt(0).map(|v| v == TRUE).unwrap_or(default);
    }
    if TYPEOF(arg) == SEXPTYPE::INTSXP && XLENGTH(arg) > 0 {
        return a
            .try_integer_elt(0)
            .map(|v| v != 0 && v != NA_INTEGER)
            .unwrap_or(default);
    }
    default
}

fn recycle_posixlt_year_names(ans: SEXP, src: SEXP, n: R_xlen_t) {
    if n <= 0 || XLENGTH(src) < 6 || XLENGTH(ans) < 6 {
        return;
    }
    let year_src = VECTOR_ELT(src, 5);
    let nm = getAttrib(year_src, R_NamesSymbol());
    if nm.is_null() || nm == R_NilValue() || TYPEOF(nm) != SEXPTYPE::STRSXP {
        return;
    }
    let ni = XLENGTH(nm);
    if ni <= 0 {
        return;
    }
    let out_nm = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    let _g = protect(out_nm);
    for i in 0..n {
        SET_STRING_ELT(out_nm, i, STRING_ELT(nm, i % ni));
    }
    setAttrib(VECTOR_ELT(ans, 5), R_NamesSymbol(), out_nm);
}

fn recycle_vector_mod(src: SEXP, n: R_xlen_t) -> SEXP {
    unsafe {
        let ni = XLENGTH(src);
        if ni == n {
            return crate::mainutils::duplicate::Rf_duplicate(src);
        }
        let ty = TYPEOF(src);
        let out = Rf_allocVector3(ty, n);
        let _o = protect(out);
        if ni <= 0 {
            return out;
        }
        if ty == SEXPTYPE::REALSXP {
            for i in 0..n {
                *REAL(out).add(i as usize) = *REAL(src).add((i % ni) as usize);
            }
        } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
            for i in 0..n {
                *INTEGER(out).add(i as usize) = *INTEGER(src).add((i % ni) as usize);
            }
        } else if ty == SEXPTYPE::STRSXP {
            for i in 0..n {
                SET_STRING_ELT(out, i, STRING_ELT(src, i % ni));
            }
        }
        let nm = getAttrib(src, R_NamesSymbol());
        if !nm.is_null() && nm != R_NilValue() && TYPEOF(nm) == SEXPTYPE::STRSXP && XLENGTH(nm) > 0
        {
            let nmi = XLENGTH(nm);
            let out_nm = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _g = protect(out_nm);
            for i in 0..n {
                SET_STRING_ELT(out_nm, i, STRING_ELT(nm, i % nmi));
            }
            setAttrib(out, R_NamesSymbol(), out_nm);
        }
        out
    }
}

fn balance_posixlt_fill_only(
    x: SEXP,
    n: R_xlen_t,
    nlen: &[i64; 11],
    nn: i32,
    keep_class: bool,
) -> SEXP {
    let ans = crate::mainutils::duplicate::Rf_duplicate(x);
    let _a = protect(ans);
    for i in 0..nn {
        let iu = i as usize;
        if nlen[iu] != n {
            SET_VECTOR_ELT(
                ans,
                i as R_xlen_t,
                recycle_vector_mod(VECTOR_ELT(x, i as R_xlen_t), n),
            );
        }
    }
    recycle_posixlt_year_names(ans, x, n);
    if !keep_class {
        setAttrib(ans, R_ClassSymbol(), R_NilValue());
    }
    setAttrib(
        ans,
        crate::sexp::symbol::install_str("balanced"),
        Rf_ScalarLogical(NA_INTEGER),
    );
    ans
}

/// Ported from `do_balancePOSIXlt()` in datetime.c.
pub fn do_balancePOSIXlt(call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let matched = match_named_then_positional(call, args, &["x", "fill.only", "classed"]);

        let x = matched[0];
        // GNU do_balancePOSIXlt branches on PRIMVAL(op): code 1 is
        // unCfillPOSIXlt(x), i.e. fill-only and unclassed.
        let offset_code = if _op.is_null() {
            0
        } else {
            let idx = crate::sexp::accessors::PRIMOFFSET(_op);
            crate::mainutils::names::R_FunTab
                .get(idx as usize)
                .map(|entry| entry.offset)
                .unwrap_or(0)
        };
        let fill_only = if offset_code == 1 {
            true
        } else {
            logical_arg_true(matched[1], false)
        };
        let keep_class = if offset_code == 1 {
            false
        } else {
            logical_arg_true(matched[2], true)
        };

        if TYPEOF(x) != SEXPTYPE::VECSXP {
            std::panic::panic_any(RError {
                message: "a valid \"POSIXlt\" object is a list of at least 9 elements".to_string(),
            });
        }
        let bal = getAttrib(x, crate::sexp::symbol::install_str("balanced"));
        if !bal.is_null()
            && bal != R_NilValue()
            && TYPEOF(bal) == SEXPTYPE::LGLSXP
            && XLENGTH(bal) > 0
            && *INTEGER(bal) == TRUE
        {
            if keep_class {
                return x;
            }
            let out = crate::mainutils::duplicate::Rf_duplicate(x);
            let _o = protect(out);
            setAttrib(out, R_ClassSymbol(), R_NilValue());
            return out;
        }

        let n_comp = LENGTH(x);
        if n_comp < 9 {
            std::panic::panic_any(RError {
                message: "a valid \"POSIXlt\" object is a list of at least 9 elements".to_string(),
            });
        }

        let nn = std::cmp::min(n_comp, 11);
        let mut nlen = [0i64; 11];
        let mut n: R_xlen_t = 0;
        for i in 0..nn {
            let len = XLENGTH(VECTOR_ELT(x, i as R_xlen_t));
            nlen[i as usize] = len;
            if len > n {
                n = len;
            }
        }

        if fill_only {
            return balance_posixlt_fill_only(x, n, &nlen, nn, keep_class);
        }

        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, nn as R_xlen_t);
        let _ans_guard = protect(ans);
        for i in 0..9 {
            let sexp: c_int = if i > 0 {
                SEXPTYPE::INTSXP.into()
            } else {
                SEXPTYPE::REALSXP.into()
            };
            SET_VECTOR_ELT(ans, i as R_xlen_t, Rf_allocVector3(sexp, n));
        }
        if nn >= 10 {
            SET_VECTOR_ELT(ans, 9, Rf_allocVector3(SEXPTYPE::STRSXP, n));
        }
        if nn >= 11 {
            SET_VECTOR_ELT(ans, 10, Rf_allocVector3(SEXPTYPE::INTSXP, n));
        }

        let ansnames = Rf_allocVector3(SEXPTYPE::STRSXP, nn as R_xlen_t);
        let _ansnames_guard = protect(ansnames);
        for i in 0..nn {
            let cstr = CString::new(ltnames[i as usize]).unwrap_or_default();
            SET_STRING_ELT(
                ansnames,
                i as R_xlen_t,
                crate::sexp::constructors::mk_char_bytes(cstr.to_bytes()),
            );
        }

        for i in 0..n {
            let iu = i as usize;
            let idx = |comp: usize| -> usize {
                let len = nlen[comp] as usize;
                if len == 0 { 0 } else { iu % len }
            };
            let sec_col = VECTOR_ELT(x, 0);
            let secs = if TYPEOF(sec_col) == SEXPTYPE::REALSXP && XLENGTH(sec_col) > 0 {
                *REAL(sec_col).add(idx(0))
            } else if TYPEOF(sec_col) == SEXPTYPE::INTSXP && XLENGTH(sec_col) > 0 {
                let v = *INTEGER(sec_col).add(idx(0));
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            let fsecs = secs.floor();

            let mut tm = stm::new();
            tm.tm_sec = if R_FINITE(secs) {
                fsecs as c_int
            } else {
                NA_INTEGER
            };
            tm.tm_min = posixlt_int_elt(VECTOR_ELT(x, 1), idx(1));
            tm.tm_hour = posixlt_int_elt(VECTOR_ELT(x, 2), idx(2));
            tm.tm_mday = posixlt_int_elt(VECTOR_ELT(x, 3), idx(3));
            tm.tm_mon = posixlt_int_elt(VECTOR_ELT(x, 4), idx(4));
            tm.tm_year = posixlt_int_elt(VECTOR_ELT(x, 5), idx(5));
            tm.tm_wday = posixlt_int_elt(VECTOR_ELT(x, 6), idx(6));
            tm.tm_yday = posixlt_int_elt(VECTOR_ELT(x, 7), idx(7));
            tm.tm_isdst = posixlt_int_elt(VECTOR_ELT(x, 8), idx(8));

            let valid = R_FINITE(secs)
                && tm.tm_min != NA_INTEGER
                && tm.tm_hour != NA_INTEGER
                && tm.tm_mday != NA_INTEGER
                && tm.tm_mon != NA_INTEGER
                && tm.tm_year != NA_INTEGER;

            if valid {
                validate_tm(&mut tm);
                mkdate00(&mut tm);
            }

            makelt(
                &tm,
                ans,
                i as R_xlen_t,
                valid,
                if valid {
                    secs - fsecs
                } else if R_FINITE(secs) {
                    NA_REAL
                } else {
                    secs
                },
            );

            if nn >= 10 {
                let zone_col = VECTOR_ELT(x, 9);
                let mut zone = String::new();
                if TYPEOF(zone_col) == SEXPTYPE::STRSXP && nlen[9] > 0 {
                    let ch = STRING_ELT(zone_col, idx(9) as R_xlen_t);
                    if !ch.is_null() && ch != R_NaString() {
                        zone = charsxp_text(ch, "zone");
                    }
                }
                let out_zone = if valid && tm.tm_isdst >= 0 {
                    if zone.is_empty() {
                        tzname_str(tm.tm_isdst.clamp(0, 1) as usize)
                    } else {
                        zone
                    }
                } else {
                    String::new()
                };
                SET_STRING_ELT(
                    VECTOR_ELT(ans, 9),
                    i as R_xlen_t,
                    crate::sexp::constructors::mk_char_bytes(
                        CString::new(out_zone).unwrap_or_default().to_bytes(),
                    ),
                );
            }
            if nn >= 11 {
                let gmtoff = if nlen[10] > 0 {
                    posixlt_int_elt(VECTOR_ELT(x, 10), idx(10))
                } else {
                    NA_INTEGER
                };
                *INTEGER(VECTOR_ELT(ans, 10)).add(iu) = if valid { gmtoff } else { NA_INTEGER };
            }
        }

        setAttrib(ans, R_NamesSymbol(), ansnames);
        if keep_class {
            let klass = getAttrib(x, R_ClassSymbol());
            if !klass.is_null() && klass != R_NilValue() {
                setAttrib(ans, R_ClassSymbol(), klass);
            }
        }
        recycle_posixlt_year_names(ans, x, n);
        let tzone = getAttrib(x, crate::sexp::symbol::install_str("tzone"));
        if !tzone.is_null() && tzone != R_NilValue() {
            setAttrib(ans, crate::sexp::symbol::install_str("tzone"), tzone);
        }
        setAttrib(
            ans,
            crate::sexp::symbol::install_str("balanced"),
            Rf_ScalarLogical(TRUE),
        );
        ans
    }
}

// ---------------------------------------------------------------------------
// do_Sys_time -- Sys.time()
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// do_difftime -- difftime(time1, time2, units)
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// do_ISOdatetime -- ISOdatetime(year, month, day, hour, min, sec, tz)
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// R_isLeapYear -- FFI-compatible leap year check (absolute year)
// ---------------------------------------------------------------------------

/// Check whether a year (absolute, e.g. 2000) is a leap year.
/// This is the FFI-compatible version using absolute years.
pub fn R_isLeapYear(year: c_int) -> c_int {
    if isleap(year) { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// FFI-compatible standalone functions
// ---------------------------------------------------------------------------











// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
