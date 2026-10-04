#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/main.c — main REPL and global variable definitions.
//!
//! Provides Rf_mainloop(), R_ReplFile(), Rf_ReplIteration(), and other
//! core REPL functions. The interactive file/console loop is still headless,
//! but top-level task callbacks are tracked per session.

use std::ffi::CString;
use std::os::raw::c_int;

use crate::eval::eval::Rf_eval;
use crate::mainutils::rfile::{RFile, r_ferror, r_fread};
use crate::sexp::accessors::{VECTOR_ELT, XLENGTH};
use crate::sexp::constructors::Rf_mkString;
use crate::sexp::context::RError;
use crate::sexp::ffi::{FALSE, NA_INTEGER, SEXP, SEXPTYPE, TRUE};
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

#[derive(Clone)]
pub(crate) struct ToplevelTaskCallback {
    pub id: c_int,
    pub name: String,
    pub fun: crate::sexp::object::Sexp<'static>,
    pub data: crate::sexp::object::Sexp<'static>,
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
    with_required_current_instance(|inst| unsafe { (*inst).eval_state.interactive })
}

/// Set R_Interactive flag.
pub unsafe fn R_SetInteractive(v: c_int) {
    with_required_current_instance(|inst| unsafe { (*inst).eval_state.interactive = v });
}

/// Get R_Quiet flag.
pub unsafe fn R_Quiet() -> c_int {
    with_required_current_instance(|inst| unsafe { (*inst).eval_state.quiet })
}

/// Set R_Quiet flag.
pub unsafe fn R_SetQuiet(v: c_int) {
    with_required_current_instance(|inst| unsafe { (*inst).eval_state.quiet = v });
}

/// Get R_NoEcho flag.
pub unsafe fn R_NoEcho() -> c_int {
    with_required_current_instance(|inst| unsafe { (*inst).eval_state.no_echo })
}

/// Get R_Verbose flag.
pub unsafe fn R_Verbose() -> c_int {
    with_required_current_instance(|inst| unsafe { (*inst).eval_state.verbose })
}

// ---------------------------------------------------------------------------
/// Get evaluation depth.
pub unsafe fn R_GetEvalDepth() -> c_int {
    with_required_current_instance(|inst| unsafe { (*inst).eval_state.eval_depth })
}

/// Set evaluation depth.
pub unsafe fn R_SetEvalDepth(v: c_int) {
    with_required_current_instance(|inst| unsafe { (*inst).eval_state.eval_depth = v });
}

// ---------------------------------------------------------------------------
/// Get protection stack top.
pub unsafe fn R_PPStackTop() -> c_int {
    with_required_current_instance(|inst| unsafe { (*inst).eval_state.pp_stack_top })
}

/// Set protection stack top.
pub unsafe fn R_SetPPStackTop(v: c_int) {
    with_required_current_instance(|inst| unsafe { (*inst).eval_state.pp_stack_top = v });
}

// ---------------------------------------------------------------------------
/// Get warnings collection flag.
pub unsafe fn R_GetCollectWarnings() -> c_int {
    with_required_current_instance(|inst| unsafe { (*inst).eval_state.collect_warnings })
}

/// Set warnings collection flag.
pub unsafe fn R_SetCollectWarnings(v: c_int) {
    with_required_current_instance(|inst| unsafe { (*inst).eval_state.collect_warnings = v });
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
    with_required_current_instance(|inst| unsafe {
        (*inst).eval_state.parse_error_msg.as_ptr() as *const std::os::raw::c_char
    })
}

fn main_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

unsafe fn read_c_file_to_string(fp: *mut RFile) -> Result<String, String> {
    unsafe {
        if fp.is_null() {
            return Err("file pointer is NULL".to_string());
        }
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let read = r_fread(buffer.as_mut_ptr().cast(), 1, buffer.len(), fp);
            if read > 0 {
                bytes.extend_from_slice(&buffer[..read]);
            }
            if read < buffer.len() {
                if r_ferror(fp) != 0 {
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
pub unsafe fn R_ReplFile(fp: *mut RFile, rho: SEXP) {
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
            with_required_current_instance(|inst| (*inst).global_env)
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

pub unsafe fn R_Parse1File(fp: *mut RFile, _prompt: c_int, status: *mut c_int) -> SEXP {
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

/// Retain the physical runtime for cleanup; revocation only denies new work.
struct TaskHandlersGuard {
    owner: crate::sexp::owner::OwnerPin,
    visible: c_int,
}
impl TaskHandlersGuard {
    fn enter(owner: crate::sexp::owner::OwnerPin) -> Option<Self> {
        let instance = owner.as_ptr();
        unsafe {
            if (*instance).main_state.running_toplevel_handlers {
                return None;
            }
            let visible = (*instance).eval_state.visible;
            (*instance).main_state.running_toplevel_handlers = true;
            Some(Self { owner, visible })
        }
    }
}
impl Drop for TaskHandlersGuard {
    fn drop(&mut self) {
        unsafe {
            let instance = self.owner.as_ptr();
            (*instance).main_state.running_toplevel_handlers = false;
            (*instance).eval_state.visible = self.visible;
        }
    }
}

fn task_callback_owner() -> crate::sexp::owner::WeakOwner {
    with_required_current_instance(|instance| unsafe { (*instance).runtime_owner.clone() })
        .unwrap_or_else(|| main_error("task callbacks require an original managed runtime"))
}

fn run_task_callback(
    access: &crate::sexp::owner::RuntimeAccess,
    callback: &ToplevelTaskCallback,
    expr: &crate::sexp::object::Sexp<'static>,
    value: &crate::sexp::object::Sexp<'static>,
    succeeded: c_int,
    visible: c_int,
    environment: &crate::sexp::object::Sexp<'static>,
) -> crate::sexp::object::SexpResult<bool> {
    use crate::sexp::object::SexpError;
    let domain = access.domain();
    let allocator = access.allocator(&domain)?;
    let mut inputs = vec![
        ("expr", expr.clone()),
        ("value", value.clone()),
        ("succeeded", domain.logical(succeeded != 0)),
        ("visible", domain.logical(visible != 0)),
    ];
    if !callback.data.is_nil() {
        inputs.push(("data", callback.data.clone()));
    }
    // Cached promises share the caller's values. Replacement operations in a
    // callback must duplicate before mutating those original objects.
    access.with_native(|_| {
        for (_, value) in &inputs {
            unsafe {
                crate::sexp::accessors::SET_NAMED(value.as_raw(), 2);
            }
        }
        Ok(())
    })?;
    let special = callback.fun.typeof_() == SEXPTYPE::SPECIALSXP;
    let mut special_frame = domain.nil();
    let mut syntax = domain.nil();
    let mut execution = domain.nil();
    for (name, value) in inputs.iter().rev() {
        let name = CString::new(*name).expect("fixed task callback argument name");
        let symbol = access.with_native(|owner| {
            owner
                .sexp(unsafe { Rf_install(name.as_ptr()) })?
                .into_owned()
        })?;
        let promise = allocator.evaluated_promise_with_expression(&symbol, environment, value)?;
        if special {
            special_frame = allocator.pairlist_cell(value, &special_frame, &symbol)?;
        }
        execution = allocator.pairlist_cell(&promise, &execution, &domain.nil())?;
        syntax = allocator.pairlist_cell(&symbol, &syntax, &domain.nil())?;
    }
    let special_environment = if special {
        // Specials inspect the observable syntax (some read CDR(call)), so its
        // argument symbols must resolve to the actual cached inputs. Initialize
        // every graph edge before the allocator permits collection or callbacks.
        let body = crate::sexp::ffi::NodeBody::Environment(crate::sexp::ffi::Envsxp {
            frame: domain.link(&special_frame)?,
            enclos: domain.link(environment)?,
            hashtab: domain.link(&domain.nil())?,
        });
        Some(allocator.allocate(|arena| {
            let pointer = arena.alloc_node(SEXPTYPE::ENVSXP);
            let node = arena.node_token(pointer)?;
            let heap = node.heap_identity();
            let mut header = heap.node_snapshot(&node)?;
            header.data = body;
            heap.replace_node(&node, header)?;
            Some(pointer)
        })?)
    } else {
        None
    };
    let call = allocator.call(&callback.fun, &syntax)?;
    access.with_native(|owner| {
        let result = if callback.fun.is_closure() {
            let result = unsafe {
                crate::eval::closure::applyClosure(
                    call.as_raw(),
                    callback.fun.as_raw(),
                    execution.as_raw(),
                    environment.as_raw(),
                    domain.nil().as_raw(),
                    0,
                )
            };
            owner.sexp(result)?.into_owned()?
        } else {
            let result = if let Some(environment) = &special_environment {
                crate::eval::apply::apply_special_safe(
                    callback.fun.clone(),
                    call.clone(),
                    syntax.clone(),
                    environment.clone(),
                )
            } else {
                crate::eval::apply::apply_builtin_safe(
                    callback.fun.clone(),
                    call.clone(),
                    execution.clone(),
                    environment.clone(),
                )
            };
            result
                .map_err(|message| SexpError::EvaluationFailed { message })?
                .into_owned()?
        };
        // Coercion can invoke ALTREP providers. The actual result owner lives
        // through coercion and the native scope checks publication afterward.
        // GNU's Rboolean contract removes only FALSE; NA and empty logical
        // results retain the callback, including nonlogical values coerced to NA.
        let keep = unsafe { crate::mainutils::coerce::asLogical(result.as_raw()) } != FALSE;
        Ok(keep)
    })
}

pub unsafe fn Rf_callToplevelHandlers(expr: SEXP, value: SEXP, succeeded: c_int, visible: c_int) {
    let owner = task_callback_owner();
    let pin = owner
        .pin()
        .unwrap_or_else(|error| main_error(error.to_string()));
    let Some(_guard) = TaskHandlersGuard::enter(pin) else {
        return;
    };
    // Direct managed native entries also need canonical nonlocal transport.
    // Nested same-owner execution shares the existing scope; this never
    // activates another runtime or adds a parallel rooting ledger.
    let _transfers = crate::sexp::transfer::TransferScopeGuard::enter(owner.clone())
        .unwrap_or_else(|error| main_error(error.to_string()));
    // A revoked callback ends this notification. The guard still restores its
    // original physical fields; no subsequent callback or result is published.
    let _ = crate::sexp::owner::with_runtime(&owner, |access| {
        let domain = access.domain();
        let expr = if expr.is_null() {
            domain.nil()
        } else {
            domain.wrap(expr)?
        };
        let value = if value.is_null() {
            domain.nil()
        } else {
            domain.wrap(value)?
        };
        let environment = access.with_native(|owner| {
            let raw = unsafe { (*owner.as_ptr()).global_env };
            owner.sexp(raw)?.into_owned()
        })?;
        let mut visited = std::collections::HashSet::new();
        loop {
            access.require_active()?;
            let callback = access.with_native(|owner| {
                Ok(unsafe {
                    (*owner.as_ptr())
                        .main_state
                        .task_callbacks
                        .iter()
                        .find(|callback| !visited.contains(&callback.id))
                        .cloned()
                })
            })?;
            let Some(callback) = callback else {
                break;
            };
            visited.try_reserve(1).map_err(|_| {
                crate::sexp::object::SexpError::AllocationFailed {
                    object: "task callback iteration",
                }
            })?;
            visited.insert(callback.id);
            let keep = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_task_callback(
                    access,
                    &callback,
                    &expr,
                    &value,
                    succeeded,
                    visible,
                    &environment,
                )
            }))
            .ok()
            .and_then(Result::ok)
            .unwrap_or(false);
            access.require_active()?;
            if !keep {
                let retired = access.with_native(|owner| {
                    let callbacks = unsafe { &mut (*owner.as_ptr()).main_state.task_callbacks };
                    Ok(callbacks
                        .iter()
                        .position(|item| item.id == callback.id)
                        .map(|position| callbacks.remove(position)))
                })?;
                // Release actual owners outside the callback field borrow.
                drop(retired);
            }
        }
        Ok::<(), crate::sexp::object::SexpError>(())
    });
}

pub unsafe fn Rf_addTaskCallback(fun: SEXP, data: SEXP) -> c_int {
    let owner = task_callback_owner();
    crate::sexp::owner::with_runtime(&owner, |access| {
        let domain = access.domain();
        let fun = domain.wrap(fun)?.into_owned()?;
        let data = if data.is_null() {
            domain.nil()
        } else {
            domain.wrap(data)?.into_owned()?
        };
        if !fun.is_function() {
            return Err(crate::sexp::object::SexpError::EvaluationFailed {
                message: "task callback must be a function".into(),
            });
        }
        access.with_native(|owner| {
            let state = unsafe { &mut (*owner.as_ptr()).main_state };
            let id = state.next_task_callback_id.checked_add(1).ok_or(
                crate::sexp::object::SexpError::AllocationFailed {
                    object: "task callback identity",
                },
            )?;
            state.task_callbacks.try_reserve(1).map_err(|_| {
                crate::sexp::object::SexpError::AllocationFailed {
                    object: "task callback storage",
                }
            })?;
            state.task_callbacks.push(ToplevelTaskCallback {
                id,
                name: id.to_string(),
                fun,
                data,
            });
            state.next_task_callback_id = id;
            Ok(id)
        })
    })
    .and_then(|result| result)
    .unwrap_or_else(|error| main_error(error.to_string()))
}

pub unsafe fn Rf_removeTaskCallback(which: SEXP) -> c_int {
    let owner = task_callback_owner();
    crate::sexp::owner::with_runtime(&owner, |access| {
        let domain = access.domain();
        let which = if which.is_null() {
            domain.nil()
        } else {
            domain.wrap(which)?
        };
        let selector = access.with_native(|_| task_callback_selector(which))?;
        let retired = access.with_native(|owner| {
            let callbacks = unsafe { &mut (*owner.as_ptr()).main_state.task_callbacks };
            let position = match selector {
                TaskCallbackSelector::Id(id) => {
                    callbacks.iter().position(|callback| callback.id == id)
                }
                TaskCallbackSelector::Name(name) => {
                    callbacks.iter().position(|callback| callback.name == name)
                }
                TaskCallbackSelector::Missing => None,
            };
            Ok(position.map(|position| callbacks.remove(position)))
        })?;
        let found = retired.is_some();
        drop(retired);
        Ok(if found { TRUE } else { FALSE })
    })
    .and_then(|result| result)
    .unwrap_or_else(|error| main_error(error.to_string()))
}

enum TaskCallbackSelector {
    Id(c_int),
    Name(String),
    Missing,
}

fn task_callback_selector(
    mut which: crate::sexp::object::Sexp<'static>,
) -> crate::sexp::object::SexpResult<TaskCallbackSelector> {
    let mut seen = std::collections::HashSet::new();
    loop {
        let selector = match which.typeof_() {
            SEXPTYPE::INTSXP if which.len() > 0 => {
                let id = which.try_integer_elt(0)?;
                if id == NA_INTEGER {
                    TaskCallbackSelector::Missing
                } else {
                    TaskCallbackSelector::Id(id)
                }
            }
            SEXPTYPE::REALSXP => {
                let id = unsafe { crate::mainutils::coerce::asInteger(which.as_raw()) };
                if id == NA_INTEGER {
                    TaskCallbackSelector::Missing
                } else {
                    TaskCallbackSelector::Id(id)
                }
            }
            SEXPTYPE::STRSXP if which.len() > 0 => which
                .try_string_value_elt(0)?
                .map(TaskCallbackSelector::Name)
                .unwrap_or(TaskCallbackSelector::Missing),
            SEXPTYPE::SYMSXP => {
                let printname = which.try_printname()?;
                let bytes = unsafe { crate::sexp::accessors::CHAR(printname.as_raw()) };
                if bytes.is_null() {
                    TaskCallbackSelector::Missing
                } else {
                    TaskCallbackSelector::Name(
                        unsafe { std::ffi::CStr::from_ptr(bytes) }
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
            }
            SEXPTYPE::LISTSXP => {
                seen.try_reserve(1).map_err(|_| {
                    crate::sexp::object::SexpError::AllocationFailed {
                        object: "task callback selector",
                    }
                })?;
                if !seen.insert(which.as_raw().addr()) {
                    return Ok(TaskCallbackSelector::Missing);
                }
                which = which.try_car()?.into_owned()?;
                continue;
            }
            _ => TaskCallbackSelector::Missing,
        };
        return Ok(selector);
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
    unsafe { crate::sexp::memory::with_arena(|a| a.total_bytes_allocated() as u64)
}
}

pub unsafe fn R_GetNSize() -> u64 {
    unsafe {
    crate::sexp::memory::with_arena(|a| a.node_count() as u64) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::accessors::TYPEOF;
    use crate::sexp::constructors::{Rf_cons, Rf_ScalarLogical};

    use crate::mainutils::rfile::{r_fclose, r_fopen};
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

    fn open_c_source(contents: &str) -> (PathBuf, *mut RFile) {
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
        let fp = unsafe { r_fopen(c_path.as_ptr(), c"r".as_ptr()) };
        assert!(!fp.is_null(), "failed to open source file");
        (path, fp)
    }

    unsafe fn close_c_source(path: PathBuf, fp: *mut RFile) {
        unsafe {
            r_fclose(fp);
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
            let expr = R_Parse1File(fp, 0, &mut status);
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
            R_ReplFile(fp, crate::sexp::globals::R_GlobalEnv());
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
        let _session = RSession::new_for_gc_tests();
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
        let left = RSession::new_for_gc_tests();
        let right = RSession::new_for_gc_tests();

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

#[cfg(test)]
mod owned_task_callback_tests {
    use super::*;
    use crate::sexp::{object::Sexp, owner::OwnerToken, session::RSession};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    fn source(session: &RSession, script: &str) -> Sexp<'static> {
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let factory = owner.node_factory();
            let expression = owner
                .with_arena(|arena| crate::eval::parser::parse(script, arena, factory.domain()))
                .unwrap()
                .unwrap();
            factory
                .wrap(unsafe {
                    Rf_eval(expression.as_raw(), session.global_env().unwrap().as_raw())
                })
                .unwrap()
                .into_owned()
                .unwrap()
        })
    }

    fn integer(session: &RSession, value: c_int) -> Sexp<'static> {
        session
            .owner_token()
            .unwrap()
            .node_factory()
            .allocate(|arena| {
                crate::sexp::builder::scalar_integer_in(arena, value).map(|value| value.as_raw())
            })
            .unwrap()
            .into_owned()
            .unwrap()
    }

    #[test]
    fn owned_task_callback_storage_roots_and_releases_actual_values() {
        let session = RSession::new_for_gc_tests();
        let fun = source(&session, "function(expr,value,succeeded,visible,data) TRUE");
        let data = integer(&session, 37);
        let fun_node = fun.allocation().unwrap().clone();
        let data_node = data.allocation().unwrap().clone();
        let id = unsafe { Rf_addTaskCallback(fun.as_raw(), data.as_raw()) };
        drop(fun);
        drop(data);
        session.owner_token().unwrap().full_gc().unwrap();
        assert!(fun_node.is_live());
        assert!(data_node.is_live());
        let selector = integer(&session, id);
        assert_eq!(unsafe { Rf_removeTaskCallback(selector.as_raw()) }, TRUE);
        session.owner_token().unwrap().full_gc().unwrap();
        assert!(!fun_node.is_live());
        assert!(!data_node.is_live());
        assert_eq!(
            unsafe { Rf_removeTaskCallback(std::ptr::null_mut()) },
            FALSE
        );
        let empty = source(&session, "integer(0)");
        assert_eq!(unsafe { Rf_removeTaskCallback(empty.as_raw()) }, FALSE);
    }

    #[test]
    fn owned_task_callback_selector_handles_cyclic_list_and_string_identity() {
        let session = RSession::new_for_gc_tests();
        let fun = source(&session, "function(...) TRUE");
        let id = unsafe { Rf_addTaskCallback(fun.as_raw(), R_NilValue()) };
        let factory = session.owner_token().unwrap().node_factory();
        let cycle = factory
            .pairlist_cell(&factory.nil(), &factory.nil(), &factory.nil())
            .unwrap();
        unsafe {
            crate::sexp::accessors::SETCAR(cycle.as_raw(), cycle.as_raw());
        }
        assert_eq!(unsafe { Rf_removeTaskCallback(cycle.as_raw()) }, FALSE);
        session.owner_token().unwrap().full_gc().unwrap();
        let name = source(&session, &format!("'{id}'"));
        assert_eq!(unsafe { Rf_removeTaskCallback(name.as_raw()) }, TRUE);
    }

    #[test]
    fn owned_task_callback_direct_managed_entry_supports_return_and_on_exit() {
        let session = RSession::new_for_gc_tests();
        let fun = source(
            &session,
            "function(...) { on.exit(callback_return_exit <<- TRUE); callback_return_body <<- TRUE; return(TRUE) }",
        );
        assert!(crate::sexp::transfer::active_owner_pin().is_err());
        let id = unsafe { Rf_addTaskCallback(fun.as_raw(), R_NilValue()) };
        unsafe {
            Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE);
        }
        let selector = integer(&session, id);
        assert_eq!(unsafe { Rf_removeTaskCallback(selector.as_raw()) }, TRUE);
        assert!(crate::sexp::transfer::active_owner_pin().is_err());
        assert_eq!(
            source(&session, "callback_return_body").logical_elt(0),
            Some(TRUE)
        );
        assert_eq!(
            source(&session, "callback_return_exit").logical_elt(0),
            Some(TRUE)
        );
    }

    #[test]
    fn owned_task_callback_missing_and_primitive_results_keep_until_false_like_gnu() {
        let session = RSession::new_for_gc_tests();
        // GNU bac583951 preserves NA/empty/nonlogical callbacks. Rboolean
        // removes only FALSE, including a successfully coerced numeric zero.
        for script in [
            "function(...) NA",
            "function(...) logical(0)",
            "function(...) NULL",
            ".Primitive('list')",
            ".Primitive('expression')",
            ".Primitive('{')",
        ] {
            let fun = source(&session, script);
            let id = unsafe { Rf_addTaskCallback(fun.as_raw(), R_NilValue()) };
            unsafe {
                Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE);
            }
            let selector = integer(&session, id);
            assert_eq!(
                unsafe { Rf_removeTaskCallback(selector.as_raw()) },
                TRUE,
                "{script}"
            );
        }
        for script in [
            "function(...) FALSE",
            "function(...) 0L",
            "function(...) 'FALSE'",
        ] {
            let fun = source(&session, script);
            let id = unsafe { Rf_addTaskCallback(fun.as_raw(), R_NilValue()) };
            unsafe {
                Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE);
            }
            let selector = integer(&session, id);
            assert_eq!(
                unsafe { Rf_removeTaskCallback(selector.as_raw()) },
                FALSE,
                "{script}"
            );
        }
    }

    #[test]
    fn owned_task_callback_caches_language_values_and_source_symbols() {
        let session = RSession::new_for_gc_tests();
        let fun = source(
            &session,
            "function(expr,value,succeeded,visible,data) { callback_observed <<- list(expr,value,succeeded,visible,data,substitute(expr),substitute(value),substitute(data)); invisible(FALSE) }",
        );
        let expr = source(&session, "quote(stop('expr must remain data'))");
        let value = source(&session, "quote(stop('value must remain data'))");
        let data = source(&session, "quote(stop('data must remain data'))");
        unsafe {
            Rf_addTaskCallback(fun.as_raw(), data.as_raw());
            R_SetVisible(TRUE);
        }
        unsafe {
            Rf_callToplevelHandlers(expr.as_raw(), value.as_raw(), TRUE, FALSE);
        }
        assert_eq!(unsafe { R_GetVisible() }, TRUE);
        let observed = source(&session, "callback_observed");
        assert_eq!(observed.vector_elt(0).unwrap().as_raw(), expr.as_raw());
        assert_eq!(observed.vector_elt(1).unwrap().as_raw(), value.as_raw());
        assert_eq!(observed.vector_elt(2).unwrap().logical_elt(0), Some(TRUE));
        assert_eq!(observed.vector_elt(3).unwrap().logical_elt(0), Some(FALSE));
        assert_eq!(observed.vector_elt(4).unwrap().as_raw(), data.as_raw());
        let check = source(
            &session,
            "identical(callback_observed[[6]],quote(expr)) && identical(callback_observed[[7]],quote(value)) && identical(callback_observed[[8]],quote(data))",
        );
        assert_eq!(check.logical_elt(0), Some(TRUE));
        session.with_active_in(|instance| unsafe {
            assert!((*instance).main_state.task_callbacks.is_empty());
            assert!(!(*instance).main_state.running_toplevel_handlers);
        });
    }

    #[test]
    fn owned_task_callback_mutation_preserves_original_input_sharing() {
        let session = RSession::new_for_gc_tests();
        let fun = source(
            &session,
            "function(expr,value,succeeded,visible,data) { value[1] <- 99L; data[1] <- 88L; callback_mutated <<- list(value,data); FALSE }",
        );
        let value = integer(&session, 3);
        let data = integer(&session, 4);
        unsafe {
            Rf_addTaskCallback(fun.as_raw(), data.as_raw());
            Rf_callToplevelHandlers(R_NilValue(), value.as_raw(), TRUE, TRUE);
        }
        assert_eq!(value.integer_elt(0), Some(3));
        assert_eq!(data.integer_elt(0), Some(4));
        let mutated = source(&session, "callback_mutated");
        assert_eq!(mutated.vector_elt(0).unwrap().integer_elt(0), Some(99));
        assert_eq!(mutated.vector_elt(1).unwrap().integer_elt(0), Some(88));
    }

    #[test]
    fn owned_task_callback_iteration_handles_preceding_self_removal_and_addition() {
        let session = RSession::new_for_gc_tests();
        source(&session, "callback_log <- integer(0)");
        let a = source(
            &session,
            "function(...) { callback_log <<- c(callback_log,1L); TRUE }",
        );
        let b = source(
            &session,
            "function(...) { callback_log <<- c(callback_log,2L); .Internal(removeTaskCallback(1L)); .Internal(removeTaskCallback(2L)); .Internal(addTaskCallback(function(...) { callback_log <<- c(callback_log,4L); FALSE },NULL)); TRUE }",
        );
        let c = source(
            &session,
            "function(...) { callback_log <<- c(callback_log,3L); FALSE }",
        );
        unsafe {
            assert_eq!(Rf_addTaskCallback(a.as_raw(), R_NilValue()), 1);
            assert_eq!(Rf_addTaskCallback(b.as_raw(), R_NilValue()), 2);
            assert_eq!(Rf_addTaskCallback(c.as_raw(), R_NilValue()), 3);
            Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE);
        }
        let log = source(&session, "callback_log");
        assert_eq!(
            (0..log.len())
                .map(|i| log.integer_elt(i).unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        session.with_active_in(|instance| unsafe {
            assert!((*instance).main_state.task_callbacks.is_empty());
        });
    }

    #[test]
    fn owned_task_callback_snapshot_survives_collecting_reentrant_removal() {
        let session = RSession::new_for_gc_tests();
        let fun = source(
            &session,
            "function(expr,value,succeeded,visible,data) { callback_survived <<- data; FALSE }",
        );
        let data = integer(&session, 71);
        let fun_node = fun.allocation().unwrap().clone();
        let data_node = data.allocation().unwrap().clone();
        let id = unsafe { Rf_addTaskCallback(fun.as_raw(), data.as_raw()) };
        let selector = integer(&session, id);
        drop(fun);
        drop(data);
        let calls = Rc::new(Cell::new(0));
        let seen = calls.clone();
        session.with_active_in(|instance| unsafe {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if seen.replace(seen.get() + 1) == 0 {
                    (*instance).memory_state.gc_force_gap = 0;
                    assert_eq!(Rf_removeTaskCallback(selector.as_raw()), TRUE);
                    Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE);
                    OwnerToken::from_raw(instance).full_gc().unwrap();
                    assert!(fun_node.is_live());
                    assert!(data_node.is_live());
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE);
            assert!(!(*instance).main_state.running_toplevel_handlers);
        });
        assert!(calls.get() > 0);
        assert_eq!(
            source(&session, "callback_survived").integer_elt(0),
            Some(71)
        );
    }

    #[test]
    fn owned_task_callback_error_and_collecting_panic_restore_running_and_visibility() {
        let session = RSession::new_for_gc_tests();
        let bad = source(&session, "function(...) stop('callback failure')");
        let good = source(
            &session,
            "function(...) { callback_after_error <<- 1L; FALSE }",
        );
        unsafe {
            Rf_addTaskCallback(bad.as_raw(), R_NilValue());
            Rf_addTaskCallback(good.as_raw(), R_NilValue());
            R_SetVisible(FALSE);
            Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE);
        }
        assert_eq!(unsafe { R_GetVisible() }, FALSE);
        assert_eq!(
            source(&session, "callback_after_error").integer_elt(0),
            Some(1)
        );
        let callback = source(&session, "function(...) TRUE");
        unsafe {
            Rf_addTaskCallback(callback.as_raw(), R_NilValue());
            R_SetVisible(FALSE);
        }
        let fired = Rc::new(Cell::new(false));
        let injected = fired.clone();
        session.with_active_in(|instance| unsafe {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !injected.replace(true) {
                    (*instance).memory_state.gc_force_gap = 0;
                    panic!("collecting callback unwind");
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE);
            assert!(!(*instance).main_state.running_toplevel_handlers);
            assert_eq!((*instance).eval_state.visible, FALSE);
        });
        assert!(fired.get());
    }

    #[test]
    fn owned_task_callback_cleanup_pins_original_after_callback_drops_facade() {
        let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
        let original = facade
            .borrow()
            .as_ref()
            .unwrap()
            .owner_token()
            .unwrap()
            .weak_owner()
            .unwrap();
        let pin = original.pin().unwrap();
        let instance = pin.as_ptr();
        let fun = source(facade.borrow().as_ref().unwrap(), "function(...) TRUE");
        unsafe {
            Rf_addTaskCallback(fun.as_raw(), R_NilValue());
        }
        let other = RSession::new_for_gc_tests();
        other.with_active(|| unsafe {
            R_SetVisible(TRUE);
        });
        let callback_facade = Rc::downgrade(&facade);
        unsafe {
            crate::sexp::session::with_instance_active(instance, || {
                (*instance).eval_state.visible = FALSE;
                crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                    assert!((*instance).main_state.running_toplevel_handlers);
                    drop(callback_facade.upgrade().unwrap().borrow_mut().take());
                }));
                (*instance).memory_state.gc_force_gap = 1;
                (*instance).memory_state.gc_force_wait = 1;
                Rf_callToplevelHandlers(R_NilValue(), R_NilValue(), TRUE, TRUE);
            });
            assert!(!(*instance).main_state.running_toplevel_handlers);
            assert_eq!((*instance).eval_state.visible, FALSE);
        }
        assert!(facade.borrow().is_none());
        assert!(!original.is_live());
        other.with_active(|| assert_eq!(unsafe { R_GetVisible() }, TRUE));
    }
}
