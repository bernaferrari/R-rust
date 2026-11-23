#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/source.c — parse() internal function.
//!
//! Provides do_parse() for .Internal(parse(...)).

use std::os::raw::c_int;
use std::ptr;

use crate::sexp::accessors::{
    CAR, CDR, SET_STRING_ELT, SET_VECTOR_ELT, STRING_ELT, TAG, TYPEOF, XLENGTH,
};
use crate::sexp::constructors::{Rf_allocVector3, mk_char_str};
use crate::sexp::context::RError;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NaString, R_NilValue};
use crate::sexp::instance::{
    PARSE_ERROR_BYTES, with_current_instance, with_parse_diagnostics,
    with_parse_diagnostics_in,
};
use crate::sexp::protect::protect;

/// Parse R expressions.
///
/// This is the equivalent of R's `do_parse()` from source.c.
///
/// .Internal( parse(file, n, text, prompt, srcfile, encoding) )
fn keep_source_enabled(args: SEXP) -> bool {
    let mut p = args;
    while !p.is_null() && p != R_NilValue() {
        if crate::mainutils::essentials::tag_name(p).as_deref() == Some("keep.source") {
            return crate::mainutils::coerce::asLogical(CAR(p)) == 1;
        }
        p = CDR(p);
    }
    let opt =
        crate::mainutils::options::GetOption1(crate::sexp::symbol::install_str("keep.source"));
    !opt.is_null() && crate::mainutils::coerce::asLogical(opt) == 1
}

pub fn do_parse(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    reset_parse_state();
    // `file=` (by name or as the lone positional): read the file and
    // parse its content with strict file-parse semantics.
    let file = file_arg(args);
    if !file.is_null() && file != R_NilValue() {
        if crate::mainutils::connections::inherits_class(file, "connection") {
            let index = crate::mainutils::connections::checked_connection_index(
                crate::mainutils::connections::as_integer(file),
            );
            let mut content = String::new();
            {
                let state = crate::mainutils::connections::connection_table();
                let mut table = state.borrow_mut();
                let Some(conn) = table[index].as_mut() else {
                    drop(table);
                    parse_failure("invalid connection");
                };
                while let Some(line) =
                    crate::mainutils::connections::read_pushback_line(conn, false)
                {
                    content.push_str(&line);
                    content.push('\n');
                }
                if let Some(reader) = conn.reader.as_mut() {
                    use std::io::Read;
                    let _ = reader.read_to_string(&mut content);
                }
            }
            remember_parse_context(&content);
            return parse_content_to_exprs(&content);
        }
        let path = local_elt_to_string(file, 0);
        if !path.is_empty() {
            match crate::mainutils::browser_files::read_text_or_host(&path) {
                Ok(content) => {
                    remember_parse_context(&content);
                    if keep_source_enabled(args) {
                        return crate::mainutils::essentials::parse_with_srcrefs(
                            &content, &path,
                        );
                    }
                    return parse_content_to_exprs(&content);
                }
                Err(err) => {
                    parse_failure(format!("cannot open file '{path}': {err}"));
                }
            }
        }
    }
    let text = parse_text_arg(args);
    if text.is_null() || text == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
    }
    if TYPEOF(text) != SEXPTYPE::STRSXP {
        parse_failure("invalid 'text' argument");
    }

    // Multi-element `text` is ONE source stream: upstream concatenates
    // the vector with newlines before parsing (a function split across
    // elements parses as a whole). Join first, then strict-parse once.
    let n = XLENGTH(text);
    let mut combined = String::new();
    for i in 0..n {
        let elt = STRING_ELT(text, i);
        if elt == R_NaString() {
            parse_failure("invalid 'text' argument");
        }
        let source =
            String::from_utf8_lossy(&crate::sexp::accessors::char_cstr_bytes(elt)).into_owned();
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&source);
    }
    if combined.trim().is_empty() {
        return Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
    }
    remember_parse_context(&combined);
    return if keep_source_enabled(args) {
        crate::mainutils::essentials::parse_with_srcrefs(&combined, "<text>")
    } else {
        parse_content_to_exprs(&combined)
    };
}

// Parse whole-file content with strict file-parse semantics,
// shared by the `file=` path below.
fn parse_content_to_exprs(content: &str) -> SEXP {
    let owner = crate::sexp::owner::OwnerToken::current()
        .expect("parsing requires an active session");
    let parser_factory = owner.node_factory();
    let parsed = owner
        .with_arena(|arena| {
            crate::eval::parser::parse_expressions_strict(content, arena, parser_factory)
                .map_err(|err| err.to_string())
        })
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    match parsed {
        Ok(exprs) => {
            let result = Rf_allocVector3(SEXPTYPE::EXPRSXP, exprs.len() as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for (i, expr) in exprs.iter().enumerate() {
                SET_VECTOR_ELT(result, i as R_xlen_t, expr.clone().as_raw());
            }
            result
        }
        Err(message) => {
            parse_failure(message);
        }
    }
}

// Local element-to-string (mirrors essentials' helper without
// reaching into its private `shared` module).
fn local_elt_to_string(x: SEXP, i: R_xlen_t) -> String {
    if x.is_null() || x == R_NilValue() {
        return String::new();
    }
    if TYPEOF(x) != SEXPTYPE::STRSXP {
        return String::new();
    }
    let elt = STRING_ELT(x, i);
    if elt.is_null() {
        return String::new();
    }
    String::from_utf8_lossy(&crate::sexp::accessors::char_cstr_bytes(elt)).into_owned()
}

// Compare a tag symbol's printed name without reaching into private modules.
fn tag_name_is(tag: SEXP, want: &str) -> bool {
    if tag.is_null() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
        return false;
    }
    crate::sexp::accessors::char_cstr_bytes_opt(crate::sexp::accessors::PRINTNAME(tag))
        .is_some_and(|bytes| bytes == want.as_bytes())
}

// `file` by name, or the lone positional string when it names a
// readable file (parse(file=...) / parse(path)).
fn file_arg(args: SEXP) -> SEXP {
    let mut cell = args;
    while !cell.is_null() && cell != R_NilValue() {
        let tag = TAG(cell);
        if !tag.is_null() && tag_name_is(tag, "file") {
            return CAR(cell);
        }
        cell = CDR(cell);
    }
    // Lone positional string that names a readable file. A tagged
    // argument (e.g. parse(text="1")) matched its formal by name and
    // must never fall into the positional `file` slot.
    if !args.is_null()
        && args != R_NilValue()
        && (CDR(args).is_null() || CDR(args) == R_NilValue())
        && (TAG(args).is_null() || TAG(args) == R_NilValue())
    {
        let first = CAR(args);
        if TYPEOF(first) == SEXPTYPE::STRSXP {
            let path = local_elt_to_string(first, 0);
            if !path.is_empty() && std::path::Path::new(&path).is_file() {
                return first;
            }
        }
    }
    R_NilValue()
}

fn parse_text_arg(args: SEXP) -> SEXP {
    {
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }
        let after_file = CDR(args);
        if after_file.is_null() || after_file == R_NilValue() {
            return CAR(args);
        }
        let after_n = CDR(after_file);
        if after_n.is_null() || after_n == R_NilValue() {
            return CAR(args);
        }
        CAR(after_n)
    }
}

fn parse_input_loc(message: &str) -> (c_int, c_int) {
    let Some(start) = message.rfind("(<input>:") else {
        return (1, 1);
    };
    let rest = &message[start + "(<input>:".len()..];
    let Some((nums, _)) = rest.split_once(')') else {
        return (1, 1);
    };
    let mut parts = nums.split(':');
    let line = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let col = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    (line, col)
}

fn parse_failure(message: impl Into<String>) -> ! {
    let message = message.into();
    let (line, col) = parse_input_loc(&message);
    store_parse_error(&message, 1, col.max(1), R_GetParseErrorFile());
    let c_msg = std::ffi::CString::new(message.as_str()).unwrap_or_default();
    let cond = crate::mainutils::errors::R_makeErrorCondition(
        R_NilValue(),
        Some(c"parseError"),
        None,
        2,
        Some(&c_msg),
    );
    let _guard = protect(cond);
    let lineno = crate::sexp::constructors::Rf_ScalarInteger(line);
    let colno = crate::sexp::constructors::Rf_ScalarInteger(col);
    crate::mainutils::errors::R_setConditionField(cond, 2, c"lineno", lineno);
    crate::mainutils::errors::R_setConditionField(cond, 3, c"colno", colno);
    let condition = parse_runtime_value(cond);
    drop(crate::mainutils::errors::with_error_state(|state| {
        std::mem::replace(&mut state.signalled_condition, condition)
    }));
    std::panic::panic_any(RError { message });
}

pub(crate) fn reset_parse_state() {
    drop(with_parse_diagnostics(|state, buffer| {
        buffer.set([0; PARSE_ERROR_BYTES]);
        state.status = 0;
        state.column = 0;
        state.context_line = 0;
        state.context.clear();
        std::mem::take(&mut state.file)
    }));
}

pub(crate) fn remember_parse_context(source: &str) {
    with_parse_diagnostics(|state, _| {
        state.context.clear();
        state.context.extend(source.lines().map(str::to_owned));
        if state.context.is_empty() {
            state.context.push(String::new());
        }
        state.context_line = state.context.len() as c_int;
    });
}

/// Admit a parse-state value into session storage the way the original raw
/// `RuntimeValue::from_raw_in` did: null becomes empty, and any live owner
/// (including a borrowed fixture) may own immutable singletons.
fn parse_runtime_value(value: SEXP) -> crate::sexp::instance::RuntimeValue {
    use crate::sexp::{instance::RuntimeValue, owner::OwnerToken};
    if value.is_null() {
        return RuntimeValue::empty();
    }
    let value = OwnerToken::current()
        .and_then(|owner| owner.sexp(value))
        .and_then(|value| value.into_owned())
        .unwrap_or_else(|error| {
            crate::sexp::context::r_error(format!("invalid context value: {error}"))
        });
    RuntimeValue::from_owned(value)
}

pub(crate) fn store_parse_error(message: &str, status: c_int, col: c_int, file: SEXP) {
    with_current_instance(|instance| {
        let file = parse_runtime_value(file);
        // SAFETY: the ambient accessor retains the owner; no R code runs under the borrow.
        let displaced = unsafe {
            with_parse_diagnostics_in(instance, |state, buffer| {
                let mut msg = [0u8; PARSE_ERROR_BYTES];
                let bytes = message.as_bytes();
                let copy_len = bytes
                    .iter()
                    .position(|b| *b == 0)
                    .unwrap_or(bytes.len())
                    .min(msg.len() - 1);
                msg[..copy_len].copy_from_slice(&bytes[..copy_len]);
                buffer.set(msg);
                state.status = status;
                state.column = col.max(0);
                std::mem::replace(&mut state.file, file)
            })
        };
        drop(displaced);
    });
}

fn current_parse_error_message() -> String {
    with_parse_diagnostics(|_, buffer| buffer.get())
        .map(|bytes| {
            let len = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
            String::from_utf8_lossy(&bytes[..len]).into_owned()
        })
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| "parse error".to_string())
}

/// Raise a parse error using the current per-session parser context.
pub fn parseError(call: SEXP, linenum: c_int) {
    let message = current_parse_error_message();
    store_parse_error(&message, 1, R_GetParseErrorCol(), call);
    if linenum > 0 {
        with_parse_diagnostics(|state, _| state.context_line = linenum);
    }
    std::panic::panic_any(RError {
        message: format_parse_error_message(&message, linenum),
    });
}

/// Return the most recent parser context as a character vector.
pub fn getParseContext() -> SEXP {
    let context = with_parse_diagnostics(|state, _| state.context.clone()).unwrap_or_default();
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, context.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _guard = protect(result);
    for (index, line) in context.iter().enumerate() {
        let charsxp = mk_char_str(line);
        SET_STRING_ELT(result, index as R_xlen_t, charsxp);
    }
    result
}

fn format_parse_error_message(message: &str, linenum: c_int) -> String {
    let (lines, context_line, col) = with_parse_diagnostics(|state, _| {
        (
            state.context.clone(),
            state.context_line,
            state.column,
        )
    })
    .unwrap_or_default();
    if linenum > 0 && lines.is_empty() {
        return format!("{linenum}:{col}: {message}");
    }
    match lines.as_slice() {
        [] => message.to_string(),
        [line] if linenum > 0 => format!("{linenum}:{col}: {message}\n{context_line}: {line}"),
        [line] => format!("{message} in \"{line}\""),
        lines if linenum > 0 => {
            let prev = &lines[lines.len() - 2];
            let last = &lines[lines.len() - 1];
            format!(
                "{linenum}:{col}: {message}\n{}: {prev}\n{context_line}: {last}",
                context_line - 1
            )
        }
        lines => format!(
            "{message} in:\n\"{}\n{}\"",
            lines[lines.len() - 2],
            lines[lines.len() - 1]
        ),
    }
}

/// Parse context buffer size.
pub const PARSE_CONTEXT_SIZE: c_int = 256;

pub fn R_GetParseError() -> c_int {
    with_parse_diagnostics(|state, _| state.status).unwrap_or(0)
}

pub fn R_SetParseError(val: c_int) {
    with_parse_diagnostics(|state, _| state.status = val);
}

pub fn R_GetParseErrorCol() -> c_int {
    with_parse_diagnostics(|state, _| state.column).unwrap_or(0)
}

pub(crate) fn parse_error_file_owner() -> Option<crate::sexp::object::Sexp<'static>> {
    with_parse_diagnostics(|state, _| state.file.owned()).flatten()
}

pub fn R_GetParseErrorFile() -> SEXP {
    with_parse_diagnostics(|state, _| state.file.as_raw()).unwrap_or(ptr::null_mut())
}

pub fn R_GetParseContextLine() -> c_int {
    with_parse_diagnostics(|state, _| state.context_line).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_do_parse_stub() {
        let _session = crate::sexp::session::RSession::new();
        let result = do_parse(
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        assert_eq!(TYPEOF(result), SEXPTYPE::EXPRSXP);
        assert_eq!(XLENGTH(result), 0);
    }

    #[test]
    fn test_do_parse_text_arg() {
        let _session = crate::sexp::session::RSession::new_for_gc_tests();
        let text = crate::sexp::constructors::mk_string_str("1 + 2");
        let n = crate::sexp::constructors::Rf_ScalarInteger(-1);
        let args = crate::sexp::constructors::Rf_cons(
            R_NilValue(),
            crate::sexp::constructors::Rf_cons(
                n,
                crate::sexp::constructors::Rf_cons(text, R_NilValue()),
            ),
        );
        let mut cell = args;
        for _ in 0..3 {
            crate::sexp::accessors::SETTAG(cell, R_NilValue());
            cell = CDR(cell);
        }
        let result = do_parse(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert_eq!(TYPEOF(result), SEXPTYPE::EXPRSXP);
        assert_eq!(XLENGTH(result), 1);
    }

    #[test]
    fn test_parse_error_state() {
        let _session = crate::sexp::session::RSession::new();
        R_SetParseError(42);
        assert_eq!(R_GetParseError(), 42);
    }

    #[test]
    fn test_do_parse_records_error_message_and_context() {
        let _session = crate::sexp::session::RSession::new_for_gc_tests();
        unsafe {
            let text = crate::sexp::constructors::mk_string_str("1 +");
            let n = crate::sexp::constructors::Rf_ScalarInteger(-1);
            let args = crate::sexp::constructors::Rf_cons(
                R_NilValue(),
                crate::sexp::constructors::Rf_cons(
                    n,
                    crate::sexp::constructors::Rf_cons(text, R_NilValue()),
                ),
            );
            let mut cell = args;
            for _ in 0..3 {
                crate::sexp::accessors::SETTAG(cell, R_NilValue());
                cell = CDR(cell);
            }
            let err = std::panic::catch_unwind(|| {
                do_parse(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            });
            assert!(err.is_err());
            assert_eq!(R_GetParseError(), 1);
            assert_eq!(R_GetParseErrorCol(), 1);
            assert!(
                std::ffi::CStr::from_ptr(crate::mainutils::main::R_GetParseErrorMsg())
                    .to_string_lossy()
                    .contains("unexpected end of input")
            );

            let context = getParseContext();
            assert_eq!(TYPEOF(context), SEXPTYPE::STRSXP);
            assert_eq!(XLENGTH(context), 1);
            assert_eq!(
                std::str::from_utf8(&crate::sexp::accessors::char_cstr_bytes(STRING_ELT(
                    context, 0
                ))),
                Ok("1 +")
            );
        }
    }

    #[test]
    fn test_parse_error_panics_with_context() {
        let _session = crate::sexp::session::RSession::new();
        remember_parse_context("x <- 1\ny +");
        store_parse_error("unexpected token", 2, 3, ptr::null_mut());
        let err = std::panic::catch_unwind(|| {
            parseError(ptr::null_mut(), 2);
        });
        assert!(err.is_err());
        assert_eq!(R_GetParseError(), 1);
        assert_eq!(R_GetParseContextLine(), 2);
    }

    #[test]
    fn test_parse_message_pointer_survives_diagnostic_updates() {
        use crate::mainutils::main::R_GetParseErrorMsg;
        use crate::sexp::session::RSession;
        use std::ffi::CStr;
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            reset_parse_state();
            store_parse_error("old\0ignored", 1, -1, ptr::null_mut());
            let original = R_GetParseErrorMsg();
            assert_eq!(CStr::from_ptr(original).to_bytes(), b"old");
            assert_eq!(R_GetParseErrorCol(), 0);
            remember_parse_context("x\ny");
            R_SetParseError(2);
            assert_eq!(R_GetParseContextLine(), 2);
            assert_eq!(CStr::from_ptr(original).to_bytes(), b"old");
            store_parse_error(&"x".repeat(300), 3, 7, ptr::null_mut());
            assert_eq!(R_GetParseErrorMsg(), original);
            assert_eq!(CStr::from_ptr(original).to_bytes(), &[b'x'; 255]);
            assert_eq!(R_GetParseErrorCol(), 7);
            reset_parse_state();
            assert_eq!(R_GetParseErrorMsg(), original);
            assert!(CStr::from_ptr(original).to_bytes().is_empty());
            assert_eq!(R_GetParseContextLine(), 0);
            assert_eq!(R_GetParseError(), 0);
        });
    }
}
