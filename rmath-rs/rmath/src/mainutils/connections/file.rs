//! File/url connections: `file`, `url`, `fifo`, `gzfile`/`bzfile`/`xzfile`, open/close/seek/flush, `is*` predicates — extracted verbatim from the former single-file module.
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
// do_file — file(description, open = "", mode = "r", raw = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_file(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let empty = Rf_mkString(c"".as_ptr());
        let native = Rf_mkString(c"native.enc".as_ptr());
        let default_method = Rf_mkString(c"default".as_ptr());
        let scmd = positional_or(args, 0, empty);
        let sopen = positional_or(args, 1, empty);
        let _enc = positional_or(args, 2, native);
        let _block = positional_or(args, 3, Rf_ScalarLogical(crate::sexp::ffi::TRUE));
        let _method = positional_or(args, 4, default_method);
        let raw = check_logical_arg(
            positional_or(args, 5, Rf_ScalarLogical(crate::sexp::ffi::FALSE)),
            "raw",
        );

        let description = check_string_arg(scmd, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        let ncon = next_connection();
        let mut conn = RConn::new("file", &description, &open_mode, ConnKind::File);
        conn.canseek = raw == 0;
        conn.text = !open_mode.contains('b');

        // Open immediately if open mode is non-empty
        if !open_mode.is_empty() {
            let file_result = open_file_conn(&description, &open_mode);
            match file_result {
                Ok((file, reader, writer)) => {
                    conn.file = Some(file);
                    conn.reader = reader;
                    conn.writer = writer;
                    conn.isopen = true;
                    conn.canread = open_mode.starts_with('r');
                    conn.canwrite = open_mode.starts_with('w') || open_mode.starts_with('a');
                    if open_mode.contains('+') {
                        conn.canread = true;
                        conn.canwrite = true;
                    }
                }
                Err(e) => {
                    r_error(&format!("cannot open file '{}': {}", description, e));
                }
            }
        }

        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "file");
        ans
    }
}

/// Open a file and return (file, optional reader, optional writer).
#[allow(clippy::type_complexity)]
pub fn open_file_conn(
    path: &str,
    mode: &str,
) -> io::Result<(File, Option<BufReader<File>>, Option<BufWriter<File>>)> {
    let mut opts = OpenOptions::new();
    if mode.contains('r') {
        opts.read(true);
    }
    if mode.contains('w') {
        opts.write(true).create(true).truncate(true);
    }
    if mode.contains('a') {
        opts.append(true);
    }
    let file = opts.open(path)?;

    let reader = if mode.contains('r') {
        Some(BufReader::new(file.try_clone()?))
    } else {
        None
    };

    let writer = if mode.contains('w') || mode.contains('a') {
        Some(BufWriter::new(file.try_clone()?))
    } else {
        None
    };

    Ok((file, reader, writer))
}

pub fn open_gz_conn(conn: &mut RConn, mode: &str) -> io::Result<()> {
    conn.mode = mode.to_string();
    conn.text = !mode.contains('b');
    conn.raw_data.clear();
    conn.raw_pos = 0;
    conn.file = None;
    conn.reader = None;
    conn.writer = None;

    let can_read = mode.starts_with('r') || mode.contains('+');
    let can_write = mode.starts_with('w') || mode.starts_with('a') || mode.contains('+');

    if mode.starts_with('r') || mode.starts_with('a') || mode.contains('+') {
        let path = Path::new(&conn.description);
        if path.exists() {
            let file = File::open(path)?;
            let mut decoder = GzDecoder::new(file);
            decoder.read_to_end(&mut conn.raw_data)?;
        } else if mode.starts_with('r') {
            return Err(io::Error::new(io::ErrorKind::NotFound, "file not found"));
        }
    }

    if mode.starts_with('w') && !mode.starts_with("w+") {
        conn.raw_data.clear();
    }

    conn.raw_pos = if mode.starts_with('a') {
        conn.raw_data.len()
    } else {
        0
    };
    conn.isopen = true;
    conn.canread = can_read;
    conn.canwrite = can_write;
    Ok(())
}

pub fn flush_gz_conn(conn: &mut RConn) -> io::Result<()> {
    let file = File::create(&conn.description)?;
    let mut encoder = GzEncoder::new(file, GzCompression::default());
    encoder.write_all(&conn.raw_data)?;
    encoder.finish()?;
    Ok(())
}

pub fn open_bz_conn(conn: &mut RConn, mode: &str) -> io::Result<()> {
    conn.mode = mode.to_string();
    conn.text = !mode.contains('b');
    conn.raw_data.clear();
    conn.raw_pos = 0;
    conn.file = None;
    conn.reader = None;
    conn.writer = None;

    let can_read = mode.starts_with('r') || mode.contains('+');
    let can_write = mode.starts_with('w') || mode.starts_with('a') || mode.contains('+');

    if mode.starts_with('r') || mode.starts_with('a') || mode.contains('+') {
        let path = Path::new(&conn.description);
        if path.exists() {
            let file = File::open(path)?;
            let mut decoder = BzDecoder::new(file);
            decoder.read_to_end(&mut conn.raw_data)?;
        } else if mode.starts_with('r') {
            return Err(io::Error::new(io::ErrorKind::NotFound, "file not found"));
        }
    }

    if mode.starts_with('w') && !mode.starts_with("w+") {
        conn.raw_data.clear();
    }

    conn.raw_pos = if mode.starts_with('a') {
        conn.raw_data.len()
    } else {
        0
    };
    conn.isopen = true;
    conn.canread = can_read;
    conn.canwrite = can_write;
    Ok(())
}

pub fn flush_bz_conn(conn: &mut RConn) -> io::Result<()> {
    let file = File::create(&conn.description)?;
    let mut encoder = BzEncoder::new(file, BzCompression::default());
    encoder.write_all(&conn.raw_data)?;
    encoder.finish()?;
    Ok(())
}

pub fn open_xz_conn(conn: &mut RConn, mode: &str) -> io::Result<()> {
    conn.mode = mode.to_string();
    conn.text = !mode.contains('b');
    conn.raw_data.clear();
    conn.raw_pos = 0;
    conn.file = None;
    conn.reader = None;
    conn.writer = None;

    let can_read = mode.starts_with('r') || mode.contains('+');
    let can_write = mode.starts_with('w') || mode.starts_with('a') || mode.contains('+');

    if mode.starts_with('r') || mode.starts_with('a') || mode.contains('+') {
        let path = Path::new(&conn.description);
        if path.exists() {
            let file = File::open(path)?;
            let mut reader = BufReader::new(file);
            lzma_rs::xz_decompress(&mut reader, &mut conn.raw_data)
                .map_err(|err| io::Error::other(err.to_string()))?;
        } else if mode.starts_with('r') {
            return Err(io::Error::new(io::ErrorKind::NotFound, "file not found"));
        }
    }

    if mode.starts_with('w') && !mode.starts_with("w+") {
        conn.raw_data.clear();
    }

    conn.raw_pos = if mode.starts_with('a') {
        conn.raw_data.len()
    } else {
        0
    };
    conn.isopen = true;
    conn.canread = can_read;
    conn.canwrite = can_write;
    Ok(())
}

pub fn flush_xz_conn(conn: &mut RConn) -> io::Result<()> {
    let input = io::Cursor::new(&conn.raw_data);
    let mut reader = BufReader::new(input);
    let mut output = File::create(&conn.description)?;
    lzma_rs::xz_compress(&mut reader, &mut output)
}
// ---------------------------------------------------------------------------
// do_url — url(description, open = "", mode = "r", blocking = TRUE, encoding = "", method = "default", headers = NULL)
// ---------------------------------------------------------------------------

pub unsafe fn do_url(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let empty = Rf_mkString(c"".as_ptr());
        let native = Rf_mkString(c"native.enc".as_ptr());
        let default_method = Rf_mkString(c"default".as_ptr());
        let scmd = arg_by_name_or_position(args, 0, &["description"], R_NilValue());
        let sopen = arg_by_name_or_position(args, 1, &["open"], empty);
        let _block = logical_arg_or(
            arg_by_name_or_position(args, 2, &["blocking"], Rf_ScalarLogical(1)),
            "blocking",
            1,
        );
        let _enc = arg_by_name_or_position(args, 3, &["encoding"], native);
        let _method = arg_by_name_or_position(args, 4, &["method"], default_method);
        let _headers = arg_by_name_or_position(args, 5, &["headers"], R_NilValue());

        let description = check_string_arg(scmd, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        // Check if it's a file:// URL or a regular path
        let (actual_desc, conn_class) = if let Some(path) = description.strip_prefix("file://") {
            (path.to_string(), "file".to_string())
        } else if description.starts_with("http://")
            || description.starts_with("https://")
            || description.starts_with("ftp://")
            || description.starts_with("ftps://")
        {
            r_error(
                "remote URL connections are disabled in this pure-R Android runtime; fetch bytes through the host network policy and pass a file, rawConnection, or textConnection",
            );
        } else {
            r_error("URL scheme unsupported by this method");
        };

        let ncon = next_connection();
        let mut conn = if conn_class == "url" {
            RConn::new("url", &actual_desc, &open_mode, ConnKind::Url)
        } else {
            RConn::new("file", &actual_desc, &open_mode, ConnKind::File)
        };
        conn.canseek = conn_class == "file";
        conn.text = !open_mode.contains('b');

        if conn_class == "file" && !open_mode.is_empty() {
            let file_result = open_file_conn(&actual_desc, &open_mode);
            match file_result {
                Ok((file, reader, writer)) => {
                    conn.file = Some(file);
                    conn.reader = reader;
                    conn.writer = writer;
                    conn.isopen = true;
                    conn.canread = open_mode.starts_with('r');
                    conn.canwrite = open_mode.starts_with('w') || open_mode.starts_with('a');
                    if open_mode.contains('+') {
                        conn.canread = true;
                        conn.canwrite = true;
                    }
                }
                Err(e) => {
                    r_error(&format!("cannot open file '{}': {}", actual_desc, e));
                }
            }
        }

        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, &conn_class);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_fifo — fifo(description, open, mode)
// ---------------------------------------------------------------------------

pub unsafe fn do_fifo(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scmd = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);
        args = CDR(args);
        let _enc = CAR(args);

        let description = check_string_arg(scmd, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        let ncon = next_connection();
        let mut conn = RConn::new("fifo", &description, &open_mode, ConnKind::Fifo);
        conn.canseek = false;

        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "fifo");
        ans
    }
}

// ---------------------------------------------------------------------------
// do_gzfile — gzfile(description, open = "", compression = 6)
// ---------------------------------------------------------------------------

pub unsafe fn do_gzfile(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scmd = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);
        args = CDR(args);
        let _compression = CAR(args);

        let description = check_string_arg(scmd, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        let mut conn = RConn::new("gzfile", &description, &open_mode, ConnKind::GzFile);
        conn.canseek = false;
        conn.text = !open_mode.contains('b');

        if !open_mode.is_empty() {
            if let Err(e) = open_gz_conn(&mut conn, &open_mode) {
                r_error(&format!("cannot open file '{}': {}", description, e));
            }
        }

        let ncon = next_connection();
        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "gzfile");
        ans
    }
}

// ---------------------------------------------------------------------------
// do_bzfile — bzfile(description, open, compression)
// ---------------------------------------------------------------------------

pub unsafe fn do_bzfile(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scmd = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);
        args = CDR(args);
        let _compression = CAR(args);

        let description = check_string_arg(scmd, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        let mut conn = RConn::new("bzfile", &description, &open_mode, ConnKind::BzFile);
        conn.canseek = false;
        conn.text = !open_mode.contains('b');

        if !open_mode.is_empty() {
            if let Err(e) = open_bz_conn(&mut conn, &open_mode) {
                r_error(&format!("cannot open file '{}': {}", description, e));
            }
        }

        let ncon = next_connection();
        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "bzfile");
        ans
    }
}

// ---------------------------------------------------------------------------
// do_xzfile — xzfile(description, open, compression)
// ---------------------------------------------------------------------------

pub unsafe fn do_xzfile(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scmd = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);
        args = CDR(args);
        let _compression = CAR(args);

        let description = check_string_arg(scmd, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        let mut conn = RConn::new("xzfile", &description, &open_mode, ConnKind::XzFile);
        conn.canseek = false;
        conn.text = !open_mode.contains('b');

        if !open_mode.is_empty() {
            if let Err(e) = open_xz_conn(&mut conn, &open_mode) {
                r_error(&format!("cannot open file '{}': {}", description, e));
            }
        }

        let ncon = next_connection();
        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "xzfile");
        ans
    }
}

// ---------------------------------------------------------------------------
// do_open — open(con, open = "", blocking = TRUE)
// ---------------------------------------------------------------------------

#[allow(clippy::assigning_clones)]
pub unsafe fn do_open(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);
        args = CDR(args);
        let _block = check_logical_arg(CAR(args), "blocking");

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        if i < 3 {
            r_error("cannot open standard connections");
        }

        let open_str = check_string_arg(sopen, "open");
        let open_mode = if open_str.is_empty() {
            "r".to_string()
        } else {
            open_str
        };

        let mut table = connection_table();
        let Some(conn) = table[i].as_mut() else {
            r_error("invalid connection");
        };
        if conn.isopen {
            return R_NilValue();
        }

        conn.mode = open_mode.clone();
        conn.text = !open_mode.contains('b');

        match &conn.kind {
            ConnKind::File => {
                let file_result = open_file_conn(&conn.description, &open_mode);
                match file_result {
                    Ok((file, reader, writer)) => {
                        conn.file = Some(file);
                        conn.reader = reader;
                        conn.writer = writer;
                        conn.isopen = true;
                        conn.canread = open_mode.starts_with('r');
                        conn.canwrite = open_mode.starts_with('w') || open_mode.starts_with('a');
                        if open_mode.contains('+') {
                            conn.canread = true;
                            conn.canwrite = true;
                        }
                    }
                    Err(e) => {
                        r_error(&format!("cannot open the connection: {}", e));
                    }
                }
            }
            ConnKind::GzFile => {
                if let Err(e) = open_gz_conn(conn, &open_mode) {
                    r_error(&format!("cannot open the connection: {}", e));
                }
            }
            ConnKind::BzFile => {
                if let Err(e) = open_bz_conn(conn, &open_mode) {
                    r_error(&format!("cannot open the connection: {}", e));
                }
            }
            ConnKind::XzFile => {
                if let Err(e) = open_xz_conn(conn, &open_mode) {
                    r_error(&format!("cannot open the connection: {}", e));
                }
            }
            ConnKind::Pipe => {
                let is_read = open_mode.starts_with('r');
                let mut cmd = Command::new("sh");
                cmd.arg("-c").arg(&conn.description);
                if is_read {
                    cmd.stdout(Stdio::piped());
                    cmd.stderr(Stdio::null());
                } else {
                    cmd.stdin(Stdio::piped());
                    cmd.stderr(Stdio::null());
                }
                match cmd.spawn() {
                    Ok(child) => {
                        conn.child = Some(child);
                        conn.isopen = true;
                        conn.canread = is_read;
                        conn.canwrite = !is_read;
                    }
                    Err(e) => {
                        r_error(&format!("cannot open pipe: {}", e));
                    }
                }
            }
            ConnKind::TextConnection => {
                conn.isopen = true;
            }
            ConnKind::RawConnection => {
                conn.isopen = true;
            }
            _ => {
                r_error("cannot open this type of connection");
            }
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_close — close(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_close(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        if i < 3 {
            r_error("cannot close standard connections");
        }

        // Check if it's a sink connection
        {
            let sink = sink_state();
            for j in 0..sink.sink_number {
                if i as c_int == sink.sink_cons[j] {
                    r_error("cannot close 'output' sink connection");
                }
            }
            if i as c_int == sink.error_con {
                r_error("cannot close 'message' sink connection");
            }
        }

        let mut table = connection_table();
        if let Some(ref mut conn) = table[i] {
            close_connection_inner(conn);
        }
        table[i] = None;

        R_NilValue()
    }
}

/// Internal close logic for a connection.
pub fn close_connection_inner(conn: &mut RConn) {
    if !conn.isopen {
        return;
    }

    if conn.canwrite {
        match conn.kind {
            ConnKind::GzFile => {
                let _ = flush_gz_conn(conn);
            }
            ConnKind::BzFile => {
                let _ = flush_bz_conn(conn);
            }
            ConnKind::XzFile => {
                let _ = flush_xz_conn(conn);
            }
            _ => {}
        }
    }

    conn.status = 0; // success
    conn.isopen = false;

    // Close file handles
    if let Some(mut writer) = conn.writer.take() {
        let _ = writer.flush();
    }
    conn.reader.take();
    conn.file.take();

    // Close child processes
    if let Some(mut child) = conn.child.take() {
        // Try to close stdin if writing to pipe
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.flush();
        }
        // Wait for child to finish
        match child.wait() {
            Ok(status) => {
                conn.status = status.code().unwrap_or(0);
            }
            Err(_) => {
                conn.status = -1;
            }
        }
    }

    conn.pushback.clear();
}

// ---------------------------------------------------------------------------
// do_isopen — isOpen(con, rw = "")
// ---------------------------------------------------------------------------

pub unsafe fn do_isopen(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = arg_by_name_or_position(args, 0, &["con"], R_NilValue());
        let rw = connection_rw_mode(arg_by_name_or_position(args, 1, &["rw"], R_NilValue()));

        let i = as_integer(scon) as usize;
        init_connections_table();
        let table = connection_table();
        if i >= table.len() || table[i].is_none() {
            r_error("invalid connection");
        }
        let Some(conn) = table[i].as_ref() else {
            r_error("invalid connection");
        };
        let mut res = if conn.isopen { 1 } else { 0 };
        match rw {
            1 => {
                if !conn.canread {
                    res = 0;
                }
            }
            2 => {
                if !conn.canwrite {
                    res = 0;
                }
            }
            _ => {} // intentionally unhandled: unknown connection open mode
        }
        Rf_ScalarLogical(res)
    }
}

// ---------------------------------------------------------------------------
// do_isincomplete — isIncomplete(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_isincomplete(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        let table = connection_table();
        if i >= table.len() || table[i].is_none() {
            return Rf_ScalarLogical(0);
        }
        let Some(conn) = table[i].as_ref() else {
            return Rf_ScalarLogical(0);
        };
        Rf_ScalarLogical(if conn.incomplete { 1 } else { 0 })
    }
}

// ---------------------------------------------------------------------------
// do_isseekable — isSeekable(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_isseekable(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        let table = connection_table();
        if i >= table.len() || table[i].is_none() {
            return Rf_ScalarLogical(0);
        }
        let Some(conn) = table[i].as_ref() else {
            return Rf_ScalarLogical(0);
        };
        Rf_ScalarLogical(if conn.canseek { 1 } else { 0 })
    }
}

// ---------------------------------------------------------------------------
// do_isatty — isatty(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_isatty(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { Rf_ScalarLogical(0) }
}
// ---------------------------------------------------------------------------
// do_seek — seek(con, where = NA, origin = "start", rw = "")
// ---------------------------------------------------------------------------

pub unsafe fn do_seek(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);
        args = CDR(args);
        let where_val = as_real(CAR(args));
        args = CDR(args);
        let origin = as_integer(CAR(args));
        args = CDR(args);
        let rw = as_integer(CAR(args));

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

        match &mut conn.kind {
            ConnKind::File => {
                if let Some(ref mut file) = conn.file {
                    let seek_from = match origin {
                        1 => SeekFrom::Current(where_val as i64),
                        3 => SeekFrom::End(where_val as i64),
                        _ => SeekFrom::Start(where_val as u64),
                    };

                    let old_pos = match file.stream_position() {
                        Ok(p) => p as c_double,
                        Err(_) => 0.0,
                    };

                    if !where_val.is_nan() {
                        match file.seek(seek_from) {
                            Ok(_) => {}
                            Err(e) => {
                                r_error(&format!("seek failed: {}", e));
                            }
                        }
                    }

                    return Rf_ScalarReal(old_pos);
                }
            }
            ConnKind::RawConnection => {
                let old_pos = conn.raw_pos as c_double;
                if !where_val.is_nan() {
                    let new_pos = match origin {
                        1 => (conn.raw_pos as i64 + where_val as i64) as usize,
                        3 => (conn.raw_data.len() as i64 + where_val as i64) as usize,
                        _ => where_val as usize,
                    };
                    conn.raw_pos = new_pos.min(conn.raw_data.len());
                }
                return Rf_ScalarReal(old_pos);
            }
            ConnKind::TextConnection => {
                let old_pos = conn.text_pos as c_double;
                if !where_val.is_nan() {
                    let new_pos = match origin {
                        1 => (conn.text_pos as i64 + where_val as i64) as usize,
                        3 => (conn.text_data.len() as i64 + where_val as i64) as usize,
                        _ => where_val as usize,
                    };
                    conn.text_pos = new_pos.min(conn.text_data.len());
                }
                return Rf_ScalarReal(old_pos);
            }
            _ => {
                r_error("seek not supported for this connection type");
            }
        }

        Rf_ScalarReal(0.0)
    }
}

// ---------------------------------------------------------------------------
// do_flush — flush(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_flush(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        let mut table = connection_table();
        let Some(conn) = table[i].as_mut() else {
            r_error("invalid connection");
        };

        if conn.canwrite
            && let Some(ref mut writer) = conn.writer
        {
            let _ = writer.flush();
        }
        if conn.canwrite {
            let result = match conn.kind {
                ConnKind::GzFile => flush_gz_conn(conn),
                ConnKind::BzFile => flush_bz_conn(conn),
                ConnKind::XzFile => flush_xz_conn(conn),
                _ => Ok(()),
            };
            if let Err(e) = result {
                r_error(&format!("error flushing connection: {}", e));
            }
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_readTable / do_writeTable
// ---------------------------------------------------------------------------

pub unsafe fn do_readTable(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

pub unsafe fn do_writeTable(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}
