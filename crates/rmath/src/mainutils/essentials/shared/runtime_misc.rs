//! Misc runtime scalar/arg/string/date helpers.

use super::*;

/// Read a scalar real from a numeric SEXP, with default.
pub(crate) fn real_or_default(x: SEXP, default: f64) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() || XLENGTH(x) < 1 {
            return default;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            *REAL(x)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            default
        }
    }
}

pub(crate) fn base_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

pub(crate) unsafe fn tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(cell);
        if tag.is_null() || tag == R_NilValue() {
            return None;
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() {
            return None;
        }
        let chars = CHAR(pname);
        if chars.is_null() {
            None
        } else {
            Some(std::ffi::CStr::from_ptr(chars).to_str().ok()?.to_string())
        }
    }
}

pub(crate) fn real_elt_or_default(x: SEXP, i: R_xlen_t, default: f64) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return default;
        }
        let n = XLENGTH(x);
        if n == 0 {
            return default;
        }
        let idx = i % n;
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            *REAL(x).add(idx as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(idx as usize);
            if v == NA_INTEGER { default } else { v as f64 }
        } else {
            default
        }
    }
}

pub(crate) fn numeric_elt_as_count(x: SEXP, i: R_xlen_t) -> usize {
    let value = real_elt_or_default(x, i, 0.0);
    if value.is_finite() {
        (value as i64).max(0) as usize
    } else {
        0
    }
}

pub(crate) fn named_logical_arg(args: SEXP, name: &str) -> Option<bool> {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some(name) {
                let value = CAR(current);
                if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
                    return None;
                }
                let raw = if TYPEOF(value) == SEXPTYPE::LGLSXP || TYPEOF(value) == SEXPTYPE::INTSXP
                {
                    *INTEGER(value)
                } else if TYPEOF(value) == SEXPTYPE::REALSXP {
                    *REAL(value) as c_int
                } else {
                    return None;
                };
                return (raw != NA_INTEGER).then_some(raw != 0);
            }
            current = CDR(current);
        }
        None
    }
}

pub(crate) fn logical_arg_by_name_or_position(
    args: SEXP,
    name: &str,
    position: usize,
) -> Option<bool> {
    unsafe {
        let value = arg_by_name_or_position(args, &[name], position);
        if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
            return None;
        }
        let raw = if TYPEOF(value) == SEXPTYPE::LGLSXP || TYPEOF(value) == SEXPTYPE::INTSXP {
            *INTEGER(value)
        } else if TYPEOF(value) == SEXPTYPE::REALSXP {
            let value = *REAL(value);
            if ISNAN(value) {
                NA_LOGICAL
            } else {
                value as c_int
            }
        } else {
            return None;
        };
        (raw != NA_INTEGER).then_some(raw != 0)
    }
}

pub(crate) fn integer_arg_by_name_or_position(
    args: SEXP,
    name: &str,
    position: usize,
) -> Option<c_int> {
    unsafe {
        let value = arg_by_name_or_position(args, &[name], position);
        if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
            return None;
        }
        let raw = if TYPEOF(value) == SEXPTYPE::INTSXP || TYPEOF(value) == SEXPTYPE::LGLSXP {
            *INTEGER(value)
        } else if TYPEOF(value) == SEXPTYPE::REALSXP {
            let value = *REAL(value);
            if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || value.is_nan() {
                NA_INTEGER
            } else {
                value as c_int
            }
        } else {
            return None;
        };
        Some(raw)
    }
}

pub(crate) fn arg_by_name_or_position(args: SEXP, names: &[&str], position: usize) -> SEXP {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if let Some(tag) = tag_name(current) {
                if names.iter().any(|name| tag == *name) {
                    return CAR(current);
                }
            }
            current = CDR(current);
        }

        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).is_none() {
                if positional == position {
                    return CAR(current);
                }
                positional += 1;
            }
            current = CDR(current);
        }
        R_NilValue()
    }
}

pub(crate) fn is_string_na(x: SEXP, i: R_xlen_t) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return false;
        }
        let n = XLENGTH(x);
        if n == 0 {
            return false;
        }
        STRING_ELT(x, i % n) == crate::sexp::globals::R_NaString()
    }
}

pub(crate) fn element_coerces_to_character_na(x: SEXP, i: R_xlen_t) -> bool {
    unsafe {
        let n = XLENGTH(x);
        if x.is_null() || x == R_NilValue() || n == 0 {
            return false;
        }
        let idx = i % n;
        let ty = TYPEOF(x);
        if ty == SEXPTYPE::STRSXP {
            is_string_na(x, idx)
        } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
            *INTEGER(x).add(idx as usize) == NA_INTEGER
        } else if ty == SEXPTYPE::REALSXP {
            let value = *REAL(x).add(idx as usize);
            value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
        } else {
            false
        }
    }
}

pub(crate) fn string_contains(text: &str, pattern: &str, ignore_case: bool) -> bool {
    fixed_find(text, pattern, ignore_case).is_some()
}

pub(crate) fn fixed_find(
    text: &str,
    pattern: &str,
    ignore_case: bool,
) -> Option<crate::mainutils::grep::RegexMatch> {
    let hay = text.as_bytes();
    let needle = pattern.as_bytes();
    if needle.is_empty() {
        return Some(crate::mainutils::grep::RegexMatch { start: 0, end: 0 });
    }
    if hay.len() < needle.len() {
        return None;
    }
    for start in 0..=(hay.len() - needle.len()) {
        let matched = needle.iter().enumerate().all(|(idx, expected)| {
            let actual = hay[start + idx];
            if ignore_case {
                actual.eq_ignore_ascii_case(expected)
            } else {
                actual == *expected
            }
        });
        if matched {
            return Some(crate::mainutils::grep::RegexMatch {
                start,
                end: start + needle.len(),
            });
        }
    }
    None
}

pub(crate) fn grep_value_matches(
    text: &str,
    pattern: &str,
    ignore_case: bool,
    perl: bool,
    fixed: bool,
) -> bool {
    if fixed {
        string_contains(text, pattern, ignore_case)
    } else if perl {
        crate::mainutils::grep::perl_find(pattern, text, ignore_case).is_some()
    } else {
        crate::mainutils::grep::ere_is_match(pattern, text, ignore_case)
    }
}

/// GNU grep.c: `fixed = TRUE` wins, warns, and then runs as a fixed match.
pub(crate) unsafe fn ignore_perl_when_fixed(perl: &mut bool, fixed: bool) {
    if fixed && *perl {
        let msg = CString::new("argument 'perl = TRUE' will be ignored").unwrap_or_default();
        unsafe {
            crate::mainutils::errors::Rf_warning(msg.as_ptr());
        }
        *perl = false;
    }
}

pub(crate) fn grep_match_indices(
    x: SEXP,
    pattern: &str,
    ignore_case: bool,
    perl: bool,
    fixed: bool,
    invert: bool,
) -> Vec<R_xlen_t> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        let n = XLENGTH(x);
        let mut matches = Vec::new();
        let mut utf8_warned = false;
        for i in 0..n {
            if is_string_na(x, i) {
                if invert {
                    matches.push(i);
                }
                continue;
            }
            if !utf8_warned && TYPEOF(x) == SEXPTYPE::STRSXP {
                let elt = STRING_ELT(x, i);
                if crate::sexp::accessors::getCharCE(elt) == 2 {
                    let bytes = std::ffi::CStr::from_ptr(CHAR(elt)).to_bytes();
                    if std::str::from_utf8(bytes).is_err() {
                        utf8_warned = true;
                        let msg = format!("input string {} is invalid UTF-8\0", i + 1);
                        crate::mainutils::errors::warningcall(
                            crate::sexp::globals::R_NilValue(),
                            msg.as_ptr() as *const std::os::raw::c_char,
                        );
                    }
                }
            }
            let matched =
                grep_value_matches(&elt_to_string(x, i), pattern, ignore_case, perl, fixed);
            if if invert { !matched } else { matched } {
                matches.push(i);
            }
        }
        matches
    }
}

pub(crate) fn environment_arg_or_default(
    args: SEXP,
    names: &[&str],
    position: usize,
    default: SEXP,
) -> SEXP {
    unsafe {
        let arg = arg_by_name_or_position(args, names, position);
        if !arg.is_null() && arg != R_NilValue() && TYPEOF(arg) == SEXPTYPE::ENVSXP {
            arg
        } else {
            default
        }
    }
}

pub(crate) fn copy_vector_elt(dst: SEXP, dst_idx: R_xlen_t, src: SEXP, src_idx: R_xlen_t) {
    unsafe {
        match TYPEOF(src) {
            t if t == SEXPTYPE::REALSXP => {
                *REAL(dst).add(dst_idx as usize) = *REAL(src).add(src_idx as usize);
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                *INTEGER(dst).add(dst_idx as usize) = *INTEGER(src).add(src_idx as usize);
            }
            t if t == SEXPTYPE::STRSXP => {
                SET_STRING_ELT(dst, dst_idx, STRING_ELT(src, src_idx));
            }
            t if t == SEXPTYPE::CPLXSXP => {
                *COMPLEX(dst).add(dst_idx as usize) = *COMPLEX(src).add(src_idx as usize);
            }
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                SET_VECTOR_ELT(dst, dst_idx, VECTOR_ELT(src, src_idx));
            }
            t if t == SEXPTYPE::RAWSXP => {
                *RAW(dst).add(dst_idx as usize) = *RAW(src).add(src_idx as usize);
            }
            _ => {}
        }
    }
}

pub(crate) fn map_path_strings(x: SEXP, f: fn(&str) -> String) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            if TYPEOF(x) == SEXPTYPE::STRSXP {
                let idx = if XLENGTH(x) == 0 { 0 } else { i % XLENGTH(x) };
                if STRING_ELT(x, idx) == crate::sexp::globals::R_NaString() {
                    SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                    continue;
                }
            }
            let value = f(&elt_to_string(x, i));
            SET_STRING_ELT(
                result,
                i,
                Rf_mkChar(CString::new(value).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

pub(crate) fn trim_trailing_separators(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() && !path.is_empty() {
        &path[..1]
    } else {
        trimmed
    }
}

pub(crate) fn r_basename(path: &str) -> String {
    let trimmed = trim_trailing_separators(path);
    if trimmed == "/" || trimmed == "\\" {
        return trimmed.to_string();
    }
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

pub(crate) fn r_dirname(path: &str) -> String {
    let trimmed = trim_trailing_separators(path);
    if trimmed == "/" || trimmed == "\\" {
        return trimmed.to_string();
    }
    match trimmed.rfind(['/', '\\']) {
        Some(0) => trimmed[..1].to_string(),
        Some(pos) => trimmed[..pos].to_string(),
        None => ".".to_string(),
    }
}

/// Convert an element of a vector to a String.
pub(crate) fn elt_to_string(x: SEXP, i: R_xlen_t) -> String {
    unsafe {
        if x.is_null() {
            return "NULL".to_string();
        }
        let t = TYPEOF(x);
        let n = match SEXPTYPE(t) {
            SEXPTYPE::NILSXP => 0,
            SEXPTYPE::REALSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::LGLSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::RAWSXP
            | SEXPTYPE::VECSXP
            | SEXPTYPE::EXPRSXP => XLENGTH(x),
            _ => 1,
        };
        if n == 0 {
            return String::new();
        }
        let idx = i.rem_euclid(n);

        if t == SEXPTYPE::REALSXP {
            let v = *REAL(x).add(idx as usize);
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                "NA".to_string()
            } else if v.is_nan() {
                "NaN".to_string()
            } else if v.is_infinite() {
                if v.is_sign_negative() { "-Inf" } else { "Inf" }.to_string()
            } else {
                format!("{}", v)
            }
        } else if t == SEXPTYPE::INTSXP {
            let v = *INTEGER(x).add(idx as usize);
            if v == NA_INTEGER {
                "NA".to_string()
            } else if let Some(label) = factor_label_at(x, v) {
                label
            } else {
                format!("{}", v)
            }
        } else if t == SEXPTYPE::LGLSXP {
            let v = *LOGICAL(x).add(idx as usize);
            if v == NA_INTEGER {
                "NA".to_string()
            } else if v == TRUE {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        } else if t == SEXPTYPE::CPLXSXP {
            let c = *COMPLEX(x).add(idx as usize);
            if c.r.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                || c.i.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            {
                "NA".to_string()
            } else {
                let imag = if c.i == 0.0 { 0.0 } else { c.i };
                if imag.is_sign_negative() {
                    format!("{}{}i", c.r, imag)
                } else {
                    format!("{}+{}i", c.r, imag)
                }
            }
        } else if t == SEXPTYPE::RAWSXP {
            format!("{:02x}", *RAW(x).add(idx as usize))
        } else if t == SEXPTYPE::STRSXP {
            let charsxp = crate::sexp::accessors::STRING_ELT(x, idx);
            if charsxp.is_null() || charsxp == crate::sexp::globals::R_NaString() {
                "NA".to_string()
            } else {
                let bytes = crate::sexp::accessors::charsxp_as_utf8(charsxp);
                String::from_utf8(bytes)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
            }
        } else if t == SEXPTYPE::SYMSXP {
            let pname = crate::sexp::accessors::PRINTNAME(x);
            if pname.is_null() {
                "symbol".to_string()
            } else {
                let s = crate::sexp::accessors::CHAR(pname);
                if s.is_null() {
                    "symbol".to_string()
                } else {
                    std::ffi::CStr::from_ptr(s)
                        .to_str()
                        .unwrap_or("symbol")
                        .to_string()
                }
            }
        } else if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP {
            let elt = crate::sexp::accessors::VECTOR_ELT(x, idx);
            sexp_key(elt)
        } else {
            format!("{:?}", t)
        }
    }
}

/// Stable key for one SEXP, used by duplicated() on lists of rows.
pub(crate) fn sexp_key(x: SEXP) -> String {
    unsafe {
        if x.is_null() || x == crate::sexp::globals::R_NilValue() {
            return "NULL".to_string();
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LANGSXP || t == SEXPTYPE::SYMSXP || t == SEXPTYPE::CLOSXP {
            let text = crate::mainutils::deparse::deparse1line(x, false);
            if !text.is_null() && text != crate::sexp::globals::R_NilValue() && XLENGTH(text) > 0 {
                let charsxp = crate::sexp::accessors::STRING_ELT(text, 0);
                let bytes = crate::sexp::accessors::charsxp_as_utf8(charsxp);
                return String::from_utf8_lossy(&bytes).into_owned();
            }
        }
        if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP {
            let n = XLENGTH(x);
            let mut parts = Vec::with_capacity(n as usize);
            for i in 0..n {
                parts.push(sexp_key(crate::sexp::accessors::VECTOR_ELT(x, i)));
            }
            return format!("L({})", parts.join("\x1f"));
        }
        let n = XLENGTH(x).max(1);
        let mut parts = Vec::with_capacity(n as usize);
        for i in 0..n {
            parts.push(elt_to_string(x, i));
        }
        format!("V:{}", parts.join("\x1f"))
    }
}

pub(crate) unsafe fn factor_label_at(x: SEXP, code: i32) -> Option<String> {
    unsafe {
        if code <= 0 {
            return None;
        }
        let levels =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_LevelsSymbol());
        if levels.is_null() || levels == R_NilValue() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return None;
        }
        let index = (code - 1) as R_xlen_t;
        if index >= XLENGTH(levels) {
            return None;
        }
        let charsxp = STRING_ELT(levels, index);
        if charsxp.is_null() {
            None
        } else if charsxp == crate::sexp::globals::R_NaString() {
            Some("NA".to_string())
        } else {
            Some(CStr::from_ptr(CHAR(charsxp)).to_string_lossy().into_owned())
        }
    }
}

/// `cat()` rendering: GNU print digits for real/complex/raw. paste/as.character
/// keep using `elt_to_string` (15-digit-style conversion).
pub(crate) fn cat_elt_to_string(x: SEXP, i: R_xlen_t) -> String {
    unsafe {
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        if n <= 0 {
            return elt_to_string(x, i);
        }
        let idx = i.rem_euclid(n);
        if t == SEXPTYPE::REALSXP {
            crate::sexp::output::format_cat_real(*REAL(x).add(idx as usize))
        } else if t == SEXPTYPE::CPLXSXP {
            crate::sexp::output::format_complex_value(*COMPLEX(x).add(idx as usize))
        } else if t == SEXPTYPE::RAWSXP {
            crate::sexp::output::format_raw_value(*RAW(x).add(idx as usize))
        } else {
            elt_to_string(x, i)
        }
    }
}

pub(crate) fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub(crate) fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

pub(crate) fn days_from_civil(mut year: i64, month: i64, day: i64) -> i64 {
    year -= if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub(crate) fn civil_from_days(mut days: i64) -> (i64, i64, i64) {
    days += 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_prime = (5 * doy + 2) / 153;
    let day = doy - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

pub(crate) fn parse_iso_date_days(text: &str) -> Option<f64> {
    let text = text.trim();
    if text == "Inf" || text == "+Inf" {
        return Some(f64::INFINITY);
    }
    if text == "-Inf" {
        return Some(f64::NEG_INFINITY);
    }
    if text.eq_ignore_ascii_case("NaN") {
        return Some(f64::NAN);
    }
    // GNU accepts '/' when it is the only separator.
    let sep = if text.contains('/') && !text.contains('-') {
        '/'
    } else {
        '-'
    };
    let mut parts = text.split(sep);
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<i64>().ok()?;
    let day_tok = parts.next()?;
    let (day_digits, rest) = day_tok
        .split_once(char::is_whitespace)
        .unwrap_or((day_tok, ""));
    let day = day_digits.parse::<i64>().ok()?;
    let rest = rest.trim();
    if !rest.is_empty() && rest.split(':').any(|piece| piece.parse::<u32>().is_err()) {
        return None;
    }
    if parts.next().is_some()
        || !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
    {
        return None;
    }
    Some(days_from_civil(year, month, day) as f64)
}

pub(crate) fn date_days_to_iso(days: f64) -> Option<String> {
    if days.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
        return Some("NA".to_string());
    }
    if days.is_nan() {
        return Some("NaN".to_string());
    }
    if days.is_infinite() {
        return Some(if days > 0.0 { "Inf" } else { "-Inf" }.to_string());
    }
    let (year, month, day) = date_days_to_civil(days)?;
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

pub(crate) fn date_days_to_civil(days: f64) -> Option<(i64, i64, i64)> {
    if days.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || !days.is_finite() {
        return None;
    }
    Some(civil_from_days(days.floor() as i64))
}

pub(crate) fn parse_iso_datetime_seconds(text: &str) -> Option<f64> {
    let text = text.trim();
    if text == "Inf" || text == "+Inf" {
        return Some(f64::INFINITY);
    }
    if text == "-Inf" {
        return Some(f64::NEG_INFINITY);
    }
    if text.eq_ignore_ascii_case("NaN") {
        return Some(f64::NAN);
    }
    let mut fields = text.split_whitespace();
    let date = fields.next()?;
    let time = fields.next().unwrap_or("00:00:00");
    if fields.next().is_some() {
        return None;
    }
    let days = parse_iso_date_days(date)?;
    if !days.is_finite() {
        return Some(days);
    }
    let mut parts = time.split(':');
    let hour = parts.next()?.parse::<i64>().ok()?;
    let minute = parts.next()?.parse::<i64>().ok()?;
    let second = parts.next().unwrap_or("0").parse::<i64>().ok()?;
    if parts.next().is_some()
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    Some(days * 86_400.0 + (hour * 3_600 + minute * 60 + second) as f64)
}

pub(crate) fn posix_seconds_to_iso(seconds: f64, include_tz: bool) -> Option<String> {
    posix_seconds_to_iso_with_time(seconds, include_tz, false)
}

pub(crate) fn posix_seconds_to_iso_with_time(
    seconds: f64,
    include_tz: bool,
    force_time: bool,
) -> Option<String> {
    if seconds.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || !seconds.is_finite() {
        return None;
    }
    let whole = seconds.floor() as i64;
    let days = whole.div_euclid(86_400);
    let rem = whole.rem_euclid(86_400);
    let date = date_days_to_iso(days as f64)?;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;
    let mut out = if !force_time && hour == 0 && minute == 0 && second == 0 {
        date
    } else {
        format!("{date} {hour:02}:{minute:02}:{second:02}")
    };
    if include_tz {
        out.push_str(" UTC");
    }
    Some(out)
}

pub(crate) unsafe fn sexp_has_class(x: SEXP, class_name: &str) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return false;
        }
        (0..XLENGTH(class)).any(|i| elt_to_string(class, i) == class_name)
    }
}

pub(crate) fn elt_real_safe(x: SEXP, i: R_xlen_t) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return NA_REAL;
        }
        let n = XLENGTH(x);
        if n == 0 {
            return NA_REAL;
        }
        let idx = if n == 0 { 0 } else { i % n };
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            *REAL(x).add(idx as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(idx as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        }
    }
}
