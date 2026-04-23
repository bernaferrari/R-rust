#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/Renviron.c — R environment variable handling.
//!
//! Processes .Renviron files to set environment variables, supporting
//! ${VAR-default} and ${VAR:-default} substitution syntax.

use std::env;
use std::ffi::CString;
use std::fs::File;
use std::io::{BufRead, BufReader};

use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;

const BUF_SIZE: usize = 100_000;
const MSG_SIZE: usize = 2048;
const R_PATH_MAX: usize = 4096;

fn renviron_warning(msg: &str) {
    eprintln!("{}", msg);
}

fn renviron_error(msg: &str) -> ! {
    eprintln!("FATAL ERROR: {}", msg);
    std::process::exit(2);
}

fn rmspace(s: &str) -> String {
    let trimmed_end = s.trim_end_matches(|c: char| c.is_ascii_whitespace());
    let trimmed = trimmed_end.trim_start_matches(|c: char| c.is_ascii_whitespace());
    trimmed.to_string()
}

fn subterm(s: &str) -> String {
    if !s.starts_with("${") || !s.ends_with('}') {
        return s.to_string();
    }
    let inner = &s[2..s.len() - 1];
    let inner = rmspace(inner);
    if inner.is_empty() {
        return String::new();
    }

    let (var_name, default) = if let Some(pos) = inner.find(":-") {
        (&inner[..pos], Some(inner[pos + 2..].to_string()))
    } else if let Some(pos) = inner.find('-') {
        (&inner[..pos], Some(inner[pos + 1..].to_string()))
    } else {
        (inner.as_str(), None)
    };

    let env_val = env::var(var_name).ok();

    let has_colon = inner.contains(":-");
    if has_colon {
        if let Some(ref val) = env_val {
            if !val.is_empty() {
                return val.clone();
            }
        }
    } else if let Some(ref val) = env_val {
        return val.clone();
    }

    match default {
        Some(d) => subterm(&d),
        None => String::new(),
    }
}

fn find_rbrace(s: &str) -> Option<usize> {
    let mut nl = 0i32;
    let mut nr = 0i32;
    let bytes = s.as_bytes();
    let mut i = 0;
    while nr <= nl && i < bytes.len() {
        match bytes[i] {
            b'{' => {
                nl += 1;
            }
            }
        }
    }
    None
}

fn findterm(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }

    let mut result = String::new();
    let mut remaining = s;

    loop {
        let dollar_pos = match remaining.find("${") {
            Some(pos) => pos,
            None => break,
        };

        let after_dollar = &remaining[dollar_pos + 2..];
        let brace_offset = match find_rbrace(after_dollar) {
            Some(pos) => pos,
            None => break,
        };

        result.push_str(&remaining[..dollar_pos]);

        let term_str = &remaining[dollar_pos..dollar_pos + 2 + brace_offset + 1];
        let substituted = subterm(term_str);

        if result.len() + substituted.len() < BUF_SIZE {
            result.push_str(&substituted);
        } else {
            return s.to_string();
        }
    }

    if result.len() + remaining.len() < BUF_SIZE {
        result.push_str(remaining);
    } else {
        return s.to_string();
    }
    result
}

fn putenv(a: &str, b: &str) {
    let a_c = match CString::new(a) {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut value = Vec::new();
    let b_bytes = b.as_bytes();
    let mut inquote = false;
    let mut quote: u8 = 0;
    let mut i = 0;

    while i < b_bytes.len() {
        let c = b_bytes[i];
        if !inquote && (c == b'"' || c == b'\'') {
            if i == 0 || b_bytes[i - 1] != b'\\' {
                inquote = true;
                quote = c;
                i += 1;
                continue;
            }
        }
    }

    let value_str = String::from_utf8_lossy(&value).into_owned();
    let a_str = a_c.to_string_lossy().into_owned();

    unsafe {
        env::set_var(&a_str, &value_str);
    }
}

pub fn process_renviron(filename: &str) -> bool {
    let file = match File::open(filename) {
        Ok(f) => f,
        Err(_) => return false,
    };

    let reader = BufReader::new(file);
    let mut errs = false;
    let mut msg = String::new();
    let line_prefix = "\n      ";
    let ignored_msg = "\n   They were ignored\n";
    let truncated_msg = "[... truncated]";
    let too_long = " (too long)";

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.len() >= BUF_SIZE {
            if !errs {
                errs = true;
                msg = format!("\n   File {} contains invalid line(s)", filename);
            }
        }
    }

    if errs {
        if msg.len() + ignored_msg.len() < MSG_SIZE {
            msg.push_str(ignored_msg);
        }
    }
    true
}

pub fn process_system_renviron() {
    let r_home = match env::var("R_HOME") {
        Ok(h) => h,
        Err(_) => {
            renviron_warning("R_HOME is not set");
            return;
        }
    };

    let path = format!("{}/etc/Renviron", r_home);

    let res = process_renviron(&path);
    if !res {
        renviron_warning("cannot find system Renviron");
    }
}

pub fn process_site_renviron() {
    if let Ok(p) = env::var("R_ENVIRON") {
        if !p.is_empty() {
            process_renviron(&p);
        }
    }

    let r_home = match env::var("R_HOME") {
        Ok(h) => h,
        Err(_) => return,
    };

    let path = format!("{}/etc/Renviron.site", r_home);
    process_renviron(&path);
}

fn process_arch_specific_user_renviron(s: &str) {
    process_renviron(s);
}

pub fn process_user_renviron() {
    if let Ok(s) = env::var("R_ENVIRON_USER") {
        if !s.is_empty() {
            let expanded = expand_filename(&s);
            process_renviron(&expanded);
        }
    }

    if process_renviron(".Renviron") {
        return;
    }

    if let Ok(home) = env::var("HOME") {
        let path = format!("{}/.Renviron", home);
        process_arch_specific_user_renviron(&path);
    }
}

fn expand_filename(path: &str) -> String {
    if path.starts_with("~/") {
        if let Ok(home) = env::var("HOME") {
            return format!("{}{}", home, &path[1..]);
        }
    }
    path.to_string()
}

pub  fn do_readEnviron(
    _call: SEXP,
    _op: SEXP,
    args: SEXP,
    _env: SEXP,
) -> SEXP {
    let x = CAR(args);
        if x.is_null() || TYPEOF(x) != SEXPTYPE::STRSXP || LENGTH(x) != 1 {
            r_error("argument 'x' must be a character string");
        }
}
    }
}

fn r_error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    })
}

unsafe fn string_elt(s: SEXP, i: R_xlen_t) -> String {
    unsafe {
        if s.is_null() {
            return String::new();
        }
    }
}
