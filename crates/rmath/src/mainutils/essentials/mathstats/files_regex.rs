//! sample, toString, normalizePath, tempfile, proc.time, regex matching builtins — extracted verbatim from essentials/mathstats.rs.

use super::*;
use crate::mainutils::grep::{EmptyPatternMode, empty_pattern_positions};

/// R sample.int(n, size = n, replace = FALSE) — uniform sampling from 1:n.
pub unsafe fn do_sample_int(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = real_or_default(CAR(args), 1.0) as i64;
        let size = CAR(CDR(args));
        let replace = CAR(CDR(CDR(args)));
        let prob = CAR(CDR(CDR(CDR(args))));
        crate::mainutils::rng_dispatch::sample_int_values(n, size, replace, prob)
    }
}
/// GNU `toString.default(x, width = NULL)` (base toString.R):
/// `paste(x, collapse = ", ")` — classed objects dispatch as.character —
/// then, when `width` is a positive number and the string is wider, trim to
/// `strtrim(string, width - 4)` and append `"...."` (`max(6, width)` leaves
/// something visible); `width < 0` errors, `0`/NULL keeps the full string.
pub unsafe fn do_toString(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_mkString(c"".as_ptr());
        }
        // paste(x, collapse = ", "): dispatch as.character for classed objects.
        let chars = if crate::sexp::attrib_core::isObject(x) != 0 {
            let coerce = crate::sexp::constructors::Rf_lang2(
                crate::sexp::symbol::Rf_install(c"as.character".as_ptr()),
                x,
            );
            let _coerce_guard = protect(coerce);
            crate::eval::eval::Rf_eval(coerce, rho)
        } else {
            x
        };
        let n = if chars.is_null() || chars == R_NilValue() {
            0
        } else {
            XLENGTH(chars)
        };
        let mut parts: Vec<String> = Vec::new();
        for i in 0..n {
            parts.push(elt_to_string(chars, i));
        }
        let mut string = parts.join(", ");

        let width_arg = CAR(CDR(args));
        if !width_arg.is_null() && width_arg != R_NilValue() && width_arg != R_MissingArg() {
            let w = crate::mainutils::seq::asInteger(width_arg);
            if w < 0 {
                crate::sexp::context::r_error("'width' must be positive");
            }
            if w != 0 && (w == NA_INTEGER || string.chars().count() as c_int > w) {
                if w == NA_INTEGER {
                    crate::sexp::context::r_error("missing value where TRUE/FALSE needed");
                }
                let w = 6.max(w);
                let keep = (w - 4) as usize;
                string = format!("{}....", string.chars().take(keep).collect::<String>());
            }
        }
        Rf_mkString(CString::new(string).unwrap_or_default().as_ptr())
    }
}

/// R normalizePath(path)
pub unsafe fn do_normalizePath(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut path_arg = R_NilValue();
        let mut must_work_arg = R_NilValue();
        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match tag_name(current).as_deref() {
                Some("path") => path_arg = value,
                Some("mustWork") => must_work_arg = value,
                Some("winslash") => {}
                Some(_) => {}
                None => {
                    match positional {
                        0 => path_arg = value,
                        1 => {}
                        2 => must_work_arg = value,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        if path_arg.is_null() || path_arg == R_NilValue() {
            return R_NilValue();
        }

        let must_work = if must_work_arg.is_null()
            || must_work_arg == R_NilValue()
            || XLENGTH(must_work_arg) == 0
        {
            NA_INTEGER
        } else {
            *LOGICAL(must_work_arg)
        };

        let n = XLENGTH(path_arg);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _result_guard = protect(result);
        for i in 0..n {
            let elt = STRING_ELT(path_arg, i);
            if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                if must_work == TRUE {
                    base_error(format!(
                        "path[{}]=\"{}\": No such file or directory",
                        i + 1,
                        "NA"
                    ));
                }
                if must_work == NA_INTEGER {
                    let msg = std::ffi::CString::new(format!(
                        "path[{}]=\"NA\": No such file or directory",
                        i + 1
                    ))
                    .unwrap_or_default();
                    crate::mainutils::errors::Rf_warning(msg.as_ptr());
                }
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }

            let path = CStr::from_ptr(CHAR(elt)).to_str().unwrap_or("").to_string();
            if crate::library::datasets::path_exists(&path) {
                SET_STRING_ELT(result, i, elt);
                continue;
            }
            match std::fs::canonicalize(&path) {
                Ok(p) => SET_STRING_ELT(
                    result,
                    i,
                    crate::sexp::constructors::Rf_mkChar(
                        CString::new(p.to_string_lossy().as_ref())
                            .unwrap_or_default()
                            .as_ptr(),
                    ),
                ),
                Err(err) => {
                    if must_work == TRUE {
                        base_error(format!("path[{}]=\"{}\": {}", i + 1, path, err));
                    }
                    SET_STRING_ELT(
                        result,
                        i,
                        crate::sexp::constructors::Rf_mkChar(
                            CString::new(path).unwrap_or_default().as_ptr(),
                        ),
                    );
                }
            }
        }
        result
    }
}

/// R tempfile(pattern = "file", tmpdir = tempdir(), fileext = "")
pub unsafe fn do_tempfile(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut pattern: Option<SEXP> = None;
        let mut tmpdir: Option<SEXP> = None;
        let mut fileext: Option<SEXP> = None;
        let mut positional: Vec<SEXP> = Vec::new();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let value = CAR(cell);
            let tagged = !tag.is_null() && tag != R_NilValue();
            if tagged {
                let name = std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(
                    crate::sexp::accessors::PRINTNAME(tag),
                ))
                .to_string_lossy()
                .into_owned();
                let nonempty = !value.is_null() && value != R_NilValue() && XLENGTH(value) > 0;
                match name.as_str() {
                    "pattern" if nonempty => pattern = Some(value),
                    "tmpdir" if nonempty => tmpdir = Some(value),
                    "fileext" if nonempty => fileext = Some(value),
                    _ => {}
                }
            } else {
                positional.push(value);
            }
            cell = CDR(cell);
        }
        for (i, value) in positional.iter().copied().enumerate() {
            if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
                continue;
            }
            match i {
                0 if pattern.is_none() => pattern = Some(value),
                1 if tmpdir.is_none() => tmpdir = Some(value),
                2 if fileext.is_none() => fileext = Some(value),
                _ => {}
            }
        }
        let n1 = pattern.map(|sx| XLENGTH(sx)).unwrap_or(1).max(1);
        let n2 = tmpdir.map(|sx| XLENGTH(sx)).unwrap_or(1).max(1);
        let n3 = fileext.map(|sx| XLENGTH(sx)).unwrap_or(1).max(1);
        let n = n1.max(n2).max(n3);
        let default_tmp = crate::sexp::instance::with_required_current_instance(|inst| {
            (*inst).path_policy.temp_dir().to_path_buf()
        });
        let ans = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _ans = protect(ans);
        for i in 0..n {
            let pat = pattern
                .map(|sx| elt_to_string(sx, i % n1))
                .unwrap_or_else(|| "file".to_string());
            let dir = tmpdir
                .map(|sx| PathBuf::from(elt_to_string(sx, i % n2)))
                .unwrap_or_else(|| default_tmp.clone());
            let ext = fileext
                .map(|sx| elt_to_string(sx, i % n3))
                .unwrap_or_default();
            let path = crate::mainutils::sysutils::R_tmpnam2(&pat, &dir.to_string_lossy(), &ext)
                .unwrap_or_else(|| base_error("cannot find an unused temporary filename"));
            SET_STRING_ELT(
                ans,
                i,
                Rf_mkChar(CString::new(path.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        ans
    }
}

/// R tempdir()
pub unsafe fn do_tempdir(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let temp_dir = session_temp_dir();
        let _ = std::fs::create_dir_all(&temp_dir);
        Rf_mkString(
            CString::new(temp_dir.to_string_lossy().as_ref())
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

fn session_temp_dir() -> PathBuf {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).path_policy.temp_dir().to_path_buf()
    })
}

/// R proc.time()
pub unsafe fn do_proc_time(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 5);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        // Wall-clock elapsed since first observation, rounded to
        // milliseconds exactly like GNU's proc.time() (times.c).
        static SESSION_START: std::sync::LazyLock<std::time::Instant> =
            std::sync::LazyLock::new(std::time::Instant::now);
        let elapsed = SESSION_START.elapsed().as_secs_f64();
        // user/sys CPU split (own and children) from getrusage via the
        // sys_unix port of times.c:R_getProcTime; on wasm32 these stay 0.
        // CPU ticks are floored to the clock increment (CLK_TCK) like
        // times(2) reporting, so sub-tick interpreted work matches the
        // sub-tick native work GNU reports as 0.
        let mut times = [0.0f64; 5];
        crate::unix::sys_unix::R_getProcTime(times.as_mut_ptr());
        let tick = crate::unix::sys_unix::R_getClockIncrement();
        let ticks = |secs: f64| (secs / tick).floor() * tick;
        *REAL(result).add(0) = ticks(times[0]);
        *REAL(result).add(1) = ticks(times[1]);
        *REAL(result).add(2) = 1e-3 * (1000.0 * elapsed).round();
        *REAL(result).add(3) = ticks(times[3]);
        *REAL(result).add(4) = ticks(times[4]);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 5);
        if !names.is_null() {
            let _np = protect(names);
            for (i, name) in [
                "user.self",
                "sys.self",
                "elapsed",
                "user.child",
                "sys.child",
            ]
            .iter()
            .enumerate()
            {
                let cstr = CString::new(*name).unwrap_or_default();
                SET_STRING_ELT(names, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
        }
        let class = Rf_mkString(c"proc_time".as_ptr());
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class);
        result
    }
}

/// GNU `system.time(expr, gcFirst = TRUE)` — evaluate `expr` and return a
/// `proc_time` delta (`structure(new.time - time, class="proc_time")`,
/// base/time.R). The child-time columns stay at zero (no subprocesses).
pub unsafe fn do_system_time(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let before = do_proc_time(_call, _op, R_NilValue(), rho);
        let _before_guard = protect(before);
        if !expr.is_null() && expr != R_NilValue() {
            let _ = crate::eval::eval::Rf_eval(expr, rho);
        }
        let after = do_proc_time(_call, _op, R_NilValue(), rho);
        let _after_guard = protect(after);
        for i in 0..XLENGTH(after) {
            *REAL(after).add(i as usize) -= *REAL(before).add(i as usize);
        }
        after
    }
}

/// The ASCII fast path retains GNU's byte-attribution attributes. Unicode
/// character mode publishes character counts and no byte attributes.
unsafe fn regex_coordinates(args: SEXP, pattern: &str, text: SEXP) -> (bool, bool) {
    unsafe {
        let requested = logical_arg_by_name_or_position(args, "useBytes", 5).unwrap_or(false);
        let ascii =
            pattern.is_ascii() && (0..XLENGTH(text)).all(|i| elt_to_string(text, i).is_ascii());
        (requested, requested || ascii)
    }
}

fn regex_span(text: &str, start: usize, end: usize, bytes: bool) -> (c_int, c_int) {
    if bytes {
        ((start + 1) as c_int, (end - start) as c_int)
    } else {
        (
            (text[..start].chars().count() + 1) as c_int,
            text[start..end].chars().count() as c_int,
        )
    }
}

unsafe fn regex_index_attrs(result: SEXP, requested: bool, byte_mode: bool) {
    if byte_mode {
        unsafe {
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(c"index.type".as_ptr()),
                Rf_mkString(if requested {
                    c"bytes".as_ptr()
                } else {
                    c"chars".as_ptr()
                }),
            );
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(c"useBytes".as_ptr()),
                Rf_ScalarLogical(TRUE),
            );
        }
    }
}

/// R regexpr(pattern, text) — port of grep.c:do_regexpr.
///
/// With perl = TRUE and capture groups in the pattern, attaches the
/// capture.start / capture.length matrices (one row per text element, one
/// column per group, column-major, dimnames list(NULL, capture.names)) and
/// the capture.names vector, mirroring grep.c's pcre2_pattern_info +
/// extract_match_and_groups: non-participating groups yield start 0 /
/// length 0 (PCRE2_UNSET arithmetic), non-matching elements -1 / -1, and
/// NA text elements leave the NA initialization (PR#16484).
pub unsafe fn do_regexpr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern = CAR(args);
        if pattern.is_null()
            || TYPEOF(pattern) != SEXPTYPE::STRSXP
            || XLENGTH(pattern) == 0
            || STRING_ELT(pattern, 0) == crate::sexp::globals::R_NaString()
        {
            super::base_error("invalid 'pattern' argument");
        }
        let pat = elt_to_string(CAR(args), 0);
        let text = CAR(CDR(args));
        let ignore_case = logical_arg_by_name_or_position(args, "ignore.case", 2).unwrap_or(false);
        let mut perl = logical_arg_by_name_or_position(args, "perl", 3).unwrap_or(false);
        let fixed = logical_arg_by_name_or_position(args, "fixed", 4).unwrap_or(false);
        super::shared::ignore_perl_when_fixed(&mut perl, fixed);
        crate::mainutils::grep::validate_pattern(&pat, ignore_case, perl, fixed)
            .unwrap_or_else(|message| super::base_error(&message));
        let (requested_bytes, byte_mode) = regex_coordinates(args, &pat, text);

        let n = XLENGTH(text);

        // grep.c drops perl when fixed = TRUE, so only a genuine perl run
        // gets capture attribution. A pattern that fails to compile here
        // also fails the per-element match below (reported as no match).
        let (capture_count, capture_names) = if perl && !fixed {
            crate::mainutils::grep::perl_group_info(&pat, ignore_case)
                .unwrap_or_else(|| (0, Vec::new()))
        } else {
            (0, Vec::new())
        };

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let match_len = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if match_len.is_null() {
            return R_NilValue();
        }
        let _mlp = protect(match_len);

        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(c"match.length".as_ptr()),
            match_len,
        );
        regex_index_attrs(result, requested_bytes, byte_mode);

        // Capture matrices, initialized to NA so NA text elements keep NA
        // entries (grep.c PR#16484); overwritten per match below.
        let mut capture_start = R_NilValue();
        let mut capture_len = R_NilValue();
        if capture_count > 0 {
            let names_sexp = Rf_allocVector3(SEXPTYPE::STRSXP, capture_count as R_xlen_t);
            if names_sexp.is_null() {
                return R_NilValue();
            }
            let _nsg = protect(names_sexp);
            for (g, name) in capture_names.iter().enumerate() {
                let cstr = CString::new(name.as_str()).unwrap_or_default();
                SET_STRING_ELT(names_sexp, g as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
            }

            capture_start = alloc_int_matrix(n, capture_count);
            if capture_start.is_null() {
                return R_NilValue();
            }
            let _csp = protect(capture_start);
            capture_len = alloc_int_matrix(n, capture_count);
            if capture_len.is_null() {
                return R_NilValue();
            }
            let _clp = protect(capture_len);

            let dmn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            if dmn.is_null() {
                return R_NilValue();
            }
            let _dmp = protect(dmn);
            SET_VECTOR_ELT(dmn, 0, R_NilValue());
            SET_VECTOR_ELT(dmn, 1, names_sexp);
            crate::sexp::attrib_core::setAttrib(
                capture_start,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dmn,
            );
            crate::sexp::attrib_core::setAttrib(
                capture_len,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dmn,
            );

            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(c"capture.start".as_ptr()),
                capture_start,
            );
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(c"capture.length".as_ptr()),
                capture_len,
            );
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(c"capture.names".as_ptr()),
                names_sexp,
            );

            let total = (n as usize) * capture_count;
            for j in 0..total {
                *INTEGER(capture_start).add(j) = NA_INTEGER;
                *INTEGER(capture_len).add(j) = NA_INTEGER;
            }
        }

        for i in 0..n {
            // grep.c: NA text elements yield NA, capture entries stay NA.
            if TYPEOF(text) == SEXPTYPE::STRSXP
                && STRING_ELT(text, i) == crate::sexp::globals::R_NaString()
            {
                *INTEGER(result).add(i as usize) = NA_INTEGER;
                *INTEGER(match_len).add(i as usize) = NA_INTEGER;
                continue;
            }

            let txt = elt_to_string(text, i);
            if fixed {
                match fixed_find(&txt, &pat, ignore_case) {
                    Some(m) => {
                        *INTEGER(result).add(i as usize) =
                            regex_span(&txt, m.start, m.end, byte_mode).0;
                        *INTEGER(match_len).add(i as usize) =
                            regex_span(&txt, m.start, m.end, byte_mode).1;
                    }
                    None => {
                        *INTEGER(result).add(i as usize) = -1;
                        *INTEGER(match_len).add(i as usize) = -1;
                    }
                }
            } else if capture_count > 0 {
                // perl run with capture groups: one full-capture match
                // fills the overall position and every group column.
                let caps = crate::mainutils::grep::mode_captures(
                    &pat,
                    &txt,
                    ignore_case,
                    true,
                    requested_bytes,
                );
                match caps
                    .as_ref()
                    .and_then(|c| c.first())
                    .and_then(|c| c.as_ref())
                {
                    Some(whole) => {
                        *INTEGER(result).add(i as usize) =
                            regex_span(&txt, whole.start, whole.end, byte_mode).0;
                        *INTEGER(match_len).add(i as usize) =
                            regex_span(&txt, whole.start, whole.end, byte_mode).1;
                        for g in 0..capture_count {
                            let ind = i as usize + g * n as usize;
                            match caps
                                .as_ref()
                                .and_then(|c| c.get(g + 1))
                                .and_then(|c| c.as_ref())
                            {
                                Some(cm) => {
                                    *INTEGER(capture_start).add(ind) =
                                        regex_span(&txt, cm.start, cm.end, byte_mode).0;
                                    *INTEGER(capture_len).add(ind) =
                                        regex_span(&txt, cm.start, cm.end, byte_mode).1;
                                }
                                None => {
                                    // grep.c's ovector_extract_start_length on
                                    // a PCRE2_UNSET group computes
                                    // start = -1 + 1 = 0 and length = 0.
                                    *INTEGER(capture_start).add(ind) = 0;
                                    *INTEGER(capture_len).add(ind) = 0;
                                }
                            }
                        }
                    }
                    None => {
                        *INTEGER(result).add(i as usize) = -1;
                        *INTEGER(match_len).add(i as usize) = -1;
                        fill_capture_no_match(
                            capture_start,
                            capture_len,
                            i as usize,
                            n,
                            capture_count,
                        );
                    }
                }
            } else {
                let found = if perl {
                    crate::mainutils::grep::mode_captures(
                        &pat,
                        &txt,
                        ignore_case,
                        true,
                        requested_bytes,
                    )
                    .and_then(|c| c.first().copied().flatten())
                } else {
                    crate::mainutils::grep::ere_find_at(&pat, &txt, ignore_case, 0, requested_bytes)
                };
                match found {
                    Some(m) => {
                        *INTEGER(result).add(i as usize) =
                            regex_span(&txt, m.start, m.end, byte_mode).0;
                        *INTEGER(match_len).add(i as usize) =
                            regex_span(&txt, m.start, m.end, byte_mode).1;
                    }
                    None => {
                        *INTEGER(result).add(i as usize) = -1;
                        *INTEGER(match_len).add(i as usize) = -1;
                    }
                }
            }
        }

        result
    }
}

/// Column-major integer matrix with dim c(nrow, ncol) — the shape grep.c's
/// do_regexpr uses for capture.start / capture.length.
unsafe fn alloc_int_matrix(nrow: R_xlen_t, ncol: usize) -> SEXP {
    unsafe {
        let ans = Rf_allocVector3(SEXPTYPE::INTSXP, nrow * ncol as R_xlen_t);
        if ans.is_null() {
            return ans;
        }
        let _ans_guard = protect(ans);
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if dim.is_null() {
            return R_NilValue();
        }
        let _dim_guard = protect(dim);
        *INTEGER(dim) = nrow as c_int;
        *INTEGER(dim).add(1) = ncol as c_int;
        crate::sexp::attrib_core::setAttrib(ans, crate::sexp::attrib_core::R_DimSymbol(), dim);
        ans
    }
}

/// Mark every capture entry of a non-matching text element as -1
/// (grep.c's no-match branch in do_regexpr).
unsafe fn fill_capture_no_match(
    capture_start: SEXP,
    capture_len: SEXP,
    i: usize,
    n: R_xlen_t,
    capture_count: usize,
) {
    unsafe {
        if capture_start == R_NilValue() {
            return;
        }
        for g in 0..capture_count {
            let ind = i + g * n as usize;
            *INTEGER(capture_start).add(ind) = -1;
            *INTEGER(capture_len).add(ind) = -1;
        }
    }
}

/// R gregexpr(pattern, text) for repeated non-overlapping matches. With
/// perl = TRUE and capture groups, each element carries the
/// capture.start / capture.length matrices (one row per match, one column
/// per group) and capture.names, mirroring grep.c's perl branch of
/// do_gregexpr; non-matching elements get one row of -1s.
pub unsafe fn do_gregexpr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let position_workspace = crate::mainutils::grep::regex_workspace();
        let pattern = CAR(args);
        if pattern.is_null()
            || TYPEOF(pattern) != SEXPTYPE::STRSXP
            || XLENGTH(pattern) == 0
            || STRING_ELT(pattern, 0) == crate::sexp::globals::R_NaString()
        {
            super::base_error("invalid 'pattern' argument");
        }
        let pat = elt_to_string(CAR(args), 0);
        let text = CAR(CDR(args));
        let ignore_case = logical_arg_by_name_or_position(args, "ignore.case", 2).unwrap_or(false);
        let mut perl = logical_arg_by_name_or_position(args, "perl", 3).unwrap_or(false);
        let fixed = logical_arg_by_name_or_position(args, "fixed", 4).unwrap_or(false);
        super::shared::ignore_perl_when_fixed(&mut perl, fixed);
        crate::mainutils::grep::validate_pattern(&pat, ignore_case, perl, fixed)
            .unwrap_or_else(|message| super::base_error(&message));
        let (requested_bytes, byte_mode) = regex_coordinates(args, &pat, text);

        // grep.c drops perl when fixed = TRUE, so only a genuine perl run
        // gets capture attribution (same guard as do_regexpr).
        let (capture_count, capture_names) = if perl && !fixed {
            crate::mainutils::grep::perl_group_info(&pat, ignore_case)
                .unwrap_or_else(|| (0, Vec::new()))
        } else {
            (0, Vec::new())
        };
        let n = XLENGTH(text);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            let txt = elt_to_string(text, i);
            let empty_positions = pat.is_empty().then(|| {
                let mode = if fixed || (perl && requested_bytes) {
                    EmptyPatternMode::Bytes
                } else if perl {
                    EmptyPatternMode::PerlCharacters
                } else if requested_bytes {
                    EmptyPatternMode::EreBytes
                } else {
                    EmptyPatternMode::EreCharacters
                };
                empty_pattern_positions(&txt, mode)
            });
            let capacity = empty_positions
                .as_ref()
                .map_or(0, |positions| positions.len());
            let mut starts = position_workspace.buffer(capacity);
            let mut lengths = position_workspace.buffer(capacity);
            // Keep original admitted kernel storage through R capture-matrix
            // construction instead of copying into uncharged native row vectors.
            let mut capture_rows: Option<crate::mainutils::grep::RegexMatches> = None;
            if let Some(positions) = empty_positions {
                for position in positions {
                    starts.push(position);
                    lengths.push(0);
                }
            } else if perl {
                if let Some(all) = crate::mainutils::grep::mode_captures_all(
                    &pat,
                    &txt,
                    ignore_case,
                    requested_bytes,
                ) {
                    for caps in all.iter() {
                        if let Some(whole) = caps.first().and_then(|c| c.as_ref()) {
                            starts.push(whole.start + 1);
                            lengths.push(whole.end - whole.start);
                        }
                    }
                    if capture_count != 0 {
                        capture_rows = Some(all);
                    }
                }
            } else {
                let mut offset = 0usize;
                while offset <= txt.len() {
                    let hay = txt.get(offset..).unwrap_or("");
                    let found = if fixed {
                        fixed_find(hay, &pat, ignore_case)
                    } else if perl {
                        crate::mainutils::grep::perl_find(&pat, hay, ignore_case)
                    } else {
                        crate::mainutils::grep::ere_find_at(
                            &pat,
                            &txt,
                            ignore_case,
                            offset,
                            requested_bytes,
                        )
                        .map(|m| crate::mainutils::grep::RegexMatch {
                            start: m.start - offset,
                            end: m.end - offset,
                        })
                    };
                    let Some(m) = found else {
                        break;
                    };
                    let start = offset + m.start;
                    starts.push(start + 1);
                    lengths.push(m.end - m.start);
                    let next_offset = offset + m.end;
                    offset = if m.start == m.end {
                        next_offset
                            + if requested_bytes {
                                1
                            } else {
                                txt[next_offset..].chars().next().map_or(1, char::len_utf8)
                            }
                    } else {
                        next_offset
                    };
                }
            }

            if !byte_mode && !pat.is_empty() {
                for (start, length) in starts.iter_mut().zip(lengths.iter_mut()) {
                    let (s, n) = regex_span(&txt, *start - 1, *start - 1 + *length, false);
                    *start = s as usize;
                    *length = n as usize;
                }
            }
            let missing = TYPEOF(text) == SEXPTYPE::STRSXP
                && STRING_ELT(text, i) == crate::sexp::globals::R_NaString();
            if missing {
                starts.clear();
                lengths.clear();
                capture_rows = None;
            }
            let (elt, match_lengths) = if starts.is_empty() {
                let elt = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                let match_lengths = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                if elt.is_null() || match_lengths.is_null() {
                    return R_NilValue();
                }
                let _elt_guard = protect(elt);
                let _ml_guard = protect(match_lengths);
                *INTEGER(elt) = if missing { NA_INTEGER } else { -1 };
                *INTEGER(match_lengths) = if missing { NA_INTEGER } else { -1 };
                (elt, match_lengths)
            } else {
                let elt = Rf_allocVector3(SEXPTYPE::INTSXP, starts.len() as R_xlen_t);
                let match_lengths = Rf_allocVector3(SEXPTYPE::INTSXP, starts.len() as R_xlen_t);
                if elt.is_null() || match_lengths.is_null() {
                    return R_NilValue();
                }
                let _elt_guard = protect(elt);
                let _ml_guard = protect(match_lengths);
                for (idx, start) in starts.iter().enumerate() {
                    *INTEGER(elt).add(idx) = *start as c_int;
                    *INTEGER(match_lengths).add(idx) = lengths[idx] as c_int;
                }
                (elt, match_lengths)
            };

            crate::sexp::attrib_core::setAttrib(
                elt,
                Rf_install(c"match.length".as_ptr()),
                match_lengths,
            );
            if !missing {
                regex_index_attrs(elt, requested_bytes, byte_mode);
            }
            if capture_count > 0 && !missing {
                if starts.is_empty() {
                    // grep.c's no-match branch: one row of -1s per group.
                    set_gregexpr_capture_attrs(elt, 1, capture_count, &capture_names, |_, _| {
                        (-1, -1)
                    });
                } else {
                    let spans = capture_rows
                        .as_ref()
                        .expect("successful Perl matches retain their admitted captures");
                    set_gregexpr_capture_attrs(
                        elt,
                        starts.len(),
                        capture_count,
                        &capture_names,
                        |k, g| {
                            spans[k][g + 1].map_or((0, 0), |span| {
                                regex_span(&txt, span.start, span.end, byte_mode)
                            })
                        },
                    );
                }
            }
            SET_VECTOR_ELT(result, i, elt);
        }

        result
    }
}

fn substring_chars(text: &str, start: i32, end: i32) -> String {
    if start < 1 || end < start {
        return String::new();
    }
    text.chars()
        .skip((start - 1) as usize)
        .take((end - start + 1) as usize)
        .collect()
}

/// GNU `regmatches(x, m)` for invert = FALSE.
pub unsafe fn do_regmatches(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let m = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || m.is_null() || m == R_NilValue() {
            return R_NilValue();
        }
        let ml_sym = crate::sexp::symbol::Rf_install(c"match.length".as_ptr());
        if TYPEOF(m) == SEXPTYPE::VECSXP {
            let n = XLENGTH(m);
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
            let _r = protect(result);
            for i in 0..n {
                let starts = VECTOR_ELT(m, i);
                let lengths = crate::sexp::attrib_core::getAttrib(starts, ml_sym);
                let text = elt_to_string(x, i);
                let ns = if starts.is_null() || starts == R_NilValue() {
                    0
                } else {
                    XLENGTH(starts)
                };
                let mut parts: Vec<String> = Vec::new();
                for j in 0..ns {
                    let so = if TYPEOF(starts) == SEXPTYPE::INTSXP {
                        *INTEGER(starts).add(j as usize)
                    } else {
                        -1
                    };
                    if so <= 0 {
                        continue;
                    }
                    let ml = if !lengths.is_null()
                        && lengths != R_NilValue()
                        && TYPEOF(lengths) == SEXPTYPE::INTSXP
                        && XLENGTH(lengths) > j
                    {
                        *INTEGER(lengths).add(j as usize)
                    } else {
                        0
                    };
                    parts.push(substring_chars(&text, so, so + ml - 1));
                }
                let elt = Rf_allocVector3(SEXPTYPE::STRSXP, parts.len() as i64);
                let _e = protect(elt);
                for (j, p) in parts.iter().enumerate() {
                    let c = CString::new(p.as_str()).unwrap_or_default();
                    SET_STRING_ELT(elt, j as i64, Rf_mkChar(c.as_ptr()));
                }
                SET_VECTOR_ELT(result, i, elt);
            }
            result
        } else {
            let n = XLENGTH(m);
            let lengths = crate::sexp::attrib_core::getAttrib(m, ml_sym);
            let mut parts: Vec<String> = Vec::new();
            for i in 0..n {
                let so = if TYPEOF(m) == SEXPTYPE::INTSXP {
                    *INTEGER(m).add(i as usize)
                } else {
                    -1
                };
                if so <= 0 {
                    continue;
                }
                let ml = if !lengths.is_null()
                    && lengths != R_NilValue()
                    && TYPEOF(lengths) == SEXPTYPE::INTSXP
                    && XLENGTH(lengths) > i
                {
                    *INTEGER(lengths).add(i as usize)
                } else {
                    0
                };
                let text = elt_to_string(x, i);
                parts.push(substring_chars(&text, so, so + ml - 1));
            }
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, parts.len() as i64);
            let _r = protect(result);
            for (j, p) in parts.iter().enumerate() {
                let c = CString::new(p.as_str()).unwrap_or_default();
                SET_STRING_ELT(result, j as i64, Rf_mkChar(c.as_ptr()));
            }
            result
        }
    }
}

/// Attach capture.start / capture.length / capture.names attrs to one
/// gregexpr element: n_match x capture_count column-major matrices with
/// dimnames list(NULL, names) — the shape grep.c's do_gregexpr builds per
/// element. `span` maps (match_idx, group_idx) to (start, length).
unsafe fn set_gregexpr_capture_attrs(
    elt: SEXP,
    n_match: usize,
    capture_count: usize,
    capture_names: &[String],
    span: impl Fn(usize, usize) -> (c_int, c_int),
) {
    unsafe {
        let names_sexp = Rf_allocVector3(SEXPTYPE::STRSXP, capture_count as R_xlen_t);
        if names_sexp.is_null() {
            return;
        }
        let _nsg = protect(names_sexp);
        for (g, name) in capture_names.iter().enumerate() {
            let cstr = CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(names_sexp, g as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }

        let capture_start = alloc_int_matrix(n_match as R_xlen_t, capture_count);
        let capture_len = alloc_int_matrix(n_match as R_xlen_t, capture_count);
        if capture_start.is_null() || capture_len.is_null() {
            return;
        }
        let _csp = protect(capture_start);
        let _clp = protect(capture_len);
        for k in 0..n_match {
            for g in 0..capture_count {
                let (start, len) = span(k, g);
                let ind = k + g * n_match;
                *INTEGER(capture_start).add(ind) = start;
                *INTEGER(capture_len).add(ind) = len;
            }
        }

        let dmn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if dmn.is_null() {
            return;
        }
        let _dmp = protect(dmn);
        SET_VECTOR_ELT(dmn, 0, R_NilValue());
        SET_VECTOR_ELT(dmn, 1, names_sexp);
        crate::sexp::attrib_core::setAttrib(
            capture_start,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dmn,
        );
        crate::sexp::attrib_core::setAttrib(
            capture_len,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dmn,
        );

        crate::sexp::attrib_core::setAttrib(
            elt,
            Rf_install(c"capture.start".as_ptr()),
            capture_start,
        );
        crate::sexp::attrib_core::setAttrib(
            elt,
            Rf_install(c"capture.length".as_ptr()),
            capture_len,
        );
        crate::sexp::attrib_core::setAttrib(elt, Rf_install(c"capture.names".as_ptr()), names_sexp);
    }
}

/// R regexec(pattern, text) for the overall fixed match.
pub unsafe fn do_regexec(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern = CAR(args);
        if pattern.is_null()
            || TYPEOF(pattern) != SEXPTYPE::STRSXP
            || XLENGTH(pattern) == 0
            || STRING_ELT(pattern, 0) == crate::sexp::globals::R_NaString()
        {
            super::base_error("invalid 'pattern' argument");
        }
        let pat = elt_to_string(CAR(args), 0);
        let text = CAR(CDR(args));
        let ignore_case = logical_arg_by_name_or_position(args, "ignore.case", 2).unwrap_or(false);
        let mut perl = logical_arg_by_name_or_position(args, "perl", 3).unwrap_or(false);
        let fixed = logical_arg_by_name_or_position(args, "fixed", 4).unwrap_or(false);
        super::shared::ignore_perl_when_fixed(&mut perl, fixed);
        crate::mainutils::grep::validate_pattern(&pat, ignore_case, perl, fixed)
            .unwrap_or_else(|message| super::base_error(&message));
        let (requested_bytes, byte_mode) = regex_coordinates(args, &pat, text);

        let n = XLENGTH(text);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            let txt = elt_to_string(text, i);
            let missing = TYPEOF(text) == SEXPTYPE::STRSXP
                && STRING_ELT(text, i) == crate::sexp::globals::R_NaString();
            let captures = if missing {
                None
            } else if fixed {
                fixed_find(&txt, &pat, ignore_case).map(crate::mainutils::grep::single_capture)
            } else if perl {
                crate::mainutils::grep::mode_captures(
                    &pat,
                    &txt,
                    ignore_case,
                    true,
                    requested_bytes,
                )
            } else {
                crate::mainutils::grep::mode_captures(
                    &pat,
                    &txt,
                    ignore_case,
                    false,
                    requested_bytes,
                )
            };
            let (elt, match_lengths) = if let Some(captures) = captures {
                let elt = Rf_allocVector3(SEXPTYPE::INTSXP, captures.len() as R_xlen_t);
                let match_lengths = Rf_allocVector3(SEXPTYPE::INTSXP, captures.len() as R_xlen_t);
                if elt.is_null() || match_lengths.is_null() {
                    return R_NilValue();
                }
                let _elt_guard = protect(elt);
                let _ml_guard = protect(match_lengths);
                for (idx, capture) in captures.iter().enumerate() {
                    if let Some(m) = capture {
                        *INTEGER(elt).add(idx) = regex_span(&txt, m.start, m.end, byte_mode).0;
                        *INTEGER(match_lengths).add(idx) =
                            regex_span(&txt, m.start, m.end, byte_mode).1;
                    } else {
                        *INTEGER(elt).add(idx) = if perl { 0 } else { -1 };
                        *INTEGER(match_lengths).add(idx) = if perl { 0 } else { -1 };
                    }
                }
                (elt, match_lengths)
            } else {
                let elt = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                let match_lengths = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                if elt.is_null() || match_lengths.is_null() {
                    return R_NilValue();
                }
                let _elt_guard = protect(elt);
                let _ml_guard = protect(match_lengths);
                *INTEGER(elt) = if missing { NA_INTEGER } else { -1 };
                *INTEGER(match_lengths) = if missing { NA_INTEGER } else { -1 };
                (elt, match_lengths)
            };

            crate::sexp::attrib_core::setAttrib(
                elt,
                Rf_install(c"match.length".as_ptr()),
                match_lengths,
            );
            // GNU regexec's Perl branch publishes useBytes before index.type.
            if perl && byte_mode {
                crate::sexp::attrib_core::setAttrib(
                    elt,
                    Rf_install(c"useBytes".as_ptr()),
                    Rf_ScalarLogical(TRUE),
                );
                crate::sexp::attrib_core::setAttrib(
                    elt,
                    Rf_install(c"index.type".as_ptr()),
                    Rf_mkString(if requested_bytes {
                        c"bytes".as_ptr()
                    } else {
                        c"chars".as_ptr()
                    }),
                );
            } else {
                regex_index_attrs(elt, requested_bytes, byte_mode);
            }
            SET_VECTOR_ELT(result, i, elt);
        }

        result
    }
}

/// GNU `.Internal(regexec(pattern, text, ignore.case, fixed, useBytes))`
/// contract.
///
/// GNU's R-level `regexec` closure handles `perl` itself (warning when it
/// conflicts with `fixed`) and hands the internal five arguments without a
/// `perl` slot. This engine registers `do_regexec` as the R-level builtin
/// with the six-argument shape, so `.Internal` calls arriving from upstream
/// base source closures get a `perl = FALSE` cell inserted before
/// delegation.
pub unsafe fn do_regexec_internal(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut argc = 0;
        let mut p = args;
        while !p.is_null() && p != R_NilValue() {
            argc += 1;
            p = CDR(p);
        }
        if argc != 5 {
            return do_regexec(call, op, args, rho);
        }
        let pattern = CAR(args);
        let text = CAR(CDR(args));
        let ignore_case = CAR(CDR(CDR(args)));
        let fixed = CAR(CDR(CDR(CDR(args))));
        let use_bytes = CAR(CDR(CDR(CDR(CDR(args)))));
        let _g1 = protect(pattern);
        let _g2 = protect(text);
        let _g3 = protect(ignore_case);
        let _g4 = protect(fixed);
        let _g5 = protect(use_bytes);
        let perl_false = crate::sexp::constructors::Rf_ScalarLogical(0);
        let _g6 = protect(perl_false);
        let tail = Rf_cons(use_bytes, R_NilValue());
        let _g7 = protect(tail);
        let fixed_cell = Rf_cons(fixed, tail);
        let _g8 = protect(fixed_cell);
        let perl_cell = Rf_cons(perl_false, fixed_cell);
        let _g9 = protect(perl_cell);
        let ig_cell = Rf_cons(ignore_case, perl_cell);
        let _g10 = protect(ig_cell);
        let text_cell = Rf_cons(text, ig_cell);
        let _g11 = protect(text_cell);
        let spread = Rf_cons(pattern, text_cell);
        let _g12 = protect(spread);
        do_regexec(call, op, spread, rho)
    }
}

unsafe fn set_regexpr_attrs(x: SEXP, match_lengths: SEXP) {
    unsafe {
        crate::sexp::attrib_core::setAttrib(x, Rf_install(c"match.length".as_ptr()), match_lengths);
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(c"index.type".as_ptr()),
            Rf_mkString(c"chars".as_ptr()),
        );
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(c"useBytes".as_ptr()),
            Rf_ScalarLogical(TRUE),
        );
    }
}

unsafe fn set_regexec_perl_attrs(x: SEXP, match_lengths: SEXP) {
    unsafe {
        crate::sexp::attrib_core::setAttrib(x, Rf_install(c"match.length".as_ptr()), match_lengths);
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(c"useBytes".as_ptr()),
            Rf_ScalarLogical(TRUE),
        );
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(c"index.type".as_ptr()),
            Rf_mkString(c"chars".as_ptr()),
        );
    }
}
