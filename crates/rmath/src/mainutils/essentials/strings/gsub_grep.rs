//! gsub/sub/grep/agrep family and fixed-string replacement

use super::*;

#[cfg(test)]
mod replacement_missing_tests;

/// R's `trimws(x, which="both")` — trim whitespace from strings.
pub unsafe fn do_trimws(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            let trimmed = s.trim();
            let cstr = CString::new(trimmed).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                crate::sexp::accessors::SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
        }
        result
    }
}

/// R's `gsub(pattern, replacement, x)` — global string substitution (literal).
pub unsafe fn do_gsub(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_string_replace(args, true) }
}

/// R's `sub(pattern, replacement, x)` — first match substitution (literal).
pub unsafe fn do_sub(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_string_replace(args, false) }
}

unsafe fn string_arg_is_na(arg: SEXP) -> bool {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || XLENGTH(arg) == 0 {
            return false;
        }
        if TYPEOF(arg) == SEXPTYPE::STRSXP {
            return STRING_ELT(arg, 0) == crate::sexp::globals::R_NaString();
        }
        if TYPEOF(arg) == SEXPTYPE::LGLSXP {
            return *LOGICAL(arg) == crate::sexp::ffi::NA_LOGICAL;
        }
        false
    }
}

unsafe fn na_integer_vector(n: R_xlen_t) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        for i in 0..n {
            *INTEGER(result).add(i as usize) = NA_INTEGER;
        }
        result
    }
}

/// R's `grep(pattern, x, ..., value = FALSE)` for fixed and ERE matching.
pub unsafe fn do_grep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern_arg = arg_by_name_or_position(args, &["pattern"], 0);
        let x_arg = arg_by_name_or_position(args, &["x", "text"], 1);
        if pattern_arg.is_null() || x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        // fixed=TRUE warns before an NA pattern returns.
        let mut perl = logical_arg_by_name_or_position(args, "perl", 4).unwrap_or(false);
        let fixed = logical_arg_by_name_or_position(args, "fixed", 5).unwrap_or(false);
        super::shared::ignore_perl_when_fixed(&mut perl, fixed);
        if string_arg_is_na(pattern_arg) {
            let value = logical_arg_by_name_or_position(args, "value", 3).unwrap_or(false);
            let n = XLENGTH(x_arg);
            if !value {
                return na_integer_vector(n);
            }
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _result_guard = protect(result);
            for i in 0..n {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
            }
            let src_names = crate::sexp::attrib_core::getAttrib(
                x_arg,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            if !src_names.is_null()
                && TYPEOF(src_names) == SEXPTYPE::STRSXP
                && XLENGTH(src_names) == n
            {
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    src_names,
                );
            }
            return result;
        }
        let ignore_case = logical_arg_by_name_or_position(args, "ignore.case", 2).unwrap_or(false);
        let value = logical_arg_by_name_or_position(args, "value", 3).unwrap_or(false);
        let invert = logical_arg_by_name_or_position(args, "invert", 7).unwrap_or(false);
        let pattern = elt_to_string(pattern_arg, 0);
        crate::mainutils::grep::validate_pattern(&pattern, ignore_case, perl, fixed)
            .unwrap_or_else(|message| base_error(&message));
        let matches = grep_match_indices(x_arg, &pattern, ignore_case, perl, fixed, invert);

        if value {
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, matches.len() as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let index = &matches;
            for (out_idx, src_idx) in index.iter().enumerate() {
                if TYPEOF(x_arg) == SEXPTYPE::STRSXP {
                    SET_STRING_ELT(result, out_idx as R_xlen_t, STRING_ELT(x_arg, *src_idx));
                } else {
                    SET_STRING_ELT(
                        result,
                        out_idx as R_xlen_t,
                        Rf_mkChar(
                            CString::new(elt_to_string(x_arg, *src_idx))
                                .unwrap_or_default()
                                .as_ptr(),
                        ),
                    );
                }
            }
            let src_names = crate::sexp::attrib_core::getAttrib(
                x_arg,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            if !src_names.is_null()
                && TYPEOF(src_names) == SEXPTYPE::STRSXP
                && XLENGTH(src_names) == XLENGTH(x_arg)
            {
                let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, XLENGTH(result));
                let _out_names = protect(out_names);
                for (out_idx, src_idx) in index.iter().enumerate() {
                    SET_STRING_ELT(
                        out_names,
                        out_idx as R_xlen_t,
                        STRING_ELT(src_names, *src_idx),
                    );
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    out_names,
                );
            }
            result
        } else {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, matches.len() as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = INTEGER(result);
            for (out_idx, src_idx) in matches.into_iter().enumerate() {
                *dst.add(out_idx) = (src_idx + 1) as c_int;
            }
            result
        }
    }
}

/// R's `grepl(pattern, x, ...)` for fixed and ERE matching.
pub unsafe fn do_grepl(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern_arg = arg_by_name_or_position(args, &["pattern"], 0);
        let x_arg = arg_by_name_or_position(args, &["x", "text"], 1);
        if pattern_arg.is_null() || x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let ignore_case = logical_arg_by_name_or_position(args, "ignore.case", 2).unwrap_or(false);
        let mut perl = logical_arg_by_name_or_position(args, "perl", 4).unwrap_or(false);
        let fixed = logical_arg_by_name_or_position(args, "fixed", 5).unwrap_or(false);
        super::shared::ignore_perl_when_fixed(&mut perl, fixed);
        let pattern = elt_to_string(pattern_arg, 0);
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        if string_arg_is_na(pattern_arg) {
            for i in 0..n {
                *dst.add(i as usize) = NA_LOGICAL;
            }
            return result;
        }
        crate::mainutils::grep::validate_pattern(&pattern, ignore_case, perl, fixed)
            .unwrap_or_else(|message| base_error(&message));
        for i in 0..n {
            if is_string_na(x_arg, i) {
                // GNU grepl: NA in x does not match a non-missing pattern.
                *dst.add(i as usize) = FALSE;
                continue;
            }
            let matched =
                grep_value_matches(&elt_to_string(x_arg, i), &pattern, ignore_case, perl, fixed);
            *dst.add(i as usize) = if matched { TRUE } else { FALSE };
        }
        result
    }
}

/// R's `agrep(pattern, x, ...)` — approximate fixed-string matching.
pub unsafe fn do_agrep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern_arg = arg_by_name_or_position(args, &["pattern"], 0);
        let x_arg = arg_by_name_or_position(args, &["x", "text"], 1);
        if pattern_arg.is_null() || x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        if string_arg_is_na(pattern_arg) {
            let value = named_logical_arg(args, "value").unwrap_or(false);
            let n = XLENGTH(x_arg);
            if !value {
                return na_integer_vector(n);
            }
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _result_guard = protect(result);
            for i in 0..n {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
            }
            let src_names = crate::sexp::attrib_core::getAttrib(
                x_arg,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            if !src_names.is_null()
                && TYPEOF(src_names) == SEXPTYPE::STRSXP
                && XLENGTH(src_names) == n
            {
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    src_names,
                );
            }
            return result;
        }
        let value = named_logical_arg(args, "value").unwrap_or(false);
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let max_distance = agrep_max_distance(args, pattern_arg);
        let fixed = named_logical_arg(args, "fixed").unwrap_or(true);
        let pattern = elt_to_string(pattern_arg, 0);
        let patterns: Vec<&str> = if fixed {
            vec![pattern.as_str()]
        } else {
            pattern.split('|').collect()
        };
        let matches = agrep_match_any(x_arg, &patterns, max_distance, ignore_case);

        if value {
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, matches.len() as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let names = crate::sexp::attrib_core::getAttrib(
                x_arg,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            let out_names = if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && XLENGTH(names) == XLENGTH(x_arg)
            {
                let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, matches.len() as R_xlen_t);
                let _names_guard = protect(out_names);
                out_names
            } else {
                R_NilValue()
            };
            for (out_idx, src_idx) in matches.iter().enumerate() {
                SET_STRING_ELT(result, out_idx as R_xlen_t, STRING_ELT(x_arg, *src_idx));
                if out_names != R_NilValue() {
                    SET_STRING_ELT(out_names, out_idx as R_xlen_t, STRING_ELT(names, *src_idx));
                }
            }
            if out_names != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    out_names,
                );
            }
            result
        } else {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, matches.len() as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = INTEGER(result);
            for (out_idx, src_idx) in matches.into_iter().enumerate() {
                *dst.add(out_idx) = (src_idx + 1) as c_int;
            }
            result
        }
    }
}

/// R's `agrepl(pattern, x, ...)` — logical approximate matching.
pub unsafe fn do_agrepl(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern_arg = arg_by_name_or_position(args, &["pattern"], 0);
        let x_arg = arg_by_name_or_position(args, &["x", "text"], 1);
        if pattern_arg.is_null() || x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let max_distance = agrep_max_distance(args, pattern_arg);
        let pattern = elt_to_string(pattern_arg, 0);
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let matched = !is_string_na(x_arg, i)
                && approximate_contains(
                    &pattern,
                    &elt_to_string(x_arg, i),
                    max_distance,
                    ignore_case,
                );
            *dst.add(i as usize) = if matched { TRUE } else { FALSE };
        }
        result
    }
}

/// R's `pcre_config()` — report regex engine feature switches.
pub unsafe fn do_pcre_config(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    const FEATURES: [(&str, c_int); 4] = [
        ("UTF-8", TRUE),
        ("Unicode properties", TRUE),
        ("JIT", FALSE),
        ("stack", FALSE),
    ];

    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, FEATURES.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let data = LOGICAL(result);
        for (i, (_, value)) in FEATURES.iter().enumerate() {
            *data.add(i) = *value;
        }

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, FEATURES.len() as R_xlen_t);
        if !names.is_null() {
            let _names_guard = protect(names);
            for (i, (name, _)) in FEATURES.iter().enumerate() {
                SET_STRING_ELT(
                    names,
                    i as R_xlen_t,
                    Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
                );
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
        }

        result
    }
}

fn agrep_max_distance(args: SEXP, pattern_arg: SEXP) -> usize {
    unsafe {
        let raw = arg_by_name_or_position(args, &["max.distance"], 2);
        let value = if raw.is_null() || raw == R_NilValue() {
            0.1
        } else {
            real_or_default(raw, 0.1)
        };
        if value <= 0.0 {
            return 0;
        }
        if value < 1.0 {
            let pattern_len = elt_to_string(pattern_arg, 0).chars().count().max(1);
            (value * pattern_len as f64).ceil() as usize
        } else {
            value.ceil() as usize
        }
    }
}

unsafe fn agrep_match_any(
    x: SEXP,
    patterns: &[&str],
    max_distance: usize,
    ignore_case: bool,
) -> Vec<R_xlen_t> {
    unsafe {
        let n = XLENGTH(x);
        let mut matches = Vec::new();
        for i in 0..n {
            if is_string_na(x, i) {
                continue;
            }
            let text = elt_to_string(x, i);
            if patterns
                .iter()
                .any(|pattern| approximate_contains(pattern, &text, max_distance, ignore_case))
            {
                matches.push(i);
            }
        }
        matches
    }
}

fn approximate_contains(pattern: &str, text: &str, max_distance: usize, ignore_case: bool) -> bool {
    let pattern = if ignore_case {
        pattern.to_ascii_lowercase()
    } else {
        pattern.to_string()
    };
    let text = if ignore_case {
        text.to_ascii_lowercase()
    } else {
        text.to_string()
    };
    let pat = pattern.as_bytes();
    let hay = text.as_bytes();
    if pat.is_empty() {
        return true;
    }
    if crate::mainutils::grep::levenshtein_distance(pat, hay) <= max_distance {
        return true;
    }
    let min_len = pat.len().saturating_sub(max_distance).max(1);
    let max_len = (pat.len() + max_distance).min(hay.len());

    for start in 0..hay.len() {
        for len in min_len..=max_len {
            let end = start + len;
            if end > hay.len() {
                break;
            }
            if crate::mainutils::grep::levenshtein_distance(pat, &hay[start..end]) <= max_distance {
                return true;
            }
        }
    }
    false
}

unsafe fn do_string_replace(args: SEXP, global: bool) -> SEXP {
    unsafe {
        let pattern_arg = CAR(args);
        let replacement_arg = CAR(CDR(args));
        let x_arg = CAR(CDR(CDR(args)));
        let ignore_case = logical_arg_by_name_or_position(args, "ignore.case", 3).unwrap_or(false);
        let mut perl = logical_arg_by_name_or_position(args, "perl", 4).unwrap_or(false);
        let fixed = logical_arg_by_name_or_position(args, "fixed", 5).unwrap_or(false);
        super::shared::ignore_perl_when_fixed(&mut perl, fixed);
        if x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        if pattern_arg.is_null() || replacement_arg.is_null() {
            return R_NilValue();
        }
        let use_bytes = logical_arg_by_name_or_position(args, "useBytes", 6).unwrap_or(false);
        // Missing patterns short-circuit every matcher, including raw literals.
        if string_arg_is_na(pattern_arg) {
            let n = XLENGTH(x_arg);
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _result_guard = protect(result);
            for i in 0..n {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
            }
            crate::mainutils::coerce::SHALLOW_DUPLICATE_ATTRIB(result, x_arg);
            return result;
        }
        let replacement_missing = string_arg_is_na(replacement_arg);
        let pattern = elt_to_string(pattern_arg, 0);
        if fixed && pattern.is_empty() {
            base_error("zero-length pattern");
        }
        let replacement = elt_to_string(replacement_arg, 0);
        crate::mainutils::grep::validate_pattern(&pattern, ignore_case, perl, fixed)
            .unwrap_or_else(|message| base_error(&message));
        let pattern_is_literal = pattern.bytes().all(|b| {
            !matches!(
                b,
                b'.' | b'*'
                    | b'+'
                    | b'?'
                    | b'|'
                    | b'('
                    | b')'
                    | b'['
                    | b']'
                    | b'{'
                    | b'}'
                    | b'\\'
                    | b'^'
                    | b'$'
            )
        });
        if use_bytes
            && (fixed || (pattern_is_literal && !pattern.is_empty()))
            && TYPEOF(x_arg) == SEXPTYPE::STRSXP
            && TYPEOF(pattern_arg) == SEXPTYPE::STRSXP
        {
            let n = XLENGTH(x_arg);
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _result_guard = protect(result);
            let pat = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(pattern_arg, 0)))
                .to_bytes()
                .to_vec();
            let rep = if TYPEOF(replacement_arg) == SEXPTYPE::STRSXP && XLENGTH(replacement_arg) > 0
            {
                std::ffi::CStr::from_ptr(CHAR(STRING_ELT(replacement_arg, 0)))
                    .to_bytes()
                    .to_vec()
            } else {
                Vec::new()
            };
            let parsed_replacement = if fixed || replacement_missing {
                None
            } else {
                Some(crate::mainutils::grep::literal_replacement_bytes(
                    &rep, perl,
                ))
            };
            let replacement_bytes = parsed_replacement.as_deref().unwrap_or(&rep);
            for i in 0..n {
                let src_ch = STRING_ELT(x_arg, i);
                if src_ch.is_null()
                    || src_ch == crate::sexp::globals::R_NaString()
                    || pat.is_empty()
                {
                    SET_STRING_ELT(result, i, src_ch);
                    continue;
                }
                let src = std::ffi::CStr::from_ptr(CHAR(src_ch)).to_bytes();
                let mut out = Vec::new();
                let mut p = 0;
                let mut changed = false;
                while p < src.len() {
                    let matched = src
                        .get(p..)
                        .and_then(|tail| tail.get(..pat.len()))
                        .is_some_and(|candidate| {
                            if !fixed && ignore_case {
                                candidate.eq_ignore_ascii_case(&pat)
                            } else {
                                candidate == pat
                            }
                        });
                    if matched {
                        changed = true;
                        if replacement_missing {
                            break;
                        }
                        out.extend_from_slice(replacement_bytes);
                        p += pat.len();
                        if !global {
                            out.extend_from_slice(&src[p..]);
                            break;
                        }
                    } else {
                        out.push(src[p]);
                        p += 1;
                    }
                }
                if !changed {
                    SET_STRING_ELT(result, i, src_ch);
                } else if replacement_missing {
                    SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                } else {
                    let cstr = std::ffi::CString::new(out).unwrap_or_default();
                    SET_STRING_ELT(
                        result,
                        i,
                        crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()),
                    );
                }
            }
            crate::mainutils::coerce::SHALLOW_DUPLICATE_ATTRIB(result, x_arg);
            return result;
        }
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            if TYPEOF(x_arg) == SEXPTYPE::STRSXP
                && STRING_ELT(x_arg, i) == crate::sexp::globals::R_NaString()
            {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }
            // GNU sub/gsub in a single-byte locale treats native strings as
            // opaque bytes; keep them lossless via the Latin-1 round-trip.
            let (s, latin1) = if TYPEOF(x_arg) == SEXPTYPE::STRSXP {
                let ch = STRING_ELT(x_arg, i);
                let bytes = if ch.is_null() {
                    &[][..]
                } else {
                    std::ffi::CStr::from_ptr(CHAR(ch)).to_bytes()
                };
                native_bytes_to_string(bytes)
            } else {
                (elt_to_string(x_arg, i), false)
            };
            if replacement_missing {
                // Match existence includes empty matches. Deleting text cannot
                // distinguish those and must not send Perl/fixed syntax to TRE.
                let value = if grep_value_matches(&s, &pattern, ignore_case && !fixed, perl, fixed)
                {
                    crate::sexp::globals::R_NaString()
                } else if TYPEOF(x_arg) == SEXPTYPE::STRSXP {
                    STRING_ELT(x_arg, i)
                } else {
                    let original =
                        CString::new(string_to_native_bytes(&s, latin1)).unwrap_or_default();
                    crate::sexp::constructors::Rf_mkChar(original.as_ptr())
                };
                SET_STRING_ELT(result, i, value);
                continue;
            }
            let fixed_replacement = if fixed && global {
                Some(s.replace(&pattern, &replacement))
            } else if fixed {
                Some(s.replacen(&pattern, &replacement, 1))
            } else {
                None
            };
            let regex_replacement = if fixed {
                None
            } else if perl {
                crate::mainutils::grep::perl_replace_with_case(
                    &pattern,
                    &s,
                    &replacement,
                    global,
                    ignore_case,
                    if use_bytes {
                        crate::mainutils::grep::ReplacementCase::Bytes
                    } else {
                        crate::mainutils::grep::ReplacementCase::Unicode
                    },
                )
            } else {
                crate::mainutils::grep::ere_replace(&pattern, &s, &replacement, global, ignore_case)
            };
            // Keep the actual regex output ticket through final R publication.
            let replaced = fixed_replacement
                .as_deref()
                .or(regex_replacement.as_deref())
                .unwrap_or(&s);
            let cstr = CString::new(string_to_native_bytes(replaced, latin1)).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                crate::sexp::accessors::SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
        }
        // GNU sub/gsub copy x's attributes (names, dim, dimnames) to the result.
        crate::mainutils::coerce::SHALLOW_DUPLICATE_ATTRIB(result, x_arg);
        result
    }
}

/// Decode string bytes for regex work. Valid UTF-8 passes through; native
/// (single-byte locale) strings are decoded Latin-1, a lossless
/// byte<->char bijection, so sub/gsub never corrupts them to U+FFFD the
/// way a lossy UTF-8 decode would.
fn native_bytes_to_string(bytes: &[u8]) -> (String, bool) {
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), false),
        Err(_) => (bytes.iter().map(|&b| b as char).collect(), true),
    }
}

/// Re-encode a regex result, undoing the Latin-1 decoding when the source
/// element needed it.
fn string_to_native_bytes(s: &str, latin1: bool) -> Vec<u8> {
    if latin1 {
        s.chars()
            .map(|c| if (c as u32) < 0x100 { c as u8 } else { b'?' })
            .collect()
    } else {
        s.as_bytes().to_vec()
    }
}
