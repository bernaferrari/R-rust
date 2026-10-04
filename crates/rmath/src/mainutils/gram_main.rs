#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/gram.c — parser entry points.
//!
//! The original C parser is generated from `gram.y`. This Rust port routes the
//! C-shaped parser entry points through the hand-written Rust parser used by
//! session evaluation so legacy callers observe the same syntax support.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

use crate::sexp::accessors::{CHAR, STRING_ELT, TYPEOF, XLENGTH};
#[cfg(test)]
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NaString, R_NilValue};
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::object::{NodeDomain, Sexp, SexpMut};
use crate::sexp::owner::{OwnerToken, RuntimeAccess, with_runtime};
#[cfg(test)]
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// Parse status constants
// ---------------------------------------------------------------------------

pub const PARSE_OK: c_int = 1;
pub const PARSE_INCOMPLETE: c_int = 2;
pub const PARSE_ERROR: c_int = 4;
pub const PARSE_EOF: c_int = 8;

// ---------------------------------------------------------------------------
// R_ParseVector — parse text into R expressions
// ---------------------------------------------------------------------------

/// Parse a character vector of R code into a list of expressions.
pub unsafe fn R_ParseVector(text: SEXP, n: c_int, status: *mut c_int, _srcfile: SEXP) -> SEXP {
    unsafe {
        with_parse_runtime(|access| match parse_vector(text, n, access) {
            Ok(exprs) => {
                let result = exprs_to_exprsxp(exprs, access);
                set_parse_status(status, PARSE_OK);
                result
            }
            Err(_) => {
                let result = exprs_to_exprsxp(Vec::new(), access);
                set_parse_status(status, PARSE_ERROR);
                result
            }
        })
    }
}

// ---------------------------------------------------------------------------
// R_ParseEvalString — parse and evaluate a string
// ---------------------------------------------------------------------------

/// Parse and evaluate a string of R code.
pub unsafe fn R_ParseEvalString(s: *const c_char, envir: SEXP) -> SEXP {
    unsafe {
        let source = c_string_source(s).unwrap_or_else(|| parse_failure("invalid parse string"));
        parse_eval_source(&source, envir)
    }
}

// ---------------------------------------------------------------------------
// R_ParseEval — parse and evaluate a string with completion handler
// ---------------------------------------------------------------------------

/// Parse and evaluate a string of R code with completion.
pub unsafe fn R_ParseEval(s: *const c_char, envir: SEXP) -> SEXP {
    unsafe { R_ParseEvalString(s, envir) }
}

// ---------------------------------------------------------------------------
// R_ParseEvalBuffer — parse and evaluate a buffer
// ---------------------------------------------------------------------------

/// Parse and evaluate a buffer of R code.
pub unsafe fn R_ParseEvalBuffer(buf: *const c_char, len: c_int, envir: SEXP) -> SEXP {
    unsafe {
        let source =
            buffer_source(buf, len).unwrap_or_else(|| parse_failure("invalid parse buffer"));
        parse_eval_source(&source, envir)
    }
}

// ---------------------------------------------------------------------------
// R_CurrentParseLine — current parse line number
// ---------------------------------------------------------------------------

/// Get the current parse line number.
pub unsafe fn R_CurrentParseLine() -> c_int {
    unsafe { crate::mainutils::source::R_GetParseContextLine() }
}

// ---------------------------------------------------------------------------
// R_ParseFilename — get the current parse filename
// ---------------------------------------------------------------------------

/// Get the current parse filename.
pub unsafe fn R_ParseFilename() -> *const c_char {
    unsafe {
        let instance = crate::sexp::instance::with_required_current_instance(|instance| instance);
        let owner_pin = crate::sexp::context::pin_context_owner_in(instance);
        let file_owner = crate::mainutils::source::parse_error_file_owner();
        let file = file_owner
            .as_ref()
            .map_or(std::ptr::null_mut(), |value| value.as_raw());
        if !file.is_null()
            && file != R_NilValue()
            && TYPEOF(file) == SEXPTYPE::STRSXP
            && XLENGTH(file) > 0
        {
            let charsxp = STRING_ELT(file, 0);
            crate::sexp::context::require_context_owner_live(owner_pin.as_ref());
            let chars_owner = crate::sexp::context::own_control_value(charsxp);
            let charsxp = chars_owner.as_raw();
            if !charsxp.is_null() && charsxp != R_NaString() {
                let value = CHAR(charsxp);
                if !value.is_null() {
                    return value;
                }
            }
        }
    }
    static EMPTY: [c_char; 1] = [0];
    EMPTY.as_ptr()
}

// ---------------------------------------------------------------------------
// R_ParseContext — parse context management
// ---------------------------------------------------------------------------

/// Enter a new parse context.
pub unsafe fn R_ParseContext(buf: *const c_char, len: c_int) -> c_int {
    unsafe {
        let Some(source) = buffer_source(buf, len) else {
            crate::mainutils::source::store_parse_error(
                "invalid parse context buffer",
                PARSE_ERROR,
                0,
                R_NilValue(),
            );
            return PARSE_ERROR;
        };
        crate::mainutils::source::remember_parse_context(&source);
    }
    0
}

/// End the current parse context.
pub unsafe fn R_ParseContextEnd() {}

// ---------------------------------------------------------------------------
// R_ParseVectorBuffer — parse from buffer
// ---------------------------------------------------------------------------

/// Parse from a character buffer.
pub unsafe fn R_ParseVectorBuffer(
    text: *const c_char,
    len: R_xlen_t,
    n: c_int,
    status: *mut c_int,
    _srcfile: SEXP,
) -> SEXP {
    unsafe {
        with_parse_runtime(|access| {
            let Some(source) = c_int::try_from(len)
                .ok()
                .and_then(|length| buffer_source(text, length))
            else {
                let result = exprs_to_exprsxp(Vec::new(), access);
                set_parse_status(status, PARSE_ERROR);
                return result;
            };
            match parse_source_list(std::iter::once(Ok(source)), n, access.domain(), access) {
                Ok(exprs) => {
                    let result = exprs_to_exprsxp(exprs, access);
                    set_parse_status(status, PARSE_OK);
                    result
                }
                Err(_) => {
                    let result = exprs_to_exprsxp(Vec::new(), access);
                    set_parse_status(status, PARSE_ERROR);
                    result
                }
            }
        })
    }
}

unsafe fn set_parse_status(status: *mut c_int, value: c_int) {
    unsafe {
        if !status.is_null() {
            *status = value;
        }
    }
}

/// Native entry: retain the exact managed runtime throughout parser callbacks.
/// No execution authority or arena loan escapes the closed operation.
unsafe fn with_parse_runtime<T>(
    operation: impl for<'execution> FnOnce(&'execution RuntimeAccess) -> T,
) -> T {
    let token = unsafe { OwnerToken::current() }
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let owner = token.weak_owner().unwrap_or_else(|| {
        crate::sexp::context::r_error("parsing requires a managed runtime")
    });
    with_runtime(&owner, operation)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

unsafe fn parse_vector<'session>(
    text: SEXP,
    n: c_int,
    access: &RuntimeAccess,
) -> Result<Vec<Sexp<'session>>, String> {
    unsafe {
        let domain = access.domain();
        if text.is_null() || text == domain.nil().as_raw() {
            return Ok(Vec::new());
        }
        let text_owner = domain.wrap(text).map_err(|error| error.to_string())?;
        if TYPEOF(text_owner.as_raw()) != SEXPTYPE::STRSXP {
            return Err("parse input must be a character vector".to_string());
        }

        let len = XLENGTH(text_owner.as_raw());
        access.require_active().map_err(|error| error.to_string())?;
        let mut sources = Vec::new();
        for i in 0..len {
            let elt = STRING_ELT(text_owner.as_raw(), i);
            access.require_active().map_err(|error| error.to_string())?;
            if elt == R_NaString() || elt.is_null() {
                return Err("parse input contains NA".to_string());
            }
            let element = domain.wrap(elt).map_err(|error| error.to_string())?;
            let bytes = CHAR(element.as_raw());
            if bytes.is_null() {
                return Err("parse input contains an invalid string".to_string());
            }
            sources.push(Ok(CStr::from_ptr(bytes).to_string_lossy().into_owned()));
        }

        parse_source_list(sources.into_iter(), n, domain, access)
    }
}

fn parse_source_list<'session, I>(
    sources: I,
    n: c_int,
    domain: NodeDomain<'session>,
    access: &RuntimeAccess,
) -> Result<Vec<Sexp<'session>>, String>
where
    I: Iterator<Item = Result<String, String>>,
{
    let limit = if n > 0 { Some(n as usize) } else { None };
    let mut combined = String::new();
    for source in sources {
        let line = source?;
        if line.trim().is_empty() {
            continue;
        }
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&line);
    }
    if combined.trim().is_empty() {
        return Ok(Vec::new());
    }

    crate::mainutils::source::remember_parse_context(&combined);
    let mut exprs = access
        .with_arena(|arena| {
            crate::eval::parser::parse_expressions(&combined, arena, domain)
                .map_err(|err| err.to_string())
        })
        .map_err(|error| error.to_string())??;
    crate::eval::parser::flush_literal_warnings();
    access.require_active().map_err(|error| error.to_string())?;
    if let Some(limit) = limit {
        exprs.truncate(limit);
    }
    Ok(exprs)
}

fn parse_one_source<'session>(
    source: &str,
    access: &RuntimeAccess,
) -> Result<Sexp<'session>, String> {
    crate::mainutils::source::remember_parse_context(source);
    let domain = access.domain();
    let parsed = access
        .with_arena(|arena| {
            crate::eval::parser::parse(source, arena, domain).map_err(|err| err.to_string())
        })
        .map_err(|error| error.to_string())?;
    crate::eval::parser::flush_literal_warnings();
    access.require_active().map_err(|error| error.to_string())?;
    parsed
}

fn exprs_to_exprsxp(exprs: Vec<Sexp<'_>>, access: &RuntimeAccess) -> SEXP {
    let domain = access.domain();
    let length = R_xlen_t::try_from(exprs.len())
        .unwrap_or_else(|_| parse_failure("too many parsed expressions"));
    let allocator = access
        .allocator(&domain)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let result = allocator
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::EXPRSXP, length)))
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let mut result = SexpMut::try_from_checked(result)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for (i, expr) in exprs.iter().enumerate() {
        result
            .try_set_vector_elt(
                i as R_xlen_t,
                domain
                    .wrap(expr.as_raw())
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
            )
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    access
        .require_active()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    result.as_raw()
}

unsafe fn parse_eval_source(source: &str, envir: SEXP) -> SEXP {
    unsafe {
        with_parse_runtime(|access| {
            let rho = if envir.is_null() || envir == R_NilValue() {
                with_required_current_instance(|instance| (*instance).global_env)
            } else {
                envir
            };
            // Parsing can emit warnings with arbitrary runtime callbacks.
            // Capture the environment before any parser callback runs.
            let environment = access
                .domain()
                .wrap(rho)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let parsed = parse_one_source(source, access);
            access
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let expr = parsed.unwrap_or_else(|message| parse_failure(message));
            let raw = crate::eval::eval::Rf_eval(expr.as_raw(), environment.as_raw());
            access
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let result = access
                .domain()
                .wrap(raw)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            result.as_raw()
        })
    }
}

unsafe fn c_string_source(s: *const c_char) -> Option<String> {
    unsafe {
        if s.is_null() {
            None
        } else {
            Some(CStr::from_ptr(s).to_string_lossy().into_owned())
        }
    }
}

unsafe fn buffer_source(buf: *const c_char, len: c_int) -> Option<String> {
    unsafe {
        if buf.is_null() || len < 0 {
            return None;
        }
        let bytes = std::slice::from_raw_parts(buf.cast::<u8>(), len as usize);
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn parse_failure(message: impl Into<String>) -> ! {
    let message = message.into();
    unsafe {
        crate::mainutils::source::store_parse_error(&message, 1, 1, R_NilValue());
    }
    std::panic::panic_any(RError { message });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ffi::{CStr, CString};
    use std::ptr;

    use crate::sexp::accessors::{REAL, TYPEOF, VECTOR_ELT, XLENGTH};

    use super::*;

    #[test]
    fn owned_parser_publication_survives_reentrant_collecting_callbacks() {
        use crate::sexp::object::SessionNodeFactory;
        use std::cell::Cell;
        use std::rc::Rc;
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let text = factory.strings(&["1; 2"]).unwrap();
        let raw_text = text.as_raw();
        let text_node = crate::sexp::memory::checked_projection(raw_text).unwrap().1;
        let observed = text_node.clone();
        let notifications = Rc::new(Cell::new(0));
        let calls = notifications.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            calls.set(calls.get() + 1);
            let owner = crate::sexp::instance::current_instance_ptr().unwrap();
            assert!(!crate::sexp::memory::is_arena_lent(owner));
            crate::sexp::gengc::full_gc();
            // The first drain follows parsing. The input is held until
            // source copying and parsing finish; later drains need only trees.
            if calls.get() == 1 {
                assert!(observed.is_live());
            }
        }));
        session.with_active_in(|owner| unsafe {
            (*owner).memory_state.gc_force_gap = 1;
            (*owner).memory_state.gc_force_wait = 1;
        });
        drop(text);
        let mut status = 0;
        let result = factory
            .wrap(unsafe { R_ParseVector(raw_text, -1, &mut status, ptr::null_mut()) })
            .unwrap();
        assert_eq!(status, PARSE_OK);
        assert_eq!(result.len(), 2);
        assert_eq!(
            result.clone().try_vector_elt(0).unwrap().real_elt(0),
            Some(1.0)
        );
        assert_eq!(
            result.clone().try_vector_elt(1).unwrap().real_elt(0),
            Some(2.0)
        );
        assert!(notifications.get() >= 2);
        // Stop the input-specific observer before proving eventual release.
        session.with_active_in(|owner| unsafe {
            (*owner).memory_state.gc_force_gap = 0;
            (*owner).gc_state.callbacks.clear();
        });
        crate::sexp::gengc::full_gc();
        assert!(!text_node.is_live());
    }

    #[test]
    fn owned_parser_rejects_publication_after_collecting_callback_closes_runtime() {

        use std::cell::RefCell;
        use std::rc::Rc;
        let session = Rc::new(RefCell::new(
            crate::sexp::session::RSession::new_for_gc_tests(),
        ));
        let factory = session
            .borrow()
            .owner_token()
            .unwrap()
            .weak_owner()
            .unwrap()
            .node_factory()
            .unwrap();
        let text = factory.strings(&["1; 2"]).unwrap();
        let raw_text = text.as_raw();
        let closing = Rc::downgrade(&session);
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            closing.upgrade().unwrap().borrow_mut().close();
        }));
        session.borrow().with_active_in(|owner| unsafe {
            (*owner).memory_state.gc_force_gap = 1;
            (*owner).memory_state.gc_force_wait = 1;
        });
        drop(text);
        let mut status = 0;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            R_ParseVector(raw_text, -1, &mut status, ptr::null_mut())
        }));
        assert!(outcome.is_err());
        assert!(outcome.unwrap_err().is::<RError>());
        assert!(!session.borrow().is_active());
        assert_eq!(
            status, 0,
            "revoked parser must not publish a success status"
        );
    }

    #[test]
    fn owned_parse_eval_environment_survives_parser_collection() {
        use crate::sexp::object::SessionNodeFactory;
        use std::cell::Cell;
        use std::rc::Rc;
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let environment = factory
            .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::ENVSXP)))
            .unwrap();
        let raw_environment = environment.as_raw();
        let environment_node = crate::sexp::memory::checked_projection(raw_environment)
            .unwrap()
            .1;
        let observed = environment_node.clone();
        let notifications = Rc::new(Cell::new(0));
        let calls = notifications.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            calls.set(calls.get() + 1);
            crate::sexp::gengc::full_gc();
            assert!(observed.is_live());
        }));
        session.with_active_in(|owner| unsafe {
            (*owner).memory_state.gc_force_gap = 1;
            (*owner).memory_state.gc_force_wait = 1;
        });
        drop(environment);
        let result = factory
            .wrap(unsafe { R_ParseEvalString(c"7".as_ptr(), raw_environment) })
            .unwrap();
        assert_eq!(result.real_elt(0), Some(7.0));
        assert!(notifications.get() > 0);
        session.with_active_in(|owner| unsafe {
            (*owner).memory_state.gc_force_gap = 0;
            (*owner).gc_state.callbacks.clear();
        });
        crate::sexp::gengc::full_gc();
        assert!(!environment_node.is_live());
    }

    #[test]
    fn test_parse_status_constants() {
        let _session = crate::sexp::session::RSession::new();
        assert!(PARSE_OK > 0);
        assert!(PARSE_INCOMPLETE > 0);
        assert!(PARSE_ERROR > 0);
        assert!(PARSE_EOF > 0);
    }

    #[test]
    fn test_parse_vector_uses_rust_parser() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let text = Rf_mkString(CString::new("1 + 2").unwrap().as_ptr());
            let _text_guard = protect(text);
            let mut status: c_int = 0;
            let result = R_ParseVector(text, 1, &mut status, ptr::null_mut());
            assert_eq!(status, PARSE_OK);
            assert_eq!(TYPEOF(result), SEXPTYPE::EXPRSXP);
            assert_eq!(XLENGTH(result), 1);
            assert_eq!(TYPEOF(VECTOR_ELT(result, 0)), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_parse_eval_string_evaluates_source() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let source = CString::new("1 + 2").unwrap();
            let result = R_ParseEvalString(source.as_ptr(), ptr::null_mut());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert_eq!(*REAL(result), 3.0);
        }
    }

    #[test]
    fn test_current_parse_line() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(R_CurrentParseLine(), 0);
            assert_eq!(R_ParseContext(c"x <- 1\ny".as_ptr(), 8), 0);
            assert_eq!(R_CurrentParseLine(), 2);
        }
    }

    #[test]
    fn test_parse_filename() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = R_ParseFilename();
            assert!(!s.is_null());
        }
    }

    #[test]
    fn test_parse_filename_uses_parse_error_file() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let file = Rf_mkString(c"script.R".as_ptr());
            crate::mainutils::source::store_parse_error("parse error", 1, 1, file);
            let s = R_ParseFilename();
            assert_eq!(CStr::from_ptr(s).to_str(), Ok("script.R"));
        }
    }
}
