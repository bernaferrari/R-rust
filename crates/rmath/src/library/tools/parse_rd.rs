//! Enough of GNU `parseRd` for the reg-tests-1c macro cases.
//!
use crate::sexp::accessors::{CAR, CDR, SET_STRING_ELT, TYPEOF};
use crate::sexp::constructors::Rf_allocVector3;

use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::protect;

pub unsafe extern "C-unwind" fn c_parse_rd(
    _call: SEXP,
    _op: SEXP,
    args: SEXP,
    _env: SEXP,
) -> SEXP {
    unsafe {
        let arg = CAR(CDR(args));
        let text = if TYPEOF(arg) == SEXPTYPE::STRSXP {
            let ch = crate::sexp::accessors::STRING_ELT(arg, 0);
            std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(ch))
                .to_string_lossy()
                .into_owned()
        } else {
            let con = crate::mainutils::coerce::asInteger(arg);
            let mut bytes = Vec::new();
            loop {
                let c = crate::mainutils::connections::connection_fgetc(con);
                if c < 0 {
                    break;
                }
                bytes.push(c as u8);
            }
            String::from_utf8_lossy(&bytes).into_owned()
        };
        let passed = nth_arg(args, 8);
        let mut extra = vec![("R".to_string(), "R".to_string())];
        extra.extend(macros_from_env(passed));
        let (expanded, defined) = expand_user_macros(&text, &extra);
        let macro_env = if defined.is_empty() {
            crate::sexp::globals::R_NilValue()
        } else {
            macro_environment(&defined, passed)
        };
        rd_text(&expanded, macro_env)
    }
}

fn lines_with_newlines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if ch == '\n' {
            lines.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        if !current.ends_with('\n') {
            current.push('\n');
        }
        lines.push(current);
    }
    lines
}
fn strip_rd_comments(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '%' {
            out.push('\\');
            out.push('%');
            i += 2;
            continue;
        }
        if chars[i] == '%' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}


fn expand_user_macros(input: &str, extra: &[(String, String)]) -> (String, Vec<(String, String)>) {
    let input = strip_rd_comments(input);
    let chars: Vec<char> = input.chars().collect();
    let mut macros: Vec<(String, String)> = extra.to_vec();
    let mut defined = Vec::new();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '%' {
            out.push('%');
            i += 2;
            continue;
        }
        if starts_with(&chars, i, "\\newcommand") || starts_with(&chars, i, "\\renewcommand") {
            let key = if starts_with(&chars, i, "\\renewcommand") { "\\renewcommand" } else { "\\newcommand" };
            i += key.chars().count();
            if let Some((name, next)) = read_braced(&chars, i) {
                let name = name.trim_start_matches('\\').to_string();
                i = next;
                if let Some((body, next)) = read_braced(&chars, i) {
                    macros.push((name.clone(), body.clone()));
                    defined.push((name, body));
                    i = next;
                    continue;
                }
            }
        }
        if chars[i] == '\\' {
            i += 1;
            let mut name = String::new();
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                name.push(chars[i]);
                i += 1;
            }
            if let Some((_, body)) = macros.iter().rev().find(|(n, _)| n == &name) {
                let mut args = Vec::new();
                while let Some((arg, next)) = read_braced(&chars, i) {
                    args.push(arg);
                    i = next;
                }
                let body = substitute_args(body, &args);
                let (expanded, _) = expand_user_macros(&body, extra);
                out.push_str(&expanded);
                continue;
            }
            out.push('\\');
            out.push_str(&name);
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    (out, defined)
}

fn starts_with(chars: &[char], i: usize, text: &str) -> bool {
    chars[i..].iter().collect::<String>().starts_with(text)
}

fn read_braced(chars: &[char], mut i: usize) -> Option<(String, usize)> {
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i >= chars.len() || chars[i] != '{' {
        return None;
    }
    let mut depth = 0i32;
    let mut body = String::new();
    while i < chars.len() {
        let ch = chars[i];
        i += 1;
        if ch == '{' {
            depth += 1;
            if depth > 1 {
                body.push(ch);
            }
            continue;
        }
        if ch == '}' {
            depth -= 1;
            if depth == 0 {
                return Some((body, i));
            }
            body.push(ch);
            continue;
        }
        body.push(ch);
    }
    None
}

fn substitute_args(body: &str, args: &[String]) -> String {
    let chars: Vec<char> = body.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
            let n = chars[i + 1].to_digit(10).unwrap_or(0) as usize;
            if n > 0 {
                if let Some(arg) = args.get(n - 1) {
                    out.push_str(arg);
                }
            }
            i += 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn nth_arg(args: SEXP, n: usize) -> SEXP {
    unsafe {
        let mut cell = args;
        for _ in 0..n {
            if cell.is_null() || cell == crate::sexp::globals::R_NilValue() {
                return crate::sexp::globals::R_NilValue();
            }
            cell = CDR(cell);
        }
        if cell.is_null() {
            crate::sexp::globals::R_NilValue()
        } else {
            CAR(cell)
        }
    }
}

fn macros_from_env(env: SEXP) -> Vec<(String, String)> {
    unsafe {
        if env.is_null() || TYPEOF(env) != SEXPTYPE::ENVSXP {
            return Vec::new();
        }
        let mut out = Vec::new();
        collect_macro_frame(crate::sexp::accessors::FRAME(env), &mut out);
        let hash = crate::sexp::accessors::HASHTAB(env);
        if TYPEOF(hash) == SEXPTYPE::VECSXP {
            let n = crate::sexp::accessors::XLENGTH(hash);
            for i in 0..n {
                collect_macro_frame(crate::sexp::accessors::VECTOR_ELT(hash, i), &mut out);
            }
        }
        out
    }
}

fn collect_macro_frame(mut cell: SEXP, out: &mut Vec<(String, String)>) {
    unsafe {
        while !cell.is_null() && cell != crate::sexp::globals::R_NilValue() {
            let tag = crate::sexp::accessors::TAG(cell);
            let val = CAR(cell);
            if TYPEOF(tag) == SEXPTYPE::SYMSXP
                && TYPEOF(val) == SEXPTYPE::STRSXP
                && crate::sexp::accessors::XLENGTH(val) == 1
            {
                let name = std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(
                    crate::sexp::accessors::PRINTNAME(tag),
                ))
                .to_string_lossy()
                .into_owned();
                let body = std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(
                    crate::sexp::accessors::STRING_ELT(val, 0),
                ))
                .to_string_lossy()
                .into_owned();
                out.push((name.trim_start_matches('\\').to_string(), body));
            }
            cell = CDR(cell);
        }
    }
}

fn macro_environment(defined: &[(String, String)], parent: SEXP) -> SEXP {
    unsafe {
        let parent = if TYPEOF(parent) == SEXPTYPE::ENVSXP {
            parent
        } else {
            crate::sexp::globals::R_BaseEnv()
        };
        let env = crate::sexp::envir::R_NewHashedEnv(parent, 0);
        let _guard = protect(env);
        for (name, body) in defined {
            let key = std::ffi::CString::new(format!("\\{name}")).unwrap_or_default();
            let sym = crate::sexp::symbol::Rf_install(key.as_ptr());
            let c_body = std::ffi::CString::new(body.as_str()).unwrap_or_default();
            let value = crate::sexp::constructors::Rf_mkString(c_body.as_ptr());
            crate::sexp::envir::defineVar(sym, value, env);
        }
        env
    }
}

fn rd_text(text: &str, macro_env: SEXP) -> SEXP {
    unsafe {
        let shown = if text.trim().is_empty() { "\n" } else { text };
        let mut line = shown.to_string();
        if !line.ends_with('\n') {
            line.push('\n');
        }
        let elt = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _elt = protect(elt);
        let c = std::ffi::CString::new(line).unwrap_or_default();
        SET_STRING_ELT(elt, 0, crate::sexp::constructors::Rf_mkChar(c.as_ptr()));
        let tag = std::ffi::CString::new("TEXT").unwrap_or_default();
        let tag_sym = crate::sexp::symbol::Rf_install(c"Rd_tag".as_ptr());
        crate::sexp::attrib_core::setAttrib(
            elt,
            tag_sym,
            crate::sexp::constructors::Rf_mkString(tag.as_ptr()),
        );
        let rd = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _rd = protect(rd);
        crate::sexp::accessors::SET_VECTOR_ELT(rd, 0, elt);
        let class_sym = crate::sexp::symbol::Rf_install(c"class".as_ptr());
        let class_name = std::ffi::CString::new("Rd").unwrap_or_default();
        crate::sexp::attrib_core::setAttrib(
            rd,
            class_sym,
            crate::sexp::constructors::Rf_mkString(class_name.as_ptr()),
        );
        if !macro_env.is_null() && macro_env != crate::sexp::globals::R_NilValue() {
            let macros_sym = crate::sexp::symbol::Rf_install(c"macros".as_ptr());
            crate::sexp::attrib_core::setAttrib(rd, macros_sym, macro_env);
        }
        rd
    }
}
