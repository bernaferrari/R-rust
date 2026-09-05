//! Essentials domain module `io` — extracted verbatim from essentials.rs.

use super::*;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
use crate::sexp::context::RError;
use crate::sexp::ffi::{FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// I/O builtins: cat() to file, writeLines(), file.exists()
// ---------------------------------------------------------------------------

/// Resolve a file path from a table-reading builtin against the package
/// directory currently being sourced, when one is active.
///
/// Upstream evaluates install-time package code (crayon's
/// `read.table("tools/ansi-palettes.txt", ...)`) with the package root as
/// the working directory, baking the result into the lazy-load database.
/// This port sources package R files at load time instead, so a relative
/// path from that code is re-rooted at the package directory. Absolute
/// paths and ordinary session reads (no package being sourced) pass
/// through unchanged.
fn resolve_package_relative_path(file_path: String) -> String {
    let given = std::path::Path::new(&file_path);
    if given.is_absolute() {
        return file_path;
    }
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.loading_package_dir
            .as_ref()
            .map(|dir| dir.join(given))
    })
    .map(|joined| joined.to_string_lossy().into_owned())
    .unwrap_or(file_path)
}

/// R's `writeLines(text, con = stdout(), sep = "\n", useBytes = FALSE)`.
pub unsafe fn do_writeLines(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let text = CAR(args);
        if text.is_null() || text == R_NilValue() {
            return R_NilValue();
        }

        let mut con = R_NilValue();
        let mut sep = "\n".to_string();
        let mut positional = 0;
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            match tag_name(current).as_deref() {
                Some("con") => con = arg,
                Some("sep") => sep = elt_to_string(arg, 0),
                Some("useBytes") => {}
                _ => {
                    match positional {
                        0 => con = arg,
                        1 => sep = elt_to_string(arg, 0),
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let path = if con.is_null() || con == R_NilValue() {
            "/dev/stdout".to_string()
        } else if TYPEOF(con) == SEXPTYPE::INTSXP {
            let sep_sxp = Rf_mkString(CString::new(sep).unwrap_or_default().as_ptr());
            let normalized = Rf_cons(
                text,
                Rf_cons(
                    con,
                    Rf_cons(sep_sxp, Rf_cons(Rf_ScalarLogical(FALSE), R_NilValue())),
                ),
            );
            return crate::mainutils::connections::do_writeLines(_call, _op, normalized, _rho);
        } else {
            elt_to_string(con, 0)
        };

        let n = if TYPEOF(text) == SEXPTYPE::STRSXP {
            XLENGTH(text)
        } else {
            1
        };
        if path == "/dev/stdout" {
            let mut output = String::new();
            for i in 0..n {
                output.push_str(&elt_to_string(text, i));
                output.push_str(&sep);
            }
            if crate::sexp::output::is_capturing() {
                crate::sexp::output::capture_stdout(&output);
            } else {
                print!("{}", output);
            }
        } else if let Ok(mut file) = std::fs::File::create(&path) {
            use std::io::Write;
            for i in 0..n {
                let _ = file.write_all(elt_to_string(text, i).as_bytes());
                let _ = file.write_all(sep.as_bytes());
            }
        }
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

/// R's `readLines(con)` — read lines from file.
pub unsafe fn do_readLines(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let con = CAR(args);
        if con.is_null() {
            return R_NilValue();
        }
        let path = elt_to_string(con, 0);

        let lines = std::fs::read_to_string(&path).unwrap_or_default();
        let line_vec: Vec<&str> = lines.lines().collect();
        let n = line_vec.len();

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (i, line) in line_vec.iter().enumerate() {
            let cstr = CString::new(*line).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i) = charsxp;
            }
        }
        result
    }
}

/// R's `file.exists(...)` — check if files exist.
pub unsafe fn do_file_exists(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let path = elt_to_string(x, i);
            *dst.add(i as usize) = if std::path::Path::new(&path).exists() {
                TRUE
            } else {
                FALSE
            };
        }
        result
    }
}

/// R's `list.files(path)` — list files in directory.
pub unsafe fn do_list_files(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let path_arg = CAR(args);
        let path = if path_arg.is_null() || path_arg == R_NilValue() {
            ".".to_string()
        } else {
            elt_to_string(path_arg, 0)
        };

        let entries: Vec<String> = std::fs::read_dir(&path)
            .map(|dir| {
                dir.filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();

        let n = entries.len();
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (i, name) in entries.iter().enumerate() {
            let cstr = CString::new(name.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i) = charsxp;
            }
        }
        result
    }
}

/// R's `system(command, intern = FALSE)` — run a system command.
pub unsafe fn do_system(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let cmd = CAR(args);
        if cmd.is_null() {
            return R_NilValue();
        }
        let cmd_str = elt_to_string(cmd, 0);
        let intern = if !CDR(args).is_null() && CDR(args) != R_NilValue() {
            logical_arg(CAR(CDR(args)), false)
        } else {
            false
        };

        if system_commands_disabled_by_runtime_policy() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "system() is disabled by the session capability policy".to_string(),
            });
        }

        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd_str)
            .output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if intern {
                    let lines: Vec<&str> = stdout.lines().collect();
                    let result = Rf_allocVector3(SEXPTYPE::STRSXP, lines.len() as R_xlen_t);
                    for (i, line) in lines.iter().enumerate() {
                        let cstr = CString::new(*line).unwrap_or_default();
                        SET_STRING_ELT(result, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                    }
                    result
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    if !stdout.is_empty() {
                        crate::sexp::output::capture_stdout(&stdout);
                    }
                    if !stderr.is_empty() {
                        crate::sexp::output::capture_stderr(&stderr);
                    }
                    crate::sexp::globals::set_R_Visible(FALSE);
                    Rf_ScalarInteger(out.status.code().unwrap_or(1))
                }
            }
            Err(_) => {
                crate::sexp::globals::set_R_Visible(FALSE);
                Rf_ScalarInteger(127)
            }
        }
    }
}

/// R's `system2(command, args, stdout, stderr, wait, input)` — run a command.
pub unsafe fn do_system2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let command_arg = arg_by_name_or_position(args, &["command"], 0);
        if command_arg.is_null() || command_arg == R_NilValue() {
            return R_NilValue();
        }
        let command = elt_to_string(command_arg, 0);
        let argv_arg = arg_by_name_or_position(args, &["args"], 1);
        let argv = if argv_arg.is_null() || argv_arg == R_NilValue() {
            Vec::new()
        } else {
            (0..XLENGTH(argv_arg))
                .map(|i| elt_to_string(argv_arg, i))
                .filter(|arg| !arg.is_empty() && arg != "NA")
                .collect::<Vec<_>>()
        };
        let stdout_arg = arg_by_name_or_position(args, &["stdout"], 2);
        let capture_stdout = logical_arg(stdout_arg, false);

        if system_commands_disabled_by_runtime_policy() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "system2() is disabled by the session capability policy".to_string(),
            });
        }

        let output = std::process::Command::new(&command).args(&argv).output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                if capture_stdout {
                    let lines = stdout.lines().map(str::to_string).collect::<Vec<_>>();
                    return string_vector(&lines);
                }
                if !stdout.is_empty() {
                    crate::sexp::output::capture_stdout(&stdout);
                }
                if !stderr.is_empty() {
                    crate::sexp::output::capture_stderr(&stderr);
                }
                crate::sexp::globals::set_R_Visible(FALSE);
                Rf_ScalarInteger(out.status.code().unwrap_or(1))
            }
            Err(_) => {
                crate::sexp::globals::set_R_Visible(FALSE);
                Rf_ScalarInteger(127)
            }
        }
    }
}

pub(crate) fn system_commands_disabled_by_runtime_policy() -> bool {
    !crate::sexp::instance::with_current_instance(|inst| {
        inst.eval_state.capabilities.allow_system_commands
    })
    .unwrap_or(false)
}

pub(crate) fn pipe_commands_disabled_by_runtime_policy() -> bool {
    !crate::sexp::instance::with_current_instance(|inst| {
        inst.eval_state.capabilities.allow_pipe_commands
    })
    .unwrap_or(false)
}

/// R's `stopifnot(...)` — stop if any condition is FALSE.
pub unsafe fn do_stopifnot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let cond = CAR(current);
            if !cond.is_null()
                && TYPEOF(cond) == SEXPTYPE::LGLSXP
                && LENGTH(cond) > 0
                && *LOGICAL(cond) == 0
            {
                // Upstream stopifnot() raises stop(call. = FALSE): the error
                // renders without call attribution. Render explicitly so the
                // builtin-dispatch attribution wrapper does not add a call.
                crate::mainutils::errors::errorcall_str(
                    crate::sexp::globals::R_NilValue(),
                    "FALSE is not TRUE",
                );
            }
            current = CDR(current);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// R's `nargs()` — number of arguments in the current call.
pub unsafe fn do_nargs(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let Some(context) = current_function_context() else {
            base_error("'nargs' used outside a function");
        };
        Rf_ScalarInteger(pairlist_len((*context).promiseargs))
    }
}

// ---------------------------------------------------------------------------
// Connection basics (simplified)
// ---------------------------------------------------------------------------

/// R's `file(description)` — create a file connection.
/// Delegates to the session-owned connection table.
pub unsafe fn do_file(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_file(_call, _op, args, _rho) }
}

/// R's `url(description)` — create a URL connection.
/// Delegates to the session-owned connection table.
pub unsafe fn do_url(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_url(_call, _op, args, _rho) }
}

/// R's `close(con)` — close a connection.
/// Delegates to the session-owned connection table.
pub unsafe fn do_close(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_close(_call, _op, args, _rho) }
}

/// R's `flush(con)` — flush a connection.
/// Delegates to the session-owned connection table.
pub unsafe fn do_flush(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_flush(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// Extended connection constructors
// ---------------------------------------------------------------------------

/// R's `gzfile(description, open, encoding, compression)` — gzip connection.
/// Simplified: delegates to connections.rs when available, else returns description.
pub unsafe fn do_gzfile(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let desc = CAR(args);
        if desc.is_null() || desc == R_NilValue() {
            return R_NilValue();
        }
        // Delegate to connections.rs full implementation
        crate::mainutils::connections::do_gzfile(_call, _op, args, _rho)
    }
}

/// R's `pipe(description, open, encoding)` — pipe connection.
/// Simplified: delegates to connections.rs when available, else returns description.
pub unsafe fn do_pipe(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let desc = CAR(args);
        if desc.is_null() || desc == R_NilValue() {
            return R_NilValue();
        }
        // Delegate to connections.rs full implementation
        crate::mainutils::connections::do_pipe(_call, _op, args, _rho)
    }
}

/// R's `fifo(description, open, blocking)` — FIFO connection.
/// Simplified: delegates to connections.rs when available, else returns description.
pub unsafe fn do_fifo(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let desc = CAR(args);
        if desc.is_null() || desc == R_NilValue() {
            return R_NilValue();
        }
        // Delegate to connections.rs full implementation
        crate::mainutils::connections::do_fifo(_call, _op, args, _rho)
    }
}

/// R's `socketConnection(host, port, open, blocking, server, encoding)` — socket connection.
/// Simplified: stub that returns NULL.
pub unsafe fn do_socketConnection(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _host = CAR(args);
        let _port = CAR(CDR(args));
        // Socket connections not yet fully supported
        crate::mainutils::connections::do_sockConnection(_call, _op, args, _rho)
    }
}

// ---------------------------------------------------------------------------
// Connection queries and operations
// ---------------------------------------------------------------------------

/// R's `isOpen(con, rw)` — check if a connection is open.
/// Simplified: delegates to connections.rs.
pub unsafe fn do_isOpen(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_isopen(_call, _op, args, _rho) }
}

/// R's `isIncomplete(con)` — check if a connection has incomplete read.
/// Simplified: delegates to connections.rs.
pub unsafe fn do_isIncomplete(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_isincomplete(_call, _op, args, _rho) }
}

/// R's `isSeekable(con)` — check if a connection supports seeking.
/// Delegates to the session-owned connection table.
pub unsafe fn do_isSeekable(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_isseekable(_call, _op, args, _rho) }
}

/// R's `seek(con, where, origin, rw)` — seek in a connection.
/// Simplified: delegates to connections.rs.
pub unsafe fn do_seek(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_seek(_call, _op, args, _rho) }
}

/// R's `pushBack(lines, con, newLine)` — push back lines to a connection.
/// Simplified: no-op stub.
pub unsafe fn do_pushBack(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_pushBack(_call, _op, args, _rho) }
}

/// R's `pushBackClear(con)` — clear push back buffer.
pub unsafe fn do_pushBackClear(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_pushBackClear(_call, _op, args, _rho) }
}

/// R's `pushBackLength(con)` — get push back buffer length.
/// Simplified: returns 0.
pub unsafe fn do_pushBackLength(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_pushBackLength(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// Binary I/O
// ---------------------------------------------------------------------------

/// R's `readBin(con, what, n, size, signed, endian)` — read binary data.
/// Delegates to connections.rs for full implementation.
pub unsafe fn do_readBin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_readBin(_call, _op, args, _rho) }
}

/// R's `writeBin(object, con, size, endian, useBytes)` — write binary data.
/// Delegates to connections.rs for full implementation.
pub unsafe fn do_writeBin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_writeBin(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// Complete I/O: scan, write.table, sink
// ---------------------------------------------------------------------------

fn scan_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

fn split_scan_fields(contents: &str, sep: &str, nmax: i64) -> Vec<String> {
    let limit = if nmax > 0 { nmax as usize } else { usize::MAX };
    let fields: Box<dyn Iterator<Item = &str> + '_> = if sep.is_empty() {
        Box::new(contents.split_whitespace())
    } else {
        Box::new(
            contents
                .split(sep)
                .map(str::trim)
                .filter(|field| !field.is_empty()),
        )
    };
    fields.take(limit).map(ToOwned::to_owned).collect()
}

fn parse_scan_logical(value: &str) -> Option<c_int> {
    match value {
        "TRUE" | "True" | "true" | "T" | "1" => Some(TRUE),
        "FALSE" | "False" | "false" | "F" | "0" => Some(FALSE),
        "NA" => Some(NA_LOGICAL),
        _ => None,
    }
}

pub(crate) unsafe fn named_arg(args: SEXP, name: &str) -> Option<SEXP> {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let tag = TAG(current);
            if !tag.is_null() && tag != R_NilValue() {
                let printname = PRINTNAME(tag);
                if !printname.is_null() {
                    let tag_name = CStr::from_ptr(CHAR(printname)).to_string_lossy();
                    if tag_name == name {
                        return Some(CAR(current));
                    }
                }
            }
            current = CDR(current);
        }
        None
    }
}

/// R's `scan(file, what, nmax, sep, ...)` — read data from a file path.
/// This covers the file-backed scalar-vector surface used by scripts and tests;
/// interactive console and connection scans report explicit R errors.
pub unsafe fn do_scan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Resolve arguments the way stock .Internal(scan(...)) sees them, by
        // named tag first, then positional slot:
        // (file, what, nmax, sep, dec, quote, skip, nlines, na.strings,
        //  flush, fill, text, fileEncoding).
        let by_slot = |names: &[&str], position: usize| -> SEXP {
            arg_by_name_or_position(args, names, position)
        };
        // Stock precedence: text= is used only when file= is missing
        // (scan.R: `if (missing(file) && !missing(text)) file <- textConnection(text)`).
        let text_arg = by_slot(&["text"], 11);
        let file_arg = by_slot(&["file"], 0);
        let what_arg = by_slot(&["what"], 1);
        let nmax_arg = by_slot(&["nmax"], 2);
        let file_given = !file_arg.is_null() && file_arg != R_NilValue();
        let text = if file_given
            || text_arg.is_null()
            || text_arg == R_NilValue()
            || XLENGTH(text_arg) == 0
        {
            None
        } else {
            if TYPEOF(text_arg) != SEXPTYPE::STRSXP {
                scan_error("invalid 'text' argument");
            }
            Some(elt_to_string(text_arg, 0))
        };
        let contents = if let Some(text) = text {
            text
        } else {
            if file_arg.is_null() || file_arg == R_NilValue() || file_arg == R_MissingArg() {
                scan_error("scan() requires a file path in the Android/headless runtime");
            }
            if TYPEOF(file_arg) != SEXPTYPE::STRSXP || XLENGTH(file_arg) < 1 {
                scan_error("scan() currently supports character file paths only");
            }
            let filename = elt_to_string(file_arg, 0);
            if filename.is_empty() {
                scan_error("scan() cannot read from an interactive console in this runtime");
            }
            match std::fs::read_to_string(&filename) {
                Ok(s) => s,
                Err(err) => scan_error(format!("cannot open file '{filename}': {err}")),
            }
        };
        let what_type = if what_arg.is_null() || what_arg == R_NilValue() {
            SEXPTYPE::REALSXP.as_c_int()
        } else {
            TYPEOF(what_arg)
        };
        let nmax = if nmax_arg.is_null() || nmax_arg == R_NilValue() {
            -1_i64
        } else {
            real_or_default(nmax_arg, -1.0) as i64
        };
        let nlines = match named_arg(args, "nlines") {
            Some(nl) if !nl.is_null() && nl != R_NilValue() => real_or_default(nl, -1.0) as i64,
            _ => -1_i64,
        };
        // nlines: only the first `nlines` lines are read (partial last line kept).
        let contents = if nlines >= 0 {
            let mut kept: Vec<&str> = contents.split('\n').take(nlines as usize).collect();
            if let Some(last) = kept.last_mut() {
                if let Some(stripped) = last.strip_suffix('\r') {
                    *last = stripped;
                }
            }
            kept.join("\n")
        } else {
            contents
        };
        let sep_arg = by_slot(&["sep"], 3);
        let sep = if sep_arg.is_null() || sep_arg == R_NilValue() {
            String::new()
        } else {
            elt_to_string(sep_arg, 0)
        };
        let values = split_scan_fields(&contents, &sep, nmax);
        let n = values.len() as R_xlen_t;
        let quiet = match named_arg(args, "quiet") {
            Some(q) if !q.is_null() && q != R_NilValue() && XLENGTH(q) > 0 => {
                TYPEOF(q) != SEXPTYPE::LGLSXP || *LOGICAL(q) != 0
            }
            _ => false,
        };
        if !quiet {
            let item_word = if n == 1 { "item" } else { "items" };
            crate::sexp::output::capture_stdout(&format!("Read {n} {item_word}\n"));
        }
        if what_type == SEXPTYPE::INTSXP {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            let dst = INTEGER(result);
            for (i, value) in values.iter().enumerate() {
                let parsed = if value == "NA" {
                    NA_INTEGER
                } else {
                    value.parse::<c_int>().unwrap_or_else(|_| {
                        scan_error(format!("scan() expected an integer, got '{value}'"))
                    })
                };
                *dst.add(i) = parsed;
            }
            result
        } else if what_type == SEXPTYPE::REALSXP {
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            let dst = REAL(result);
            for (i, value) in values.iter().enumerate() {
                let parsed = if value == "NA" {
                    NA_REAL
                } else {
                    crate::mainutils::coerce::parse_double_str(value).unwrap_or_else(|| {
                        scan_error(format!("scan() expected a real, got '{value}'"))
                    })
                };
                *dst.add(i) = parsed;
            }
            result
        } else if what_type == SEXPTYPE::LGLSXP {
            let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            let dst = LOGICAL(result);
            for (i, value) in values.iter().enumerate() {
                let parsed = parse_scan_logical(value).unwrap_or_else(|| {
                    scan_error(format!("scan() expected a logical, got '{value}'"))
                });
                *dst.add(i) = parsed;
            }
            result
        } else if what_type == SEXPTYPE::STRSXP {
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            for (i, value) in values.iter().enumerate() {
                let cstr = CString::new(value.as_str()).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
            result
        } else {
            scan_error("scan() only supports integer, numeric, logical, and character 'what'")
        }
    }
}

/// R's `write.table(x, file, sep=" ", ...)` — write data to file.
pub unsafe fn do_write_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let file_arg = CAR(CDR(args));
        let sep_arg = CAR(CDR(CDR(args)));
        if x_arg.is_null()
            || x_arg == R_NilValue()
            || file_arg.is_null()
            || file_arg == R_NilValue()
        {
            return R_NilValue();
        }
        let filename = elt_to_string(file_arg, 0);
        let sep = if sep_arg.is_null() || sep_arg == R_NilValue() {
            " "
        } else {
            &elt_to_string(sep_arg, 0)
        };

        let mut output = String::new();
        let n = XLENGTH(x_arg);
        let t = TYPEOF(x_arg);

        if t == SEXPTYPE::VECSXP {
            // Data frame-like: write columns
            let ncols = n;
            let nrows = if n > 0 {
                XLENGTH(VECTOR_ELT(x_arg, 0))
            } else {
                0
            };
            if ncols == 0 {
                output.push_str("\"\"\n");
            }
            // Write header with column names
            let names_sym = Rf_install(c"names".as_ptr());
            let names = crate::sexp::attrib_core::getAttrib(x_arg, names_sym);
            if ncols > 0
                && !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
            {
                let mut header = Vec::new();
                for j in 0..ncols {
                    let charsxp = crate::sexp::accessors::STRING_ELT(names, j);
                    if !charsxp.is_null() {
                        let s = crate::sexp::accessors::CHAR(charsxp);
                        if !s.is_null() {
                            header.push(
                                std::ffi::CStr::from_ptr(s)
                                    .to_str()
                                    .unwrap_or("")
                                    .to_string(),
                            );
                        } else {
                            header.push(String::new());
                        }
                    } else {
                        header.push(String::new());
                    }
                }
                output.push_str(&header.join(sep));
                output.push('\n');
            }
            // Write rows
            for i in 0..nrows {
                let mut row = Vec::new();
                for j in 0..ncols {
                    let col = VECTOR_ELT(x_arg, j);
                    if !col.is_null() && col != R_NilValue() {
                        row.push(elt_to_string(col, i));
                    } else {
                        row.push("NA".to_string());
                    }
                }
                output.push_str(&row.join(sep));
                output.push('\n');
            }
        } else {
            // Atomic vector: write as single column
            for i in 0..n {
                output.push_str(&elt_to_string(x_arg, i));
                output.push('\n');
            }
        }

        let _ = std::fs::write(&filename, output);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// R's `sink(file, append, type, split)` — redirect output to a connection.
pub unsafe fn do_sink(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut file_arg = R_NilValue();
        let mut append_arg = Rf_ScalarLogical(FALSE);
        let mut type_arg = Rf_mkString(c"output".as_ptr());
        let mut split_arg = Rf_ScalarLogical(FALSE);
        let mut positional = 0usize;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            match tag_name(current).as_deref() {
                Some("file") => file_arg = arg,
                Some("append") => append_arg = arg,
                Some("type") => type_arg = arg,
                Some("split") => split_arg = arg,
                _ => {
                    match positional {
                        0 => file_arg = arg,
                        1 => append_arg = arg,
                        2 => type_arg = arg,
                        3 => split_arg = arg,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let split = logical_scalar_or(split_arg, FALSE);
        let is_message_sink = !type_arg.is_null()
            && type_arg != R_NilValue()
            && TYPEOF(type_arg) == SEXPTYPE::STRSXP
            && elt_to_string(type_arg, 0) == "message";

        let (target, close_on_exit) = if file_arg.is_null() || file_arg == R_NilValue() {
            if is_message_sink {
                (Rf_ScalarInteger(2), FALSE)
            } else {
                (Rf_ScalarInteger(-1), FALSE)
            }
        } else if inherits_class(file_arg, "connection") {
            (file_arg, FALSE)
        } else if TYPEOF(file_arg) == SEXPTYPE::STRSXP {
            if is_message_sink {
                base_error("'file' must be NULL or an already open connection");
            }
            let append = logical_scalar_or(append_arg, FALSE) != FALSE;
            let open = if append { "a" } else { "w" };
            let open_sxp = Rf_mkString(CString::new(open).unwrap_or_default().as_ptr());
            let encoding_sxp = Rf_mkString(c"native.enc".as_ptr());
            let file_args = Rf_cons(
                file_arg,
                Rf_cons(
                    open_sxp,
                    Rf_cons(
                        encoding_sxp,
                        Rf_cons(
                            Rf_ScalarLogical(TRUE),
                            Rf_cons(Rf_ScalarLogical(FALSE), R_NilValue()),
                        ),
                    ),
                ),
            );
            (
                crate::mainutils::connections::do_file(_call, _op, file_args, _rho),
                TRUE,
            )
        } else {
            base_error("'file' must be NULL, a connection or a character string");
        };
        if is_message_sink && split != FALSE {
            base_error("cannot split the message connection");
        }

        let normalized = Rf_cons(
            target,
            Rf_cons(
                Rf_ScalarLogical(close_on_exit),
                Rf_cons(
                    Rf_ScalarLogical(if is_message_sink { TRUE } else { FALSE }),
                    Rf_cons(Rf_ScalarLogical(split), R_NilValue()),
                ),
            ),
        );
        crate::mainutils::connections::do_sink(_call, _op, normalized, _rho);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// R's `sink.number(type)` — report output or message sink depth.
pub unsafe fn do_sink_number(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let type_arg = if args.is_null() || args == R_NilValue() {
            Rf_mkString(c"output".as_ptr())
        } else {
            CAR(args)
        };
        let is_message_sink = !type_arg.is_null()
            && type_arg != R_NilValue()
            && TYPEOF(type_arg) == SEXPTYPE::STRSXP
            && elt_to_string(type_arg, 0) == "message";
        let normalized = Rf_cons(
            Rf_ScalarLogical(if is_message_sink { TRUE } else { FALSE }),
            R_NilValue(),
        );
        crate::mainutils::connections::do_sinkNumber(_call, _op, normalized, _rho)
    }
}

unsafe fn logical_scalar_or(arg: SEXP, default: c_int) -> c_int {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || arg == R_MissingArg() {
            return default;
        }
        if TYPEOF(arg) == SEXPTYPE::LGLSXP || TYPEOF(arg) == SEXPTYPE::INTSXP {
            *INTEGER(arg)
        } else {
            default
        }
    }
}

// ---------------------------------------------------------------------------
// Complete I/O
// ---------------------------------------------------------------------------

/// R-like `cat_args(...)` — cat with better formatting.
pub unsafe fn do_cat_args(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_cat(_call, _op, args, _rho) }
}

/// R-like `message_args(...)` — message with domain.
pub unsafe fn do_message_args(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let output = condition_message_text(args, &["domain", "appendLF"]);
        eprintln!("{}", output);
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

/// R's `packageStartupMessage(...)` — startup message.
pub unsafe fn do_package_startup_message(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let output = condition_message_text(args, &["domain", "appendLF"]);
        eprintln!("{}", output);
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Complete I/O — capture.output, withVisible, invisible, suppress*,
// ---------------------------------------------------------------------------

/// R's `capture.output(expr)` — capture printed stdout as a character vector.
pub unsafe fn do_capture_output(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() || expr == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        crate::sexp::output::start_capture();
        let _ = crate::eval::eval::Rf_eval(expr, rho);
        let captured = crate::sexp::output::stop_capture();

        let stdout = captured.stdout.trim_end_matches('\n');
        let lines: Vec<&str> = if stdout.is_empty() {
            Vec::new()
        } else {
            stdout.split('\n').collect()
        };

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, lines.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for (i, line) in lines.iter().enumerate() {
            let cstr = CString::new(*line).unwrap_or_default();
            let charsxp = Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
        result
    }
}

/// R's `withVisible(x)` — returns a list with $value and $visible.
pub unsafe fn do_with_visible(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let visible = crate::sexp::globals::R_Visible();
        // Return a VECSXP (list) with two elements: value, visible
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        crate::sexp::accessors::SET_VECTOR_ELT(result, 0, x);
        let vis_vec = Rf_ScalarLogical(visible);
        crate::sexp::accessors::SET_VECTOR_ELT(result, 1, vis_vec);
        // Set names
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !names.is_null() {
            let _n_p = crate::sexp::protect::protect(names);
            let v_str = c"value";
            let vi_str = c"visible";
            let v_char = crate::sexp::constructors::Rf_mkChar(v_str.as_ptr());
            let vi_char = crate::sexp::constructors::Rf_mkChar(vi_str.as_ptr());
            if !v_char.is_null() {
                let data = (*names).gengc_next_node as *mut SEXP;
                *data.add(0) = v_char;
            }
            if !vi_char.is_null() {
                let data = (*names).gengc_next_node as *mut SEXP;
                *data.add(1) = vi_char;
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
        result
    }
}

/// R's `invisible(x)` — return x, setting visibility to FALSE.
pub unsafe fn do_invisible(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `suppressWarnings(expr)` — evaluate expr with warnings suppressed.
///
/// Two mechanisms: stderr produced inside is dropped (covers the R-level
/// `warning()` builtin), and the errors-machinery suppress depth is raised so
/// C-level warnings (`Rf_warning` → collection) are dropped before collection,
/// matching upstream's muffle restart.
pub unsafe fn do_suppress_warnings(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        struct DepthGuard;
        impl Drop for DepthGuard {
            fn drop(&mut self) {
                crate::mainutils::errors::exit_suppress_warnings();
            }
        }
        crate::mainutils::errors::enter_suppress_warnings();
        let _depth_guard = DepthGuard;
        crate::sexp::output::start_capture();
        let result = crate::eval::eval::Rf_eval(expr, rho);
        let captured = crate::sexp::output::stop_capture();
        if !captured.stdout.is_empty() {
            crate::sexp::output::capture_stdout(&captured.stdout);
        }
        result
    }
}

/// R's `suppressMessages(expr)` — evaluate expr with captured diagnostics suppressed.
pub unsafe fn do_suppress_messages(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        // Depth-gated suppression of signal-time message emission (the
        // capture-based approach cannot muffle messages once they ride the
        // interleaved stdout stream alongside print output).
        struct DepthGuard;
        impl Drop for DepthGuard {
            fn drop(&mut self) {
                crate::mainutils::errors::exit_suppress_messages();
            }
        }
        crate::mainutils::errors::enter_suppress_messages();
        let _depth_guard = DepthGuard;
        let result = crate::eval::eval::Rf_eval(expr, rho);
        result
    }
}

/// R's `force(x)` — force evaluation of a promise.
pub unsafe fn do_force(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // If x is a PROMSXP, force it
        if TYPEOF(x) == SEXPTYPE::PROMSXP {
            crate::sexp::envir::forcePromise(x)
        } else {
            x
        }
    }
}

// ---------------------------------------------------------------------------
// Complete I/O — enhanced cat, message, warning
// ---------------------------------------------------------------------------

/// R's enhanced `cat(..., file, sep, fill, labels, append)` — simplified.
pub unsafe fn do_cat_enhanced(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: delegates to existing do_cat
        do_cat(_call, _op, args, _rho)
    }
}

/// R's enhanced `message(..., domain, appendLF)` — simplified.
pub unsafe fn do_message_enhanced(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let output = condition_message_text(args, &["domain", "appendLF"]);
        eprintln!("{}", output);
        R_NilValue()
    }
}

/// R's enhanced `warning(..., call., immediate., noBreaks., domain.)` — simplified.
pub unsafe fn do_warning_enhanced(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut output =
            condition_message_text(args, &["call.", "immediate.", "noBreaks.", "domain"]);
        if output.is_empty() {
            output = "warning".to_string();
        }
        eprintln!("Warning: {}", output);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Complete I/O — read.csv, write.csv, read.table
// ---------------------------------------------------------------------------

/// R's `read.csv(file, header=TRUE, sep=",")` — read a CSV file (simplified).
/// Returns a list (data.frame) of columns as REALSXP vectors.
pub unsafe fn do_read_csv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let header_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };

        let file_path = resolve_package_relative_path(elt_to_string(file_arg, 0));
        let header = if header_arg.is_null() || header_arg == R_NilValue() {
            true
        } else {
            let v = real_or_default(header_arg, 1.0);
            v != 0.0
        };

        // Read file
        let content = match std::fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {}", file_path, e);
                return R_NilValue();
            }
        };

        let mut lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return R_NilValue();
        }

        let col_names: Vec<String> = if header {
            let header_line = lines.remove(0);
            header_line
                .split(',')
                .map(|s| s.trim().to_string())
                .collect()
        } else {
            lines[0]
                .split(',')
                .enumerate()
                .map(|(i, _)| format!("V{}", i + 1))
                .collect()
        };

        let ncols = col_names.len();
        if ncols == 0 {
            return R_NilValue();
        }

        // Parse data rows
        let mut col_data: Vec<Vec<f64>> = vec![Vec::new(); ncols];
        for line in &lines {
            let fields: Vec<&str> = line.split(',').collect();
            for j in 0..ncols {
                let val = if j < fields.len() {
                    fields[j].trim().parse::<f64>().unwrap_or(NA_REAL)
                } else {
                    NA_REAL
                };
                col_data[j].push(val);
            }
        }

        // Build list result
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
        let _p2 = protect(names_vec);

        for j in 0..ncols {
            let nrow = col_data[j].len();
            let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow as R_xlen_t);
            if !col.is_null() {
                let dst = REAL(col);
                for (i, &v) in col_data[j].iter().enumerate() {
                    *dst.add(i) = v;
                }
            }
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(j) = col;

            let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let nmdata = (*names_vec).gengc_next_node as *mut SEXP;
                *nmdata.add(j) = charsxp;
            }
        }

        // Set names
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names_vec);
        // Set class to data.frame
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _p3 = protect(class_vec);
        let cstr = c"data.frame";
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let cdata = (*class_vec).gengc_next_node as *mut SEXP;
            *cdata.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class_vec);
        result
    }
}

/// R's `write.csv(x, file, row.names=TRUE)` — write a CSV file (simplified).
pub unsafe fn do_write_csv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let file_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };
        let row_names_arg = if CDR(args).is_null()
            || CDR(args) == R_NilValue()
            || CDR(CDR(args)).is_null()
            || CDR(CDR(args)) == R_NilValue()
        {
            R_NilValue()
        } else {
            CAR(CDR(CDR(args)))
        };

        let file_path = elt_to_string(file_arg, 0);
        let write_row_names = if row_names_arg.is_null() || row_names_arg == R_NilValue() {
            true
        } else {
            let v = real_or_default(row_names_arg, 1.0);
            v != 0.0
        };

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let t = TYPEOF(x);
        let mut lines: Vec<String> = Vec::new();

        if t == SEXPTYPE::VECSXP {
            // Data.frame-like list
            let ncols = XLENGTH(x);
            let nrow = if ncols > 0 {
                let first_col = VECTOR_ELT(x, 0);
                XLENGTH(first_col)
            } else {
                0
            };

            // Get column names
            let names = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"names".as_ptr()));

            // Header
            let mut header_parts: Vec<String> = Vec::new();
            if write_row_names {
                header_parts.push(String::new());
            }
            for j in 0..ncols {
                let nm = if !names.is_null() {
                    elt_to_string(names, j)
                } else {
                    format!("V{}", j + 1)
                };
                header_parts.push(format!("\"{}\"", nm));
            }
            lines.push(header_parts.join(","));

            // Data rows
            for i in 0..nrow {
                let mut row_parts: Vec<String> = Vec::new();
                if write_row_names {
                    row_parts.push((i + 1).to_string());
                }
                for j in 0..ncols {
                    let col = VECTOR_ELT(x, j);
                    row_parts.push(elt_to_string(col, i));
                }
                lines.push(row_parts.join(","));
            }
        } else if t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP {
            // Simple vector — write as single column
            let n = XLENGTH(x);
            lines.push("\"x\"".to_string());
            for i in 0..n {
                lines.push(elt_to_string(x, i));
            }
        }

        let content = lines.join("\n") + "\n";
        if let Err(e) = std::fs::write(&file_path, content) {
            eprintln!("Error writing '{}': {}", file_path, e);
        }

        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

/// One parsed table cell: the field text plus whether it was quoted.
/// Quoting only survives into blank/`NA` decisions — na.strings match
/// either way (upstream scan accepts a quoted "NA" as NA) while a blank
/// unquoted field is NA for typed columns and "" for character ones.
#[derive(Clone)]
struct TableField {
    text: String,
    quoted: bool,
}

/// Lexing rules shared by record parsing: separator, quote set, comment
/// character, and whitespace handling, mirroring upstream scan semantics.
struct TableParseSpec {
    sep: Option<char>,
    quotes: Vec<char>,
    comment: Option<char>,
    strip_white: bool,
    blank_lines_skip: bool,
}

fn table_push_field(
    record: &mut Vec<TableField>,
    field: &mut String,
    quoted: &mut bool,
    started: &mut bool,
    spec: &TableParseSpec,
) {
    let mut text = std::mem::take(field);
    if !*quoted && spec.strip_white {
        text = text.trim().to_string();
    }
    record.push(TableField {
        text,
        quoted: *quoted,
    });
    *quoted = false;
    *started = false;
}

/// Split raw file text into records of fields, honoring the separator,
/// quote characters (including doubled quotes and newlines inside quoted
/// fields), the comment character, and blank-line skipping.
fn parse_table_records(content: &str, spec: &TableParseSpec) -> Vec<Vec<TableField>> {
    let mut records: Vec<Vec<TableField>> = Vec::new();
    let mut record: Vec<TableField> = Vec::new();
    let mut field = String::new();
    let mut field_quoted = false;
    let mut field_started = false;
    let mut quote: Option<char> = None;
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == q {
                if chars.peek() == Some(&q) {
                    chars.next();
                    field.push(q);
                } else {
                    quote = None;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        if spec.quotes.contains(&c) {
            quote = Some(c);
            field_quoted = true;
            field_started = true;
            continue;
        }
        if Some(c) == spec.sep {
            table_push_field(
                &mut record,
                &mut field,
                &mut field_quoted,
                &mut field_started,
                spec,
            );
            continue;
        }
        if spec.sep.is_none() && (c == ' ' || c == '\t' || c == '\r') {
            if field_started {
                table_push_field(
                    &mut record,
                    &mut field,
                    &mut field_quoted,
                    &mut field_started,
                    spec,
                );
            }
            continue;
        }
        if c == '\n' {
            if record.is_empty() && !field_started && !field_quoted {
                if spec.blank_lines_skip {
                    continue;
                }
                records.push(Vec::new());
                continue;
            }
            if field_started || field_quoted || spec.sep.is_some() {
                table_push_field(
                    &mut record,
                    &mut field,
                    &mut field_quoted,
                    &mut field_started,
                    spec,
                );
            }
            records.push(std::mem::take(&mut record));
            continue;
        }
        if c == '\r' {
            continue;
        }
        if Some(c) == spec.comment {
            while let Some(&nc) = chars.peek() {
                if nc == '\n' {
                    break;
                }
                chars.next();
            }
            continue;
        }
        field.push(c);
        field_started = true;
    }
    if quote.is_some() || field_started || field_quoted || !record.is_empty() {
        if field_started || field_quoted || spec.sep.is_some() {
            table_push_field(
                &mut record,
                &mut field,
                &mut field_quoted,
                &mut field_started,
                spec,
            );
        }
        records.push(record);
    }
    records
}

/// Declared column types from `colClasses`, recycled across columns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TableColClass {
    Logical,
    Integer,
    Numeric,
    Character,
    Factor,
    Null,
    Infer,
}

unsafe fn parse_table_col_classes(arg: SEXP, ncols: usize) -> Vec<TableColClass> {
    unsafe {
        let mut declared: Vec<Option<TableColClass>> = vec![None; ncols];
        if !arg.is_null() && arg != R_NilValue() && TYPEOF(arg) == SEXPTYPE::STRSXP {
            let n = XLENGTH(arg) as usize;
            if n > 0 {
                for j in 0..ncols {
                    let src = j % n;
                    if is_string_na(arg, src as R_xlen_t) {
                        continue;
                    }
                    let name = elt_to_string(arg, src as R_xlen_t);
                    declared[j] = Some(match name.as_str() {
                        "logical" => TableColClass::Logical,
                        "integer" => TableColClass::Integer,
                        "numeric" | "double" | "real" => TableColClass::Numeric,
                        "character" => TableColClass::Character,
                        "factor" => TableColClass::Factor,
                        "NULL" => TableColClass::Null,
                        _ => TableColClass::Character,
                    });
                }
            }
        }
        declared
            .into_iter()
            .map(|d| d.unwrap_or(TableColClass::Infer))
            .collect()
    }
}

/// Upstream `make.logical` accepts the strict TRUE/FALSE spellings.
fn parse_table_logical(text: &str) -> Option<c_int> {
    match text {
        "T" | "TRUE" | "true" | "True" => Some(TRUE),
        "F" | "FALSE" | "false" | "False" => Some(FALSE),
        _ => None,
    }
}

/// Parse an R numeric literal: f64 syntax plus the leading-dot forms
/// (".5", "-.5") that Rust's parser rejects.
fn parse_table_double(text: &str) -> Option<f64> {
    let s = text.trim();
    if let Ok(v) = s.parse::<f64>() {
        return Some(v);
    }
    if let Some(rest) = s.strip_prefix('.') {
        format!("0.{rest}").parse::<f64>().ok()
    } else if let Some(rest) = s.strip_prefix("-.") {
        format!("-0.{rest}").parse::<f64>().ok()
    } else if let Some(rest) = s.strip_prefix("+.") {
        format!("0.{rest}").parse::<f64>().ok()
    } else {
        None
    }
}

fn table_field_is_na(field: &TableField, na_strings: &[String]) -> bool {
    na_strings.contains(&field.text)
}

fn table_field_is_blank(field: &TableField) -> bool {
    !field.quoted && field.text.is_empty()
}

/// Infer one column's type the way `type.convert` does: logical, then
/// integer, then numeric, falling back to character.
fn infer_table_col_class(fields: &[&TableField], na_strings: &[String]) -> TableColClass {
    let mut is_logical = true;
    let mut is_integer = true;
    let mut is_numeric = true;
    for field in fields {
        if table_field_is_na(field, na_strings) || table_field_is_blank(field) {
            continue;
        }
        if parse_table_logical(&field.text).is_none() {
            is_logical = false;
        }
        if field.text.trim().parse::<i32>().is_err() {
            is_integer = false;
        }
        if parse_table_double(&field.text).is_none() {
            is_numeric = false;
        }
    }
    if is_logical {
        TableColClass::Logical
    } else if is_integer {
        TableColClass::Integer
    } else if is_numeric {
        TableColClass::Numeric
    } else {
        TableColClass::Character
    }
}

/// R's `read.table(file, header = FALSE, sep = "", quote = "\"'", dec = ".",
/// na.strings = "NA", colClasses = NA, nrows = -1, skip = 0, fill, ...)`.
/// Reads a delimited file into a data.frame, honoring the arguments the
/// corpus exercises: separator, quoting (doubled quotes, embedded
/// separators/newlines inside quoted fields), comment characters,
/// na.strings, colClasses (typed, "NULL" to skip, NA to infer), header
/// handling, and blank-line skipping.
pub unsafe fn do_read_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let named_only = usize::MAX;
        let file_arg = arg_by_name_or_position(args, &["file"], 0);
        let header_arg = arg_by_name_or_position(args, &["header"], 1);
        let sep_arg = arg_by_name_or_position(args, &["sep"], 2);
        let quote_arg = arg_by_name_or_position(args, &["quote"], 3);
        let col_classes_arg =
            arg_by_name_or_position(args, &["colClasses"], named_only);
        let na_strings_arg =
            arg_by_name_or_position(args, &["na.strings", "NA.strings"], named_only);
        let comment_arg = arg_by_name_or_position(args, &["comment.char"], named_only);
        let strip_white_arg = arg_by_name_or_position(args, &["strip.white"], named_only);
        let blank_skip_arg =
            arg_by_name_or_position(args, &["blank.lines.skip"], named_only);
        let fill_arg = arg_by_name_or_position(args, &["fill"], named_only);
        let nrows_arg = arg_by_name_or_position(args, &["nrows"], named_only);
        let skip_arg = arg_by_name_or_position(args, &["skip"], named_only);
        let text_arg = arg_by_name_or_position(args, &["text"], named_only);
        let row_names_arg = arg_by_name_or_position(args, &["row.names"], named_only);
        let col_names_arg = arg_by_name_or_position(args, &["col.names"], named_only);

        let sep_text = if sep_arg.is_null() || sep_arg == R_NilValue() {
            String::new()
        } else {
            elt_to_string(sep_arg, 0)
        };
        let sep = sep_text.chars().next();
        let header = !header_arg.is_null()
            && header_arg != R_NilValue()
            && real_or_default(header_arg, 0.0) != 0.0;
        let quote_text = if quote_arg.is_null() || quote_arg == R_NilValue() {
            "\"'".to_string()
        } else {
            elt_to_string(quote_arg, 0)
        };
        let comment_text = if comment_arg.is_null() || comment_arg == R_NilValue() {
            "#".to_string()
        } else {
            elt_to_string(comment_arg, 0)
        };
        let na_strings: Vec<String> = if na_strings_arg.is_null() || na_strings_arg == R_NilValue() {
            vec!["NA".to_string()]
        } else if TYPEOF(na_strings_arg) == SEXPTYPE::STRSXP {
            (0..XLENGTH(na_strings_arg))
                .map(|i| elt_to_string(na_strings_arg, i))
                .collect()
        } else {
            vec!["NA".to_string()]
        };
        let blank_lines_skip = blank_skip_arg.is_null()
            || blank_skip_arg == R_NilValue()
            || real_or_default(blank_skip_arg, 1.0) != 0.0;
        let fill = if fill_arg.is_null() || fill_arg == R_NilValue() {
            !blank_lines_skip
        } else {
            real_or_default(fill_arg, 0.0) != 0.0
        };
        let strip_white = if strip_white_arg.is_null() || strip_white_arg == R_NilValue() {
            sep.is_some()
        } else {
            real_or_default(strip_white_arg, if sep.is_some() { 1.0 } else { 0.0 }) != 0.0
        };
        let nrows: i64 = if nrows_arg.is_null() || nrows_arg == R_NilValue() {
            -1
        } else {
            real_or_default(nrows_arg, -1.0) as i64
        };
        let skip: usize = if skip_arg.is_null() || skip_arg == R_NilValue() {
            0
        } else {
            real_or_default(skip_arg, 0.0).max(0.0) as usize
        };

        let content: String = if !text_arg.is_null() && text_arg != R_NilValue() && TYPEOF(text_arg) == SEXPTYPE::STRSXP {
            (0..XLENGTH(text_arg))
                .map(|i| elt_to_string(text_arg, i))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            if file_arg.is_null() || file_arg == R_NilValue() || TYPEOF(file_arg) != SEXPTYPE::STRSXP {
                scan_error("invalid 'file' argument");
            }
            let file_path = resolve_package_relative_path(elt_to_string(file_arg, 0));
            match std::fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(e) => {
                    scan_error(format!("cannot open file '{file_path}': {e}"));
                }
            }
        };

        let spec = TableParseSpec {
            sep,
            quotes: quote_text.chars().collect(),
            comment: comment_text.chars().next(),
            strip_white,
            blank_lines_skip,
        };
        let mut records = parse_table_records(&content, &spec);
        if skip > 0 {
            let drop = skip.min(records.len());
            records.drain(0..drop);
        }
        if nrows >= 0 {
            records.truncate(nrows as usize);
        }

        if records.is_empty() {
            return R_NilValue();
        }

        let mut data: Vec<Vec<TableField>> = records;
        let mut col_names: Vec<String> = Vec::new();
        if header {
            let header_row = data.remove(0);
            col_names = header_row
                .iter()
                .map(|f| f.text.trim().to_string())
                .collect();
        } else if !col_names_arg.is_null()
            && col_names_arg != R_NilValue()
            && TYPEOF(col_names_arg) == SEXPTYPE::STRSXP
        {
            col_names = (0..XLENGTH(col_names_arg))
                .map(|i| elt_to_string(col_names_arg, i))
                .collect();
        }

        let mut ncols = col_names.len();
        for row in &data {
            ncols = ncols.max(row.len());
        }
        if ncols == 0 {
            return R_NilValue();
        }
        while col_names.len() < ncols {
            col_names.push(format!("V{}", col_names.len() + 1));
        }

        let mut padded: Vec<Vec<TableField>> = Vec::with_capacity(data.len());
        for (i, row) in data.iter().enumerate() {
            if row.len() > ncols {
                scan_error(format!(
                    "line {} did not have {} elements",
                    i + 1,
                    ncols
                ));
            }
            let mut row = row.clone();
            while row.len() < ncols {
                if !fill && header {
                    scan_error(format!(
                        "line {} did not have {} elements",
                        i + 1,
                        ncols
                    ));
                }
                row.push(TableField {
                    text: String::new(),
                    quoted: false,
                });
            }
            padded.push(row);
        }
        let data = padded;
        let nrow = data.len() as R_xlen_t;

        let classes = parse_table_col_classes(col_classes_arg, ncols);

        // Optional row.names: a single integer names the column to use,
        // a character vector supplies the names directly.
        let row_names_column: Option<usize> = if !row_names_arg.is_null() && row_names_arg != R_NilValue() {
            let row_names_type = TYPEOF(row_names_arg);
            if row_names_type == SEXPTYPE::INTSXP && XLENGTH(row_names_arg) == 1 {
                Some(INTEGER_ELT(row_names_arg, 0).unsigned_abs() as usize)
            } else if row_names_type == SEXPTYPE::REALSXP && XLENGTH(row_names_arg) == 1 {
                Some(REAL_ELT(row_names_arg, 0) as usize)
            } else {
                None
            }
        } else {
            None
        };

        let mut out_names: Vec<String> = Vec::new();
        let mut out_cols: Vec<SEXP> = Vec::new();
        for j in 0..ncols {
            let declared = classes[j];
            let fields: Vec<&TableField> = data.iter().map(|row| &row[j]).collect();
            let class = if declared == TableColClass::Infer {
                infer_table_col_class(&fields, &na_strings)
            } else {
                declared
            };
            if class == TableColClass::Null || Some(j + 1) == row_names_column {
                continue;
            }
            let col = match class {
                TableColClass::Logical => {
                    let col = Rf_allocVector3(SEXPTYPE::LGLSXP, nrow);
                    let _guard = protect(col);
                    for (i, field) in fields.iter().enumerate() {
                        let value = if table_field_is_na(field, &na_strings)
                            || table_field_is_blank(field)
                        {
                            NA_INTEGER
                        } else {
                            parse_table_logical(&field.text).unwrap_or(NA_INTEGER)
                        };
                        *LOGICAL(col).add(i) = value;
                    }
                    col
                }
                TableColClass::Integer => {
                    let col = Rf_allocVector3(SEXPTYPE::INTSXP, nrow);
                    let _guard = protect(col);
                    for (i, field) in fields.iter().enumerate() {
                        let value = if table_field_is_na(field, &na_strings)
                            || table_field_is_blank(field)
                        {
                            NA_INTEGER
                        } else {
                            field.text.trim().parse::<i32>().unwrap_or(NA_INTEGER)
                        };
                        *INTEGER(col).add(i) = value;
                    }
                    col
                }
                TableColClass::Numeric => {
                    let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow);
                    let _guard = protect(col);
                    for (i, field) in fields.iter().enumerate() {
                        let value = if table_field_is_na(field, &na_strings)
                            || table_field_is_blank(field)
                        {
                            NA_REAL
                        } else {
                            parse_table_double(&field.text).unwrap_or(NA_REAL)
                        };
                        *REAL(col).add(i) = value;
                    }
                    col
                }
                TableColClass::Character | TableColClass::Factor => {
                    let col = Rf_allocVector3(SEXPTYPE::STRSXP, nrow);
                    let _guard = protect(col);
                    for (i, field) in fields.iter().enumerate() {
                        let value = if table_field_is_na(field, &na_strings) {
                            crate::mainutils::relop::NA_STRING()
                        } else {
                            let cstr = CString::new(field.text.as_str()).unwrap_or_default();
                            crate::sexp::constructors::Rf_mkChar(cstr.as_ptr())
                        };
                        SET_STRING_ELT(col, i as R_xlen_t, value);
                    }
                    if class == TableColClass::Factor {
                        let mut levels: Vec<String> = fields
                            .iter()
                            .map(|f| f.text.clone())
                            .filter(|t| !na_strings.contains(t))
                            .collect();
                        levels.sort();
                        levels.dedup();
                        crate::mainutils::essentials::tables::set_factor_attrs(col, &levels);
                    }
                    col
                }
                TableColClass::Null | TableColClass::Infer => unreachable!(),
            };
            out_cols.push(col);
            out_names.push(col_names[j].clone());
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, out_cols.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, out_cols.len() as R_xlen_t);
        let _names_guard = protect(names_vec);
        for (j, col) in out_cols.iter().enumerate() {
            SET_VECTOR_ELT(result, j as R_xlen_t, *col);
            let cstr = CString::new(out_names[j].as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                SET_STRING_ELT(names_vec, j as R_xlen_t, charsxp);
            }
        }
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names_vec);

        let explicit_row_names = !row_names_arg.is_null()
            && row_names_arg != R_NilValue()
            && TYPEOF(row_names_arg) == SEXPTYPE::STRSXP
            && XLENGTH(row_names_arg) == nrow;
        if explicit_row_names {
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(c"row.names".as_ptr()),
                row_names_arg,
            );
        } else if let Some(k) = row_names_column
            && k >= 1
            && k <= ncols
        {
            let labels = Rf_allocVector3(SEXPTYPE::STRSXP, nrow);
            let _labels_guard = protect(labels);
            for i in 0..nrow {
                let cstr = CString::new(data[i as usize][k - 1].text.as_str()).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                SET_STRING_ELT(labels, i, charsxp);
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(c"row.names".as_ptr()),
                labels,
            );
        } else {
            crate::mainutils::essentials::functional::set_compact_row_names(result, nrow);
        }
        crate::mainutils::essentials::functional::set_data_frame_class(result);
        result
    }
}

// ---------------------------------------------------------------------------
// Complete I/O — European CSV, delimited, fixed-width
// ---------------------------------------------------------------------------

/// R's `read.csv2(file, ...)` — European CSV reader (semicolons as separator).
pub unsafe fn do_read_csv2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let file_path = resolve_package_relative_path(elt_to_string(file_arg, 0));

        // Read file
        let content = match std::fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {}", file_path, e);
                return R_NilValue();
            }
        };

        let mut lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return R_NilValue();
        }

        // Header from first line
        let header_line = lines.remove(0);
        let col_names: Vec<String> = header_line
            .split(';')
            .map(|s| s.trim().trim_matches('"').to_string())
            .collect();

        let ncols = col_names.len();
        if ncols == 0 {
            return R_NilValue();
        }

        // Parse data rows — European format uses comma as decimal separator
        let mut col_data: Vec<Vec<f64>> = vec![Vec::new(); ncols];
        for line in &lines {
            let fields: Vec<&str> = line.split(';').collect();
            for j in 0..ncols {
                let val = if j < fields.len() {
                    // Replace comma decimal with dot
                    let cleaned = fields[j].trim().replace(',', ".");
                    cleaned.parse::<f64>().unwrap_or(NA_REAL)
                } else {
                    NA_REAL
                };
                col_data[j].push(val);
            }
        }

        // Build list result (data.frame)
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
        let _p2 = protect(names_vec);

        for j in 0..ncols {
            let nrow = col_data[j].len();
            let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow as R_xlen_t);
            if !col.is_null() {
                let dst = REAL(col);
                for (i, &v) in col_data[j].iter().enumerate() {
                    *dst.add(i) = v;
                }
            }
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(j) = col;

            let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let nmdata = (*names_vec).gengc_next_node as *mut SEXP;
                *nmdata.add(j) = charsxp;
            }
        }

        // Set names
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names_vec);
        // Set class to data.frame
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _p3 = protect(class_vec);
        let cstr = c"data.frame";
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let cdata = (*class_vec).gengc_next_node as *mut SEXP;
            *cdata.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class_vec);
        result
    }
}

/// R's `write.csv2(x, file, ...)` — European CSV writer (semicolons, comma decimal).
pub unsafe fn do_write_csv2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let file_arg = CAR(CDR(args));
        let file_path = elt_to_string(file_arg, 0);

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let ncols = XLENGTH(x) as usize;

        // Get names if available
        let names_attr = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"names".as_ptr()));

        let mut out = String::new();
        if ncols == 0 {
            out.push_str("\"\"\n");
        }

        // Header
        if ncols > 0 && !names_attr.is_null() && names_attr != R_NilValue() {
            let mut headers: Vec<String> = Vec::new();
            for j in 0..ncols {
                let nm = elt_to_string(names_attr, j as R_xlen_t);
                headers.push(format!("\"{}\"", nm));
            }
            out.push_str(&headers.join(";"));
            out.push('\n');
        }

        let t = TYPEOF(x);
        // Data rows — dispatch on type exactly like do_write_csv:
        // only VECSXP payloads are column pointers; atomic vectors are
        // formatted as a single column; anything else is an error.
        if ncols > 0 {
            // TYPEOF returns c_int; only a VECSXP payload may be read as
            // column pointers. Atomic vectors format as a single column.
            let nrows = if t == SEXPTYPE::VECSXP {
                let data = (*x).gengc_next_node as *mut SEXP;
                let col = *data;
                if !col.is_null() {
                    XLENGTH(col) as usize
                } else {
                    0
                }
            } else if t == SEXPTYPE::REALSXP
                || t == SEXPTYPE::INTSXP
                || t == SEXPTYPE::LGLSXP
                || t == SEXPTYPE::STRSXP
            {
                XLENGTH(x) as usize
            } else {
                std::panic::panic_any(RError {
                    message: format!("cannot handle 'x' of type {:?}", t),
                });
            };

            for i in 0..nrows {
                let mut row: Vec<String> = Vec::new();
                for j in 0..ncols {
                    let val = if t == SEXPTYPE::VECSXP {
                        let data = (*x).gengc_next_node as *mut SEXP;
                        let col = *data.add(j);
                        if !col.is_null() {
                            elt_to_string(col, i as R_xlen_t)
                        } else {
                            "NA".to_string()
                        }
                    } else {
                        elt_to_string(x, i as R_xlen_t)
                    };
                    // Use comma as decimal separator for European format
                    let eu_val = val.replace('.', ",");
                    row.push(format!("\"{}\"", eu_val));
                }
                out.push_str(&row.join(";"));
                out.push('\n');
            }
        }

        // Write to file
        if let Err(e) = std::fs::write(&file_path, &out) {
            eprintln!("Error writing '{}': {}", file_path, e);
        }

        R_NilValue()
    }
}

/// R's `read.delim(file, ...)` — delimited file reader.
pub unsafe fn do_read_delim(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let sep_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };

        let file_path = resolve_package_relative_path(elt_to_string(file_arg, 0));
        let sep = if sep_arg.is_null() || sep_arg == R_NilValue() {
            "\t".to_string()
        } else {
            elt_to_string(sep_arg, 0)
        };

        // Read file
        let content = match std::fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {}", file_path, e);
                return R_NilValue();
            }
        };

        let mut lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return R_NilValue();
        }

        // Header
        let header_line = lines.remove(0);
        let col_names: Vec<String> = header_line
            .split(&sep)
            .map(|s| s.trim().to_string())
            .collect();

        let ncols = col_names.len();
        if ncols == 0 {
            return R_NilValue();
        }

        // Parse rows
        let mut col_data: Vec<Vec<f64>> = vec![Vec::new(); ncols];
        for line in &lines {
            let fields: Vec<&str> = line.split(&sep).collect();
            for j in 0..ncols {
                let val = if j < fields.len() {
                    fields[j].trim().parse::<f64>().unwrap_or(NA_REAL)
                } else {
                    NA_REAL
                };
                col_data[j].push(val);
            }
        }

        // Build data.frame result
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
        let _p2 = protect(names_vec);

        for j in 0..ncols {
            let nrow = col_data[j].len();
            let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow as R_xlen_t);
            if !col.is_null() {
                let dst = REAL(col);
                for (i, &v) in col_data[j].iter().enumerate() {
                    *dst.add(i) = v;
                }
            }
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(j) = col;

            let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let nmdata = (*names_vec).gengc_next_node as *mut SEXP;
                *nmdata.add(j) = charsxp;
            }
        }

        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names_vec);
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _p3 = protect(class_vec);
        let cstr = c"data.frame";
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let cdata = (*class_vec).gengc_next_node as *mut SEXP;
            *cdata.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class_vec);
        result
    }
}

/// R's `read.fwf(file, widths, ...)` — fixed-width file reader.
pub unsafe fn do_read_fwf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let widths_arg = CAR(CDR(args));

        let file_path = resolve_package_relative_path(elt_to_string(file_arg, 0));

        let nfields = XLENGTH(widths_arg);
        if nfields == 0 {
            // Upstream read.fwf computes its block widths via
            // rep_len(sep, length(first) - 1L); stock R attributes this
            // error to that call.
            let sym = |name: &str| {
                crate::sexp::symbol::Rf_install(
                    std::ffi::CString::new(name).unwrap_or_default().as_ptr(),
                )
            };
            let one_l = crate::sexp::constructors::Rf_ScalarInteger(1);
            let length_first = crate::sexp::constructors::Rf_lang2(sym("length"), sym("first"));
            let minus = crate::sexp::constructors::Rf_lang3(sym("-"), length_first, one_l);
            let _guards = protect(one_l);
            let _guard2 = protect(length_first);
            let _guard3 = protect(minus);
            let nil = R_NilValue();
            let tail = crate::sexp::constructors::Rf_cons(minus, nil);
            let _guard4 = protect(tail);
            let args2 = crate::sexp::constructors::Rf_cons(sym("sep"), tail);
            let _guard5 = protect(args2);
            let rep_len_call = crate::sexp::constructors::Rf_cons(sym("rep_len"), args2);
            if !rep_len_call.is_null() {
                (*rep_len_call)
                    .sxpinfo
                    .set_type(crate::sexp::ffi::SEXPTYPE::LANGSXP);
            }
            crate::mainutils::errors::errorcall_str(rep_len_call, "invalid 'length.out' value");
        }
        let mut widths: Vec<i64> = Vec::new();
        for i in 0..nfields {
            let w = if TYPEOF(widths_arg) == SEXPTYPE::REALSXP {
                let rp = REAL(widths_arg);
                *rp.add(i as usize) as i64
            } else if TYPEOF(widths_arg) == SEXPTYPE::INTSXP {
                let ip = INTEGER(widths_arg);
                *ip.add(i as usize) as i64
            } else {
                1_i64
            };
            widths.push(w);
        }

        // Read file
        let content = match std::fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {}", file_path, e);
                return R_NilValue();
            }
        };

        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return R_NilValue();
        }

        let ncols = widths.iter().filter(|&&width| width >= 0).count();
        let nrows = lines.len();

        // Parse fixed-width fields
        let mut col_data: Vec<Vec<f64>> = vec![vec![NA_REAL; nrows]; ncols];
        for (i, line) in lines.iter().enumerate() {
            let mut pos = 0usize;
            let mut out_col = 0usize;
            for &width in &widths {
                let span = width.unsigned_abs() as usize;
                if width < 0 {
                    pos = pos.saturating_add(span);
                    continue;
                }
                if span > 0 && pos + span <= line.len() {
                    let field = &line[pos..pos + span];
                    col_data[out_col][i] = field.trim().parse::<f64>().unwrap_or(NA_REAL);
                }
                pos = pos.saturating_add(span);
                out_col += 1;
            }
        }

        // Build data.frame
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
        let _p2 = protect(names_vec);

        for j in 0..ncols {
            let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrows as R_xlen_t);
            if !col.is_null() {
                let dst = REAL(col);
                for (i, &v) in col_data[j].iter().enumerate() {
                    *dst.add(i) = v;
                }
            }
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(j) = col;

            let cstr = CString::new(format!("V{}", j + 1)).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let nmdata = (*names_vec).gengc_next_node as *mut SEXP;
                *nmdata.add(j) = charsxp;
            }
        }

        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names_vec);
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _p3 = protect(class_vec);
        let cstr = c"data.frame";
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let cdata = (*class_vec).gengc_next_node as *mut SEXP;
            *cdata.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class_vec);
        result
    }
}

/// R's `readChar(con, nchars)` — read characters from connection.
pub unsafe fn do_readChar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let con_arg = CAR(args);
        let nchars_arg = CAR(CDR(args));
        let nchars = real_or_default(nchars_arg, -1.0) as i64;

        if inherits_class(con_arg, "connection") {
            let connection = connection_index(con_arg);
            let text = read_chars_from_connection(connection, nchars);
            return Rf_mkString(CString::new(text).unwrap_or_default().as_ptr());
        }

        let path = elt_to_string(con_arg, 0);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| {
            base_error(format!("cannot read file '{}': {}", path, e));
        });
        let take = if nchars >= 0 {
            (nchars as usize).min(bytes.len())
        } else {
            bytes.len()
        };
        let result = String::from_utf8_lossy(&bytes[..take]).into_owned();
        Rf_mkString(CString::new(result).unwrap_or_default().as_ptr())
    }
}

unsafe fn read_chars_from_connection(connection: c_int, nchars: i64) -> String {
    let mut bytes = Vec::new();
    if nchars >= 0 {
        for _ in 0..nchars {
            let byte = crate::mainutils::connections::connection_fgetc(connection);
            if byte < 0 {
                break;
            }
            bytes.push(byte as u8);
        }
    } else {
        loop {
            let byte = crate::mainutils::connections::connection_fgetc(connection);
            if byte < 0 {
                break;
            }
            bytes.push(byte as u8);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// R's `writeChar(object, con, nchars)` — write characters to connection.
pub unsafe fn do_writeChar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let object_arg = CAR(args);
        let con_arg = CAR(CDR(args));
        let nchars_arg = CAR(CDR(CDR(args)));
        let eos_arg = CAR(CDR(CDR(CDR(args))));

        let mut text = elt_to_string(object_arg, 0);
        let nchars = real_or_default(nchars_arg, text.len() as f64) as i64;
        if nchars >= 0 && (nchars as usize) < text.len() {
            text.truncate(nchars as usize);
        }
        if !eos_arg.is_null() && eos_arg != R_NilValue() && TYPEOF(eos_arg) == SEXPTYPE::STRSXP {
            text.push_str(&elt_to_string(eos_arg, 0));
        }

        if inherits_class(con_arg, "connection") {
            let connection = connection_index(con_arg);
            crate::mainutils::connections::connection_write_bytes(connection, text.as_bytes());
        } else {
            let path = elt_to_string(con_arg, 0);
            if let Err(e) = std::fs::write(&path, text.as_bytes()) {
                base_error(format!("cannot write file '{}': {}", path, e));
            }
        }

        R_NilValue()
    }
}

unsafe fn connection_index(con: SEXP) -> c_int {
    unsafe {
        if con.is_null()
            || con == R_NilValue()
            || TYPEOF(con) != SEXPTYPE::INTSXP
            || LENGTH(con) < 1
        {
            base_error("invalid connection");
        }
        *INTEGER(con)
    }
}
