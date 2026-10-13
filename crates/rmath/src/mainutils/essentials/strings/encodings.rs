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

fn iconv_norm_enc(name: &str) -> &str {
    // GNU treats `ASCII//TRANSLIT` and `UTF-8//IGNORE` as the base
    // encoding plus a substitution policy. The base is what we can encode.
    let name = name.split("//").next().unwrap_or(name);
    match name {
        "" | "native.enc" => "native",
        "UTF-8" | "UTF8" | "utf-8" | "utf8" => "UTF-8",
        "latin1" | "LATIN1" | "ISO-8859-1" | "ISO8859-1" | "iso-8859-1" => "latin1",
        "ASCII" | "US-ASCII" | "ascii" | "ANSI_X3.4-1968" => "ASCII",
        other => other,
    }
}

fn iconv_charsxp_bytes(ch: SEXP) -> &'static [u8] {
    unsafe {
        if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
            return &[];
        }
        let n = XLENGTH(ch);
        if n <= 0 {
            &[]
        } else {
            std::slice::from_raw_parts(CHAR(ch) as *const u8, n as usize)
        }
    }
}

fn iconv_decode(bytes: &[u8], from: &str, marked: &str) -> Option<Vec<char>> {
    let enc = if from == "native" {
        if marked == "latin1" {
            "latin1"
        } else if marked == "UTF-8" || marked == "unknown" {
            "UTF-8"
        } else {
            "UTF-8"
        }
    } else {
        from
    };
    match enc {
        "latin1" => Some(bytes.iter().map(|&b| char::from(b)).collect()),
        "ASCII" => {
            if bytes.iter().all(|&b| b < 0x80) {
                Some(bytes.iter().map(|&b| char::from(b)).collect())
            } else {
                None
            }
        }
        _ => std::str::from_utf8(bytes).ok().map(|s| s.chars().collect()),
    }
}

fn iconv_encode(chars: &[char], to: &str, sub: Option<&str>, translit: bool) -> Option<Vec<u8>> {
    match to {
        "UTF-8" | "native" => {
            let mut out = Vec::new();
            for &ch in chars {
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
            Some(out)
        }
        "latin1" => {
            let mut out = Vec::new();
            for &ch in chars {
                let cp = ch as u32;
                if cp <= 0xff {
                    out.push(cp as u8);
                } else {
                    match sub {
                        None => return None,
                        Some("") => {}
                        Some("?") => out.push(b'?'),
                        Some("byte") => {
                            let mut buf = [0u8; 4];
                            let raw = ch.encode_utf8(&mut buf).as_bytes();
                            for b in raw {
                                out.extend_from_slice(format!("<{b:02x}>").as_bytes());
                            }
                        }
                        Some("Unicode") => {
                            out.extend_from_slice(format!("<U+{cp:04X}>").as_bytes());
                        }
                        Some("c99") => {
                            if cp <= 0xffff {
                                out.extend_from_slice(format!("\\u{cp:04x}").as_bytes());
                            } else {
                                out.extend_from_slice(format!("\\U{cp:08x}").as_bytes());
                            }
                        }
                        Some(other) => out.extend_from_slice(other.as_bytes()),
                    }
                }
            }
            Some(out)
        }
        "ASCII" => {
            let mut out = Vec::new();
            for &ch in chars {
                let cp = ch as u32;
                if cp <= 0x7f {
                    out.push(cp as u8);
                } else {
                    // With //TRANSLIT, macOS libiconv substitutes an ASCII
                    // approximation first; only unmapped code points fall
                    // through to the 'sub' policy.
                    let transliterated = if translit {
                        ascii_translit(cp).map(|text| text.as_bytes().to_vec())
                    } else {
                        None
                    };
                    match transliterated {
                        Some(bytes) => out.extend_from_slice(&bytes),
                        None => match sub {
                            None => return None,
                            Some("") => {}
                            Some("?") => out.push(b'?'),
                            Some("byte") => {
                                if cp <= 0xff {
                                    out.extend_from_slice(format!("<{cp:02x}>").as_bytes());
                                } else {
                                    let mut buf = [0u8; 4];
                                    let raw = ch.encode_utf8(&mut buf).as_bytes();
                                    for b in raw {
                                        out.extend_from_slice(format!("<{b:02x}>").as_bytes());
                                    }
                                }
                            }
                            Some("Unicode") => {
                                out.extend_from_slice(format!("<U+{cp:04X}>").as_bytes());
                            }
                            Some("c99") => {
                                if cp <= 0xffff {
                                    out.extend_from_slice(format!("\\u{cp:04x}").as_bytes());
                                } else {
                                    out.extend_from_slice(format!("\\U{cp:08x}").as_bytes());
                                }
                            }
                            Some(other) => out.extend_from_slice(other.as_bytes()),
                        },
                    }
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// macOS libiconv's Latin-1 supplement transliterations for
/// `ASCII//TRANSLIT` (0xA0-0xFF). `None` marks code points iconv cannot
/// transliterate.
fn ascii_translit(cp: u32) -> Option<&'static str> {
    Some(match cp {
        0xA0 => " ",
        0xA1 => "!",
        0xA2 => "c",
        0xA3 => "lb",
        0xA4 => return None,
        0xA5 => "yen",
        0xA6 => "|",
        0xA7 => "SS",
        0xA8 => "\"",
        0xA9 => "(c)",
        0xAA => "a",
        0xAB => "<<",
        0xAC => "not",
        0xAD => "-",
        0xAE => "(R)",
        0xAF => return None,
        0xB0 => "^0",
        0xB1 => "+/-",
        0xB2 => "^2",
        0xB3 => "^3",
        0xB4 => "'",
        0xB5 => "u",
        0xB6 => "P",
        0xB7 => ".",
        0xB8 => ",",
        0xB9 => "^1",
        0xBA => "o",
        0xBB => ">>",
        0xBC..=0xBF => return None,
        0xC0 => "`A",
        0xC1 => "'A",
        0xC2 => "^A",
        0xC3 => "~A",
        0xC4 => "\"A",
        0xC5 => "A",
        0xC6 => "AE",
        0xC7 => "C",
        0xC8 => "`E",
        0xC9 => "'E",
        0xCA => "^E",
        0xCB => "\"E",
        0xCC => "`I",
        0xCD => "'I",
        0xCE => "^I",
        0xCF => "\"I",
        0xD0 => "D",
        0xD1 => "~N",
        0xD2 => "`O",
        0xD3 => "'O",
        0xD4 => "^O",
        0xD5 => "~O",
        0xD6 => "\"O",
        0xD7 => "x",
        0xD8 => "O",
        0xD9 => "`U",
        0xDA => "'U",
        0xDB => "^U",
        0xDC => "\"U",
        0xDD => "'Y",
        0xDE => "Th",
        0xDF => "ss",
        0xE0 => "`a",
        0xE1 => "'a",
        0xE2 => "^a",
        0xE3 => "~a",
        0xE4 => "\"a",
        0xE5 => "a",
        0xE6 => "ae",
        0xE7 => "c",
        0xE8 => "`e",
        0xE9 => "'e",
        0xEA => "^e",
        0xEB => "\"e",
        0xEC => "`i",
        0xED => "'i",
        0xEE => "^i",
        0xEF => "\"i",
        0xF0 => "d",
        0xF1 => "~n",
        0xF2 => "`o",
        0xF3 => "'o",
        0xF4 => "^o",
        0xF5 => "~o",
        0xF6 => "\"o",
        0xF7 => ":",
        0xF8 => "o",
        0xF9 => "`u",
        0xFA => "'u",
        0xFB => "^u",
        0xFC => "\"u",
        0xFD => "'y",
        0xFE => "th",
        0xFF => "\"y",
        _ => return None,
    })
}

/// GNU `iconv(x, from, to, sub, mark, toRaw)`.
pub unsafe fn do_iconv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        if TYPEOF(x) != SEXPTYPE::STRSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid 'x' argument",
            );
        }
        let from = {
            let a = arg_by_name_or_position(args, &["from"], 1);
            if a.is_null() || a == R_NilValue() || TYPEOF(a) != SEXPTYPE::STRSXP || XLENGTH(a) == 0
            {
                String::new()
            } else {
                elt_to_string(a, 0)
            }
        };
        let to = {
            let a = arg_by_name_or_position(args, &["to"], 2);
            if a.is_null() || a == R_NilValue() || TYPEOF(a) != SEXPTYPE::STRSXP || XLENGTH(a) == 0
            {
                String::new()
            } else {
                elt_to_string(a, 0)
            }
        };
        let from_n = iconv_norm_enc(&from);
        let to_n = iconv_norm_enc(&to);
        let translit = to
            .split("//")
            .nth(1)
            .is_some_and(|flag| flag.eq_ignore_ascii_case("TRANSLIT"));
        let sub_arg = arg_by_name_or_position(args, &["sub"], 3);
        let sub = if sub_arg.is_null()
            || sub_arg == R_NilValue()
            || (TYPEOF(sub_arg) == SEXPTYPE::LGLSXP
                && XLENGTH(sub_arg) > 0
                && *LOGICAL(sub_arg) == NA_LOGICAL)
            || (TYPEOF(sub_arg) == SEXPTYPE::STRSXP && is_string_na(sub_arg, 0))
        {
            None
        } else if TYPEOF(sub_arg) == SEXPTYPE::STRSXP && XLENGTH(sub_arg) > 0 {
            Some(elt_to_string(sub_arg, 0))
        } else {
            None
        };
        let mark_arg = arg_by_name_or_position(args, &["mark"], 4);
        let mark = mark_arg.is_null()
            || mark_arg == R_NilValue()
            || (TYPEOF(mark_arg) == SEXPTYPE::LGLSXP
                && XLENGTH(mark_arg) > 0
                && *LOGICAL(mark_arg) == TRUE)
            || TYPEOF(mark_arg) == SEXPTYPE::NILSXP;
        let n = XLENGTH(x);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for i in 0..n {
            let ch = STRING_ELT(x, i);
            if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
                SET_STRING_ELT(out, i, crate::sexp::globals::R_NaString());
                continue;
            }
            let marked = if crate::sexp::accessors::IS_BYTES(ch) != 0 {
                "bytes"
            } else if crate::sexp::accessors::IS_LATIN1(ch) != 0 {
                "latin1"
            } else if crate::sexp::accessors::IS_UTF8(ch) != 0 {
                "UTF-8"
            } else {
                "unknown"
            };
            let bytes = iconv_charsxp_bytes(ch);
            let Some(chars) = iconv_decode(bytes, from_n, marked) else {
                SET_STRING_ELT(out, i, crate::sexp::globals::R_NaString());
                continue;
            };
            let sub_ref = sub.as_deref();
            let Some(encoded) = iconv_encode(&chars, to_n, sub_ref, translit) else {
                SET_STRING_ELT(out, i, crate::sexp::globals::R_NaString());
                continue;
            };
            let marked_ch = crate::sexp::constructors::Rf_mkCharLen(
                encoded.as_ptr() as *const std::os::raw::c_char,
                encoded.len() as c_int,
            );
            if mark && crate::sexp::accessors::IS_ASCII(marked_ch) == 0 {
                let kind = match to_n {
                    "UTF-8" => "UTF-8",
                    "latin1" => "latin1",
                    _ => "unknown",
                };
                crate::sexp::accessors::mark_charsxp_encoding(marked_ch, kind);
            }
            SET_STRING_ELT(out, i, marked_ch);
        }
        out
    }
}

const ICONVLIST: &[&str] = &[
    "ASCII",
    "BIG5",
    "CP1252",
    "EUC-JP",
    "EUC-KR",
    "GB18030",
    "GBK",
    "ISO-8859-1",
    "ISO8859-1",
    "LATIN1",
    "SHIFT_JIS",
    "US-ASCII",
    "UTF-16",
    "UTF-16BE",
    "UTF-16LE",
    "UTF-32",
    "UTF-8",
];

/// GNU `iconvlist()`.
pub unsafe fn do_iconvlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _ = args;
        let n = ICONVLIST.len() as i64;
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for (i, name) in ICONVLIST.iter().enumerate() {
            let c = CString::new(*name).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(out, i as i64, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}
