//! Pattern utilities: glob2rx, adist, aregexec, abbreviate

use super::*;

fn glob2rx_escape_open(s: &str, open: char) -> String {
    let b = s.as_bytes();
    let open_b = open as u8;
    let mut out = String::with_capacity(s.len() + 4);
    let mut i = 0;
    while i < b.len() {
        if i + 1 < b.len() && b[i] != b'\\' && b[i + 1] == open_b {
            out.push(b[i] as char);
            out.push('\\');
            out.push(open);
            i += 2;
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    out
}

fn glob2rx_one(pattern: &str, trim_head: bool, trim_tail: bool) -> String {
    let mut p = format!("^{pattern}$");
    p = p.replace('.', r"\.");
    p = p.replace('*', ".*");
    p = p.replace('?', ".");
    p = glob2rx_escape_open(&p, '(');
    p = glob2rx_escape_open(&p, '[');
    p = glob2rx_escape_open(&p, '{');
    if trim_tail {
        if let Some(i) = p.find(".*$") {
            p.replace_range(i..i + 3, "");
        }
    }
    if trim_head {
        if let Some(i) = p.find("^.*") {
            p.replace_range(i..i + 3, "");
        }
    }
    p
}

/// GNU `glob2rx(pattern, trim.head=FALSE, trim.tail=TRUE)`.
pub unsafe fn do_glob2rx(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern = CAR(args);
        if pattern.is_null() || pattern == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = if TYPEOF(pattern) == SEXPTYPE::STRSXP {
            XLENGTH(pattern)
        } else {
            0
        };
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let mut trim_head = false;
        let mut trim_tail = true;
        let mut cell = CDR(args);
        let mut positional = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let named = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let is_true = !value.is_null()
                && value != R_NilValue()
                && ((TYPEOF(value) == SEXPTYPE::LGLSXP
                    && XLENGTH(value) > 0
                    && *LOGICAL(value) == TRUE)
                    || (TYPEOF(value) == SEXPTYPE::INTSXP
                        && XLENGTH(value) > 0
                        && *INTEGER(value) != 0
                        && *INTEGER(value) != NA_INTEGER));
            let is_false = !value.is_null()
                && value != R_NilValue()
                && ((TYPEOF(value) == SEXPTYPE::LGLSXP
                    && XLENGTH(value) > 0
                    && *LOGICAL(value) == FALSE)
                    || (TYPEOF(value) == SEXPTYPE::INTSXP
                        && XLENGTH(value) > 0
                        && *INTEGER(value) == 0));
            if named == "trim.head" || (named.is_empty() && positional == 0) {
                trim_head = is_true;
            } else if named == "trim.tail" || (named.is_empty() && positional == 1) {
                trim_tail = !is_false;
            }
            if named.is_empty() {
                positional += 1;
            }
            cell = CDR(cell);
        }
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for i in 0..n {
            let ch = STRING_ELT(pattern, i);
            let raw = if ch.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned()
            };
            let converted = glob2rx_one(&raw, trim_head, trim_tail);
            let c = CString::new(converted).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(out, i, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}

fn adist_levenshtein(a: &str, b: &str) -> i32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    let mut prev: Vec<i32> = (0..=m as i32).collect();
    let mut curr = vec![0i32; m + 1];
    for i in 1..=n {
        curr[0] = i as i32;
        for j in 1..=m {
            let sub = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + sub);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

fn adist_path(a: &str, b: &str) -> (i32, String, i32, i32, i32) {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    let mut dp = vec![vec![0i32; m + 1]; n + 1];
    for i in 0..=n {
        dp[i][0] = i as i32;
    }
    for j in 0..=m {
        dp[0][j] = j as i32;
    }
    for i in 1..=n {
        for j in 1..=m {
            let sub = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + sub);
        }
    }
    let mut i = n;
    let mut j = m;
    let mut ops = Vec::new();
    let mut del = 0i32;
    let mut ins = 0i32;
    let mut sub = 0i32;
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a[i - 1] == b[j - 1] && dp[i][j] == dp[i - 1][j - 1] {
            ops.push('M');
            i -= 1;
            j -= 1;
        } else if i > 0 && j > 0 && dp[i][j] == dp[i - 1][j - 1] + 1 {
            ops.push('S');
            sub += 1;
            i -= 1;
            j -= 1;
        } else if j > 0 && dp[i][j] == dp[i][j - 1] + 1 {
            ops.push('I');
            ins += 1;
            j -= 1;
        } else if i > 0 {
            ops.push('D');
            del += 1;
            i -= 1;
        }
    }
    ops.reverse();
    (dp[n][m], ops.into_iter().collect(), del, ins, sub)
}

unsafe fn ints_to_char_vector(list: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(list);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if out.is_null() {
            return out;
        }
        for i in 0..n {
            let elt = VECTOR_ELT(list, i);
            let text = if TYPEOF(elt) == SEXPTYPE::INTSXP {
                let mut s = String::new();
                for k in 0..XLENGTH(elt) {
                    let cp = *INTEGER(elt).add(k as usize);
                    if let Some(ch) = char::from_u32(cp as u32) {
                        s.push(ch);
                    }
                }
                s
            } else {
                String::new()
            };
            if let Ok(cs) = std::ffi::CString::new(text) {
                SET_STRING_ELT(out, i, Rf_mkChar(cs.as_ptr()));
            }
        }
        out
    }
}

/// GNU `adist(x, y)` Levenshtein distances as an integer matrix.
pub unsafe fn do_adist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut x = CAR(args);
        if TYPEOF(x) == SEXPTYPE::VECSXP {
            x = ints_to_char_vector(x);
        }
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return crate::mainutils::array::allocMatrix(SEXPTYPE::INTSXP.as_c_int(), 0, 0);
        }
        let nx = XLENGTH(x);
        let mut y = x;
        let mut ignore_case = false;
        let mut counts = false;
        let mut cell = CDR(args);
        let mut positional = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let named = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if named == "ignore.case" {
                if TYPEOF(value) == SEXPTYPE::LGLSXP && XLENGTH(value) > 0 {
                    ignore_case = *LOGICAL(value) == TRUE;
                }
            } else if named == "counts" || (named.is_empty() && positional == 2) {
                if TYPEOF(value) == SEXPTYPE::LGLSXP && XLENGTH(value) > 0 {
                    counts = *LOGICAL(value) == TRUE;
                }
            } else if named == "y" || (named.is_empty() && positional == 0) {
                // First argument after x: the GNU closure passes the
                // utf8ToInt lists (VECSXP), not character vectors.
                y = value;
            }
            if named.is_empty() {
                positional += 1;
            }
            cell = CDR(cell);
        }
        if TYPEOF(y) == SEXPTYPE::VECSXP {
            y = ints_to_char_vector(y);
        }
        if y.is_null() || y == R_NilValue() || TYPEOF(y) != SEXPTYPE::STRSXP {
            y = x;
        }
        let ny = XLENGTH(y);
        let mat = crate::mainutils::array::allocMatrix(
            SEXPTYPE::INTSXP.as_c_int(),
            nx as c_int,
            ny as c_int,
        );
        let _m = protect(mat);
        let mut paths: Vec<(String, i32, i32, i32)> = Vec::new();
        for j in 0..ny {
            let ych = STRING_ELT(y, j);
            let mut ys = if ych.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ych))
                    .to_string_lossy()
                    .into_owned()
            };
            if ignore_case {
                ys = ys.to_lowercase();
            }
            for i in 0..nx {
                let xch = STRING_ELT(x, i);
                let mut xs = if xch.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(CHAR(xch))
                        .to_string_lossy()
                        .into_owned()
                };
                if ignore_case {
                    xs = xs.to_lowercase();
                }
                let (dist, trafo, del, ins, sub) = adist_path(&xs, &ys);
                *INTEGER(mat).add((i + j * nx) as usize) = dist;
                if counts {
                    paths.push((trafo, del, ins, sub));
                }
            }
        }
        if counts {
            let trafos = Rf_allocVector3(SEXPTYPE::STRSXP, nx * ny);
            let _t = protect(trafos);
            let cnt = Rf_allocVector3(SEXPTYPE::INTSXP, nx * ny * 3);
            let _c = protect(cnt);
            for (k, (trafo, del, ins, sub)) in paths.iter().enumerate() {
                let cstr = std::ffi::CString::new(trafo.as_str()).unwrap_or_default();
                SET_STRING_ELT(
                    trafos,
                    k as i64,
                    crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()),
                );
                *INTEGER(cnt).add(k) = *del;
                *INTEGER(cnt).add(k + (nx * ny) as usize) = *ins;
                *INTEGER(cnt).add(k + (2 * nx * ny) as usize) = *sub;
            }
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
            *INTEGER(dim).add(0) = nx as i32;
            *INTEGER(dim).add(1) = ny as i32;
            *INTEGER(dim).add(2) = 3;
            crate::sexp::attrib_core::setAttrib(cnt, crate::sexp::attrib_core::R_DimSymbol(), dim);
            let dnames = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
            let layer = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
            for (i, name) in ["del", "ins", "sub"].iter().enumerate() {
                let cstr = std::ffi::CString::new(*name).unwrap();
                SET_STRING_ELT(
                    layer,
                    i as i64,
                    crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()),
                );
            }
            SET_VECTOR_ELT(dnames, 2, layer);
            crate::sexp::attrib_core::setAttrib(
                cnt,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dnames,
            );
            let tdim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            *INTEGER(tdim).add(0) = nx as i32;
            *INTEGER(tdim).add(1) = ny as i32;
            crate::sexp::attrib_core::setAttrib(
                trafos,
                crate::sexp::attrib_core::R_DimSymbol(),
                tdim,
            );
            crate::sexp::attrib_core::setAttrib(
                mat,
                crate::sexp::symbol::Rf_install(c"trafos".as_ptr()),
                trafos,
            );
            crate::sexp::attrib_core::setAttrib(
                mat,
                crate::sexp::symbol::Rf_install(c"counts".as_ptr()),
                cnt,
            );
        }
        mat
    }
}

fn aregexec_match(pat: &str, text: &str, maxd: i32) -> Option<(i32, i32)> {
    let t: Vec<char> = text.chars().collect();
    let n = t.len();
    let plen = pat.chars().count() as i32;
    for start in 0..=n {
        for delta in 0..=maxd {
            for &len in &[plen, plen - delta, plen + delta] {
                if len < 0 {
                    continue;
                }
                let len = len as usize;
                if start + len > n {
                    continue;
                }
                let sub: String = t[start..start + len].iter().collect();
                if adist_levenshtein(pat, &sub) <= maxd {
                    return Some(((start + 1) as i32, len as i32));
                }
            }
        }
    }
    None
}

/// GNU `aregexec(pattern, text, max.distance)` for literal patterns.
pub unsafe fn do_aregexec(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pat = CAR(args);
        let text = CAR(CDR(args));
        let mut maxd = 0.1;
        let mut cell = CDR(CDR(args));
        // The GNU closure passes .Internal(aregexec(pattern, text,
        // bounds, costs, ...)): only the third argument (the expanded
        // match-distance bounds) carries the distance. Later unnamed
        // arguments (costs, flags) must not overwrite it.
        let mut positional = 2;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "max.distance" || (name.is_empty() && positional == 2) {
                let v = CAR(cell);
                if !v.is_null() && v != R_NilValue() && XLENGTH(v) > 0 {
                    let candidate = if TYPEOF(v) == SEXPTYPE::REALSXP {
                        *REAL(v)
                    } else if TYPEOF(v) == SEXPTYPE::INTSXP {
                        let iv = *INTEGER(v);
                        if iv == crate::sexp::ffi::NA_INTEGER {
                            f64::NAN
                        } else {
                            iv as f64
                        }
                    } else {
                        f64::NAN
                    };
                    // Scalar max.distance expands to bounds = c(x, NA*4);
                    // the list form leaves bounds[1] ("cost") NA and sets
                    // the shared "all" bound instead.
                    if !candidate.is_nan() {
                        maxd = candidate;
                    } else if XLENGTH(v) > 4
                        && TYPEOF(v) == SEXPTYPE::REALSXP
                        && !(*REAL(v).add(4)).is_nan()
                    {
                        maxd = *REAL(v).add(4);
                    }
                }
            }
            if name.is_empty() {
                positional += 1;
            }
            cell = CDR(cell);
        }
        if pat.is_null() || pat == R_NilValue() || text.is_null() || text == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let pstr = if TYPEOF(pat) == SEXPTYPE::STRSXP && XLENGTH(pat) > 0 {
            let ch = STRING_ELT(pat, 0);
            if ch.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned()
            }
        } else {
            String::new()
        };
        let max_int = if maxd >= 1.0 {
            maxd.floor() as i32
        } else {
            (maxd * pstr.chars().count() as f64).floor() as i32
        };
        let n = if TYPEOF(text) == SEXPTYPE::STRSXP {
            XLENGTH(text)
        } else {
            0
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        let _r = protect(result);
        let ml_sym = Rf_install(c"match.length".as_ptr());
        for i in 0..n {
            let ch = STRING_ELT(text, i);
            let txt = if ch.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned()
            };
            let elt = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
            let _e = protect(elt);
            let ml = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
            let _m = protect(ml);
            if let Some((start, len)) = aregexec_match(&pstr, &txt, max_int) {
                *INTEGER(elt) = start;
                *INTEGER(ml) = len;
            } else {
                *INTEGER(elt) = -1;
                *INTEGER(ml) = -1;
            }
            crate::sexp::attrib_core::setAttrib(elt, ml_sym, ml);
            SET_VECTOR_ELT(result, i, elt);
        }
        result
    }
}

fn abbreviate_first_char(s: &[u8], i: usize) -> bool {
    i > 0 && s[i - 1].is_ascii_whitespace()
}

fn abbreviate_last_char(s: &[u8], i: usize) -> bool {
    i > 0 && !s[i - 1].is_ascii_whitespace() && (i + 1 >= s.len() || s[i + 1].is_ascii_whitespace())
}

fn abbreviate_lc_vowel(c: u8) -> bool {
    matches!(c, b'a' | b'e' | b'i' | b'o' | b'u')
}

fn abbreviate_strip(inchar: &str, minlen: usize, usecl: bool) -> String {
    let trimmed = inchar.trim();
    if trimmed.len() < minlen {
        return trimmed.to_string();
    }
    let mut s: Vec<u8> = trimmed.as_bytes().to_vec();
    let mut nspace = 0usize;
    let mut j = 1i32;
    let mut i = s.len() - 1;
    while i > 0 {
        if s[i].is_ascii_whitespace() {
            if j != 0 {
                s.truncate(i);
            } else {
                nspace += 1;
            }
        } else {
            j = 0;
        }
        if s.len().saturating_sub(nspace) <= minlen {
            return abbreviate_drop_spaces(s, minlen);
        }
        i -= 1;
    }
    if usecl {
        i = s.len().saturating_sub(1);
        while i > 0 {
            if abbreviate_lc_vowel(s[i]) && abbreviate_last_char(&s, i) {
                s.remove(i);
            }
            if s.len().saturating_sub(nspace) <= minlen {
                return abbreviate_drop_spaces(s, minlen);
            }
            i -= 1;
        }
        i = s.len().saturating_sub(1);
        while i > 0 {
            if abbreviate_lc_vowel(s[i]) && !abbreviate_first_char(&s, i) {
                s.remove(i);
            }
            if s.len().saturating_sub(nspace) <= minlen {
                return abbreviate_drop_spaces(s, minlen);
            }
            i -= 1;
        }
        i = s.len().saturating_sub(1);
        while i > 0 {
            if s[i].is_ascii_lowercase() && abbreviate_last_char(&s, i) {
                s.remove(i);
            }
            if s.len().saturating_sub(nspace) <= minlen {
                return abbreviate_drop_spaces(s, minlen);
            }
            i -= 1;
        }
        i = s.len().saturating_sub(1);
        while i > 0 {
            if s[i].is_ascii_lowercase() && !abbreviate_first_char(&s, i) {
                s.remove(i);
            }
            if s.len().saturating_sub(nspace) <= minlen {
                return abbreviate_drop_spaces(s, minlen);
            }
            i -= 1;
        }
    }
    i = s.len().saturating_sub(1);
    while i > 0 {
        if !abbreviate_first_char(&s, i) && !s[i].is_ascii_whitespace() {
            s.remove(i);
        }
        if s.len().saturating_sub(nspace) <= minlen {
            return abbreviate_drop_spaces(s, minlen);
        }
        i -= 1;
    }
    abbreviate_drop_spaces(s, minlen)
}

fn abbreviate_drop_spaces(mut s: Vec<u8>, minlen: usize) -> String {
    if s.len() > minlen {
        let mut i = s.len() - 1;
        while i > 0 {
            if s[i].is_ascii_whitespace() {
                s.remove(i);
            }
            i -= 1;
        }
    }
    String::from_utf8(s).unwrap_or_default()
}

fn abbreviate_unique(names: &[String], minlength: usize) -> Vec<String> {
    if minlength == 0 {
        return vec![String::new(); names.len()];
    }
    let mut minlen = minlength;
    let mut x: Vec<String> = names.to_vec();
    let mut these_idx: Vec<usize> = (0..names.len()).collect();
    loop {
        for &idx in &these_idx {
            x[idx] = abbreviate_strip(&names[idx], minlen, true);
        }
        let mut seen = std::collections::HashMap::<String, usize>::new();
        let mut dup2 = vec![false; x.len()];
        let mut any = false;
        for (i, val) in x.iter().enumerate() {
            if let Some(&first) = seen.get(val) {
                dup2[i] = true;
                dup2[first] = true;
                any = true;
            } else {
                seen.insert(val.clone(), i);
            }
        }
        if !any {
            break;
        }
        minlen += 1;
        these_idx = (0..x.len()).filter(|&i| dup2[i]).collect();
    }
    x
}

/// GNU `abbreviate(names.arg, minlength=4)`.
pub unsafe fn do_abbreviate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names_s = CAR(args);
        if names_s.is_null() || names_s == R_NilValue() || TYPEOF(names_s) != SEXPTYPE::STRSXP {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = XLENGTH(names_s);
        let mut minlength = 4i32;
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let m = CAR(rest);
            if TYPEOF(m) == SEXPTYPE::INTSXP && XLENGTH(m) > 0 {
                minlength = *INTEGER(m);
            } else if TYPEOF(m) == SEXPTYPE::REALSXP && XLENGTH(m) > 0 {
                minlength = *REAL(m) as i32;
            }
        }
        let mut old = Vec::with_capacity(n as usize);
        for i in 0..n {
            let ch = STRING_ELT(names_s, i);
            if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
                old.push(String::new());
                continue;
            }
            let raw = std::ffi::CStr::from_ptr(CHAR(ch))
                .to_string_lossy()
                .into_owned();
            old.push(raw.trim().to_string());
        }
        let mut unique_names = Vec::new();
        let mut first_of: Vec<usize> = Vec::with_capacity(n as usize);
        for name in &old {
            if let Some(pos) = unique_names.iter().position(|u| u == name) {
                first_of.push(pos);
            } else {
                first_of.push(unique_names.len());
                unique_names.push(name.clone());
            }
        }
        let minlen = if minlength < 0 { 0 } else { minlength as usize };
        let uniq = abbreviate_unique(&unique_names, minlen);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        let nm = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _n = protect(nm);
        for i in 0..n as usize {
            let ch = STRING_ELT(names_s, i as i64);
            if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
                SET_STRING_ELT(out, i as i64, crate::sexp::globals::R_NaString());
                SET_STRING_ELT(nm, i as i64, crate::sexp::globals::R_NaString());
                continue;
            }
            let abbr = &uniq[first_of[i]];
            let c = CString::new(abbr.as_str()).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(out, i as i64, Rf_mkChar(c.as_ptr()));
            let oc = CString::new(old[i].as_str()).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(nm, i as i64, Rf_mkChar(oc.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(out, crate::sexp::attrib_core::R_NamesSymbol(), nm);
        out
    }
}
