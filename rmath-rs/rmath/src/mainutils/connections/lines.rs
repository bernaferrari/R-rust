//! Line-oriented I/O: `readLines` / `writeLines` — extracted verbatim from the former single-file module.
#![allow(unused_imports)]
use super::*;
use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::os::raw::{c_double, c_int};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::ptr;

use bzip2::Compression as BzCompression;
use bzip2::read::BzDecoder;
use bzip2::write::BzEncoder;
use flate2::Compression as GzCompression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{NA_INTEGER, NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::instance::{RInstance, with_current_instance, with_required_current_instance};
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// do_readLines — readLines(con, n = -1, ok = TRUE, warn = TRUE, encoding = "", skipNul = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_readLines(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);
        let mut n_val = -1;
        let mut _ok = 1;
        let mut _warn = 1;
        let mut _encoding = R_NilValue();
        let mut _skipNul = 0;
        let mut positional = 0;
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            match connection_arg_tag_name(current).as_deref() {
                Some("n") => n_val = as_integer(arg),
                Some("ok") => _ok = check_logical_arg(arg, "ok"),
                Some("warn") => _warn = check_logical_arg(arg, "warn"),
                Some("encoding") => _encoding = arg,
                Some("skipNul") => _skipNul = check_logical_arg(arg, "skipNul"),
                _ => {
                    match positional {
                        0 => n_val = as_integer(arg),
                        1 => _ok = check_logical_arg(arg, "ok"),
                        2 => _warn = check_logical_arg(arg, "warn"),
                        3 => _encoding = arg,
                        4 => _skipNul = check_logical_arg(arg, "skipNul"),
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let n = if n_val < 0 {
            i64::MAX as usize
        } else {
            n_val as usize
        };
        let skip_nul = _skipNul != 0;

        if TYPEOF(scon) == SEXPTYPE::STRSXP {
            let path = check_string_arg(scon, "con");
            let contents = std::fs::read(&path)
                .unwrap_or_else(|e| r_error(&format!("cannot open file '{}': {}", path, e)));
            let lines = nul_normalized_lines(&contents, n, skip_nul);
            let ans = Rf_allocVector(SEXPTYPE::STRSXP, lines.len() as c_int);
            if !ans.is_null() {
                for (idx, line) in lines.iter().enumerate() {
                    let c_line = CString::new(line.as_str())
                        .unwrap_or_else(|_| CString::new("").unwrap_or_default());
                    let charsxp = Rf_mkChar(c_line.as_ptr());
                    SET_STRING_ELT(ans, idx as R_xlen_t, charsxp);
                }
            }
            return ans;
        }

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;

        let mut table = connection_table();
        let Some(conn) = table[i].as_mut() else {
            r_error("invalid connection");
        };

        if !conn.isopen {
            r_error("connection is not open");
        }
        if !conn.canread {
            r_error("cannot read from this connection");
        }

        conn.incomplete = false;
        let mut lines: Vec<String> = Vec::new();
        while lines.len() < n {
            let Some(line) = read_pushback_line(conn, skip_nul) else {
                break;
            };
            lines.push(line);
        }
        let backend_limit = n.saturating_sub(lines.len());

        match &conn.kind {
            ConnKind::File => {
                if let Some(ref mut reader) = conn.reader {
                    for _ in 0..backend_limit {
                        let mut line = String::new();
                        match reader.read_line(&mut line) {
                            Ok(0) => break, // EOF
                            Ok(_) => {
                                if line.ends_with('\n') {
                                    line.pop();
                                }
                                lines.push(nul_normalized_line(line.into_bytes(), skip_nul));
                            }
                            Err(e) => {
                                r_error(&format!("error reading from connection: {}", e));
                            }
                        }
                    }
                }
            }
            ConnKind::GzFile | ConnKind::BzFile | ConnKind::XzFile => {
                for _ in 0..backend_limit {
                    let Some(line) = read_raw_line(conn, skip_nul) else {
                        break;
                    };
                    lines.push(line);
                }
            }
            ConnKind::Pipe => {
                if let Some(ref mut child) = conn.child
                    && let Some(ref mut stdout) = child.stdout
                {
                    let _reader = BufReader::new(stdout);
                    #[allow(clippy::never_loop)]
                    for _ in 0..backend_limit {
                        break;
                    }
                }
            }
            ConnKind::TextConnection => {
                // Read from text_data buffer
                let data = conn.text_data.clone();
                let pos = conn.text_pos;
                let remaining = &data[pos..];
                for line_str in remaining.split('\n') {
                    if lines.len() >= n {
                        break;
                    }
                    lines.push(nul_normalized_line(line_str.as_bytes().to_vec(), skip_nul));
                }
                if backend_limit == 0 {
                    // Pushback satisfied this read without touching the underlying data.
                } else if lines.len() >= n {
                    // Update position
                    let mut new_pos = pos;
                    for _ in 0..backend_limit {
                        if let Some(idx) = data[new_pos..].find('\n') {
                            new_pos += idx + 1;
                        } else {
                            break;
                        }
                    }
                    conn.text_pos = new_pos;
                } else {
                    conn.text_pos = data.len();
                }
            }
            ConnKind::RawConnection => {
                let remaining = &conn.raw_data[conn.raw_pos..];
                let mut line = Vec::new();
                let mut consumed = 0usize;
                for &byte in remaining {
                    consumed += 1;
                    if byte == b'\n' {
                        lines.push(nul_normalized_line(std::mem::take(&mut line), skip_nul));
                    } else {
                        line.push(byte);
                    }
                    if lines.len() >= n {
                        break;
                    }
                }
                conn.raw_pos += consumed;
                if !line.is_empty() && lines.len() < n {
                    lines.push(nul_normalized_line(line, skip_nul));
                }
            }
            ConnKind::Terminal(name) if name == "stdin" => {
                let stdin = io::stdin();
                let mut reader = stdin.lock();
                for _ in 0..backend_limit {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            if line.ends_with('\n') {
                                line.pop();
                            }
                            lines.push(nul_normalized_line(line.into_bytes(), skip_nul));
                        }
                        Err(e) => {
                            r_error(&format!("error reading from stdin: {}", e));
                        }
                    }
                }
            }
            _ => {
                r_error("cannot read from this connection type");
            }
        }

        // Build result STRSXP
        let nlines = lines.len() as c_int;
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, nlines);
        if !ans.is_null() {
            for (idx, line) in lines.iter().enumerate() {
                let c_line = CString::new(line.as_str())
                    .unwrap_or_else(|_| CString::new("").unwrap_or_default());
                let charsxp = Rf_mkChar(c_line.as_ptr());
                SET_STRING_ELT(ans, idx as R_xlen_t, charsxp);
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_writeLines — writeLines(text, con, sep = "\n", useBytes = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_writeLines(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let text = CAR(args);
        args = CDR(args);
        let scon = CAR(args);
        args = CDR(args);
        let sep = CAR(args);
        args = CDR(args);
        let _useBytes = check_logical_arg(CAR(args), "useBytes");

        if text.is_null() || TYPEOF(text) != SEXPTYPE::STRSXP {
            r_error("invalid 'text' argument");
        }
        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }

        let sep_str = check_string_arg(sep, "sep");
        let text_len = LENGTH(text) as R_xlen_t;

        let i = as_integer(scon) as usize;
        let mut table = connection_table();
        let Some(conn) = table[i].as_mut() else {
            r_error("invalid connection");
        };

        if !conn.isopen {
            r_error("connection is not open");
        }
        if !conn.canwrite {
            r_error("cannot write to this connection");
        }

        match &conn.kind {
            ConnKind::File => {
                if let Some(ref mut writer) = conn.writer {
                    for j in 0..text_len {
                        let line = string_elt(text, j);
                        if let Err(e) = write!(writer, "{}{}", line, sep_str) {
                            r_error(&format!("error writing to connection: {}", e));
                        }
                    }
                    let _ = writer.flush();
                }
            }
            ConnKind::GzFile | ConnKind::BzFile | ConnKind::XzFile => {
                for j in 0..text_len {
                    let line = string_elt(text, j);
                    conn.raw_data.extend_from_slice(line.as_bytes());
                    conn.raw_data.extend_from_slice(sep_str.as_bytes());
                }
                conn.raw_pos = conn.raw_data.len();
            }
            ConnKind::Terminal(name) if name == "stdout" => {
                let stdout = io::stdout();
                let mut writer = stdout.lock();
                for j in 0..text_len {
                    let line = string_elt(text, j);
                    if let Err(e) = write!(writer, "{}{}", line, sep_str) {
                        r_error(&format!("error writing to stdout: {}", e));
                    }
                }
            }
            ConnKind::Terminal(name) if name == "stderr" => {
                let stderr = io::stderr();
                let mut writer = stderr.lock();
                for j in 0..text_len {
                    let line = string_elt(text, j);
                    if let Err(e) = write!(writer, "{}{}", line, sep_str) {
                        r_error(&format!("error writing to stderr: {}", e));
                    }
                }
            }
            ConnKind::TextConnection => {
                let mut lines = conn.text_lines.borrow_mut();
                for j in 0..text_len {
                    let line = string_elt(text, j);
                    lines.push(line);
                }
            }
            ConnKind::Pipe => {
                if let Some(ref mut child) = conn.child
                    && let Some(ref mut stdin) = child.stdin
                {
                    for j in 0..text_len {
                        let line = string_elt(text, j);
                        if let Err(e) = write!(stdin, "{}{}", line, sep_str) {
                            r_error(&format!("error writing to pipe: {}", e));
                        }
                    }
                    let _ = stdin.flush();
                }
            }
            _ => {
                r_error("cannot write to this connection type");
            }
        }

        R_NilValue()
    }
}
