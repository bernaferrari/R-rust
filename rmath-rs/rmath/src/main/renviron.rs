#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/Renviron.c
//!
//! Processes .Renviron files to set environment variables.
//! Supports ${FOO-bar} and ${FOO:-bar} substitution syntax.

use std::ffi::CStr;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::main::relop::ScalarLogical;
use crate::main::sysutils::translateChar;
use crate::sexp::accessors::*;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const BUF_SIZE: usize = 100000;
const MSG_SIZE: usize = 2048;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Remove leading and trailing whitespace from a C string (in place).
unsafe fn rmspace(s: *mut c_char) -> *mut c_char {
    unsafe {
        let mut len = libc::strlen(s);
        if len == 0 {
            return s;
        }
        // trim trailing
        while len > 0 && libc::isspace(*s.add(len - 1) as i32) != 0 {
            len -= 1;
            *s.add(len) = 0;
        }
        // trim leading
        let mut i: usize = 0;
        while i < len && libc::isspace(*s.add(i) as i32) != 0 {
            i += 1;
        }
        s.add(i)
    }
}

/// Look for ${FOO-bar} or ${FOO:-bar} constructs, recursively.
/// Returns empty string on error.
unsafe fn subterm(s: *mut c_char) -> *const c_char {
    unsafe {
        static mut ANS: [c_char; BUF_SIZE] = [0; BUF_SIZE];

        let len = libc::strlen(s);
        if len < 3 {
            return s;
        }
        // Check for ${...}
        if *s as u8 != b'$' || *s.add(1) as u8 != b'{' {
            return s;
        }
        if *s.add(len - 1) as u8 != b'}' {
            return s;
        }

        // Remove trailing }
        *s.add(len - 1) = 0;
        let inner = s.add(2); // skip ${
        let trimmed = rmspace(inner);

        let tlen = libc::strlen(trimmed);
        if tlen == 0 {
            return b"\0".as_ptr() as *const c_char;
        }

        // Find '-'
        let mut p: *const c_char = ptr::null();
        for i in 0..tlen {
            if *trimmed.add(i) as u8 == b'-' {
                p = trimmed.add(i);
                break;
            }
        }

        let mut colon = false;
        let q: *const c_char;
        if !p.is_null() {
            q = p.add(1);
            let offset = p as usize - trimmed as usize;
            if offset > 1 && *p.sub(1) as u8 == b':' {
                colon = true;
                *(p.sub(1) as *mut c_char) = 0;
            } else {
                *(p as *mut c_char) = 0;
            }
        } else {
            q = ptr::null();
        }

        let env_val = libc::getenv(trimmed);

        if colon {
            if !env_val.is_null() && libc::strlen(env_val) > 0 {
                return env_val;
            }
        } else {
            if !env_val.is_null() {
                return env_val;
            }
        }

        if !q.is_null() {
            // Need to recurse — copy q to a mutable buffer
            let qlen = libc::strlen(q);
            let mut tmp = [0i8; BUF_SIZE];
            ptr::copy_nonoverlapping(q, tmp.as_mut_ptr(), qlen.min(BUF_SIZE - 1));
            subterm(tmp.as_mut_ptr());
            ptr::copy_nonoverlapping(
                tmp.as_ptr(),
                core::ptr::addr_of_mut!(ANS) as *mut c_char,
                libc::strlen(tmp.as_ptr()),
            );
            return core::ptr::addr_of!(ANS) as *const c_char;
        }

        b"\0".as_ptr() as *const c_char
    }
}

/// Skip along until we find an unmatched right brace.
unsafe fn findRbrace(s: *const c_char) -> *const c_char {
    unsafe {
        let mut p = s;
        let mut nl: c_int = 0;
        let mut nr: c_int = 0;

        while nr <= nl {
            let pl = libc::strchr(p, b'{' as i32);
            let pr = libc::strchr(p, b'}' as i32);
            if pr.is_null() {
                return ptr::null();
            }
            if pl.is_null() || pr < pl {
                p = pr.add(1);
                nr += 1;
            } else {
                p = pl.add(1);
                nl += 1;
            }
        }
        // find the last '}'
        libc::strchr(s, b'}' as i32)
    }
}

/// Find and expand ${...} terms in a string.
unsafe fn findterm(s: *const c_char, buf: *mut c_char, bufsize: usize) {
    unsafe {
        let mut pos = 0;
        let slen = libc::strlen(s);
        let mut i: usize = 0;

        while i < slen {
            if *s.add(i) as u8 == b'$' && i + 1 < slen && *s.add(i + 1) as u8 == b'{' {
                // Found ${, look for matching }
                let rbrace = findRbrace(s.add(i + 2));
                if !rbrace.is_null() {
                    let term_len = rbrace as usize - s.add(i) as usize + 1;
                    // Copy ${...} to temp buffer and expand
                    let mut tmp = [0i8; BUF_SIZE];
                    let copy_len = term_len.min(BUF_SIZE - 1);
                    ptr::copy_nonoverlapping(s.add(i), tmp.as_mut_ptr(), copy_len);
                    tmp[copy_len] = 0;
                    let expanded = subterm(tmp.as_mut_ptr());
                    let elen = libc::strlen(expanded);
                    if pos + elen < bufsize {
                        ptr::copy_nonoverlapping(expanded, buf.add(pos), elen);
                        pos += elen;
                    }
                    i += term_len;
                    continue;
                }
            }
            if pos < bufsize {
                *buf.add(pos) = *s.add(i);
                pos += 1;
            }
            i += 1;
        }
        if pos < bufsize {
            *buf.add(pos) = 0;
        }
    }
}

/// Set an environment variable, processing quotes and escapes.
unsafe fn Putenv(a: *const c_char, b: *const c_char) {
    unsafe {
        let alen = libc::strlen(a);
        let blen = libc::strlen(b);

        // Allocate buffer for the value
        let value_buf = libc::malloc(blen + 1) as *mut c_char;
        if value_buf.is_null() {
            Renviron_error(b"allocation failure in reading Renviron\0".as_ptr() as *const c_char);
            return;
        }

        // Process the value: remove quotes, handle escapes
        let mut inquote = false;
        let mut quote: c_char = 0;
        let mut vi: usize = 0;
        let mut pi: usize = 0;
        while pi < blen {
            let c = *b.add(pi);
            if !inquote && (c == b'"' as c_char || c == b'\'' as c_char) {
                if pi == 0 || *b.add(pi - 1) != b'\\' as c_char {
                    inquote = true;
                    quote = c;
                    pi += 1;
                    continue;
                }
            }
            if inquote && c == quote && *b.add(pi - 1) != b'\\' as c_char {
                inquote = false;
                pi += 1;
                continue;
            }
            if !inquote && c == b'\\' as c_char {
                if pi + 1 < blen {
                    let next = *b.add(pi + 1);
                    if next == b'\n' as c_char {
                        pi += 2;
                        continue;
                    } else if next == b'\\' as c_char {
                        *value_buf.add(vi) = b'\\' as c_char;
                        vi += 1;
                        pi += 2;
                        continue;
                    }
                }
            }
            if inquote && c == b'\\' as c_char && pi + 1 < blen && *b.add(pi + 1) == quote {
                pi += 2;
                continue;
            }
            *value_buf.add(vi) = c;
            vi += 1;
            pi += 1;
        }
        *value_buf.add(vi) = 0;

        libc::setenv(a, value_buf, 1);
        libc::free(value_buf as *mut std::ffi::c_void);
    }
}

unsafe fn Renviron_warning(msg: *const c_char) {
    unsafe {
        let s = CStr::from_ptr(msg);
        eprintln!("{}", s.to_string_lossy());
    }
}

unsafe fn Renviron_error(msg: *const c_char) {
    unsafe {
        let s = CStr::from_ptr(msg);
        eprintln!("FATAL ERROR: {}", s.to_string_lossy());
        std::process::exit(2);
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Process a Renviron file.
/// Returns 1 on success, 0 if file could not be opened.
pub unsafe fn process_Renviron(filename: *const c_char) -> c_int {
    unsafe {
        if filename.is_null() {
            return 0;
        }

        let fname = match CStr::from_ptr(filename).to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => return 0,
        };

        let file = match File::open(&fname) {
            Ok(f) => f,
            Err(_) => return 0,
        };

        let reader = BufReader::new(file);
        let mut errs = false;
        let mut line_buf = [0i8; BUF_SIZE];
        let mut msg_buf = [0i8; MSG_SIZE];

        for line_result in reader.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => continue,
            };

            // Copy to C buffer for processing
            let line_bytes = line.as_bytes();
            let copy_len = line_bytes.len().min(BUF_SIZE - 1);
            ptr::copy_nonoverlapping(
                line_bytes.as_ptr(),
                line_buf.as_mut_ptr() as *mut u8,
                copy_len,
            );
            line_buf[copy_len] = 0;

            let s = rmspace(line_buf.as_mut_ptr());
            let slen = libc::strlen(s);

            if slen == 0 || *s as u8 == b'#' {
                continue;
            }

            // Look for '='
            let mut eq_pos: Option<usize> = None;
            for i in 0..slen {
                if *s.add(i) as u8 == b'=' {
                    eq_pos = Some(i);
                    break;
                }
            }

            if eq_pos.is_none() {
                if !errs {
                    errs = true;
                    let prefix = b"\n   File \0";
                    let suffix = b" contains invalid line(s)\0";
                    let mut pos = 0;
                    let plen = libc::strlen(prefix.as_ptr() as *const c_char);
                    if pos + plen < MSG_SIZE {
                        ptr::copy_nonoverlapping(
                            prefix.as_ptr(),
                            msg_buf.as_mut_ptr() as *mut u8,
                            plen,
                        );
                        pos += plen;
                    }
                    let fnlen = fname.len().min(MSG_SIZE - pos - 1);
                    ptr::copy_nonoverlapping(
                        fname.as_ptr(),
                        msg_buf.as_mut_ptr().add(pos) as *mut u8,
                        fnlen,
                    );
                    pos += fnlen;
                    if pos < MSG_SIZE {
                        msg_buf[pos] = 0;
                    }
                }
                continue;
            }

            let eq = eq_pos.unwrap();
            // Split at '='
            *s.add(eq) = 0; // null-terminate the key
            let lhs = rmspace(s);
            let rhs_ptr = rmspace(s.add(eq + 1));

            // Expand ${...} in rhs
            let mut rhs_expanded = [0i8; BUF_SIZE];
            findterm(rhs_ptr, rhs_expanded.as_mut_ptr(), BUF_SIZE);

            let ll = libc::strlen(lhs);
            let rl = libc::strlen(rhs_expanded.as_ptr());
            if ll > 0 && rl > 0 {
                Putenv(lhs, rhs_expanded.as_ptr());
            }
        }

        if errs {
            Renviron_warning(msg_buf.as_ptr());
        }

        1
    }
}

/// Process system Renviron: R_HOME/etc/Renviron.
pub unsafe fn process_system_Renviron() {
    unsafe {
        // This is a simplified version — the full version reads R_HOME
        // For now, try the common location
        let r_home = std::env::var("R_HOME").unwrap_or_default();
        let path = format!("{}/etc/Renviron", r_home);
        process_Renviron(std::ffi::CString::new(path).unwrap().as_ptr());
    }
}

/// Process site Renviron.
pub unsafe fn process_site_Renviron() {
    unsafe {
        if let Ok(p) = std::env::var("R_ENVIRON") {
            if !p.is_empty() {
                process_Renviron(std::ffi::CString::new(p).unwrap().as_ptr());
                return;
            }
        }
        let r_home = std::env::var("R_HOME").unwrap_or_default();
        let path = format!("{}/etc/Renviron.site", r_home);
        process_Renviron(std::ffi::CString::new(path).unwrap().as_ptr());
    }
}

/// Process user Renviron.
pub unsafe fn process_user_Renviron() {
    unsafe {
        if let Ok(s) = std::env::var("R_ENVIRON_USER") {
            if !s.is_empty() {
                process_Renviron(std::ffi::CString::new(s).unwrap().as_ptr());
                return;
            }
        }
        // Try ./.Renviron
        process_Renviron(b".Renviron\0".as_ptr() as *const c_char);
        // Try ~/.Renviron
        if let Ok(home) = std::env::var("HOME") {
            let path = format!("{}/.Renviron", home);
            process_Renviron(std::ffi::CString::new(path).unwrap().as_ptr());
        }
    }
}

/// R .Internal interface for readEnviron.
pub unsafe fn do_readEnviron(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::STRSXP.0 || LENGTH(x) != 1 {
            // argument must be a character string
            return R_NilValue();
        }
        let elt = STRING_ELT(x, 0);
        if elt.is_null() || elt == R_NilValue() {
            return ScalarLogical(0);
        }
        let fn_c = translateChar(elt);
        let fn_str = CStr::from_ptr(fn_c).to_string_lossy();
        let res = process_Renviron(std::ffi::CString::new(fn_str.as_ref()).unwrap().as_ptr());
        ScalarLogical(if res != 0 { 1 } else { 0 })
    }
}
