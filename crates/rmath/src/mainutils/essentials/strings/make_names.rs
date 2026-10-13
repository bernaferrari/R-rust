//! make.unique and make.names

use super::*;

/// GNU `.Internal(make.unique(names, sep))`.
pub unsafe fn do_make_unique(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = CAR(args);
        if names.is_null() || names == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = XLENGTH(names);
        let mut sep = ".".to_string();
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let s = CAR(rest);
            if TYPEOF(s) == SEXPTYPE::STRSXP && XLENGTH(s) > 0 {
                let ch = STRING_ELT(s, 0);
                if !ch.is_null() {
                    sep = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        let mut bases: Vec<String> = Vec::with_capacity(n as usize);
        let mut seen = std::collections::HashSet::<String>::new();
        for i in 0..n {
            let ch = if TYPEOF(names) == SEXPTYPE::STRSXP {
                STRING_ELT(names, i)
            } else {
                std::ptr::null_mut()
            };
            let base = if ch.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned()
            };
            seen.insert(base.clone());
            bases.push(base);
        }
        let mut kept = std::collections::HashSet::<String>::new();
        let mut next = std::collections::HashMap::<String, u32>::new();
        for (i, base) in bases.into_iter().enumerate() {
            let unique = if kept.insert(base.clone()) {
                next.entry(base.clone()).or_insert(1);
                base
            } else {
                let mut k = next.get(&base).copied().unwrap_or(1);
                loop {
                    let cand = format!("{base}{sep}{k}");
                    k += 1;
                    if seen.insert(cand.clone()) {
                        next.insert(base, k);
                        break cand;
                    }
                }
            };
            let c = CString::new(unique).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(out, i as R_xlen_t, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}

const MAKE_NAMES_KEYWORDS: &[&str] = &[
    "NULL",
    "NA",
    "TRUE",
    "FALSE",
    "Inf",
    "NaN",
    "NA_integer_",
    "NA_real_",
    "NA_character_",
    "NA_complex_",
    "function",
    "while",
    "repeat",
    "for",
    "if",
    "in",
    "else",
    "next",
    "break",
];

fn make_names_is_valid(name: &str) -> bool {
    if name == "..." {
        return true;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first != '.' && !first.is_ascii_alphabetic() {
        return false;
    }
    if first == '.' {
        if let Some(second) = name.as_bytes().get(1) {
            if second.is_ascii_digit() {
                return false;
            }
        }
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
    {
        return false;
    }
    !MAKE_NAMES_KEYWORDS.contains(&name)
}

/// GNU `make.names(names, unique=FALSE, allow_=TRUE)`.
pub unsafe fn do_make_names(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = CAR(args);
        if names.is_null() || names == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = if TYPEOF(names) == SEXPTYPE::STRSXP {
            XLENGTH(names)
        } else {
            0
        };
        let mut unique = false;
        let mut allow_ = true;
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
            if named == "unique" || (named.is_empty() && positional == 0) {
                unique = is_true;
            } else if named == "allow_" || (named.is_empty() && positional == 1) {
                allow_ = is_true;
            }
            if named.is_empty() {
                positional += 1;
            }
            cell = CDR(cell);
        }
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        let mut originals = Vec::with_capacity(n as usize);
        let mut from_na = Vec::with_capacity(n as usize);
        let mut results = Vec::with_capacity(n as usize);
        for i in 0..n {
            let ch = STRING_ELT(names, i);
            let is_na = ch == crate::sexp::globals::R_NaString();
            let raw = if ch.is_null() {
                String::new()
            } else if is_na {
                "NA".to_string()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned()
            };
            originals.push(raw.clone());
            from_na.push(is_na);
            let mut s = raw;
            let need_prefix = if s.is_empty() {
                true
            } else {
                let b = s.as_bytes();
                if b[0] == b'.' {
                    b.len() >= 2 && b[1].is_ascii_digit()
                } else {
                    !b[0].is_ascii_alphabetic()
                }
            };
            if need_prefix {
                s.insert(0, 'X');
            }
            let bytes = unsafe { s.as_bytes_mut() };
            for b in bytes.iter_mut() {
                if *b == b'.' || (allow_ && *b == b'_') {
                    continue;
                }
                if !b.is_ascii_alphanumeric() {
                    *b = b'.';
                }
            }
            if !make_names_is_valid(&s) {
                s.push('.');
            }
            results.push(s);
        }
        if unique {
            let mut order: Vec<usize> = (0..results.len()).collect();
            order.sort_by_key(|&i| {
                if from_na[i] {
                    2
                } else if originals[i] != results[i] {
                    1
                } else {
                    0
                }
            });
            let tmp = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _t = protect(tmp);
            for (j, &i) in order.iter().enumerate() {
                let c =
                    CString::new(results[i].as_str()).unwrap_or_else(|_| CString::new("").unwrap());
                SET_STRING_ELT(tmp, j as i64, Rf_mkChar(c.as_ptr()));
            }
            let sep = Rf_mkString(c".".as_ptr());
            let _s = protect(sep);
            let uargs = Rf_cons(tmp, Rf_cons(sep, R_NilValue()));
            let _u = protect(uargs);
            let uniq = do_make_unique(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                uargs,
                std::ptr::null_mut(),
            );
            let _uq = protect(uniq);
            for (j, &i) in order.iter().enumerate() {
                let ch = STRING_ELT(uniq, j as i64);
                results[i] = std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned();
            }
        }
        for (i, s) in results.iter().enumerate() {
            let c = CString::new(s.as_str()).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(out, i as i64, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}
