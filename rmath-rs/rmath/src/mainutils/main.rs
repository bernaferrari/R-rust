#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/main.c — main REPL and global variable definitions.
//!
//! Provides Rf_mainloop(), R_ReplFile(), Rf_ReplIteration(), and other
//! core REPL functions. The interactive file/console loop is still headless,
//! but top-level task callbacks are tracked per session.

use std::ffi::CString;
use std::os::raw::{c_int, c_void};

use crate::eval::eval::Rf_eval;
use crate::sexp::accessors::{CAR, INTEGER_ELT, LENGTH, STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH};
use crate::sexp::constructors::{Rf_ScalarLogical, Rf_cons, Rf_mkString};
use crate::sexp::context::RError;
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_NilValue, R_Visible, set_R_Visible};
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

/// Console buffer size.
pub const CONSOLE_BUFFER_SIZE: usize = 1024;

#[derive(Default)]
pub(crate) struct MainRuntimeState {
    pub task_callbacks: Vec<ToplevelTaskCallback>,
    pub next_task_callback_id: c_int,
    pub running_toplevel_handlers: bool,
}

pub(crate) struct ToplevelTaskCallback {
    pub id: c_int,
    pub name: String,
    pub fun: SEXP,
    pub data: SEXP,
}

// ---------------------------------------------------------------------------
// Global symbols (initialized lazily)
// ---------------------------------------------------------------------------

/// Get the .Last.value symbol.
pub unsafe fn R_LastvalueSymbol() -> SEXP {
    unsafe { Rf_install(c".Last.value".as_ptr()) }
}

/// Get the .Random.seed symbol.
pub unsafe fn R_SeedsSymbol() -> SEXP {
    unsafe { Rf_install(c".Random.seed".as_ptr()) }
}

// ---------------------------------------------------------------------------
// R_Visible accessor
// ---------------------------------------------------------------------------

/// Get R_Visible flag.
pub unsafe fn R_GetVisible() -> c_int {
    unsafe { if R_Visible() != 0 { TRUE } else { FALSE } }
}

/// Set R_Visible flag.
pub unsafe fn R_SetVisible(v: c_int) {
    unsafe {
        set_R_Visible(v);
    }
}

/// Get R_Interactive flag.
pub unsafe fn R_Interactive() -> c_int {
    with_required_current_instance(|inst| inst.eval_state.interactive)
}

/// Set R_Interactive flag.
pub unsafe fn R_SetInteractive(v: c_int) {
    with_required_current_instance(|inst| inst.eval_state.interactive = v);
}

/// Get R_Quiet flag.
pub unsafe fn R_Quiet() -> c_int {
    with_required_current_instance(|inst| inst.eval_state.quiet)
}

/// Set R_Quiet flag.
pub unsafe fn R_SetQuiet(v: c_int) {
    with_required_current_instance(|inst| inst.eval_state.quiet = v);
}

/// Get R_NoEcho flag.
pub unsafe fn R_NoEcho() -> c_int {
    with_required_current_instance(|inst| inst.eval_state.no_echo)
}

/// Get R_Verbose flag.
pub unsafe fn R_Verbose() -> c_int {
    with_required_current_instance(|inst| inst.eval_state.verbose)
}

// ---------------------------------------------------------------------------
/// Get evaluation depth.
pub unsafe fn R_GetEvalDepth() -> c_int {
    with_required_current_instance(|inst| inst.eval_state.eval_depth)
}

/// Set evaluation depth.
pub unsafe fn R_SetEvalDepth(v: c_int) {
    with_required_current_instance(|inst| inst.eval_state.eval_depth = v);
}

// ---------------------------------------------------------------------------
/// Get protection stack top.
pub unsafe fn R_PPStackTop() -> c_int {
    with_required_current_instance(|inst| inst.eval_state.pp_stack_top)
}

/// Set protection stack top.
pub unsafe fn R_SetPPStackTop(v: c_int) {
    with_required_current_instance(|inst| inst.eval_state.pp_stack_top = v);
}

// ---------------------------------------------------------------------------
/// Get warnings collection flag.
pub unsafe fn R_GetCollectWarnings() -> c_int {
    with_required_current_instance(|inst| inst.eval_state.collect_warnings)
}

/// Set warnings collection flag.
pub unsafe fn R_SetCollectWarnings(v: c_int) {
    with_required_current_instance(|inst| inst.eval_state.collect_warnings = v);
}

// ---------------------------------------------------------------------------
// Time limits (stubs)
// ---------------------------------------------------------------------------

pub unsafe fn resetTimeLimits() {
    // Unimplemented
}

pub unsafe fn checkTimeLimits() {
    // Unimplemented
}

// ---------------------------------------------------------------------------
// SrcRef state (stubs)
// ---------------------------------------------------------------------------

pub unsafe fn R_InitSrcRefState(_cntxt: *mut std::ffi::c_void) {
    // Unimplemented
}

pub unsafe fn R_FinalizeSrcRefState() {
    // Unimplemented
}

// ---------------------------------------------------------------------------
// Parse status
// ---------------------------------------------------------------------------

pub const PARSE_OK: c_int = 0;
pub const PARSE_INCOMPLETE: c_int = 1;
pub const PARSE_ERROR: c_int = 2;
pub const PARSE_EOF: c_int = 3;
pub const PARSE_NULL: c_int = 4;

pub unsafe fn R_GetParseErrorMsg() -> *const std::os::raw::c_char {
    with_required_current_instance(|inst| {
        inst.eval_state.parse_error_msg.as_ptr() as *const std::os::raw::c_char
    })
}

fn main_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

unsafe fn read_c_file_to_string(fp: *mut c_void) -> Result<String, String> {
    unsafe {
        if fp.is_null() {
            return Err("file pointer is NULL".to_string());
        }
        let file = fp.cast::<libc::FILE>();
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let read = libc::fread(
                buffer.as_mut_ptr().cast(),
                1,
                buffer.len(),
                file.cast::<libc::FILE>(),
            );
            if read > 0 {
                bytes.extend_from_slice(&buffer[..read]);
            }
            if read < buffer.len() {
                if libc::ferror(file) != 0 {
                    return Err("failed reading R source file".to_string());
                }
                break;
            }
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

unsafe fn parse_source_to_exprs(source: &str, n: c_int, status: *mut c_int) -> SEXP {
    unsafe {
        let source = CString::new(source).unwrap_or_default();
        let text = Rf_mkString(source.as_ptr());
        let _text_guard = protect(text);
        let mut parse_status = PARSE_NULL;
        let status_ptr: *mut c_int = if status.is_null() {
            &mut parse_status as *mut c_int
        } else {
            status
        };
        let exprs = crate::mainutils::gram_main::R_ParseVector(text, n, status_ptr, R_NilValue());
        if !status.is_null() {
            let gram_status = *status;
            *status = match gram_status {
                crate::mainutils::gram_main::PARSE_OK => PARSE_OK,
                crate::mainutils::gram_main::PARSE_INCOMPLETE => PARSE_INCOMPLETE,
                crate::mainutils::gram_main::PARSE_EOF => PARSE_EOF,
                crate::mainutils::gram_main::PARSE_ERROR => PARSE_ERROR,
                _ => PARSE_ERROR,
            };
        }
        exprs
    }
}

// ---------------------------------------------------------------------------
// R_ReplFile — REPL from file
// ---------------------------------------------------------------------------

/// Run the REPL reading from a file.
///
/// This is the equivalent of R's `R_ReplFile()` from main.c.
pub unsafe fn R_ReplFile(fp: *mut c_void, rho: SEXP) {
    unsafe {
        let source = read_c_file_to_string(fp).unwrap_or_else(|message| main_error(message));
        if source.trim().is_empty() {
            return;
        }
        let mut status = PARSE_NULL;
        let exprs = parse_source_to_exprs(&source, -1, &mut status);
        let _exprs_guard = protect(exprs);
        if status != PARSE_OK {
            main_error("parse error while reading R source file");
        }
        let env = if rho.is_null() || rho == R_NilValue() {
            with_required_current_instance(|inst| inst.global_env)
        } else {
            rho
        };
        for i in 0..XLENGTH(exprs) {
            let expr = VECTOR_ELT(exprs, i);
            let value = Rf_eval(expr, env);
            Rf_callToplevelHandlers(expr, value, TRUE, R_GetVisible());
        }
    }
}

// ---------------------------------------------------------------------------
// Rf_ReplIteration — single REPL iteration
// ---------------------------------------------------------------------------

/// Perform a single REPL iteration.
///
/// This is the equivalent of R's `Rf_ReplIteration()` from main.c.
pub unsafe fn Rf_ReplIteration(
    _rho: SEXP,
    _savestack: c_int,
    _browselevel: c_int,
    _state: *mut std::ffi::c_void,
) -> c_int {
    main_error("interactive REPL iteration is not available in the headless Rust runtime")
}

// ---------------------------------------------------------------------------
// Rf_ReplConsole — interactive REPL
// ---------------------------------------------------------------------------

/// Run the interactive REPL.
///
/// This is the equivalent of R's `Rf_ReplConsole()` from main.c.
pub unsafe fn Rf_ReplConsole(_rho: SEXP, _savestack: c_int, _browselevel: c_int) {
    main_error("interactive console REPL is not available in the headless Rust runtime")
}

// ---------------------------------------------------------------------------
// Rf_mainloop — main R loop (stub)
// ---------------------------------------------------------------------------

/// The main R read-eval-print loop.
///
/// This is the equivalent of R's `Rf_mainloop()` from main.c.
/// Called from main() after Rf_initialize_R().
pub unsafe fn Rf_mainloop() {
    // Headless: no REPL loop. For embedded use, call eval() directly.
}

// ---------------------------------------------------------------------------
// R_Parse1File — parse one expression from file
// ---------------------------------------------------------------------------

pub unsafe fn R_Parse1File(fp: *mut c_void, _prompt: c_int, status: *mut c_int) -> SEXP {
    unsafe {
        let source = match read_c_file_to_string(fp) {
            Ok(source) => source,
            Err(message) => {
                if !status.is_null() {
                    *status = PARSE_ERROR;
                }
                main_error(message);
            }
        };
        if source.trim().is_empty() {
            if !status.is_null() {
                *status = PARSE_EOF;
            }
            return R_NilValue();
        }
        let mut parse_status = PARSE_NULL;
        let exprs = parse_source_to_exprs(&source, 1, &mut parse_status);
        let _exprs_guard = protect(exprs);
        if !status.is_null() {
            *status = parse_status;
        }
        if parse_status != PARSE_OK || exprs.is_null() || XLENGTH(exprs) == 0 {
            return R_NilValue();
        }
        VECTOR_ELT(exprs, 0)
    }
}

// ---------------------------------------------------------------------------
// setup_Rmainloop — setup before mainloop (stub)
// ---------------------------------------------------------------------------

pub unsafe fn setup_Rmainloop() {
    // Unimplemented
}

// ---------------------------------------------------------------------------
// Top-level handlers
// ---------------------------------------------------------------------------

pub unsafe fn Rf_callToplevelHandlers(expr: SEXP, value: SEXP, succeeded: c_int, visible: c_int) {
    if with_required_current_instance(|inst| {
        if inst.main_state.running_toplevel_handlers {
            true
        } else {
            inst.main_state.running_toplevel_handlers = true;
            false
        }
    }) {
        return;
    }

    let mut index = 0usize;
    loop {
        let current = with_required_current_instance(|inst| {
            inst.main_state.task_callbacks.get(index).map(|callback| {
                (
                    callback.id,
                    callback.fun,
                    callback.data,
                    callback.name.clone(),
                )
            })
        });
        let Some((id, fun, data, _name)) = current else {
            break;
        };

        let keep = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            let call = make_task_callback_call(fun, expr, value, succeeded, visible, data);
            let result = crate::eval::eval::Rf_eval(call, crate::sexp::globals::R_GlobalEnv());
            crate::mainutils::coerce::asLogical(result) == TRUE
        }))
        .unwrap_or(false);

        let position = with_required_current_instance(|inst| {
            inst.main_state
                .task_callbacks
                .iter()
                .position(|callback| callback.id == id)
        });
        match (keep, position) {
            (true, Some(pos)) => index = pos + 1,
            (false, Some(pos)) => {
                with_required_current_instance(|inst| {
                    inst.main_state.task_callbacks.remove(pos);
                });
                index = pos;
            }
            (_, None) => {}
        }
    }

    with_required_current_instance(|inst| {
        inst.main_state.running_toplevel_handlers = false;
    });
}

pub unsafe fn Rf_addTaskCallback(fun: SEXP, data: SEXP) -> c_int {
    unsafe {
        if fun.is_null()
            || !matches!(
                SEXPTYPE(TYPEOF(fun)),
                SEXPTYPE::CLOSXP | SEXPTYPE::BUILTINSXP | SEXPTYPE::SPECIALSXP
            )
        {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "task callback must be a function".to_string(),
            });
        }
    }

    with_required_current_instance(|inst| {
        inst.main_state.next_task_callback_id += 1;
        let id = inst.main_state.next_task_callback_id;
        inst.main_state.task_callbacks.push(ToplevelTaskCallback {
            id,
            name: id.to_string(),
            fun,
            data,
        });
        id
    })
}

pub unsafe fn Rf_removeTaskCallback(which: SEXP) -> c_int {
    unsafe {
        let target = task_callback_selector(which);
        with_required_current_instance(|inst| {
            let position = match target {
                TaskCallbackSelector::Id(id) => inst
                    .main_state
                    .task_callbacks
                    .iter()
                    .position(|callback| callback.id == id),
                TaskCallbackSelector::Name(name) => inst
                    .main_state
                    .task_callbacks
                    .iter()
                    .position(|callback| callback.name == name),
                TaskCallbackSelector::Missing => None,
            };
            if let Some(position) = position {
                inst.main_state.task_callbacks.remove(position);
                TRUE
            } else {
                FALSE
            }
        })
    }
}

unsafe fn make_task_callback_call(
    fun: SEXP,
    expr: SEXP,
    value: SEXP,
    succeeded: c_int,
    visible: c_int,
    data: SEXP,
) -> SEXP {
    unsafe {
        let expr = if expr.is_null() { R_NilValue() } else { expr };
        let value = if value.is_null() { R_NilValue() } else { value };
        let data = if data.is_null() { R_NilValue() } else { data };
        let mut args = R_NilValue();
        for arg in [
            data,
            Rf_ScalarLogical(if visible != 0 { TRUE } else { FALSE }),
            Rf_ScalarLogical(if succeeded != 0 { TRUE } else { FALSE }),
            value,
            expr,
        ] {
            args = Rf_cons(arg, args);
        }
        let call = Rf_cons(fun, args);
        if !call.is_null() {
            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        call
    }
}

enum TaskCallbackSelector {
    Id(c_int),
    Name(String),
    Missing,
}

unsafe fn task_callback_selector(which: SEXP) -> TaskCallbackSelector {
    unsafe {
        if which.is_null() || which == R_NilValue() {
            return TaskCallbackSelector::Missing;
        }
        match SEXPTYPE(TYPEOF(which)) {
            SEXPTYPE::INTSXP => {
                let id = INTEGER_ELT(which, 0);
                if id == NA_INTEGER {
                    TaskCallbackSelector::Missing
                } else {
                    TaskCallbackSelector::Id(id)
                }
            }
            SEXPTYPE::REALSXP => {
                let id = crate::mainutils::coerce::asInteger(which);
                if id == NA_INTEGER {
                    TaskCallbackSelector::Missing
                } else {
                    TaskCallbackSelector::Id(id)
                }
            }
            SEXPTYPE::STRSXP if LENGTH(which) > 0 => {
                let charsxp = STRING_ELT(which, 0 as R_xlen_t);
                let name = crate::sexp::accessors::CHAR(charsxp);
                if name.is_null() {
                    TaskCallbackSelector::Missing
                } else {
                    TaskCallbackSelector::Name(
                        std::ffi::CStr::from_ptr(name)
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
            }
            SEXPTYPE::SYMSXP => {
                let charsxp = crate::sexp::accessors::PRINTNAME(which);
                let name = crate::sexp::accessors::CHAR(charsxp);
                if name.is_null() {
                    TaskCallbackSelector::Missing
                } else {
                    TaskCallbackSelector::Name(
                        std::ffi::CStr::from_ptr(name)
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
            }
            SEXPTYPE::LISTSXP => task_callback_selector(CAR(which)),
            _ => TaskCallbackSelector::Missing,
        }
    }
}

// ---------------------------------------------------------------------------
// Memory profiling
// ---------------------------------------------------------------------------

pub unsafe fn R_GetMaxVSize() -> u64 {
    unsafe { crate::mainutils::memory_main::R_GetMaxVSize_memory() }
}

pub unsafe fn R_GetMaxNSize() -> u64 {
    unsafe { crate::mainutils::memory_main::R_GetMaxNSize_memory() }
}

pub unsafe fn R_GetVSize() -> u64 {
    crate::sexp::memory::with_arena(|a| a.total_bytes_allocated() as u64)
}

pub unsafe fn R_GetNSize() -> u64 {
    crate::sexp::memory::with_arena(|a| a.node_count() as u64)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::sexp::session::RSession;

    use super::*;

    fn assert_r_error(action: impl FnOnce()) -> RError {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action))
            .expect_err("expected RError panic");
        payload
            .downcast_ref::<RError>()
            .expect("expected RError payload")
            .clone()
    }

    fn open_c_source(contents: &str) -> (PathBuf, *mut libc::FILE) {
        let path = std::env::temp_dir().join(format!(
            "rport-main-test-{}-{}.R",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, contents).expect("write test source");
        let c_path = CString::new(path.to_string_lossy().as_bytes()).unwrap();
        let fp = unsafe { libc::fopen(c_path.as_ptr(), c"r".as_ptr()) };
        assert!(!fp.is_null(), "failed to open source file");
        (path, fp)
    }

    unsafe fn close_c_source(path: PathBuf, fp: *mut libc::FILE) {
        unsafe {
            libc::fclose(fp);
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_repl_stub() {
        unsafe {
            Rf_mainloop();
        }
    }

    #[test]
    fn test_parse_status_constants() {
        assert_eq!(PARSE_OK, 0);
        assert_eq!(PARSE_EOF, 3);
    }

    #[test]
    fn test_parse_one_file_reads_and_parses_expression() {
        let _session = RSession::new();
        let (path, fp) = open_c_source("1 + 2\n");
        unsafe {
            let mut status = -1;
            let expr = R_Parse1File(fp.cast::<c_void>(), 0, &mut status);
            assert_eq!(status, PARSE_OK);
            assert!(!expr.is_null());
            assert_ne!(expr, R_NilValue());
            let value = Rf_eval(expr, crate::sexp::globals::R_GlobalEnv());
            assert_eq!(TYPEOF(value), SEXPTYPE::REALSXP);
            assert_eq!(*crate::sexp::accessors::REAL(value), 3.0);
            close_c_source(path, fp);
        }
    }

    #[test]
    fn test_repl_file_evaluates_source_in_environment() {
        let _session = RSession::new();
        let (path, fp) = open_c_source("repl_file_value <- 41\n");
        unsafe {
            R_ReplFile(fp.cast::<c_void>(), crate::sexp::globals::R_GlobalEnv());
            let sym = Rf_install(c"repl_file_value".as_ptr());
            let value =
                crate::sexp::envir::R_findVarInFrame(crate::sexp::globals::R_GlobalEnv(), sym);
            assert_eq!(TYPEOF(value), SEXPTYPE::REALSXP);
            assert_eq!(*crate::sexp::accessors::REAL(value), 41.0);
            close_c_source(path, fp);
        }
    }

    #[test]
    fn test_interactive_repl_iteration_errors_explicitly() {
        let _session = RSession::new();
        let err = assert_r_error(|| unsafe {
            Rf_ReplIteration(R_NilValue(), 0, 0, std::ptr::null_mut());
        });
        assert!(err.message.contains("interactive REPL iteration"));
    }

    #[test]
    fn test_r_quiet() {
        let _session = RSession::new();
        unsafe {
            assert_eq!(R_Quiet(), 0);
            R_SetQuiet(1);
            assert_eq!(R_Quiet(), 1);
            R_SetQuiet(0);
        }
    }

    #[test]
    fn test_r_interactive() {
        let _session = RSession::new();
        unsafe {
            assert_eq!(R_Interactive(), 1);
            R_SetInteractive(0);
            assert_eq!(R_Interactive(), 0);
            R_SetInteractive(1);
        }
    }

    #[test]
    fn test_eval_depth() {
        let _session = RSession::new();
        unsafe {
            R_SetEvalDepth(10);
            assert_eq!(R_GetEvalDepth(), 10);
            R_SetEvalDepth(0);
        }
    }

    unsafe fn task_callback_closure(keep: c_int) -> SEXP {
        unsafe {
            let mut formals = R_NilValue();
            for name in ["data", "visible", "succeeded", "value", "expr"] {
                let cell = Rf_cons(crate::sexp::globals::R_MissingArg(), formals);
                crate::sexp::accessors::SETTAG(
                    cell,
                    Rf_install(std::ffi::CString::new(name).unwrap().as_ptr()),
                );
                formals = cell;
            }
            crate::mainutils::dstruct::mkCLOSXP(
                formals,
                Rf_ScalarLogical(keep),
                crate::sexp::globals::R_GlobalEnv(),
            )
        }
    }

    #[test]
    fn test_top_level_callbacks_keep_or_remove_by_result() {
        let _session = RSession::new();
        unsafe {
            let keep = task_callback_closure(TRUE);
            let drop = task_callback_closure(FALSE);
            let keep_id = Rf_addTaskCallback(keep, R_NilValue());
            let drop_id = Rf_addTaskCallback(drop, R_NilValue());

            Rf_callToplevelHandlers(
                R_NilValue(),
                crate::sexp::constructors::Rf_ScalarInteger(1),
                TRUE,
                TRUE,
            );

            assert_eq!(
                Rf_removeTaskCallback(crate::sexp::constructors::Rf_ScalarInteger(keep_id)),
                TRUE
            );
            assert_eq!(
                Rf_removeTaskCallback(crate::sexp::constructors::Rf_ScalarInteger(drop_id)),
                FALSE
            );
        }
    }

    #[test]
    fn test_top_level_callbacks_are_session_local() {
        let left = RSession::new();
        let right = RSession::new();

        let left_id = left.with_protected(|| unsafe {
            Rf_addTaskCallback(task_callback_closure(TRUE), R_NilValue())
        });

        right.with_protected(|| unsafe {
            assert_eq!(
                Rf_removeTaskCallback(crate::sexp::constructors::Rf_ScalarInteger(left_id)),
                FALSE
            );
        });

        left.with_protected(|| unsafe {
            assert_eq!(
                Rf_removeTaskCallback(crate::sexp::constructors::Rf_ScalarInteger(left_id)),
                TRUE
            );
        });
    }

    #[test]
    fn test_session_main_state_is_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        left.with_active(|| unsafe {
            R_SetQuiet(1);
            R_SetInteractive(0);
            R_SetEvalDepth(12);
            R_SetPPStackTop(4);
            R_SetCollectWarnings(5);
            R_SetVisible(FALSE);
            assert_eq!(R_Quiet(), 1);
            assert_eq!(R_Interactive(), 0);
            assert_eq!(R_GetEvalDepth(), 12);
            assert_eq!(R_PPStackTop(), 4);
            assert_eq!(R_GetCollectWarnings(), 5);
            assert_eq!(R_GetVisible(), FALSE);
        });

        right.with_active(|| unsafe {
            assert_eq!(R_Quiet(), 0);
            assert_eq!(R_Interactive(), 1);
            assert_eq!(R_GetEvalDepth(), 0);
            assert_eq!(R_PPStackTop(), 0);
            assert_eq!(R_GetCollectWarnings(), 0);
            assert_eq!(R_GetVisible(), TRUE);
        });

        left.with_active(|| unsafe {
            assert_eq!(R_Quiet(), 1);
            assert_eq!(R_Interactive(), 0);
            assert_eq!(R_GetEvalDepth(), 12);
            assert_eq!(R_PPStackTop(), 4);
            assert_eq!(R_GetCollectWarnings(), 5);
            assert_eq!(R_GetVisible(), FALSE);
        });
    }
}
