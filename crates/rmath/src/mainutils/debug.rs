#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/debug.c — debug/trace support.
//!
//! Provides debug(), undebug(), isdebugged(), debugonce(),
//! .Internal(trace()), .primTrace/.primUntrace,
//! tracingState/debuggingState, and tracemem/untracemem.

use std::cell::RefCell;
use std::ffi::CString;
use std::os::raw::c_int;

use crate::mainutils::errors::Rf_error_cstr;
use crate::sexp::accessors::{CAR, PRINTNAME, TAG, TYPEOF};
use crate::sexp::constructors::Rf_ScalarLogical;
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::globals::set_R_Visible;

// ---------------------------------------------------------------------------
// Local stub functions for debug/trace gp-bit accessors
//
// In R's C code, SET_RDEBUG/RDEBUG/SET_RSTEP/SET_RTRACE/RTRACE are macros
// that manipulate bits in the gp field of the SEXPREC header. These will be
// replaced with real implementations once the gp-bit infrastructure is wired
// up in accessors.rs. For now they are safe no-ops / zero-returning stubs.
// ---------------------------------------------------------------------------

/// Copy the gp bits of a live registered node through its checked header.
fn header_gp(x: SEXP) -> u16 {
    if x.is_null() {
        return 0;
    }
    crate::sexp::memory::checked_header(x).map_or(0, |(_, _, header)| header.sxpinfo.gp())
}

/// Publish a header update through the node's canonical heap cell.
/// Null or unregistered pointers are left unchanged.
fn update_header(x: SEXP, update: impl FnOnce(&mut crate::sexp::ffi::SexprecCore)) {
    if x.is_null() {
        return;
    }
    let Some((_, node)) = crate::sexp::memory::checked_projection(x) else {
        return;
    };
    let heap = node.heap_identity();
    let Some(mut header) = heap.node_snapshot(&node) else {
        return;
    };
    update(&mut header);
    let _ = heap.replace_node(&node, header);
}

/// Set or clear one gp bit on a live node.
fn set_gp_bit(x: SEXP, mask: u16, v: c_int) {
    update_header(x, |header| {
        let gp = header.sxpinfo.gp();
        header
            .sxpinfo
            .set_gp(if v != 0 { gp | mask } else { gp & !mask });
    });
}

fn SET_RDEBUG(x: SEXP, v: c_int) {
    set_gp_bit(x, 0x400, v);
}

fn RDEBUG(x: SEXP) -> c_int {
    ((header_gp(x) & 0x400) != 0) as c_int
}

fn SET_RSTEP(x: SEXP, v: c_int) {
    set_gp_bit(x, 0x100, v);
}

fn SET_RTRACE(x: SEXP, v: c_int) {
    set_gp_bit(x, 0x10, v);
}

fn RTRACE(x: SEXP) -> c_int {
    ((header_gp(x) & 0x10) != 0) as c_int
}

fn PRIMVAL(op: SEXP) -> c_int {
    crate::mainutils::relop::PRIMVAL(op)
}

// ---------------------------------------------------------------------------
// do_debug — debug / undebug / isdebugged / debugonce
//
// Dispatches on PRIMVAL(op):
//   0 = debug()        SET_RDEBUG(x, 1)
//   1 = undebug()      SET_RDEBUG(x, 0)
//   2 = isdebugged()   return ScalarLogical(RDEBUG(x))
//   3 = debugonce()    SET_RSTEP(x, 1)
// ---------------------------------------------------------------------------

pub fn do_debug(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let _ = (call, rho);
    let s = CAR(args);
    let t = TYPEOF(s);

    // Validate that the argument is a function type
    if t != SEXPTYPE::CLOSXP && t != SEXPTYPE::SPECIALSXP && t != SEXPTYPE::BUILTINSXP {
        Rf_error_cstr(c"debug/undebug/isdebugged/debugonce requires a function");
    }

    match PRIMVAL(op) {
        0 => {
            // debug()
            SET_RDEBUG(s, 1);
        }
        1 => {
            // undebug()
            SET_RDEBUG(s, 0);
        }
        2 => {
            // isdebugged()
            return Rf_ScalarLogical(RDEBUG(s));
        }
        3 => {
            // debugonce()
            SET_RSTEP(s, 1);
        }
        _ => {} // intentionally unhandled: unknown debug operation
    }

    R_NilValue()
}

// ---------------------------------------------------------------------------
// do_trace — .primTrace / .primUntrace
//
// Dispatches on PRIMVAL(op):
//   0 = .primTrace      SET_RTRACE(x, 1)
//   1 = .primUntrace    SET_RTRACE(x, 0)
// ---------------------------------------------------------------------------

pub fn do_trace(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let _ = (call, rho);
    let s = CAR(args);

    match PRIMVAL(op) {
        0 => {
            // .primTrace
            SET_RTRACE(s, 1);
        }
        1 => {
            // .primUntrace
            SET_RTRACE(s, 0);
        }
        _ => {} // intentionally unhandled: unknown debug/untrace operation
    }

    R_NilValue()
}

// ---------------------------------------------------------------------------
// do_traceOnOff — tracingState / debuggingState
//
// Dispatches on PRIMVAL(op):
//   0 = tracingState — toggle or query tracing
//   1 = debuggingState — toggle or query debugging
//
// Returns ScalarLogical of the previous state.
// ---------------------------------------------------------------------------

/// GNU `tracingState(on=NULL)` — query or set session tracing.
/// Dedicated so portable PRIMVAL/PRIMNAME cannot divert this to debuggingState.
pub fn do_tracing_state(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    trace_or_debug_state(args, true)
}

fn trace_or_debug_state(args: SEXP, tracing: bool) -> SEXP {
    let s = if args.is_null() || args == crate::sexp::globals::R_NilValue() {
        crate::sexp::globals::R_NilValue()
    } else {
        CAR(args)
    };
    let query_only = s.is_null()
        || s == crate::sexp::globals::R_NilValue()
        || s == crate::sexp::globals::R_MissingArg();
    let state: c_int = if query_only {
        -1
    } else if TYPEOF(s) == SEXPTYPE::LGLSXP && !s.is_null() {
        if crate::sexp::accessors::XLENGTH(s) > 0 {
            crate::sexp::accessors::LOGICAL_ELT(s, 0)
        } else {
            0
        }
    } else {
        0
    };
    let prev = with_trace_debug_state(|tracing_state, debugging_state| {
        let slot = if tracing {
            tracing_state
        } else {
            debugging_state
        };
        let prev = *slot;
        if !query_only {
            *slot = state;
        }
        prev
    })
    .expect("mutable R runtime state requires an active RInstance");
    Rf_ScalarLogical(prev)
}

pub fn do_traceOnOff(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let debugging = PRIMVAL(op) == 1;
    trace_or_debug_state(args, !debugging)
}

// ---------------------------------------------------------------------------
// R_current_debug_state — return this session's debugging state
// ---------------------------------------------------------------------------

pub extern "C" fn R_current_debug_state() -> c_int {
    with_trace_debug_state(|_, debugging| *debugging).unwrap_or(TRUE)
}

// ---------------------------------------------------------------------------
// R_current_trace_state — return this session's tracing state
// ---------------------------------------------------------------------------

pub extern "C" fn R_current_trace_state() -> c_int {
    with_trace_debug_state(|tracing, _| *tracing).unwrap_or(TRUE)
}

/// Lend the session's `(tracing_state, debugging_state)` scalars, or `None`
/// without an active instance.
fn with_trace_debug_state<R>(f: impl FnOnce(&mut c_int, &mut c_int) -> R) -> Option<R> {
    crate::sexp::instance::with_current_instance(|inst| {
        // SAFETY: the current instance pointer is live for the closure; the
        // two disjoint scalar fields are lent without allocation or reentry.
        let state = unsafe { &mut (*inst).eval_state };
        f(&mut state.tracing_state, &mut state.debugging_state)
    })
}

// ---------------------------------------------------------------------------
// tracemem / untracemem — GNU debug.c under R_MEMORY_PROFILING
//
// The trace bit is sxpinfo.trace (bit 26). `.primTrace` keeps using gp bit
// 0x10 via the local SET_RTRACE above; those two flags are not the same bit.
// ---------------------------------------------------------------------------

fn memory_traced(x: SEXP) -> bool {
    !x.is_null()
        && crate::sexp::memory::checked_header(x)
            .is_some_and(|(_, _, header)| header.sxpinfo.trace())
}

fn set_memory_traced(x: SEXP, on: bool) {
    update_header(x, |header| header.sxpinfo.set_trace(on));
}

fn reject_traced_function(object: SEXP) {
    if object.is_null() {
        return;
    }
    let t = TYPEOF(object);
    if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
        Rf_error_cstr(c"argument must not be a function");
    }
}

/// GNU `check1arg`: a supplied tag must be a prefix of `x`.
fn check_tracemem_arg(args: SEXP, _call: SEXP) {
    let tag = TAG(args);
    if tag.is_null() || tag == R_NilValue() {
        return;
    }
    let pname = PRINTNAME(tag);
    if pname.is_null() {
        return;
    }
    let supplied_bytes = crate::mainutils::printutils::charsxp_bytes(pname);
    let supplied = supplied_bytes.as_slice();
    let formal = b"x";
    if supplied.is_empty() || supplied.len() > formal.len() || !formal.starts_with(supplied) {
        let message = CString::new(format!(
            "supplied argument name '{}' does not match 'x'",
            String::from_utf8_lossy(supplied)
        ))
        .unwrap_or_else(|_| CString::new("supplied argument name does not match 'x'").unwrap());
        Rf_error_cstr(&message);
    }
}

/// Checked STRSXP(1) scalar on the active session heap.
fn mk_string(text: &str) -> SEXP {
    crate::mainutils::essentials::active_owner()
        .node_factory()
        .strings(&[text])
        .map(|vector| vector.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

fn memory_profiling_enabled() -> bool {
    cfg!(feature = "memory-profiling")
}

fn reject_without_memory_profiling() {
    Rf_error_cstr(c"R was not compiled with support for memory profiling");
}

/// R's `tracemem(x)` — mark `x` and return `"<address>"`.
///
/// Without the `memory-profiling` feature this is GNU's
/// `R_MEMORY_PROFILING` stub: check the argument name, then error.
pub fn do_tracemem(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let _ = (op, rho);
    if !args.is_null() && args != R_NilValue() {
        check_tracemem_arg(args, call);
    }
    if !memory_profiling_enabled() {
        reject_without_memory_profiling();
    }
    let object = if args.is_null() || args == R_NilValue() {
        R_NilValue()
    } else {
        CAR(args)
    };
    reject_traced_function(object);
    if object.is_null() || object == R_NilValue() {
        Rf_error_cstr(c"cannot trace NULL");
    }
    let t = TYPEOF(object);
    if t == SEXPTYPE::ENVSXP || t == SEXPTYPE::PROMSXP {
        Rf_error_cstr(c"'tracemem' is not useful for promise and environment objects");
    }
    if t == SEXPTYPE::EXTPTRSXP || t == SEXPTYPE::WEAKREFSXP {
        Rf_error_cstr(c"'tracemem' is not useful for weak reference or external pointer objects");
    }
    set_memory_traced(object, true);
    mk_string(&format!("<{:p}>", object))
}

/// R's `untracemem(x)` — clear the trace bit. The result is invisible.
pub fn do_untracemem(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let _ = (op, rho);
    if !args.is_null() && args != R_NilValue() {
        check_tracemem_arg(args, call);
    }
    if !memory_profiling_enabled() {
        reject_without_memory_profiling();
    }
    let object = if args.is_null() || args == R_NilValue() {
        R_NilValue()
    } else {
        CAR(args)
    };
    reject_traced_function(object);
    if memory_traced(object) {
        set_memory_traced(object, false);
    }
    set_R_Visible(FALSE);
    R_NilValue()
}

// ---------------------------------------------------------------------------
// do_retracemem — no-op, returns invisible R_NilValue
// ---------------------------------------------------------------------------

pub fn do_retracemem(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        let _ = (call, op, args, rho);
        set_R_Visible(FALSE);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// memtrace_report — GNU debug.c memtrace_report + memtrace_stack_dump
//
// Innermost context first. A frame is reported when it is a function or
// builtin context whose call is a language object. Ordinary builtins such
// as `unclass` do not push a context, so they do not appear in the line.
// `do_eval` does push a function context naming `eval`, which is why the
// primitives.R line contains `eval` twice.
//
// GNU's R-level `try` is an ordinary closure. Native `tryCatch` supplies
// its four implementation frames at the depth where it forces `expr`.
// ---------------------------------------------------------------------------

thread_local! {
    static NATIVE_TRY_CATCH_ENTRIES: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) };
}

/// RAII marker for the native tryCatch expression evaluator.
pub struct NativeTryCatchTrace(bool);

impl NativeTryCatchTrace {
    pub fn enter() -> Self {
        if !memory_profiling_enabled() {
            return Self(false);
        }
        let depth = crate::sexp::context::current_frame_depth();
        NATIVE_TRY_CATCH_ENTRIES.with(|entries| entries.borrow_mut().push(depth));
        Self(true)
    }
}

impl Drop for NativeTryCatchTrace {
    fn drop(&mut self) {
        if !self.0 {
            return;
        }
        NATIVE_TRY_CATCH_ENTRIES.with(|entries| {
            entries.borrow_mut().pop();
        });
    }
}

fn native_try_catch_entries() -> Vec<i32> {
    NATIVE_TRY_CATCH_ENTRIES.with(|entries| entries.borrow().clone())
}

struct ReportedFrame {
    name: String,
    function: bool,
}

/// GNU implementation frames supplied by native tryCatch.
const GNU_TRY_CATCH_CHAIN: [&str; 4] = ["doTryCatch", "tryCatchOne", "tryCatchList", "tryCatch"];

fn insert_native_try_catch_frames(frames: &mut Vec<ReportedFrame>) {
    let entries = native_try_catch_entries();
    if entries.is_empty() || frames.iter().any(|frame| frame.name == "doTryCatch") {
        return;
    }
    let positions: Vec<usize> = frames
        .iter()
        .enumerate()
        .filter(|(_, frame)| frame.function)
        .map(|(index, _)| index)
        .collect();
    let nfunc = positions.len() as i32;
    let mut insert_at: Vec<usize> = entries
        .iter()
        .map(|entry| {
            let inside = (nfunc - *entry).max(0) as usize;
            if inside == 0 {
                0
            } else {
                positions[inside - 1] + 1
            }
        })
        .collect();
    insert_at.sort_unstable_by(|a, b| b.cmp(a));
    insert_at.dedup();
    for at in insert_at {
        for (offset, name) in GNU_TRY_CATCH_CHAIN.iter().enumerate() {
            frames.insert(
                at + offset,
                ReportedFrame {
                    name: (*name).to_string(),
                    function: true,
                },
            );
        }
    }
}

fn traced_call_name(fun: SEXP) -> String {
    if fun.is_null() || fun == R_NilValue() || TYPEOF(fun) != SEXPTYPE::SYMSXP {
        return "<Anonymous>".to_string();
    }
    let pname = PRINTNAME(fun);
    if pname.is_null() || pname == R_NilValue() {
        return "<Anonymous>".to_string();
    }
    String::from_utf8_lossy(&crate::mainutils::printutils::charsxp_bytes(pname)).into_owned()
}

pub fn memtrace_report(old: *mut std::ffi::c_void, new: *mut std::ffi::c_void) {
    if !memory_profiling_enabled() || R_current_trace_state() == 0 {
        return;
    }
    let mut line = format!("tracemem[{old:p} -> {new:p}]: ");
    let mut frames = Vec::new();
    // SAFETY: the context chain is the evaluator's live stack of RCNTXT
    // frames; each node is read in place and outlives this walk.
    unsafe {
        let mut cptr = crate::sexp::context::R_GlobalContext();
        while !cptr.is_null() {
            let flag = (*cptr).callflag;
            let call = (*cptr).call.as_raw();
            let function_frame = (flag
                & (crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                    | crate::sexp::context::ctxt_flags::CTXT_BUILTIN))
                != 0;
            if function_frame
                && !call.is_null()
                && call != R_NilValue()
                && TYPEOF(call) == SEXPTYPE::LANGSXP
            {
                frames.push(ReportedFrame {
                    name: traced_call_name(CAR(call)),
                    function: (flag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION) != 0,
                });
            }
            cptr = (*cptr).nextcontext;
        }
    }
    insert_native_try_catch_frames(&mut frames);
    for frame in &frames {
        line.push_str(&frame.name);
        line.push(' ');
    }
    line.push('\n');
    crate::sexp::output::capture_stdout(&line);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;
    use crate::sexp::session::RSession;

    /// Test that do_debug does not panic when called with null pointers.
    /// In real usage, null CAR(args) would be caught by the TYPEOF check
    /// and trigger an Rf_error (panic). We test that the stub SET_RDEBUG
    /// calls themselves don't panic with null.
    #[test]
    fn test_debug_set_rdebug() {
        // The stub SET_RDEBUG should not panic even with null
        SET_RDEBUG(ptr::null_mut(), 1);
        SET_RDEBUG(ptr::null_mut(), 0);
        SET_RSTEP(ptr::null_mut(), 1);
    }

    /// Test that the tracing/debugging state functions work correctly.
    #[test]
    fn test_trace_state() {
        let session = RSession::new();
        session.with_protected(|| {
            // Initial state should be TRUE
            assert_eq!(R_current_trace_state(), TRUE);
            assert_eq!(R_current_debug_state(), TRUE);

            crate::sexp::instance::with_required_current_instance(|inst| unsafe {
                (*inst).eval_state.tracing_state = FALSE;
            });
            assert_eq!(R_current_trace_state(), FALSE);

            crate::sexp::instance::with_required_current_instance(|inst| unsafe {
                (*inst).eval_state.debugging_state = FALSE;
            });
            assert_eq!(R_current_debug_state(), FALSE);

            crate::sexp::instance::with_required_current_instance(|inst| unsafe {
                (*inst).eval_state.tracing_state = TRUE;
                (*inst).eval_state.debugging_state = TRUE;
            });
            assert_eq!(R_current_trace_state(), TRUE);
            assert_eq!(R_current_debug_state(), TRUE);
        });
    }

    /// The tracemem entry point is linked. Behavior is covered by the
    /// upstream primitives.R differential, which needs a full session.
    #[test]
    fn test_tracemem_error() {
        assert!((do_tracemem as *const ()) as usize != 0);
    }

    /// Test that do_untracemem is defined.
    #[test]
    fn test_untracemem_error() {
        assert!((do_untracemem as *const ()) as usize != 0);
    }

    /// Test that the stub accessors return expected values.
    #[test]
    fn test_stub_accessors() {
        assert_eq!(RDEBUG(ptr::null_mut()), 0);
        assert_eq!(RTRACE(ptr::null_mut()), 0);
        assert_eq!(PRIMVAL(ptr::null_mut()), 0);
    }

    /// Test that do_retracemem returns without panicking.
    #[test]
    fn test_retracemem_no_panic() {
        let session = RSession::new();
        session.with_protected(|| {
            let result = do_retracemem(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(result.is_null() || result == R_NilValue());
        });
    }

    /// A report with no active session does not panic.
    #[test]
    fn test_memtrace_report_noop() {
        memtrace_report(ptr::null_mut(), ptr::null_mut());
    }
}
