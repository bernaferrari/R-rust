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
        let first = CAR(args);
        let (sraw, sopen) = if TYPEOF(first) == SEXPTYPE::RAWSXP {
            (first, CAR(CDR(args)))
        } else {
            args = CDR(args);
            let sraw = CAR(args);
            args = CDR(args);
            (sraw, CAR(args))
        };
        let description = String::new();
        let open = if sopen.is_null() || sopen == R_NilValue() {
            String::new()
        } else {
            check_string_arg(sopen, "open")
        };
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

pub unsafe fn do_textConnection(_call: SEXP, _op: SEXP, mut args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let arg1 = CAR(args);
        args = CDR(args);
        let arg2 = CAR(args);
        args = CDR(args);
        let arg3 = CAR(args);
        args = CDR(args);
        let arg4 = CAR(args);

        // .Internal(textConnection(name, object, open, local, type)) vs
        // the R wrapper textConnection(object, open, local = FALSE).
        let internal_form = !is_missing_conn_arg(arg4)
            && TYPEOF(arg4) == SEXPTYPE::LGLSXP
            && TYPEOF(arg3) == SEXPTYPE::STRSXP;

        let (description, stext, open_mode, local_arg) = if internal_form {
            (
                check_string_arg(arg1, "description"),
                arg2,
                {
                    let open = check_string_arg(arg3, "open");
                    if open.is_empty() {
                        "r".to_string()
                    } else {
                        open
                    }
                },
                arg4,
            )
        } else {
            let open = if is_missing_conn_arg(arg2) {
                "r".to_string()
            } else {
                let open = check_string_arg(arg2, "open");
                if open.is_empty() {
                    "r".to_string()
                } else {
                    open
                }
            };
            let desc = if TYPEOF(arg1) == SEXPTYPE::STRSXP && LENGTH(arg1) == 1 {
                check_string_arg(arg1, "description")
            } else {
                "textConnection".to_string()
            };
            (desc, arg1, open, arg3)
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
            conn.isopen = true;
            conn.canread = false;
            conn.canwrite = true;
            if TYPEOF(stext) == SEXPTYPE::STRSXP && LENGTH(stext) == 1 {
                conn.text_var = Some(string_elt(stext, 0));
            } else if !description.is_empty() && description != "textConnection" {
                conn.text_var = Some(description.clone());
            }
            conn.text_env = text_connection_env(local_arg, env);
            if !conn.text_env.is_null() {
                crate::sexp::protect::R_PreserveObject(conn.text_env);
                conn.assign_text_output();
            }
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

unsafe fn is_missing_conn_arg(arg: SEXP) -> bool {
    arg.is_null() || arg == R_NilValue() || arg == R_MissingArg()
}

unsafe fn text_connection_env(local_arg: SEXP, caller: SEXP) -> SEXP {
    unsafe {
        if is_missing_conn_arg(local_arg) {
            return crate::sexp::globals::R_GlobalEnv();
        }
        if TYPEOF(local_arg) == SEXPTYPE::ENVSXP {
            return local_arg;
        }
        if TYPEOF(local_arg) == SEXPTYPE::LGLSXP {
            let v = as_logical(local_arg);
            if v == crate::sexp::ffi::TRUE {
                if !caller.is_null() && TYPEOF(caller) == SEXPTYPE::ENVSXP {
                    return caller;
                }
                return crate::sexp::globals::R_GlobalEnv();
            }
            if v == crate::sexp::ffi::FALSE {
                return crate::sexp::globals::R_GlobalEnv();
            }
        }
        r_error("invalid 'local' argument");
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

        let class_name = table[n].as_ref().map(|conn| conn.class.clone()).unwrap_or_else(|| "connection".to_string());
        drop(table);
        let ans = Rf_ScalarInteger(n as c_int);
        set_connection_class(ans, &class_name);
        ans
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
        let integer_index = TYPEOF(scon) == SEXPTYPE::INTSXP || TYPEOF(scon) == SEXPTYPE::REALSXP;
        if !inherits_class(scon, "connection") && !integer_index {
            r_error("'con' is not a connection");
        }
        let i = as_integer(scon) as usize;
        let table = connection_table();
        let Some(conn) = table[i].as_ref() else {
            r_error("invalid connection");
        };

        let yesno = |flag: bool| if flag { "yes" } else { "no" };
        let values = [
            conn.description.as_str(),
            conn.class.as_str(),
            conn.mode.as_str(),
            if conn.text { "text" } else { "binary" },
            if conn.isopen { "opened" } else { "closed" },
            yesno(conn.canread),
            yesno(conn.canwrite),
        ];
        let labels = [
            "description",
            "class",
            "mode",
            "text",
            "opened",
            "can read",
            "can write",
        ];
        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, 7);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 7);
        for (idx, (label, value)) in labels.iter().zip(values.iter()).enumerate() {
            let lab = CString::new(*label).unwrap_or_default();
            SET_STRING_ELT(names, idx as R_xlen_t, Rf_mkChar(lab.as_ptr()));
            let val = CString::new(*value).unwrap_or_default();
            SET_VECTOR_ELT(ans, idx as R_xlen_t, Rf_mkString(val.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            ans,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        ans
    }
}

pub unsafe fn do_getAllConnections(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let table = connection_table();
        let mut ids = Vec::new();
        for (i, slot) in table.iter().enumerate() {
            if slot.is_some() {
                ids.push(i as i32);
            }
        }
        let ans = Rf_allocVector3(SEXPTYPE::INTSXP, ids.len() as R_xlen_t);
        for (i, id) in ids.iter().enumerate() {
            *INTEGER(ans).add(i) = *id;
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
                sink.output_con = sink.sink_cons[sink.sink_number];
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
        let mut new_line = true;
        let mut cell = CDR(CDR(_args));
        while !cell.is_null() && cell != R_NilValue() {
            let tag = crate::sexp::accessors::TAG(cell);
            let name = if tag.is_null() {
                None
            } else {
                crate::sexp::symbol::symbol_name_from_ptr(tag)
            };
            if name.as_deref().unwrap_or("").is_empty() || name.as_deref() == Some("newLine") {
                new_line = check_logical_arg(CAR(cell), "newLine") != 0;
                break;
            }
            cell = CDR(cell);
        }

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
