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
use crate::sexp::encoding::{self, TextEncoding};
use crate::sexp::ffi::{NA_INTEGER, NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::instance::{RInstance, with_current_instance, with_required_current_instance};
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// do_readLines — readLines(con, n = -1, ok = TRUE, warn = TRUE, encoding = "", skipNul = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_readLines(call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
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
        let mark_utf8 =
            if _encoding.is_null() || _encoding == R_NilValue() || _encoding == R_MissingArg() {
                false
            } else {
                check_string_arg(_encoding, "encoding") == "UTF-8"
            };
        let skip_nul = _skipNul != 0;

        if TYPEOF(scon) == SEXPTYPE::STRSXP {
            let path = check_string_arg(scon, "con");
            let raw = crate::mainutils::browser_files::read_bytes_or_host(&path)
                .unwrap_or_else(|e| r_error(&format!("cannot open file '{}': {}", path, e)));
            let contents = decompress_known(&raw);
            let lines = nul_normalized_lines(&contents, n, skip_nul);
            let incomplete_line = lines.len() == count_logical_lines(&contents)
                && contents
                    .last()
                    .is_some_and(|byte| !matches!(byte, b'\n' | b'\r'));
            if lines.len() < n && _ok == 0 {
                r_error("too few lines read in readLines");
            }
            let ans = Rf_allocVector(SEXPTYPE::STRSXP, lines.len() as c_int);
            let _ans_root = protect(ans);
            if !ans.is_null() {
                for (idx, line) in lines.iter().enumerate() {
                    let charsxp = read_line_charsxp(line.as_bytes(), mark_utf8);
                    SET_STRING_ELT(ans, idx as R_xlen_t, charsxp);
                }
            }
            if incomplete_line && _warn != 0 {
                let warning = incomplete_line_warning(&path);
                crate::mainutils::errors::Rf_warningcall1(call, warning.as_ptr());
            }
            return ans;
        }

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = checked_connection_index(as_integer(scon));
        let temporarily_open = {
            let table = connection_table();
            table[i].as_ref().is_some_and(|conn| !conn.isopen)
        };
        let mut auto_guard = temporarily_open.then(|| {
            let owner = crate::sexp::owner::OwnerToken::current()
                .unwrap_or_else(|e| r_error(&e.to_string()));
            let mut guard = ConnectionCleanup::new(owner, false);
            guard.attach(i as c_int);
            guard
        });
        if let Some(guard) = &auto_guard {
            auto_open_connection(scon, "r", guard);
        }

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
        let mut incomplete_line = false;
        let mut anonymous_reservations = Vec::new();
        while lines.len() < n {
            let Some(line) = read_pushback_line(conn, skip_nul) else {
                break;
            };
            lines.push(line);
        }
        let backend_limit = n.saturating_sub(lines.len());

        match &mut conn.kind {
            ConnKind::AnonymousFile(file) => {
                for _ in 0..backend_limit {
                    let result = file.read_line().unwrap_or_else(|message| r_error(&message));
                    let Some((bytes, reservation)) = result else {
                        break;
                    };
                    lines.push(nul_normalized_line(bytes, skip_nul));
                    anonymous_reservations.push(reservation);
                }
            }
            ConnKind::File => {
                if conn.mode.contains('+') {
                    if let Some(writer) = conn.writer.as_mut() {
                        let _ = writer.flush();
                    }
                    if let Some(reader) = conn.reader.as_mut() {
                        let _ = reader.seek(SeekFrom::Start(conn.raw_pos as u64));
                    }
                }
                if let Some(reader) = conn.reader.as_mut() {
                    for _ in 0..backend_limit {
                        match read_crlf_line(reader) {
                            Ok(None) => break,
                            Ok(Some((line, incomplete))) => {
                                lines.push(nul_normalized_line(line.into_bytes(), skip_nul));
                                incomplete_line = incomplete;
                            }
                            Err(e) => {
                                r_error(&format!("error reading from connection: {}", e));
                            }
                        }
                    }
                }
            }
            ConnKind::BrowserFile | ConnKind::GzFile | ConnKind::BzFile | ConnKind::XzFile => {
                for _ in 0..backend_limit {
                    let Some(line) = read_raw_line(conn, skip_nul) else {
                        break;
                    };
                    lines.push(line);
                }
            }
            ConnKind::Pipe => {
                if let Some(child) = conn.child.as_mut()
                    && let Some(stdout) = child.stdout.as_mut()
                {
                    let mut reader = BufReader::new(stdout);
                    for _ in 0..backend_limit {
                        let mut line = String::new();
                        match reader.read_line(&mut line) {
                            Ok(0) => break,
                            Ok(_) => {
                                if line.ends_with('\n') {
                                    line.pop();
                                }
                                if line.ends_with('\r') {
                                    line.pop();
                                }
                                lines.push(line);
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
            ConnKind::TextConnection => {
                // Read from text_data buffer
                let data = conn.text_data.clone();
                let pos = conn.text_pos;
                let remaining = data.as_bytes().get(pos..).unwrap_or_default();
                let (read, consumed) =
                    read_nul_normalized_lines(remaining, backend_limit, skip_nul);
                lines.extend(read);
                conn.text_pos = pos + consumed;
            }
            ConnKind::RawConnection => {
                let remaining = &conn.raw_data[conn.raw_pos..];
                let (read, consumed) =
                    read_nul_normalized_lines(remaining, backend_limit, skip_nul);
                lines.extend(read);
                conn.raw_pos += consumed;
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
        conn.incomplete = incomplete_line;
        let incomplete_warning =
            incomplete_line.then(|| incomplete_line_warning(&conn.description));

        drop(table);
        if lines.len() < n && _ok == 0 {
            r_error("too few lines read in readLines");
        }
        let nlines = lines.len() as c_int;
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, nlines);
        let _ans_root = protect(ans);
        if !ans.is_null() {
            for (idx, line) in lines.iter().enumerate() {
                let charsxp = read_line_charsxp(line.as_bytes(), mark_utf8);
                SET_STRING_ELT(ans, idx as R_xlen_t, charsxp);
            }
        }

        if let Some(guard) = auto_guard.as_mut() {
            guard.close();
        }
        if _warn != 0
            && let Some(warning) = incomplete_warning
        {
            crate::mainutils::errors::Rf_warningcall1(call, warning.as_ptr());
        }
        ans
    }
}

unsafe fn read_line_charsxp(bytes: &[u8], mark_utf8: bool) -> SEXP {
    let encoding = if mark_utf8 {
        TextEncoding::Utf8
    } else {
        TextEncoding::Native
    };
    // SAFETY: this helper is called only from the active readLines native
    // operation, whose current runtime owns the allocation arena.
    unsafe {
        crate::sexp::memory::with_arena(|arena| {
            encoding::allocate_character(arena, bytes, encoding)
        })
    }
    .unwrap_or_else(|| crate::sexp::context::r_error("cannot allocate character string"))
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

        let i = checked_connection_index(as_integer(scon));
        let temporarily_open = {
            let table = connection_table();
            table[i].as_ref().is_some_and(|conn| !conn.isopen)
        };
        let mut auto_guard = temporarily_open.then(|| {
            let owner = crate::sexp::owner::OwnerToken::current()
                .unwrap_or_else(|e| r_error(&e.to_string()));
            let mut guard = ConnectionCleanup::new(owner, false);
            guard.attach(i as c_int);
            guard
        });
        if let Some(guard) = &auto_guard {
            auto_open_connection(scon, "w", guard);
        }
        {
            let table = connection_table();
            let Some(conn) = table[i].as_ref() else {
                r_error("invalid connection");
            };
            if !conn.isopen {
                r_error("connection is not open");
            }
            if !conn.canwrite {
                r_error("cannot write to this connection");
            }
            if matches!(&conn.kind, ConnKind::Terminal(name) if name == "stdout") {
                drop(table);
                // GNU writeLines(stdout()) follows the sink stack (Rd2txt
                // uses this under sink(textConnection) in .Rd_get_text).
                for j in 0..text_len {
                    let line = string_elt(text, j);
                    crate::sexp::output::capture_stdout(&format!("{line}{sep_str}"));
                }
                if let Some(guard) = auto_guard.as_mut() {
                    guard.close();
                }
                return R_NilValue();
            }
        }
        let anonymous = {
            let table = connection_table();
            table[i]
                .as_ref()
                .is_some_and(|conn| matches!(conn.kind, ConnKind::AnonymousFile(_)))
        };
        if anonymous {
            let token = connection_token(i as c_int).unwrap_or_else(|message| r_error(&message));
            let owner = crate::sexp::owner::OwnerToken::current()
                .unwrap_or_else(|error| r_error(&error.to_string()));
            let text = owner
                .sexp(text)
                .and_then(crate::sexp::Sexp::into_owned)
                .unwrap_or_else(|error| r_error(&error.to_string()));
            let (chunks, _reservations) = super::file::anonymous::text_chunks(&text, &sep_str)
                .unwrap_or_else(|message| r_error(&message));
            for chunk in chunks {
                let index = token
                    .require_current()
                    .unwrap_or_else(|message| r_error(&message));
                connection_write_bytes(index, &chunk);
            }
            if let Some(guard) = auto_guard.as_mut() {
                guard.close();
            }
            return R_NilValue();
        }
        let text_output = {
            let table = connection_table();
            table[i]
                .as_ref()
                .is_some_and(|conn| matches!(conn.kind, ConnKind::TextConnection))
        };
        if text_output {
            let token = connection_token(i as c_int).unwrap_or_else(|e| r_error(&e));
            let owner = crate::sexp::owner::OwnerToken::current()
                .unwrap_or_else(|e| r_error(&e.to_string()));
            let _pin = owner.pin().unwrap_or_else(|e| r_error(&e.to_string()));
            let text = owner
                .sexp(text)
                .and_then(crate::sexp::Sexp::into_owned)
                .unwrap_or_else(|e| r_error(&e.to_string()));
            for j in 0..text_len {
                let mut chunk = string_elt(text.as_raw(), j);
                owner
                    .require_active()
                    .unwrap_or_else(|e| r_error(&e.to_string()));
                let index = token.require_current().unwrap_or_else(|e| r_error(&e));
                chunk.push_str(&sep_str);
                connection_write_bytes(index, chunk.as_bytes());
            }
            if let Some(guard) = auto_guard.as_mut() {
                guard.close();
            }
            return R_NilValue();
        }
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
            ConnKind::BrowserFile => {
                for j in 0..text_len {
                    let line = string_elt(text, j);
                    let mut bytes = line.into_bytes();
                    bytes.extend_from_slice(sep_str.as_bytes());
                    write_bytes_to_conn(conn, &bytes);
                }
            }
            ConnKind::File => {
                if let Some(ref mut writer) = conn.writer {
                    for j in 0..text_len {
                        let line = string_elt(text, j);
                        if let Err(e) = write!(writer, "{}{}", line, sep_str) {
                            r_error(&format!("error writing to connection: {}", e));
                        }
                    }
                    let _ = writer.flush();
                    if conn.mode.contains('+')
                        && let Some(reader) = conn.reader.as_mut()
                    {
                        let _ = reader.seek(SeekFrom::Start(0));
                        conn.raw_pos = 0;
                    }
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
                unreachable!("stdout terminal writes take the capture path above");
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
            ConnKind::TextConnection => {}
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

        drop(table);
        if let Some(guard) = auto_guard.as_mut() {
            guard.close();
        }
        R_NilValue()
    }
}

unsafe fn auto_open_connection(con: SEXP, mode: &str, guard: &ConnectionCleanup) {
    unsafe {
        let mode = Rf_mkString(CString::new(mode).unwrap_or_default().as_ptr());
        let _mode_root = protect(mode);
        let inner = Rf_cons(mode, R_NilValue());
        let _inner_root = protect(inner);
        let call_args = Rf_cons(con, inner);
        let _args_root = protect(call_args);
        guard.check();
        super::file::do_open(ptr::null_mut(), ptr::null_mut(), call_args, R_NilValue());
        guard.check();
    }
}

pub(super) fn read_crlf_line<R: BufRead>(reader: &mut R) -> io::Result<Option<(String, bool)>> {
    let mut line = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        match reader.read(&mut byte)? {
            0 => {
                if line.is_empty() {
                    return Ok(None);
                }
                return Ok(Some((String::from_utf8_lossy(&line).into_owned(), true)));
            }
            _ => {
                if byte[0] == b'\n' {
                    return Ok(Some((String::from_utf8_lossy(&line).into_owned(), false)));
                }
                if byte[0] == b'\r' {
                    if let Some(&b'\n') = reader.fill_buf()?.first() {
                        reader.consume(1);
                    }
                    return Ok(Some((String::from_utf8_lossy(&line).into_owned(), false)));
                }
                line.push(byte[0]);
            }
        }
    }
}

fn count_logical_lines(bytes: &[u8]) -> usize {
    let mut count = 0;
    let mut offset = 0;
    while offset < bytes.len() {
        count += 1;
        let (end, after) = super::state::line_terminator(&bytes[offset..])
            .map(|(end, after)| (offset + end, offset + after))
            .unwrap_or((bytes.len(), bytes.len()));
        offset = if end == bytes.len() {
            bytes.len()
        } else {
            after
        };
    }
    count
}

fn incomplete_line_warning(description: &str) -> CString {
    CString::new(format!(
        "incomplete final line found on '{}'",
        description.replace('\\', "\\\\").replace('\'', "\\'")
    ))
    .unwrap_or_default()
}

fn decompress_known(raw: &[u8]) -> Vec<u8> {
    use std::io::Read;
    if raw.len() >= 2 && raw[0] == 0x1f && raw[1] == 0x8b {
        let mut out = Vec::new();
        if GzDecoder::new(raw).read_to_end(&mut out).is_ok() {
            return out;
        }
    }
    if raw.len() >= 3 && raw.starts_with(b"BZh") {
        let mut out = Vec::new();
        if BzDecoder::new(raw).read_to_end(&mut out).is_ok() {
            return out;
        }
    }
    if raw.len() >= 6 && raw.starts_with(&[0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00]) {
        let mut input = std::io::Cursor::new(raw);
        let mut out = Vec::new();
        if lzma_rs::xz_decompress(&mut input, &mut out).is_ok() {
            return out;
        }
    }
    raw.to_vec()
}

#[cfg(test)]
mod cleanup_identity_tests {
    use super::*;
    use crate::sexp::{
        RSession, Sexp,
        object::{PairlistBuilder, SessionNodeFactory},
    };
    unsafe fn file<'s>(f: &SessionNodeFactory<'s>, name: &str, mode: &str) -> Sexp<'s> {
        let mut args = PairlistBuilder::from_factory(f.clone());
        for value in [f.strings(&[name]).unwrap(), f.strings(&[mode]).unwrap()] {
            args.push(value, None).unwrap();
        }
        let args = args.finish().unwrap();
        unsafe {
            f.wrap(do_file(
                ptr::null_mut(),
                ptr::null_mut(),
                args.as_raw(),
                ptr::null_mut(),
            ))
            .unwrap()
        }
    }
    fn cleanup_identity(action: u8, remove: bool) {
        let mut session = RSession::new_for_gc_tests();
        session.enable_browser_files();
        session.with_active_in(|instance| unsafe {
            (*instance).browser_files.set_test_clock(0.0);
        });
        session.with_active(|| unsafe {
            let f = session.owner_token().unwrap().node_factory();
            let original = file(&f, "cleanup-original.txt", "");
            let index = original.try_integer_elt(0).unwrap();
            let mut guard = ConnectionCleanup::new(session.owner_token().unwrap(), remove);
            guard.attach(index);
            auto_open_connection(original.as_raw(), "w", &guard);
            let mut replacement_token = None;
            if action != 0 {
                let args = f.pairlist_cell(&original, &f.nil(), &f.nil()).unwrap();
                do_close(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    args.as_raw(),
                    ptr::null_mut(),
                );
                if action == 1 {
                    let fresh = file(&f, "cleanup-replacement.txt", "w");
                    assert_eq!(
                        fresh.try_integer_elt(0).unwrap(),
                        index,
                        "actual constructor must reuse the slot"
                    );
                    replacement_token = Some(connection_token(index).unwrap());
                }
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| guard.close()));
            if action == 0 {
                assert!(result.is_ok());
                let table = connection_table();
                if remove {
                    assert!(table[index as usize].is_none());
                } else {
                    assert!(!table[index as usize].as_ref().unwrap().isopen);
                }
            } else {
                assert!(
                    result.is_err(),
                    "cleanup must authenticate its original allocation"
                );
                if let Some(token) = replacement_token {
                    assert_eq!(token.require_current().unwrap(), index);
                    let table = connection_table();
                    let replacement = table[index as usize].as_ref().unwrap();
                    assert!(
                        replacement.isopen,
                        "a replacement must remain writable after stale cleanup"
                    );
                    assert_eq!(replacement.description, "cleanup-replacement.txt");
                } else {
                    assert!(connection_table()[index as usize].is_none());
                }
            }
        });
    }
    #[test]
    fn cleanup_identity_original_open_file_control() {
        cleanup_identity(0, false);
    }
    #[test]
    fn cleanup_identity_real_same_slot_replacement_cannot_be_closed() {
        cleanup_identity(1, false);
    }
    #[test]
    fn cleanup_identity_retired_original_is_checked() {
        cleanup_identity(2, false);
    }
    #[test]
    fn cleanup_identity_owned_capture_original_control_removes_only_its_body() {
        cleanup_identity(0, true);
    }
    #[test]
    fn cleanup_identity_owned_capture_same_slot_replacement_is_preserved() {
        cleanup_identity(1, true);
    }
}
