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
        let expanded = expand_user_macros(&text);
        let lines = lines_with_newlines(&expanded);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, lines.len() as i64);
        let _guard = protect(result);
        for (i, line) in lines.iter().enumerate() {
            let c = std::ffi::CString::new(line.as_str()).unwrap_or_default();
            SET_STRING_ELT(
                result,
                i as i64,
                crate::sexp::constructors::Rf_mkChar(c.as_ptr()),
            );
        }
        result
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


fn expand_user_macros(input: &str) -> String {
    let input = strip_rd_comments(input);
    let chars: Vec<char> = input.chars().collect();
    let mut macros: Vec<(String, String)> = Vec::new();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '%' {
            out.push('%');
            i += 2;
            continue;
        }
        if starts_with(&chars, i, "\\newcommand") {
            i += "\\newcommand".chars().count();
            if let Some((name, next)) = read_braced(&chars, i) {
                let name = name.trim_start_matches('\\').to_string();
                i = next;
                if let Some((body, next)) = read_braced(&chars, i) {
                    macros.push((name, body));
                    i = next;
                    continue;
                }
            }
        }
        if chars[i] == '\\' {
            let start = i;
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
                out.push_str(&substitute_args(body, &args));
                continue;
            }
            out.push('\\');
            out.push_str(&name);
            let _ = start;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
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
