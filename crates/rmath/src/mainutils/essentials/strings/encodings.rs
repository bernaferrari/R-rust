//! Encoding marks and iconv: encodeString, encoding, localeToCharset, iconv

use super::*;

/// GNU `encodeString(x, width, quote, na.encode, justify)`.
pub unsafe fn do_encodeString(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x0 = CAR(args);
        if x0.is_null() || x0 == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let x = if TYPEOF(x0) == SEXPTYPE::STRSXP {
            x0
        } else {
            crate::main::coerce::coerceVector(x0, SEXPTYPE::STRSXP.as_c_int())
        };
        let _x = protect(x);
        let mut width = 0;
        let mut quote: c_int = 0;
        let mut justify: c_int = 0;
        let mut na_encode = true;
        let mut positional = 0;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                let pname = PRINTNAME(tag);
                if !pname.is_null() {
                    Some(
                        std::ffi::CStr::from_ptr(CHAR(pname))
                            .to_string_lossy()
                            .into_owned(),
                    )
                } else {
                    None
                }
            } else {
                None
            };
            let slot = match name.as_deref() {
                Some("width") => 0,
                Some("quote") => 1,
                Some("na.encode") => 2,
                Some("justify") => 3,
                _ => {
                    let s = positional;
                    positional += 1;
                    s
                }
            };
            match slot {
                0 => {
                    let v = crate::main::coerce::asInteger(value);
                    if v != NA_INTEGER && v < 0 {
                        crate::mainutils::errors::errorcall_str(
                            crate::mainutils::errors::R_getCurrentCall(),
                            "invalid 'width' value",
                        );
                    }
                    width = v;
                }
                1 => {
                    if TYPEOF(value) == SEXPTYPE::STRSXP && XLENGTH(value) >= 1 {
                        let q = elt_to_string(value, 0);
                        if let Some(ch) = q.chars().next() {
                            quote = ch as c_int;
                        }
                    }
                }
                2 => {
                    na_encode = crate::main::coerce::asLogical(value) != 0;
                }
                3 => {
                    if TYPEOF(value) == SEXPTYPE::STRSXP {
                        let text = elt_to_string(value, 0);
                        justify = match text.as_str() {
                            "right" => 1,
                            "centre" | "center" => 2,
                            "none" => 3,
                            _ => 0,
                        };
                    } else {
                        let v = crate::main::coerce::asInteger(value);
                        if v != NA_INTEGER && (0..=3).contains(&v) {
                            justify = v;
                        }
                    }
                }
                _ => {}
            }
            cell = CDR(cell);
        }
        if justify == 3 {
            width = 0;
        }
        let len = XLENGTH(x);
        let find_width = width == NA_INTEGER;
        let mut w = width;
        if find_width && justify < 3 {
            w = 0;
            for i in 0..len {
                let s = STRING_ELT(x, i);
                if na_encode || s != crate::sexp::globals::R_NaString() {
                    w = w.max(crate::mainutils::printutils::Rstrlen(s, quote));
                }
            }
            if quote != 0 {
                w += 2;
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, len);
        let _result = protect(result);
        let adj = match justify {
            1 => crate::mainutils::printutils::Rprt_adj::right,
            2 => crate::mainutils::printutils::Rprt_adj::centre,
            3 => crate::mainutils::printutils::Rprt_adj::none,
            _ => crate::mainutils::printutils::Rprt_adj::left,
        };
        for i in 0..len {
            let s = STRING_ELT(x, i);
            if !na_encode && s == crate::sexp::globals::R_NaString() {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }
            let encoded = crate::mainutils::printutils::EncodeString(s, w, quote, adj);
            SET_STRING_ELT(result, i, Rf_mkChar(encoded));
        }
        result
    }
}

/// `.Internal(encodeString(x, width, quote, justify, na.encode))` wrapper:
/// the GNU internal contract passes `justify` (already reduced to its
/// 0-3 integer code by the closure) BEFORE `na.encode`, while the shared
/// `do_encodeString` handler parses the closure-visible positional order
/// `encodeString(x, width, quote, na.encode, justify)`. Tag the two
/// trailing arguments by name so both shapes resolve identically.
pub unsafe fn do_encodeString_internal(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut cell = crate::sexp::accessors::CDR(args);
        let mut index = 0;
        while !cell.is_null() && cell != R_NilValue() {
            if index >= 2 {
                let label = if index == 2 { "justify" } else { "na.encode" };
                let cstring = std::ffi::CString::new(label).unwrap_or_default();
                crate::sexp::accessors::SETTAG(
                    cell,
                    crate::sexp::symbol::Rf_install(cstring.as_ptr()),
                );
            }
            cell = crate::sexp::accessors::CDR(cell);
            index += 1;
        }
        do_encodeString(call, op, args, rho)
    }
}

/// GNU `Encoding(x)` — per-element "unknown"/"latin1"/"UTF-8"/"bytes".
pub unsafe fn do_encoding(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::STRSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "a character vector argument expected",
            );
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _result = protect(result);
        for i in 0..n {
            let el = STRING_ELT(x, i);
            let label = if crate::sexp::accessors::IS_BYTES(el) != 0 {
                "bytes"
            } else if crate::sexp::accessors::IS_LATIN1(el) != 0 {
                "latin1"
            } else if crate::sexp::accessors::IS_UTF8(el) != 0 {
                "UTF-8"
            } else {
                "unknown"
            };
            let cstr = CString::new(label).unwrap_or_default();
            SET_STRING_ELT(result, i, Rf_mkChar(cstr.as_ptr()));
        }
        result
    }
}

/// GNU `Encoding<-` — mark CHARSXP encodings, recycling `value`.
pub unsafe fn do_setencoding(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut x = CAR(args);
        let enc = CADR(args);
        if TYPEOF(x) != SEXPTYPE::STRSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "a character vector argument expected",
            );
        }
        if TYPEOF(enc) != SEXPTYPE::STRSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "a character vector 'value' expected",
            );
        }
        let m = XLENGTH(enc);
        if m == 0 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'value' must be of positive length",
            );
        }
        if crate::sexp::accessors::NAMED(x) > 0 {
            x = crate::mainutils::duplicate::duplicate(x);
        }
        let _x = protect(x);
        let n = XLENGTH(x);
        for i in 0..n {
            let tmp = STRING_ELT(x, i);
            if tmp == crate::sexp::globals::R_NaString() {
                continue;
            }
            // GNU: ASCII strings never have a declared encoding.
            if crate::sexp::accessors::IS_ASCII(tmp) != 0 {
                continue;
            }
            let label = elt_to_string(enc, i % m);
            let kind = match label.as_str() {
                "latin1" => "latin1",
                "UTF-8" => "UTF-8",
                "bytes" => "bytes",
                _ => "unknown",
            };
            let already = match kind {
                "latin1" => crate::sexp::accessors::IS_LATIN1(tmp) != 0,
                "UTF-8" => crate::sexp::accessors::IS_UTF8(tmp) != 0,
                "bytes" => crate::sexp::accessors::IS_BYTES(tmp) != 0,
                _ => {
                    crate::sexp::accessors::IS_LATIN1(tmp) == 0
                        && crate::sexp::accessors::IS_UTF8(tmp) == 0
                        && crate::sexp::accessors::IS_BYTES(tmp) == 0
                }
            };
            if already {
                continue;
            }
            let len = crate::sexp::accessors::LENGTH(tmp);
            let marked = crate::sexp::constructors::Rf_mkCharLen(CHAR(tmp), len);
            crate::sexp::accessors::mark_charsxp_encoding(marked, kind);
            SET_STRING_ELT(x, i, marked);
        }
        x
    }
}

/// GNU `localeToCharset(locale)`.
pub unsafe fn do_localeToCharset(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut locale = String::new();
        let first = CAR(args);
        if !first.is_null()
            && first != R_NilValue()
            && TYPEOF(first) == SEXPTYPE::STRSXP
            && XLENGTH(first) > 0
        {
            let ch = STRING_ELT(first, 0);
            if !ch.is_null() {
                locale = std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned();
            }
        }
        if locale.is_empty() {
            let cat = Rf_mkString(c"LC_CTYPE".as_ptr());
            let _c = protect(cat);
            let got = crate::mainutils::essentials::do_Sys_getlocale(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                Rf_cons(cat, R_NilValue()),
                std::ptr::null_mut(),
            );
            if TYPEOF(got) == SEXPTYPE::STRSXP && XLENGTH(got) > 0 {
                let ch = STRING_ELT(got, 0);
                if !ch.is_null() {
                    locale = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                }
            }
            if locale.is_empty() {
                locale = "C".to_string();
            }
        }
        let charset = if locale == "C" || locale == "POSIX" {
            "ASCII"
        } else if locale.to_ascii_lowercase().contains("utf-8")
            || locale.to_ascii_lowercase().contains("utf8")
        {
            "UTF-8"
        } else {
            "ASCII"
        };
        Rf_mkString(CString::new(charset).unwrap_or_default().as_ptr())
    }
}

/// Own the original argument graph before any conversion or allocation.
pub unsafe fn do_iconv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { iconv_entry(args, false) }
}
pub unsafe fn do_iconvlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { iconv_entry(args, true) }
}
unsafe fn iconv_entry(args: SEXP, inventory: bool) -> SEXP {
    use crate::sexp::{
        object::Sexp,
        owner::{OwnerToken, with_runtime},
    };
    let owner = unsafe { OwnerToken::current() }
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let _pin = owner
        .pin()
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let own = |raw| {
        owner
            .sexp(raw)
            .and_then(Sexp::into_owned)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
    };
    let args = if inventory {
        own(unsafe { R_NilValue() })
    } else {
        own(args)
    };
    let na = own(unsafe { crate::sexp::globals::R_NaString() });
    let weak = owner
        .weak_owner()
        .unwrap_or_else(|| crate::sexp::context::r_error("iconv requires a managed runtime"));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_runtime(&weak, |access| {
            if inventory {
                super::iconv_owned::inventory(access)
            } else {
                super::iconv_owned::evaluate(access, args, na, |value| {
                    access.with_native(|original| {
                        let flag = unsafe { crate::main::coerce::asLogical(value.as_raw()) };
                        original.require_active()?;
                        Ok(flag)
                    })
                })
            }
        })
    }));
    owner
        .require_active()
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    match outcome {
        Ok(Ok(Ok(value))) => value.as_raw(),
        Ok(Ok(Err(crate::sexp::object::SexpError::EvaluationFailed { message }))) => {
            crate::sexp::context::r_error(message)
        }
        Ok(Ok(Err(error))) | Ok(Err(error)) => crate::sexp::context::r_error(error.to_string()),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

#[cfg(test)]
mod portable_iconv_tests {
    fn iconv_norm_enc(name: &str) -> &str {
        name
    }
    fn conversion(
        bytes: &[u8],
        from: &str,
        to: &str,
        sub: Option<&str>,
        translit: bool,
    ) -> Option<Vec<u8>> {
        use super::super::iconv_codec::{lookup, visit};
        let mut data = Vec::new();
        visit(
            bytes,
            lookup(from)?,
            lookup(to)?,
            sub.map(str::as_bytes),
            translit,
            |part| {
                data.extend_from_slice(part);
                Ok::<(), ()>(())
            },
        )
        .ok()?
        .then_some(data)
    }
    fn iconv_decode(bytes: &[u8], from: &str, _marked: &str) -> Option<Vec<char>> {
        Some(
            String::from_utf8(conversion(bytes, from, "UTF-8", None, false)?)
                .ok()?
                .chars()
                .collect(),
        )
    }
    fn iconv_encode(
        chars: &[char],
        to: &str,
        sub: Option<&str>,
        translit: bool,
    ) -> Option<Vec<u8>> {
        conversion(
            chars.iter().collect::<String>().as_bytes(),
            "UTF-8",
            to,
            sub,
            translit,
        )
    }

    #[test]
    fn portable_iconv_inventory_only_advertises_working_ascii_conversions() {
        let names: Vec<_> = super::super::iconv_codec::names().collect();
        assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(names.contains(&"UTF-8"));
        assert!(names.contains(&"ASCII"));
        assert!(names.contains(&"ISO-8859-1"));
        assert!(!names.contains(&"UTF-16"));
        assert!(!names.contains(&"CP1252"));
        let expected: Vec<_> = "Portable ASCII 2026".chars().collect();
        for alias in names {
            let encoding = iconv_norm_enc(alias);
            let encoded = iconv_encode(&expected, encoding, None, false).unwrap();
            assert_eq!(
                iconv_decode(&encoded, encoding, "unknown"),
                Some(expected.clone()),
                "round trip {alias}"
            );
        }
    }

    #[test]
    fn portable_iconv_utf8_latin1_and_ascii_character_controls() {
        let latin = b"caf\xe9";
        let unicode: Vec<_> = "café".chars().collect();
        assert_eq!(
            iconv_decode(latin, iconv_norm_enc("ISO-8859-1"), "unknown"),
            Some(unicode.clone())
        );
        assert_eq!(
            iconv_encode(&unicode, iconv_norm_enc("UTF8"), None, false),
            Some("café".as_bytes().to_vec())
        );
        assert_eq!(
            iconv_decode("café".as_bytes(), iconv_norm_enc("utf-8"), "unknown"),
            Some(unicode.clone())
        );
        assert_eq!(
            iconv_encode(&unicode, iconv_norm_enc("LATIN1"), None, false),
            Some(latin.to_vec())
        );
        assert_eq!(iconv_decode(latin, "ASCII", "unknown"), None);
        assert_eq!(iconv_decode(latin, "UTF-8", "unknown"), None);
        assert_eq!(iconv_encode(&unicode, "ASCII", None, false), None);
        assert_eq!(iconv_encode(&['€'], "latin1", None, false), None);
        assert_eq!(
            iconv_encode(&unicode, "ASCII", Some("?"), false),
            Some(b"caf??".to_vec())
        );
    }

    #[test]
    fn portable_iconv_public_inventory_and_aliases_share_the_conversion_path() {
        let mut session = crate::sexp::session::RSession::new_without_default_packages();
        let (value, output, _) = session.eval_code_with_output_capture(
            r#"v <- iconvlist(); stopifnot(identical(v,.Internal(iconv(NULL,"","","",TRUE,FALSE))),identical(v,sort(v)),!anyDuplicated(v));
            for (enc in v) { raw <- iconv('Portable ASCII 2026',from='UTF-8',to=enc,toRaw=TRUE); stopifnot(identical(iconv(raw,from=enc,to='UTF-8'),'Portable ASCII 2026')) };
            latin <- rawToChar(as.raw(c(99,97,102,233)));
            u <- iconv(latin,from='ISO-8859-1',to='UTF-8');
            stopifnot(identical(charToRaw(u),as.raw(c(99,97,102,195,169))),identical(charToRaw(iconv(u,from='UTF-8',to='latin1')),as.raw(c(99,97,102,233)))); TRUE"#,
        );
        assert_eq!(
            value
                .unwrap_or_else(|error| panic!("{}: {output:?}", error.message))
                .logical_elt(0),
            Some(1)
        );
    }
}
