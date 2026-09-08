//! Pipe/text/raw connections, sinks, pushBack, connection introspection — extracted verbatim from the former single-file module.
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
// do_pipe — pipe(description, open = "", encoding = "")
// ---------------------------------------------------------------------------

pub unsafe fn do_pipe(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        if crate::mainutils::essentials::pipe_commands_disabled_by_runtime_policy() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "pipe() is disabled by the session capability policy".to_string(),
            });
        }

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
        let mut conn = RConn::new("pipe", &description, &open_mode, ConnKind::Pipe);
        conn.canseek = false;
        conn.text = !open_mode.contains('b');

        // Open immediately if open mode is non-empty
        if !open_mode.is_empty() {
            let is_read = open_mode.starts_with('r');
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(&description);

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
                    r_error(&format!("cannot open pipe '{}': {}", description, e));
                }
            }
        }

        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "pipe");
        ans
    }
}
// ---------------------------------------------------------------------------
// do_rawConnection — rawConnection(raw, open = "rb")
// ---------------------------------------------------------------------------

pub unsafe fn do_rawConnection(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let sfile = CAR(args);
        args = CDR(args);
        let sraw = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);

        let description = check_string_arg(sfile, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "rb".to_string()
        } else {
            open
        };

        if open_mode.contains('t') {
            r_error("invalid 'open' argument");
        }

        // Copy raw data from SEXP
        let mut raw_data = Vec::new();
        if !sraw.is_null() && TYPEOF(sraw) == SEXPTYPE::RAWSXP {
            let len = LENGTH(sraw) as usize;
            let data_ptr = RAW(sraw);
            if !data_ptr.is_null() && len > 0 {
                raw_data.extend_from_slice(std::slice::from_raw_parts(data_ptr, len));
            }
        }

        let ncon = next_connection();
        let mut conn = RConn::new(
            "rawConnection",
            &description,
            &open_mode,
            ConnKind::RawConnection,
        );
        conn.text = false;
        conn.canseek = true;
        conn.isopen = true;
        conn.canread = open_mode.starts_with('r');
        conn.canwrite = open_mode.starts_with('w') || open_mode.starts_with('a');
        if open_mode.contains('+') {
            conn.canread = true;
            conn.canwrite = true;
        }
        if open_mode.starts_with('a') {
            conn.raw_pos = raw_data.len();
        }
        conn.raw_data = raw_data;

        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "rawConnection");
        ans
    }
}

// ---------------------------------------------------------------------------
// do_textConnection — textConnection(object, open = "r", local = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_textConnection(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let sfile = CAR(args);
        args = CDR(args);
        let stext = CAR(args);
        args = CDR(args);
        let sopen = CAR(args);
        args = CDR(args);
        let _local = check_logical_arg(CAR(args), "local");

        let description = check_string_arg(sfile, "description");
        let open = check_string_arg(sopen, "open");
        let open_mode = if open.is_empty() {
            "r".to_string()
        } else {
            open
        };

        let ncon = next_connection();
        let mut conn = RConn::new(
            "textConnection",
            &description,
            &open_mode,
            ConnKind::TextConnection,
        );
        conn.canseek = false;

        if open_mode.starts_with('r') {
            // Input text connection: copy text from SEXP
            if !stext.is_null() && TYPEOF(stext) == SEXPTYPE::STRSXP {
                let len = LENGTH(stext) as R_xlen_t;
                let mut text = String::new();
                for j in 0..len {
                    let line = string_elt(stext, j);
                    text.push_str(&line);
                    text.push('\n');
                }
                conn.text_data = text;
                conn.text_pos = 0;
                conn.isopen = true;
                conn.canread = true;
                conn.canwrite = false;
            }
        } else {
            // Output text connection
            conn.isopen = true;
            conn.canread = false;
            conn.canwrite = true;
        }

        let mut table = connection_table();
        table[ncon] = Some(Box::new(conn));
        drop(table);

        let ans = Rf_ScalarInteger(ncon as c_int);
        let _ans_guard = protect(ans);
        set_connection_class(ans, "textConnection");
        ans
    }
}

// ---------------------------------------------------------------------------
// do_textConnectionValue — textConnectionValue(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_textConnectionValue(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);

        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        let table = connection_table();
        let Some(conn) = table[i].as_ref() else {
            r_error("invalid connection");
        };

        if !conn.canwrite {
            r_error("'con' is not an output textConnection");
        }

        let lines = conn.text_lines.borrow();
        let nlines = lines.len() as c_int;
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, nlines);
        if !ans.is_null() {
            for (idx, line) in lines.iter().enumerate() {
                let c_line = CString::new(line.as_str()).unwrap_or_default();
                let charsxp = Rf_mkChar(c_line.as_ptr());
                SET_STRING_ELT(ans, idx as R_xlen_t, charsxp);
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_sockConnection — socketConnection()
// ---------------------------------------------------------------------------

pub unsafe fn do_sockConnection(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    r_error("socketConnection is not supported in this pure-R Android runtime")
}

// ---------------------------------------------------------------------------
// do_serverSocket — server socket support
// ---------------------------------------------------------------------------

pub unsafe fn do_serverSocket(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    r_error("serverSocket is not supported in this pure-R Android runtime")
}

// ---------------------------------------------------------------------------
// do_download — legacy mainutils entry point
// ---------------------------------------------------------------------------

pub unsafe fn do_download(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    r_error("download.file is implemented through the utils internet boundary")
}

// ---------------------------------------------------------------------------
// do_getConnection — getConnection(n)
// ---------------------------------------------------------------------------

pub unsafe fn do_getConnection(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let sn = CAR(args);
        let n = as_integer(sn) as usize;

        init_connections_table();
        let table = connection_table();

        if n >= table.len() || table[n].is_none() {
            r_error("invalid connection");
        }

        let Some(_conn) = table[n].as_ref() else {
            r_error("invalid connection");
        };

        // Build a list with connection info
        // Return the integer index (like R's getConnection)
        drop(table);
        Rf_ScalarInteger(n as c_int)
    }
}

// ---------------------------------------------------------------------------
// do_showConnections — showConnections(all = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_showConnections(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let _all = check_logical_arg(CAR(args), "all");

        init_connections_table();
        let table = connection_table();

        // Count active connections
        let mut count = 0usize;
        for i in 0..table.len() {
            if table[i].is_some() {
                count += 1;
            }
        }

        let ans = Rf_allocVector(SEXPTYPE::STRSXP, count as c_int);
        if !ans.is_null() {
            let mut idx = 0usize;
            for i in 0..table.len() {
                if let Some(ref conn) = table[i] {
                    let desc = format!("{} {} {}", i, conn.description, conn.mode);
                    let c_desc = CString::new(desc).unwrap_or_default();
                    let charsxp = Rf_mkChar(c_desc.as_ptr());
                    SET_STRING_ELT(ans, idx as R_xlen_t, charsxp);
                    idx += 1;
                }
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_sumConnection — summary.connection()
// ---------------------------------------------------------------------------

pub unsafe fn do_sumConnection(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let scon = CAR(args);
        if !inherits_class(scon, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        let table = connection_table();
        let Some(conn) = table[i].as_ref() else {
            r_error("invalid connection");
        };

        let fields = [
            format!("description={}", conn.description),
            format!("class={}", conn.class),
            format!("mode={}", conn.mode),
            format!("opened={}", conn.isopen),
            format!("can read={}", conn.canread),
            format!("can write={}", conn.canwrite),
            format!("can seek={}", conn.canseek),
            format!("pushback={}", conn.pushback.len()),
        ];
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, fields.len() as c_int);
        for (idx, field) in fields.iter().enumerate() {
            let c_field = CString::new(field.as_str()).unwrap_or_default();
            let charsxp = Rf_mkChar(c_field.as_ptr());
            SET_STRING_ELT(ans, idx as R_xlen_t, charsxp);
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// do_sink — sink(number = NULL, close.on.exit = FALSE, type = "output", split = FALSE)
// ---------------------------------------------------------------------------

pub unsafe fn do_sink(_call: SEXP, _op: SEXP, mut args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let sn = CAR(args);
        args = CDR(args);
        let close_on_exit = check_logical_arg(CAR(args), "closeOnExit");
        args = CDR(args);
        let errcon = check_logical_arg(CAR(args), "type");
        args = CDR(args);
        let tee = check_logical_arg(CAR(args), "split");

        let icon = as_integer(sn);

        let mut sink = sink_state();

        if errcon == 0 {
            // Output sink
            if icon >= 0 {
                if sink.sink_number >= 20 {
                    r_error("sink stack is full");
                }
                sink.sink_number += 1;
                if sink.sink_cons.len() <= sink.sink_number {
                    sink.sink_cons.push(icon);
                    sink.sink_close.push(close_on_exit != 0);
                    sink.sink_split.push(tee != 0);
                } else {
                    let idx = sink.sink_number;
                    sink.sink_cons[idx] = icon;
                    sink.sink_close[idx] = close_on_exit != 0;
                    sink.sink_split[idx] = tee != 0;
                }
                sink.output_con = icon;
            } else {
                // Close sink: revert to stdout
                if sink.sink_number > 0 {
                    sink.sink_number -= 1;
                }
                sink.output_con = 1;
            }
        } else {
            // Error/message sink
            if icon < 0 || icon == 2 {
                sink.error_con = 2;
            } else {
                sink.error_con = icon;
            }
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_sinkNumber — sink.number(type = "output")
// ---------------------------------------------------------------------------

pub unsafe fn do_sinkNumber(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let errcon = check_logical_arg(CAR(args), "type");
        let sink = sink_state();
        if errcon != 0 {
            Rf_ScalarInteger(sink.error_con)
        } else {
            Rf_ScalarInteger(sink.sink_number as c_int)
        }
    }
}

// ---------------------------------------------------------------------------
// do_pushBack — pushBack(data, con)
// ---------------------------------------------------------------------------

pub unsafe fn do_pushBack(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let data = CAR(_args);
        let con = CAR(CDR(_args));
        let new_line = if CDR(CDR(_args)) == R_NilValue() {
            true
        } else {
            check_logical_arg(CAR(CDR(CDR(_args))), "newLine") != 0
        };

        if TYPEOF(data) != SEXPTYPE::STRSXP {
            r_error("'data' must be a character vector");
        }
        if !inherits_class(con, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(con);
        let len = LENGTH(data);
        for idx in (0..len).rev() {
            let mut line = string_elt(data, idx as R_xlen_t).into_bytes();
            if new_line {
                line.push(b'\n');
            }
            connection_pushback(i, &line);
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_pushBackClear — pushBackClear(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_pushBackClear(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let con = CAR(args);
        if !inherits_class(con, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(con) as usize;
        let mut table = connection_table();
        let Some(conn) = table[i].as_mut() else {
            r_error("invalid connection");
        };
        conn.pushback.clear();
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_pushBackLength — pushBackLength(con)
// ---------------------------------------------------------------------------

pub unsafe fn do_pushBackLength(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let con = CAR(args);
        if !inherits_class(con, "connection") {
            r_error("'con' is not a connection");
        }
        let i = as_integer(con) as usize;
        let table = connection_table();
        let Some(conn) = table[i].as_ref() else {
            r_error("invalid connection");
        };
        Rf_ScalarInteger(conn.pushback.len() as c_int)
    }
}
