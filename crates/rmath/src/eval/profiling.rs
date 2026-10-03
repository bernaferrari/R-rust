#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! R profiling support -- ports profiling functions from eval.c.
//!
//! This module provides the R profiler (Rprof()) and related functions.
//! The profiler samples the call stack at regular intervals and writes
//! results to a file for later analysis.
//!
//! Ported from R's src/main/eval.c profiling sections (~lines 38-935).
//!
//! Key functions:
//! - `do_Rprof` / `R_InitProfiling` / `R_EndProfiling` -- main profiling lifecycle
//! - `doprof` / `doprof_null` -- profiling signal handlers
//! - `ProfileThread` -- profiling timer thread (Unix/pthreads)
//! - `lineprof` / `getFilenum` -- line profiling helpers
//! - `pb_str` / `pb_uint` / `pb_int` / `pb_dbl` -- profiling buffer writers
//! - `pf_str` / `pf_int` -- profiling file writers
//! - `findProfContext` -- context traversal for profiling
//! - `do_bcprofstart` / `do_bcprofstop` / `do_bcprofcounts` -- BC profiling
//! - `dobcprof` / `dobcprof_null` -- BC profiling signal handlers

// wasm32 stubs (below) leave a few native-only imports unused.
#![cfg_attr(target_arch = "wasm32", allow(unused_imports))]

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;

use crate::eval::attrib_core::getAttrib;

use crate::sexp::accessors::{
    CADDR, CADR, CAR, CDR, CHAR, INTEGER, LENGTH, PRINTNAME, RAW, REAL, STRING_ELT, TYPEOF, XLENGTH,
};
use crate::sexp::constructors::{Rf_allocVector, Rf_mkString};
use crate::sexp::context::RCNTXT;
use crate::sexp::context::ctxt_flags;
use crate::sexp::envir::R_findVar;
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_FINITE, TRUE};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance::{
    NO_PROFILING_OPCODE, PROFILING_OPCODE_COUNT, ProfilingState, RInstance,
    with_required_current_instance,
};

/// Profiling timer type (ITIMER_PROF is not available on Android).
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
const PROF_TIMER: core::ffi::c_int = libc::ITIMER_PROF;
#[cfg(all(target_os = "android", not(target_arch = "wasm32")))]
const PROF_TIMER: core::ffi::c_int = 2; // ITIMER_PROF value on most Unix systems
/// WASM M1: no setitimer; timer fns below are wasm no-op stubs.
#[cfg(target_arch = "wasm32")]
const PROF_TIMER: c_int = 2;
use crate::sexp::symbol::Rf_install;
use crate::sexp::symbol::{
    R_Bracket2Symbol, R_DollarSymbol, R_DoubleColonSymbol, R_TripleColonSymbol,
};

// ---------------------------------------------------------------------------
// Stubs for functions not yet ported
// ---------------------------------------------------------------------------

unsafe fn get_R_InBCInterpreter() -> SEXP {
    unsafe { R_NilValue() }
}

unsafe fn get_R_ToplevelContext() -> *mut RCNTXT {
    super::runtime::global_context()
}

unsafe fn R_findBCInterpreterSrcref(_cptr: *mut RCNTXT) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// BC profiling constants
// ---------------------------------------------------------------------------

/// Number of bytecode opcodes -- sentinel value in the BC opcode enum.
/// In R, this is the last entry in the bytecode opcode enum (eval.c line ~4732).
/// We use 256 as a reasonable upper bound covering all defined opcodes.
const OPCOUNT: usize = PROFILING_OPCODE_COUNT;

/// Sentinel for "no current opcode" during BC profiling.
const NO_CURRENT_OPCODE: c_int = NO_PROFILING_OPCODE;

/// Buffer size for profiling output.
const PROFBUFSIZ: usize = 10500;

/// Maximum digits for IEEE double integer part printing.
const PB_MAX_DBL_DIGITS: usize = 309;

/// Profiling event type: CPU time or elapsed time.
#[derive(Clone, Copy, PartialEq)]
#[repr(C)]
pub enum rpe_type {
    RPE_CPU = 0,
    RPE_ELAPSED = 1,
}

fn profiling_event_code(event: rpe_type) -> c_int {
    event as c_int
}

fn profiling_event_from_code(event: c_int) -> rpe_type {
    if event == rpe_type::RPE_ELAPSED as c_int {
        rpe_type::RPE_ELAPSED
    } else {
        rpe_type::RPE_CPU
    }
}

fn with_profiling_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut ProfilingState) -> R,
{
    with_required_current_instance(|instance| with_profiling_state_in(instance, f))
}

pub(crate) fn with_profiling_state_in<F, R>(instance: *mut RInstance, f: F) -> R
where
    F: FnOnce(&mut ProfilingState) -> R,
{
    // P1: the &mut ProfilingState lend lives only across `f`, and every
    // caller's closure is strictly local state arithmetic (counters, flags,
    // file descriptors) — no allocation, protect, or eval can reenter here.
    f(unsafe { &mut (*instance).eval_state.profiling })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MemoryProfileSnapshot {
    current_bytes: u64,
    peak_bytes: u64,
    active_nodes: u64,
    gc_freed_nodes: u64,
}

fn memory_profile_snapshot() -> MemoryProfileSnapshot {
    with_required_current_instance(memory_profile_snapshot_in)
}

fn memory_profile_snapshot_in(instance: *mut RInstance) -> MemoryProfileSnapshot {
    // P1/P2: all borrows are statement-local reads/writes of plain counters;
    // nothing here allocates or reenters the interpreter.
    unsafe {
        let current_bytes = (*instance).arena.total_bytes_allocated();
        let peak_bytes = (*instance)
            .eval_state
            .profiling
            .memory_peak_bytes
            .max(current_bytes)
            .max((*instance).gc_state.stats.peak_memory);
        (*instance).eval_state.profiling.memory_peak_bytes = peak_bytes;

        MemoryProfileSnapshot {
            current_bytes: current_bytes as u64,
            peak_bytes: peak_bytes as u64,
            active_nodes: (*instance).arena.node_count() as u64,
            gc_freed_nodes: (*instance).gc_state.stats.freed as u64,
        }
    }
}

unsafe fn write_memory_profile_prefix(pb: *mut profbuf, snapshot: MemoryProfileSnapshot) {
    unsafe {
        pb_str(pb, b":\0".as_ptr() as *const c_char);
        pb_uint(pb, snapshot.current_bytes);
        pb_str(pb, b":\0".as_ptr() as *const c_char);
        pb_uint(pb, snapshot.peak_bytes);
        pb_str(pb, b":\0".as_ptr() as *const c_char);
        pb_uint(pb, snapshot.active_nodes);
        pb_str(pb, b":\0".as_ptr() as *const c_char);
        pb_uint(pb, snapshot.gc_freed_nodes);
        pb_str(pb, b":\0".as_ptr() as *const c_char);
    }
}

// ---------------------------------------------------------------------------
// R_Profiling -- check if profiling is active
// ---------------------------------------------------------------------------

/// Check whether R profiling is currently active.
pub fn R_Profiling_active() -> c_int {
    with_required_current_instance(R_Profiling_active_in)
}

pub(crate) fn R_Profiling_active_in(instance: *mut RInstance) -> c_int {
    unsafe { (*instance).eval_state.profiling.profiling }
}

// ---------------------------------------------------------------------------
// R_isRprofiling -- check if profiling is enabled
// ---------------------------------------------------------------------------

/// Check if R profiling is enabled (public API).
pub fn R_isRprofiling() -> c_int {
    with_required_current_instance(R_isRprofiling_in)
}

pub(crate) fn R_isRprofiling_in(instance: *mut RInstance) -> c_int {
    unsafe { (*instance).eval_state.profiling.profiling }
}

// ---------------------------------------------------------------------------
// profbuf -- profiling output buffer
// ---------------------------------------------------------------------------

/// Profiling output buffer structure.
///
/// The `pb_*` functions write to this buffer, advancing `ptr` and
/// maintaining `left`. If the write wouldn't fit leaving one more byte
/// available for the terminator, `left` is set to zero.
#[repr(C)]
struct profbuf {
    ptr: *mut c_char,
    left: usize,
}

// ---------------------------------------------------------------------------
// pb_str -- write string to profiling buffer
// ---------------------------------------------------------------------------

/// Write a string to the profiling buffer.
///
/// If the string fits (with room for terminator), add it excluding the
/// terminator. If it doesn't fit, set `left` to 0.
///
/// Ported from R's `pb_str()` in eval.c.
unsafe fn pb_str(pb: *mut profbuf, s: *const c_char) {
    unsafe {
        let mut len: usize = 0;
        while *s.add(len) != 0 {
            len += 1;
        }
        if len < (*pb).left {
            for i in 0..len {
                *(*pb).ptr.add(i) = *s.add(i);
            }
            (*pb).ptr = (*pb).ptr.add(len);
            (*pb).left -= len;
        } else {
            (*pb).left = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// pb_uint -- write uint64 to profiling buffer
// ---------------------------------------------------------------------------

/// Write an unsigned 64-bit integer to the profiling buffer.
///
/// Ported from R's `pb_uint()` in eval.c.
unsafe fn pb_uint(pb: *mut profbuf, num: u64) {
    unsafe {
        let mut digits = [0u8; 20]; // 64-bit unsigned integers
        let mut i: usize = 0;
        let mut n = num;

        loop {
            digits[i] = (n % 10) as u8 + b'0';
            i += 1;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        if i < (*pb).left {
            let mut j: usize = 0;
            // Reverse digits
            let mut k = i as isize - 1;
            while k >= 0 {
                *(*pb).ptr.add(j) = digits[k as usize] as c_char;
                j += 1;
                k -= 1;
            }
            (*pb).ptr = (*pb).ptr.add(j);
            (*pb).left -= j;
        } else {
            (*pb).left = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// pb_int -- write int64 to profiling buffer
// ---------------------------------------------------------------------------

/// Write a signed 64-bit integer to the profiling buffer.
///
/// Ported from R's `pb_int()` in eval.c.
unsafe fn pb_int(pb: *mut profbuf, num: i64) {
    unsafe {
        let mut digits = [0u8; 19]; // 64-bit signed integers
        let mut i: usize = 0;
        let negative: bool;
        let mut n = num;

        if num < 0 {
            negative = true;
            n = -num;
        } else {
            negative = false;
        }

        loop {
            digits[i] = (n % 10) as u8 + b'0';
            i += 1;
            n /= 10;
            if n == 0 {
                break;
            }
        }

        let neg_flag: usize = if negative { 1 } else { 0 };
        if neg_flag + i < (*pb).left {
            if negative {
                *(*pb).ptr = '-' as c_char;
                (*pb).ptr = (*pb).ptr.add(1);
                (*pb).left -= 1;
            }
            let mut j: usize = 0;
            let mut k = i as isize - 1;
            while k >= 0 {
                *(*pb).ptr.add(j) = digits[k as usize] as c_char;
                j += 1;
                k -= 1;
            }
            (*pb).ptr = (*pb).ptr.add(j);
            (*pb).left -= j;
        } else {
            (*pb).left = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// pb_dbl -- write double (integer part) to profiling buffer
// ---------------------------------------------------------------------------

/// Write the integer part of a double to the profiling buffer.
///
/// Careful: this is very simplistic printing of the integer parts of doubles
/// (like %0.f) used only for stack trace in profiling data.
/// Not suitable for general re-use.
///
/// Ported from R's `pb_dbl()` in eval.c.
unsafe fn pb_dbl(pb: *mut profbuf, num: c_double) {
    unsafe {
        // Handle non-finite values
        if !R_FINITE(num) {
            // Check NA first (NA is also NaN, so order matters)
            // In R, ISNA checks for the specific NA pattern
            if num.to_bits() == f64::NAN.to_bits() {
                // Could be NA or NaN -- simplified check
                pb_str(pb, b"NaN\0".as_ptr() as *const c_char);
            } else if num > 0.0 {
                pb_str(pb, b"Inf\0".as_ptr() as *const c_char);
            } else {
                pb_str(pb, b"-Inf\0".as_ptr() as *const c_char);
            }
            return;
        }

        let mut digits = [0u8; PB_MAX_DBL_DIGITS];
        let mut i: usize = 0;
        let negative: bool;
        let mut n = num;

        if num < 0.0 {
            negative = true;
            n = -n;
        } else {
            negative = false;
        }

        loop {
            digits[i] = (n % 10.0) as u8 + b'0';
            i += 1;
            n /= 10.0;
            if n < 1.0 {
                break;
            }
            if i >= PB_MAX_DBL_DIGITS {
                // Cannot happen with IEEE double
                return;
            }
        }

        let neg_flag: usize = if negative { 1 } else { 0 };
        if neg_flag + i < (*pb).left {
            if negative {
                *(*pb).ptr = '-' as c_char;
                (*pb).ptr = (*pb).ptr.add(1);
                (*pb).left -= 1;
            }
            let mut j: usize = 0;
            let mut k = i as isize - 1;
            while k >= 0 {
                *(*pb).ptr.add(j) = digits[k as usize] as c_char;
                j += 1;
                k -= 1;
            }
            (*pb).ptr = (*pb).ptr.add(j);
            (*pb).left -= j;
        } else {
            (*pb).left = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// getFilenum -- get/create file number for line profiling
// ---------------------------------------------------------------------------

/// Get a file number for line profiling output.
///
/// Does a linear search through previously recorded filenames.
/// If this one is new, try to add it.
///
/// Ported from R's `getFilenum()` in eval.c.
unsafe fn getFilenum(filename: *const c_char) -> c_int {
    unsafe {
        if filename.is_null() {
            return 0;
        }
        // Copy before callbacks or buffer access can invalidate a borrowed name.
        let filename = std::ffi::CStr::from_ptr(filename).to_bytes().to_vec();
        let instance = with_required_current_instance(|instance| instance);
        let pin = crate::sexp::context::pin_context_owner_in(instance);
        let (buffer, offsets, used, capacity, line_prof) = with_profiling_state(|state| {
            (
                state.srcfiles_buffer.owned(),
                state.srcfiles.clone(),
                state.srcfile_bytes_used,
                state.srcfile_bufcount,
                state.line_profiling,
            )
        });
        let Some(buffer) = buffer else {
            return 0;
        };
        if line_prof <= 0 || buffer.typeof_() != SEXPTYPE::RAWSXP {
            return 0;
        }
        let total = XLENGTH(buffer.as_raw()) as usize;
        crate::sexp::context::require_context_owner_live(&pin);
        let bytes = RAW(buffer.as_raw());
        crate::sexp::context::require_context_owner_live(&pin);
        if !with_profiling_state(|state| state.srcfiles_buffer.as_raw() == buffer.as_raw()) {
            return 0;
        }
        for (index, offset) in offsets.iter().copied().enumerate() {
            let Some(remaining) = total.checked_sub(offset) else {
                with_profiling_state(|state| state.profiling_error = 2);
                return 0;
            };
            if remaining == 0 || bytes.is_null() {
                with_profiling_state(|state| state.profiling_error = 2);
                return 0;
            }
            let stored = std::slice::from_raw_parts(bytes.add(offset), remaining);
            let Some(end) = stored.iter().position(|byte| *byte == 0) else {
                with_profiling_state(|state| state.profiling_error = 2);
                return 0;
            };
            if stored[..end] == filename {
                return (index + 1) as c_int;
            }
        }
        if offsets.len() >= capacity {
            with_profiling_state(|state| state.profiling_error = 1);
            return 0;
        }
        let Some(next) = used
            .checked_add(filename.len())
            .and_then(|len| len.checked_add(1))
        else {
            with_profiling_state(|state| state.profiling_error = 2);
            return 0;
        };
        if next > total || bytes.is_null() {
            with_profiling_state(|state| state.profiling_error = 2);
            return 0;
        }
        ptr::copy_nonoverlapping(filename.as_ptr(), bytes.add(used), filename.len());
        *bytes.add(next - 1) = 0;
        with_profiling_state(|state| {
            state.srcfiles.push(used);
            state.srcfile_bytes_used = next;
            state.line_profiling = state.srcfiles.len() as c_int + 1;
        });
        (offsets.len() + 1) as c_int
    }
}

// ---------------------------------------------------------------------------
// lineprof -- write line profiling data
// ---------------------------------------------------------------------------

/// Write line profiling data to the profiling output buffer.
///
/// Ported from R's `lineprof()` in eval.c.
unsafe fn lineprof(pb: *mut profbuf, srcref: SEXP) {
    unsafe {
        if srcref.is_null() || srcref == R_NilValue() {
            return;
        }
        let instance = with_required_current_instance(|instance| instance);
        let pin = crate::sexp::context::pin_context_owner_in(instance);
        let srcref = crate::sexp::context::own_control_value(srcref);
        let line = crate::mainutils::coerce::asInteger(srcref.as_raw());
        crate::sexp::context::require_context_owner_live(&pin);
        if line == NA_INTEGER {
            return;
        }
        let srcfile_sym = Rf_install(c"srcfile".as_ptr());
        crate::sexp::context::require_context_owner_live(&pin);
        let srcfile = getAttrib(srcref.as_raw(), srcfile_sym);
        if srcfile.is_null() || srcfile == R_NilValue() || TYPEOF(srcfile) != SEXPTYPE::ENVSXP {
            return;
        }
        let srcfile = crate::sexp::context::own_control_value(srcfile);
        let filename_sym = Rf_install(c"filename".as_ptr());
        crate::sexp::context::require_context_owner_live(&pin);
        let filename = R_findVar(filename_sym, srcfile.as_raw());
        crate::sexp::context::require_context_owner_live(&pin);
        let filename = crate::sexp::context::own_control_value(filename);
        if filename.typeof_() != SEXPTYPE::STRSXP {
            return;
        }
        let length = filename.len();
        crate::sexp::context::require_context_owner_live(&pin);
        if length == 0 {
            return;
        }
        let chars = crate::sexp::context::own_control_value(STRING_ELT(filename.as_raw(), 0));
        crate::sexp::context::require_context_owner_live(&pin);
        let number = getFilenum(CHAR(chars.as_raw()));
        if number != 0 {
            pb_int(pb, number as i64);
            pb_str(pb, c"#".as_ptr());
            pb_int(pb, line as i64);
            pb_str(pb, c" ".as_ptr());
        }
    }
}

// ---------------------------------------------------------------------------
// findProfContext -- find next profiling context
// ---------------------------------------------------------------------------

/// Find the next context to include in the profile trace.
///
/// When `R_Filter_Callframes` is enabled, uses a more sophisticated algorithm
/// to skip intermediate frames. Otherwise, simply returns the next context.
///
/// Ported from R's `findProfContext()` in eval.c.
unsafe fn findProfContext(cptr: *mut RCNTXT, eval_internal: SEXP) -> *mut RCNTXT {
    unsafe {
        if with_profiling_state(|state| state.filter_callframes) == 0 {
            return (*cptr).nextcontext;
        }

        let toplevel = get_R_ToplevelContext();
        if cptr == toplevel {
            return ptr::null_mut();
        }

        // Find parent context, same algorithm as in `parent.frame()`
        let parent = super::context::R_findParentContext(cptr, 1);

        // If we're in a frame called by `eval()`, find the evaluation
        // environment higher up the stack, if any.
        let mut result = parent;
        if !parent.is_null() {
            let parent_callfun = (*parent).callfun.as_raw();
            if !parent_callfun.is_null() {
                if parent_callfun == eval_internal {
                    let sysparent = (*cptr).sysparent.as_raw();
                    result = super::context::R_findExecContext((*parent).nextcontext, sysparent);
                }
            }
        }

        if !result.is_null() {
            return result;
        }

        // Base case: this interrupts the iteration over context frames
        if (*cptr).nextcontext == toplevel {
            return ptr::null_mut();
        }

        // There is no parent frame and we haven't reached the top level
        // context. Find the very first context on the stack which should
        // always be included in the profiles.
        let mut c = cptr;
        while (*c).nextcontext != toplevel && !(*c).nextcontext.is_null() {
            c = (*c).nextcontext;
        }
        c
    }
}

// ---------------------------------------------------------------------------
// pf_str -- write string to profile file
// ---------------------------------------------------------------------------

/// Write a string to the profile output file.
///
/// On Unix, this avoids calling fprintf (signal-safe).
/// Ported from R's `pf_str()` in eval.c.
#[cfg(not(target_arch = "wasm32"))]
unsafe fn pf_str(s: *const c_char) -> isize {
    unsafe {
        let outfile = with_profiling_state(|state| state.profile_outfile);
        if outfile < 0 {
            return -1;
        }

        // Compute length
        let mut nbyte: usize = 0;
        while *s.add(nbyte) != 0 {
            nbyte += 1;
        }

        let mut wbyte: usize = 0;
        loop {
            let w = libc::write(outfile, s.add(wbyte) as *const c_void, nbyte - wbyte);
            if w == -1 {
                let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                if err == libc::EINTR {
                    continue;
                } else {
                    return -1;
                }
            }
            wbyte += w as usize;
            if wbyte == nbyte || w == 0 {
                return wbyte as isize;
            }
        }
    }
}

/// WASM M1 no-op stub: no file-descriptor profiling output on wasm.
/// Same signature as the native writer; returns the R-level
/// 'not available on this platform' sentinel (-1) used when no
/// profile output file is open.
#[cfg(target_arch = "wasm32")]
unsafe fn pf_str(_s: *const c_char) -> isize {
    -1
}

// ---------------------------------------------------------------------------
// pf_int -- write integer to profile file
// ---------------------------------------------------------------------------

/// Write an integer to the profile output file.
///
/// Ported from R's `pf_int()` in eval.c.
unsafe fn pf_int(num: c_int) {
    unsafe {
        let mut buf = [0u8; 32];
        let mut pb = profbuf {
            ptr: buf.as_mut_ptr() as *mut c_char,
            left: 32,
        };
        pb_int(&mut pb, num as i64);
        // Null-terminate
        if pb.left > 0 {
            *pb.ptr = 0;
        } else {
            buf[0] = 0;
        }
        pf_str(buf.as_ptr() as *const c_char);
    }
}

// ---------------------------------------------------------------------------
// R_getCurrentSrcref -- get current source reference
// ---------------------------------------------------------------------------

/// Get the current source reference for profiling.
///
/// Ported from R's `R_getCurrentSrcref()` in eval.c.
unsafe fn R_getCurrentSrcref() -> SEXP {
    unsafe {
        let srcref = with_profiling_state(|state| state.sref.as_raw());
        let in_bc = get_R_InBCInterpreter();
        if srcref != in_bc {
            srcref
        } else {
            R_findBCInterpreterSrcref(ptr::null_mut())
        }
    }
}

// ---------------------------------------------------------------------------
// doprof -- signal handler for profiling (full implementation)
// ---------------------------------------------------------------------------

/// Profiling signal handler.
///
/// Called asynchronously (via SIGPROF on Unix) to sample the call stack.
/// This function walks the context stack and writes function names to
/// the profiling output file.
///
/// Careful: This is called in a signal handler context, so only
/// async-signal-safe operations may be used.
///
/// Ported from R's `doprof()` in eval.c.
unsafe fn doprof(_sig: c_int) {
    unsafe {
        let mut buf = [0u8; PROFBUFSIZ];
        let prevnum = with_profiling_state(|state| state.line_profiling);

        let mut pb = profbuf {
            ptr: buf.as_mut_ptr() as *mut c_char,
            left: PROFBUFSIZ,
        };

        // Memory profiling: record memory allocation sizes
        if with_profiling_state(|state| state.mem_profiling) != 0 {
            write_memory_profile_prefix(&mut pb, memory_profile_snapshot());
        }

        // GC profiling
        if with_profiling_state(|state| state.gc_profiling) != 0
            && crate::mainutils::memory_main::R_gc_running() != 0
        {
            pb_str(&mut pb, b"\"<GC>\" \0".as_ptr() as *const c_char);
        }

        let instance = with_required_current_instance(|instance| instance);
        let owner_pin = crate::sexp::context::pin_context_owner_in(instance);
        let eval_internal = if with_profiling_state(|state| state.filter_callframes) != 0 {
            let symbol = Rf_install(c"eval".as_ptr());
            crate::sexp::context::require_context_owner_live(&owner_pin);
            crate::sexp::accessors::INTERNAL(symbol)
        } else {
            ptr::null_mut()
        };
        let current_srcref = with_profiling_state(|state| state.sref.owned());
        let mut frames = Vec::new();
        let mut cptr = super::runtime::global_context();
        while !cptr.is_null() {
            cptr = findProfContext(cptr, eval_internal);
            if cptr.is_null() {
                break;
            }
            let cell = crate::sexp::context::retain_context_in(instance, cptr)
                .unwrap_or_else(|| crate::sexp::context::r_error("unavailable profiling context"));
            frames.push((
                cell,
                (*cptr).callflag,
                (*cptr).call.owned(),
                (*cptr).srcref.owned(),
            ));
        }
        if with_profiling_state(|state| state.line_profiling) != 0 {
            let srcref = current_srcref
                .as_ref()
                .map_or(ptr::null_mut(), |value| value.as_raw());
            lineprof(
                &mut pb,
                if srcref == get_R_InBCInterpreter() {
                    R_findBCInterpreterSrcref(ptr::null_mut())
                } else {
                    srcref
                },
            );
        }
        for (cell, callflag, call_owner, srcref_owner) in frames {
            let cptr = cell.get();
            let call = call_owner
                .as_ref()
                .map_or(ptr::null_mut(), |value| value.as_raw());
            if (callflag & (ctxt_flags::CTXT_FUNCTION | ctxt_flags::CTXT_BUILTIN)) != 0
                && !call.is_null()
                && TYPEOF(call) == SEXPTYPE::LANGSXP
            {
                let fun_owner = crate::sexp::context::own_control_value(CAR(call));
                let fun = fun_owner.as_raw();
                pb_str(&mut pb, b"\"\0".as_ptr() as *const c_char);

                if TYPEOF(fun) == SEXPTYPE::SYMSXP {
                    // Simple symbol: just print its name
                    pb_str(&mut pb, CHAR(PRINTNAME(fun)));
                } else if !fun.is_null() && TYPEOF(fun) == SEXPTYPE::LANGSXP {
                    let fun_head = CAR(fun);
                    let arg1_owner = crate::sexp::context::own_control_value(CADR(fun));
                    let arg2_owner = crate::sexp::context::own_control_value(CADDR(fun));
                    let arg1 = arg1_owner.as_raw();
                    let arg2 = arg2_owner.as_raw();
                    if (fun_head == R_DoubleColonSymbol()
                        || fun_head == R_TripleColonSymbol()
                        || fun_head == R_DollarSymbol())
                        && !arg1.is_null()
                        && TYPEOF(arg1) == SEXPTYPE::SYMSXP
                        && !arg2.is_null()
                        && TYPEOF(arg2) == SEXPTYPE::SYMSXP
                    {
                        // Function accessed via ::, :::, or $
                        pb_str(&mut pb, CHAR(PRINTNAME(arg1)));
                        pb_str(&mut pb, CHAR(PRINTNAME(CAR(fun))));
                        pb_str(&mut pb, CHAR(PRINTNAME(arg2)));
                    } else if fun_head == R_Bracket2Symbol()
                        && !arg1.is_null()
                        && TYPEOF(arg1) == SEXPTYPE::SYMSXP
                        && !arg2.is_null()
                        && (TYPEOF(arg2) == SEXPTYPE::SYMSXP
                            || TYPEOF(arg2) == SEXPTYPE::STRSXP
                            || TYPEOF(arg2) == SEXPTYPE::INTSXP
                            || TYPEOF(arg2) == SEXPTYPE::REALSXP)
                        && LENGTH(arg2) > 0
                    {
                        // Function accessed via [[
                        pb_str(&mut pb, CHAR(PRINTNAME(arg1)));
                        pb_str(&mut pb, b"[[\0".as_ptr() as *const c_char);

                        if TYPEOF(arg2) == SEXPTYPE::SYMSXP {
                            pb_str(&mut pb, CHAR(PRINTNAME(arg2)));
                        } else if TYPEOF(arg2) == SEXPTYPE::STRSXP {
                            pb_str(&mut pb, b"\"\0".as_ptr() as *const c_char);
                            pb_str(&mut pb, CHAR(STRING_ELT(arg2, 0)));
                            pb_str(&mut pb, b"\"\0".as_ptr() as *const c_char);
                        } else if TYPEOF(arg2) == SEXPTYPE::INTSXP {
                            pb_int(&mut pb, *INTEGER(arg2) as i64);
                        } else if TYPEOF(arg2) == SEXPTYPE::REALSXP {
                            pb_dbl(&mut pb, *REAL(arg2)); // %0.f
                        }

                        pb_str(&mut pb, b"]]\0".as_ptr() as *const c_char);
                    } else {
                        pb_str(&mut pb, b"<Anonymous>\0".as_ptr() as *const c_char);
                    }
                } else {
                    pb_str(&mut pb, b"<Anonymous>\0".as_ptr() as *const c_char);
                }

                pb_str(&mut pb, b"\" \0".as_ptr() as *const c_char);

                // Line profiling for this context
                if with_profiling_state(|state| state.line_profiling) != 0 {
                    let srcref_val = srcref_owner
                        .as_ref()
                        .map_or(ptr::null_mut(), |value| value.as_raw());
                    let in_bc = get_R_InBCInterpreter();
                    if srcref_val == in_bc {
                        lineprof(&mut pb, R_findBCInterpreterSrcref(cptr));
                    } else {
                        lineprof(&mut pb, srcref_val);
                    }
                }
            }
            crate::sexp::context::require_context_owner_live(&owner_pin);
        }

        // Null-terminate the buffer
        if pb.left > 0 {
            *pb.ptr = 0;
        } else {
            // Overflow
            buf[0] = 0;
            with_profiling_state(|state| state.profiling_error = 3);
        }

        // Write any new source file references
        let line_prof_val = with_profiling_state(|state| state.line_profiling);
        let (source_offsets, source_buffer) =
            with_profiling_state(|state| (state.srcfiles.clone(), state.srcfiles_buffer.owned()));
        let mut i = prevnum;
        while i < line_prof_val {
            pf_str(b"#File \0".as_ptr() as *const c_char);
            pf_int(i);
            pf_str(b": \0".as_ptr() as *const c_char);
            if let (Some(buffer), Some(offset)) =
                (&source_buffer, source_offsets.get((i - 1) as usize))
            {
                if *offset < XLENGTH(buffer.as_raw()) as usize {
                    let bytes = RAW(buffer.as_raw());
                    crate::sexp::context::require_context_owner_live(&owner_pin);
                    pf_str(bytes.add(*offset).cast());
                }
            }
            pf_str(b"\n\0".as_ptr() as *const c_char);
            i += 1;
        }

        // Write the profile line
        let mut len: usize = 0;
        while len < PROFBUFSIZ && buf[len] != 0 {
            len += 1;
        }
        if len > 0 {
            pf_str(buf.as_ptr() as *const c_char);
            pf_str(b"\n\0".as_ptr() as *const c_char);
        }
    }
}

// ---------------------------------------------------------------------------
// doprof_null -- null signal handler for profiling
// ---------------------------------------------------------------------------

/// Null signal handler for SIGPROF, used when profiling is being stopped.
///
/// Ported from R's `doprof_null()` in eval.c.
unsafe fn doprof_null(_sig: c_int) {
    // Just reinstall the handler
    // In C: signal(SIGPROF, doprof_null);
    // In Rust port, signal handling is managed differently
}

// ---------------------------------------------------------------------------
// ProfileThread -- profiling timer thread (Unix/pthreads)
// ---------------------------------------------------------------------------

/// Profiling timer thread function (Unix/pthreads variant).
///
/// This thread runs on a timer, sending SIGPROF to the main thread
/// at regular intervals when using elapsed-time profiling.
///
/// Ported from R's `ProfileThread()` in eval.c.
///
/// Note: In this Rust port, threading is handled via std::thread.
/// This is a simplified version that uses sleep instead of pthread_cond_timedwait.
#[cfg(not(target_arch = "wasm32"))]
fn profile_thread_entry(interval_us: u64, terminate_rx: std::sync::mpsc::Receiver<()>) {
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    loop {
        match terminate_rx.recv_timeout(Duration::from_micros(interval_us)) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                // Send profiling signal to main thread. In the C version, this
                // calls pthread_kill(R_profiled_thread, SIGPROF); here we call
                // doprof directly for the current simplified port.
                unsafe {
                    doprof(libc::SIGPROF);
                }
            }
        }
    }
}

/// WASM M1 no-op stub: no SIGPROF timer thread on wasm.
#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
fn profile_thread_entry(_interval_us: u64, _terminate_rx: std::sync::mpsc::Receiver<()>) {}

// ---------------------------------------------------------------------------
// R_EndProfiling -- stop profiling (full implementation)
// ---------------------------------------------------------------------------

/// Stop profiling and close the output file.
///
/// This follows R's `R_EndProfiling()` state transition, but deliberately does
/// not manipulate process-global profiling timers. Samples are emitted through
/// `R_WriteProfile`, keeping profiler state session-owned for Android and
/// parallel runtimes.
#[cfg(not(target_arch = "wasm32"))]
unsafe fn R_EndProfiling() {
    unsafe {
        // Close the output file
        let outfile = with_profiling_state(|state| state.profile_outfile);
        if outfile >= 0 {
            libc::close(outfile);
            with_profiling_state(|state| state.profile_outfile = -1);
        }

        // Reset state
        with_profiling_state(|state| {
            state.profiling = 0;
            state.mem_profiling = 0;
            state.gc_profiling = 0;
            state.line_profiling = 0;
        });

        // Clear derived offsets before releasing the canonical buffer owner.
        with_profiling_state(|state| {
            state.srcfiles.clear();
            state.srcfile_bytes_used = 0;
            state.srcfiles_buffer = crate::sexp::instance::RuntimeValue::empty();
        });

        // Report any profiling errors
        let err = with_profiling_state(|state| state.profiling_error);
        if err != 0 {
            if err == 3 {
                // Samples too large for I/O buffer skipped
            } else if err == 1 {
                // Too many source files
            } else if err == 2 {
                // Buffer space exhausted
            }
        }
    }
}

/// WASM M1 no-op stub: no file-descriptor profiling output on wasm.
/// Same signature as the native stop routine; resets the session-local
/// profiling flags (no fd to close since the wasm `R_InitProfiling`
/// stub never opens one).
#[cfg(target_arch = "wasm32")]
unsafe fn R_EndProfiling() {
    unsafe {
        // Reset state
        with_profiling_state(|state| {
            state.profiling = 0;
            state.mem_profiling = 0;
            state.gc_profiling = 0;
            state.line_profiling = 0;
        });

        // Clear derived offsets before releasing the canonical buffer owner.
        with_profiling_state(|state| {
            state.srcfiles.clear();
            state.srcfile_bytes_used = 0;
            state.srcfiles_buffer = crate::sexp::instance::RuntimeValue::empty();
        });
    }
}

// ---------------------------------------------------------------------------
// R_InitProfiling -- initialize profiling (full implementation)
// ---------------------------------------------------------------------------

/// Initialize profiling with the given parameters.
///
/// Opens the output file and enables session-local profiling.
///
/// R's C runtime uses process-global signals/timers for automatic sampling.
/// This Rust/Android port avoids those globals so multiple `RSession`s can run
/// in parallel without stealing each other's profiling timer. The active
/// session can emit samples with `R_WriteProfile`.
#[cfg(not(target_arch = "wasm32"))]
unsafe fn R_InitProfiling(
    filename: SEXP,
    append: c_int,
    dinterval: c_double,
    mem_profiling: c_int,
    gc_profiling: c_int,
    line_profiling: c_int,
    filter_callframes: c_int,
    numfiles: c_int,
    bufsize: c_int,
    event: rpe_type,
) {
    unsafe {
        // If already profiling, stop first
        if with_profiling_state(|state| state.profile_outfile) >= 0 {
            R_EndProfiling();
        }

        // Open the output file (Unix path)
        if filename.is_null() {
            return;
        }

        let fn_str = CHAR(filename);
        if fn_str.is_null() {
            return;
        }

        let flags = if append != 0 {
            libc::O_CREAT | libc::O_WRONLY | libc::O_APPEND
        } else {
            libc::O_CREAT | libc::O_WRONLY | libc::O_TRUNC
        };
        let mode: u32 = libc::S_IRUSR as u32
            | libc::S_IWUSR as u32
            | libc::S_IRGRP as u32
            | libc::S_IWGRP as u32
            | libc::S_IROTH as u32
            | libc::S_IWOTH as u32;

        let fd = libc::open(fn_str, flags, mode);
        if fd < 0 {
            return;
        }
        with_profiling_state(|state| state.profile_outfile = fd);

        let interval: c_int = (1e6 * dinterval + 0.5) as c_int;

        // Write header line
        if mem_profiling != 0 {
            pf_str(b"memory profiling: \0".as_ptr() as *const c_char);
        }
        if gc_profiling != 0 {
            pf_str(b"GC profiling: \0".as_ptr() as *const c_char);
        }
        if line_profiling != 0 {
            pf_str(b"line profiling: \0".as_ptr() as *const c_char);
        }
        pf_str(b"sample.interval=\0".as_ptr() as *const c_char);
        pf_int(interval);
        pf_str(b"\n\0".as_ptr() as *const c_char);

        // Set profiling state
        with_profiling_state(|state| {
            state.mem_profiling = mem_profiling;
            state.memory_peak_bytes = 0;
            state.profiling_error = 0;
            state.line_profiling = line_profiling;
            state.gc_profiling = gc_profiling;
            state.filter_callframes = filter_callframes;
        });

        // Filenames use safe byte offsets; the buffer never stores native
        // pointers or relies on alignment suitable for a pointer array.
        if line_profiling != 0 {
            let count = usize::try_from(numfiles)
                .ok()
                .filter(|count| *count > 0)
                .unwrap_or_else(|| crate::sexp::context::r_error("invalid source file capacity"));
            if bufsize < 0 {
                crate::sexp::context::r_error("invalid source filename buffer size");
            }
            let instance = with_required_current_instance(|instance| instance);
            let pin = crate::sexp::context::pin_context_owner_in(instance);
            let buffer = crate::sexp::instance::RuntimeValue::from_raw_in(
                instance,
                Rf_allocVector(SEXPTYPE::RAWSXP, bufsize),
            );
            crate::sexp::context::require_context_owner_live(&pin);
            with_profiling_state(|state| {
                state.srcfiles.clear();
                state.srcfile_bytes_used = 0;
                state.srcfile_bufcount = count;
                state.srcfiles_buffer = buffer;
            });
        }

        with_profiling_state(|state| state.profiling_event = profiling_event_code(event));

        with_profiling_state(|state| state.profiling = 1);
    }
}

/// WASM M1 no-op stub: no setitimer/SIGPROF/file-IO profiling on wasm.
/// Same signature as the native initializer; leaves the R-level
/// 'not available on this platform' state (profiling stays off,
/// output fd stays closed) so callers observe the neighboring-stub
/// behavior of returning `R_NilValue()` without starting profiling.
#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
unsafe fn R_InitProfiling(
    _filename: SEXP,
    _append: c_int,
    _dinterval: c_double,
    _mem_profiling: c_int,
    _gc_profiling: c_int,
    _line_profiling: c_int,
    _filter_callframes: c_int,
    _numfiles: c_int,
    _bufsize: c_int,
    _event: rpe_type,
) {
}

// ---------------------------------------------------------------------------
// do_Rprof -- Rprof() builtin (full implementation)
// ---------------------------------------------------------------------------

/// Implement the `Rprof()` function.
///
/// When called with a non-empty filename, starts profiling to that file.
/// When called with an empty filename, stops profiling.
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn do_Rprof(call: SEXP, op: SEXP, mut args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // BC profiling check
        if with_profiling_state(|state| state.bc_profiling) != 0 {
            // Cannot use R profiling while byte code profiling
            return R_NilValue();
        }

        // Parse arguments
        let filename_arg = CAR(args);
        if filename_arg.is_null() {
            return R_NilValue();
        }

        // Get filename string
        if TYPEOF(filename_arg) != SEXPTYPE::STRSXP || LENGTH(filename_arg) != 1 {
            return R_NilValue();
        }

        let filename_sexp = STRING_ELT(filename_arg, 0);
        args = CDR(args);

        let append_mode = crate::mainutils::coerce::asLogical(CAR(args));
        args = CDR(args);

        let dinterval = crate::mainutils::coerce::asReal(CAR(args));
        args = CDR(args);

        let mem_profiling = crate::mainutils::coerce::asLogical(CAR(args));
        args = CDR(args);

        let gc_profiling = crate::mainutils::coerce::asLogical(CAR(args));
        args = CDR(args);

        let line_profiling = crate::mainutils::coerce::asLogical(CAR(args));
        args = CDR(args);

        let filter_callframes = crate::mainutils::coerce::asLogical(CAR(args));
        args = CDR(args);

        let numfiles = crate::mainutils::coerce::asInteger(CAR(args));
        args = CDR(args);

        let bufsize = crate::mainutils::coerce::asInteger(CAR(args));
        args = CDR(args);

        // Get event type argument
        let event_arg_sexp = CAR(args);
        let event = if !event_arg_sexp.is_null()
            && TYPEOF(event_arg_sexp) == SEXPTYPE::STRSXP
            && LENGTH(event_arg_sexp) == 1
        {
            let event_str = CHAR(STRING_ELT(event_arg_sexp, 0));
            if !event_str.is_null() {
                let bytes = CStr::from_ptr(event_str);
                match bytes.to_bytes() {
                    b"cpu" | b"default" => rpe_type::RPE_CPU,
                    b"elapsed" => rpe_type::RPE_ELAPSED,
                    _ => rpe_type::RPE_CPU,
                }
            } else {
                rpe_type::RPE_CPU
            }
        } else {
            rpe_type::RPE_CPU
        };

        // Check if filename is non-empty
        let filename_len = LENGTH(filename_sexp);
        if filename_len > 0 {
            R_InitProfiling(
                filename_sexp,
                append_mode,
                dinterval,
                mem_profiling,
                gc_profiling,
                line_profiling,
                filter_callframes,
                numfiles,
                bufsize,
                event,
            );
        } else {
            R_EndProfiling();
        }

        R_NilValue()
    }
}
/// WASM M1 stub for `Rprof()`: no timer/file-descriptor profiler on wasm.
/// Starting profiling raises the catchable R platform-unavailable error
/// (matching upstream's `error(_("R profiling is not available on this system"))`
/// fallback). Stopping with an empty filename stays a silent no-op.
#[cfg(target_arch = "wasm32")]
pub unsafe fn do_Rprof_wasm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    use crate::sexp::accessors::{CAR, LENGTH, STRING_ELT, TYPEOF};
    unsafe {
        let filename_arg = CAR(args);
        let empty = filename_arg.is_null()
            || filename_arg == R_NilValue()
            || TYPEOF(filename_arg) != SEXPTYPE::STRSXP
            || LENGTH(filename_arg) != 1
            || LENGTH(STRING_ELT(filename_arg, 0)) == 0;
        if empty {
            return R_NilValue();
        }
        crate::sexp::context::r_error("R profiling is not available on this platform")
    }
}

// ---------------------------------------------------------------------------
// do_Rprof_mem -- Rprofmem() builtin
// ---------------------------------------------------------------------------

/// Implement the `Rprofmem()` function for memory profiling.
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn do_Rprofmem(_call: SEXP, _op: SEXP, mut args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let filename_arg = if args.is_null() || args == R_NilValue() {
            Rf_mkString(c"Rprofmem.out".as_ptr())
        } else {
            let arg = CAR(args);
            args = CDR(args);
            arg
        };

        if filename_arg.is_null()
            || TYPEOF(filename_arg) != SEXPTYPE::STRSXP
            || LENGTH(filename_arg) != 1
        {
            return R_NilValue();
        }

        let append_mode = if args.is_null() || args == R_NilValue() {
            FALSE
        } else {
            crate::mainutils::coerce::asLogical(CAR(args))
        };
        let filename_sexp = STRING_ELT(filename_arg, 0);

        if LENGTH(filename_sexp) > 0 {
            R_InitProfiling(
                filename_sexp,
                append_mode,
                0.02,
                TRUE,
                FALSE,
                FALSE,
                FALSE,
                100,
                PROFBUFSIZ as c_int,
                rpe_type::RPE_ELAPSED,
            );
        } else {
            R_EndProfiling();
        }

        R_NilValue()
    }
}
/// WASM M1 stub for `Rprofmem()`: no memory profiler on wasm.
/// Always raises the catchable R platform-unavailable error (matching
/// upstream's `error(_("memory profiling is not available on this system"))`
/// fallback) instead of silently returning success.
#[cfg(target_arch = "wasm32")]
pub unsafe fn do_Rprofmem_wasm(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::context::r_error("memory profiling is not available on this platform")
}

// ---------------------------------------------------------------------------
// do_Rprofaddr -- Rprofaddr() builtin
// ---------------------------------------------------------------------------

/// Implement the `Rprofaddr()` function for address profiling.
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn do_Rprofaddr(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}
/// WASM M1 stub for `Rprofaddr()`: no address profiler on wasm.
/// Raises the catchable R platform-unavailable error instead of silently
/// returning success.
#[cfg(target_arch = "wasm32")]
pub unsafe fn do_Rprofaddr(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::context::r_error("address profiling is not available on this platform")
}
/// Implement the `gcprof()` function for GC profiling.
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn do_gcprof(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}
/// WASM M1 stub for `gcprof()`: no GC profiler on wasm.
/// Raises the catchable R platform-unavailable error instead of silently
/// returning success.
#[cfg(target_arch = "wasm32")]
pub unsafe fn do_gcprof(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::context::r_error("GC profiling is not available on this platform")
}

// ---------------------------------------------------------------------------
// dobcprof -- BC profiling signal handler
// ---------------------------------------------------------------------------

/// Bytecode profiling signal handler.
///
/// Records the current bytecode opcode when the profiling timer fires.
///
/// Ported from R's `dobcprof()` in eval.c.
unsafe fn dobcprof(_sig: c_int) {
    let op = with_profiling_state(|state| state.current_opcode);
    if op >= 0 && (op as usize) < OPCOUNT {
        with_profiling_state(|state| state.opcode_counts[op as usize] += 1);
    }
    // Reinstall handler: signal(SIGPROF, dobcprof);
}

// ---------------------------------------------------------------------------
// dobcprof_null -- null BC profiling signal handler
// ---------------------------------------------------------------------------

/// Null signal handler for BC profiling, used when BC profiling stops.
///
/// Ported from R's `dobcprof_null()` in eval.c.
unsafe fn dobcprof_null(_sig: c_int) {
    // Just reinstall: signal(SIGPROF, dobcprof_null);
}

// ---------------------------------------------------------------------------
// do_bcprofstart -- start bytecode profiling
// ---------------------------------------------------------------------------

/// Start bytecode profiling.
///
/// Sets up the profiling timer and initializes opcode counts.
///
/// Ported from R's `do_bcprofstart()` in eval.c.
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn do_bcprofstart(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let dinterval: c_double = 0.02;
        let interval: c_int = (1e6 * dinterval + 0.5) as c_int;

        if with_profiling_state(|state| state.profiling) != 0 {
            // Profile timer in use
            return R_NilValue();
        }
        if with_profiling_state(|state| state.bc_profiling) != 0 {
            // Already byte code profiling
            return R_NilValue();
        }

        // Initialize the profile data
        with_profiling_state(|state| {
            state.current_opcode = NO_CURRENT_OPCODE;
            state.opcode_counts.fill(0);
        });

        // Set up the timer
        let it_interval = libc::timeval {
            tv_sec: (interval as i64 / 1000000) as libc::time_t,
            tv_usec: (interval - (interval / 1000000) * 1000000) as libc::suseconds_t,
        };
        let itv = libc::itimerval {
            it_interval,
            it_value: it_interval,
        };
        if libc::setitimer(PROF_TIMER, &itv, ptr::null_mut()) == -1 {
            return R_NilValue();
        }

        with_profiling_state(|state| state.bc_profiling = 1);

        R_NilValue()
    }
}

/// WASM M1 stub: no setitimer/SIGPROF bytecode profiling on wasm.
/// Same signature as the native starter; raises the catchable R
/// platform-unavailable error (matching upstream's
/// `error(_("byte code profiling is not supported in this build"))`
/// fallback) instead of silently returning success.
#[cfg(target_arch = "wasm32")]
pub unsafe fn do_bcprofstart(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::context::r_error("byte code profiling is not supported on this platform")
}

// ---------------------------------------------------------------------------
// do_bcprofstop -- stop bytecode profiling
// ---------------------------------------------------------------------------

/// Stop bytecode profiling.
///
/// Disables the profiling timer.
///
/// Ported from R's `do_bcprofstop()` in eval.c.
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn do_bcprofstop(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if with_profiling_state(|state| state.bc_profiling) == 0 {
            // Not byte code profiling
            return R_NilValue();
        }

        let zero_val = libc::itimerval {
            it_interval: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            it_value: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
        };
        libc::setitimer(PROF_TIMER, &zero_val, ptr::null_mut());

        with_profiling_state(|state| state.bc_profiling = 0);

        R_NilValue()
    }
}

/// WASM M1 stub: no setitimer/SIGPROF bytecode profiling on wasm.
/// Same signature as the native stopper; raises the catchable R
/// platform-unavailable error (matching upstream's
/// `error(_("byte code profiling is not supported in this build"))`
/// fallback) instead of silently returning success.
#[cfg(target_arch = "wasm32")]
pub unsafe fn do_bcprofstop(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::context::r_error("byte code profiling is not supported on this platform")
}

// ---------------------------------------------------------------------------
// do_bcprofcounts -- get bytecode profiling counts
// ---------------------------------------------------------------------------

/// Get bytecode profiling opcode counts.
///
/// Returns an integer vector of opcode counts.
///
/// Ported from R's `do_bcprofcounts()` in eval.c.
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn do_bcprofcounts(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let val = Rf_allocVector(SEXPTYPE::INTSXP, OPCOUNT as c_int);
        if val.is_null() {
            return R_NilValue();
        }
        let ip = INTEGER(val);
        if !ip.is_null() {
            with_profiling_state(|state| {
                for i in 0..OPCOUNT {
                    *ip.add(i) = state.opcode_counts[i];
                }
            });
        }
        val
    }
}
/// WASM M1 stub for `bcprofcounts()`: no bytecode profiler state on wasm.
/// Raises the catchable R platform-unavailable error (matching upstream's
/// `error(_("byte code profiling is not supported in this build"))`
/// fallback) instead of returning a silent zero vector.
#[cfg(target_arch = "wasm32")]
pub unsafe fn do_bcprofcounts(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::context::r_error("byte code profiling is not supported on this platform")
}

// ---------------------------------------------------------------------------
// R_WriteProfile -- write profiling output
// ---------------------------------------------------------------------------

/// Write current profiling sample to the output file.
///
/// Ported from R's `R_WriteProfile()` in eval.c.
pub fn R_WriteProfile(_out: c_int) {
    if R_Profiling_active() != 0 {
        unsafe {
            doprof(0);
        }
    }
}

// ---------------------------------------------------------------------------
// bc_check_sigint -- check for user interrupts in bytecode loop
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::sexp::RSession;
    use crate::sexp::constructors::{
        Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_cons, Rf_mkString,
    };
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::instance::RInstance;
    use crate::sexp::memory::with_arena;

    unsafe fn r_string(text: &str) -> SEXP {
        let c_text = std::ffi::CString::new(text).expect("test string without interior nul");
        unsafe { Rf_mkString(c_text.as_ptr()) }
    }

    unsafe fn pairlist(values: &[SEXP]) -> SEXP {
        unsafe {
            values
                .iter()
                .rev()
                .fold(R_NilValue(), |tail, value| Rf_cons(*value, tail))
        }
    }

    fn unique_profile_path(prefix: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("{prefix}-{}-{nanos}.out", std::process::id()))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn owned_profile_filename_offsets_respect_exact_capacity_and_release_buffer() {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let buffer = owner
                .node_factory()
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, 6)))
                .unwrap();
            let node = crate::sexp::memory::checked_projection(buffer.as_raw())
                .unwrap()
                .1;
            let value = crate::sexp::instance::RuntimeValue::from_raw_in(instance, buffer.as_raw());
            with_profiling_state(|state| {
                state.srcfiles_buffer = value;
                state.line_profiling = 1;
                state.srcfile_bufcount = 1;
            });
            drop(buffer);
            owner.full_gc().unwrap();
            assert!(node.is_live());
            assert_eq!(getFilenum(c"abcde".as_ptr()), 1);
            assert_eq!(getFilenum(c"abcde".as_ptr()), 1);
            assert_eq!(with_profiling_state(|state| state.srcfiles.clone()), [0]);
            assert_eq!(with_profiling_state(|state| state.srcfile_bytes_used), 6);
            assert_eq!(getFilenum(c"x".as_ptr()), 0);
            assert_eq!(with_profiling_state(|state| state.profiling_error), 1);
            with_profiling_state(|state| {
                state.srcfile_bufcount = 2;
                state.profiling_error = 0;
            });
            assert_eq!(getFilenum(c"x".as_ptr()), 0);
            assert_eq!(with_profiling_state(|state| state.profiling_error), 2);
            assert_eq!(getFilenum(c"abcde".as_ptr()), 1);
            R_EndProfiling();
            owner.full_gc().unwrap();
            assert!(!node.is_live());
            with_profiling_state(|state| {
                assert!(state.srcfiles_buffer.is_null());
                assert!(state.srcfiles.is_empty());
                assert_eq!(state.srcfile_bytes_used, 0);
            });
        });
    }

    #[test]
    fn owned_line_profile_survives_collecting_active_filename_binding() {
        use std::{cell::Cell, rc::Rc};
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let factory = crate::sexp::object::SessionNodeFactory::new(owner);
            let nil = R_NilValue();
            (*instance).eval_state.jit_enabled = 0;
            let srcfile = factory
                .wrap(crate::sexp::memory_ext::NewEnvironment(
                    nil,
                    nil,
                    crate::sexp::globals::R_GlobalEnv(),
                ))
                .unwrap();
            let function_expression = owner
                .with_arena(|arena| {
                    crate::eval::parser::parse(
                        "function() { 1L + 2L; \"script.R\" }",
                        arena,
                        factory.clone(),
                    )
                })
                .unwrap()
                .unwrap();
            let function = factory
                .wrap(crate::eval::eval::Rf_eval(
                    function_expression.as_raw(),
                    crate::sexp::globals::R_GlobalEnv(),
                ))
                .unwrap();
            let filename_symbol = Rf_install(c"filename".as_ptr());
            let srcfile_symbol = Rf_install(c"srcfile".as_ptr());
            crate::sexp::envir::make_active_binding_raw(
                srcfile.as_raw(),
                filename_symbol,
                function.as_raw(),
            );
            let srcref = factory.wrap(Rf_ScalarInteger(71)).unwrap();
            let pointer = srcref.as_raw();
            let node = crate::sexp::memory::checked_projection(pointer).unwrap().1;
            crate::sexp::attrib_core::setAttrib(pointer, srcfile_symbol, srcfile.as_raw());
            (*instance).eval_state.profiling.sref =
                crate::sexp::instance::RuntimeValue::from_raw_in(instance, pointer);
            let buffer = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, 64)))
                .unwrap();
            let buffer =
                crate::sexp::instance::RuntimeValue::from_raw_in(instance, buffer.as_raw());
            with_profiling_state(|state| {
                state.srcfiles_buffer = buffer;
                state.line_profiling = 1;
                state.srcfile_bufcount = 4;
            });
            drop(srcref);
            drop(srcfile);
            drop(function);
            drop(function_expression);
            let observed = Rc::new(Cell::new(false));
            let called = observed.clone();
            let node_observed = node.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if called.replace(true) {
                    return;
                }
                (*instance).eval_state.profiling.sref =
                    crate::sexp::instance::RuntimeValue::empty();
                crate::sexp::attrib_core::setAttrib(pointer, srcfile_symbol, R_NilValue());
                (*instance).context_stack.clear();
                crate::sexp::gengc::full_gc_in(instance);
                assert!(node_observed.is_live());
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let mut bytes = [0_u8; 32];
            let mut output = profbuf {
                ptr: bytes.as_mut_ptr().cast(),
                left: bytes.len(),
            };
            lineprof(&mut output, pointer);
            (*instance).memory_state.gc_force_gap = 0;
            assert!(observed.get());
            assert_eq!(
                CStr::from_ptr(bytes.as_ptr().cast()).to_str().unwrap(),
                "1#71 "
            );
            assert_eq!(with_profiling_state(|state| state.srcfiles.len()), 1);
            owner.full_gc().unwrap();
            assert!(!node.is_live());
            R_EndProfiling();
        });
    }

    #[test]
    fn profiling_flags_are_session_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        left.with_protected(|| {
            with_profiling_state(|state| {
                state.profiling = 1;
                state.bc_profiling = 1;
                state.current_opcode = 7;
                state.opcode_counts[7] = 11;
            });
            assert_eq!(R_Profiling_active(), 1);
            assert_eq!(R_isRprofiling(), 1);
        });

        right.with_protected(|| {
            assert_eq!(R_Profiling_active(), 0);
            assert_eq!(R_isRprofiling(), 0);
            with_profiling_state(|state| {
                assert_eq!(state.bc_profiling, 0);
                assert_eq!(state.current_opcode, NO_CURRENT_OPCODE);
                assert_eq!(state.opcode_counts[7], 0);
            });
        });

        left.with_protected(|| {
            with_profiling_state(|state| {
                assert_eq!(state.bc_profiling, 1);
                assert_eq!(state.current_opcode, 7);
                assert_eq!(state.opcode_counts[7], 11);
            });
        });
    }

    #[test]
    fn profiling_flags_can_target_instance_explicitly() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        with_profiling_state_in(&mut left as *mut RInstance, |state| {
            state.profiling = 1;
            state.bc_profiling = 1;
            state.current_opcode = 9;
            state.opcode_counts[9] = 17;
        });

        assert_eq!(R_Profiling_active_in(&mut left as *mut RInstance), 1);
        assert_eq!(R_isRprofiling_in(&mut left as *mut RInstance), 1);
        assert_eq!(R_Profiling_active_in(&mut right as *mut RInstance), 0);
        assert_eq!(R_isRprofiling_in(&mut right as *mut RInstance), 0);
        with_profiling_state_in(&mut right as *mut RInstance, |state| {
            assert_eq!(state.bc_profiling, 0);
            assert_eq!(state.current_opcode, NO_CURRENT_OPCODE);
            assert_eq!(state.opcode_counts[9], 0);
        });
    }

    #[test]
    fn bytecode_profiler_samples_current_session_only() {
        let left = RSession::new();
        let right = RSession::new();

        left.with_protected(|| {
            with_profiling_state(|state| {
                state.current_opcode = 3;
                state.opcode_counts.fill(0);
            });
            unsafe {
                dobcprof(0);
            }
            with_profiling_state(|state| assert_eq!(state.opcode_counts[3], 1));
        });

        right.with_protected(|| {
            with_profiling_state(|state| {
                assert_eq!(state.opcode_counts[3], 0);
                state.current_opcode = 3;
            });
            unsafe {
                dobcprof(0);
            }
            with_profiling_state(|state| assert_eq!(state.opcode_counts[3], 1));
        });

        left.with_protected(|| {
            with_profiling_state(|state| assert_eq!(state.opcode_counts[3], 1));
        });
    }

    #[test]
    fn memory_profile_snapshot_uses_current_session_arena() {
        let left = RSession::new();
        let right = RSession::new();

        let left_after = left.with_protected(|| {
            let before = memory_profile_snapshot();
            unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ with_arena(|arena| {
                arena.alloc_vector(SEXPTYPE::REALSXP, 128);
                arena.alloc_charsxp(b"profile-left");
            }) };
            let after = memory_profile_snapshot();
            assert!(after.current_bytes > before.current_bytes);
            assert!(after.peak_bytes >= after.current_bytes);
            assert!(after.active_nodes > before.active_nodes);
            after
        });

        right.with_protected(|| {
            let right_snapshot = memory_profile_snapshot();
            assert!(right_snapshot.current_bytes < left_after.current_bytes);
            assert!(right_snapshot.active_nodes < left_after.active_nodes);
        });
    }

    #[test]
    fn memory_profile_snapshot_can_target_instance_explicitly() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        let before = memory_profile_snapshot_in(&mut left as *mut RInstance);
        left.arena.alloc_vector(SEXPTYPE::REALSXP, 64);
        left.arena.alloc_charsxp(b"profile-explicit-left");
        let after = memory_profile_snapshot_in(&mut left as *mut RInstance);
        let right_snapshot = memory_profile_snapshot_in(&mut right as *mut RInstance);

        assert!(after.current_bytes > before.current_bytes);
        assert!(after.active_nodes > before.active_nodes);
        assert!(after.peak_bytes >= after.current_bytes);
        assert!(right_snapshot.current_bytes < after.current_bytes);
        assert!(right_snapshot.active_nodes < after.active_nodes);
    }

    #[test]
    fn memory_profile_prefix_writes_real_snapshot_values() {
        let _session = RSession::new();
        unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                arena.alloc_vector(SEXPTYPE::INTSXP, 64);
            })
        };
        let snapshot = memory_profile_snapshot();

        let mut buf = [0u8; 128];
        let mut pb = profbuf {
            ptr: buf.as_mut_ptr() as *mut c_char,
            left: buf.len(),
        };
        unsafe {
            write_memory_profile_prefix(&mut pb, snapshot);
            *pb.ptr = 0;
        }

        let text = unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) }
            .to_str()
            .expect("profile prefix should be utf8");
        let fields: Vec<u64> = text
            .trim_matches(':')
            .split(':')
            .map(|field| field.parse::<u64>().expect("numeric profile field"))
            .collect();

        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0], snapshot.current_bytes);
        assert_eq!(fields[1], snapshot.peak_bytes);
        assert_eq!(fields[2], snapshot.active_nodes);
        assert_eq!(fields[3], snapshot.gc_freed_nodes);
        assert!(fields[0] > 0);
        assert!(fields[1] >= fields[0]);
        assert!(fields[2] > 0);
    }

    #[test]
    fn public_rprof_wrapper_uses_session_profiler() {
        let session = RSession::new();
        session.with_protected(|| {
            let path = unique_profile_path("rport-rprof");
            let args = unsafe {
                pairlist(&[
                    r_string(&path),
                    Rf_ScalarLogical(FALSE),
                    Rf_ScalarReal(0.02),
                    Rf_ScalarLogical(FALSE),
                    Rf_ScalarLogical(FALSE),
                    Rf_ScalarLogical(FALSE),
                    Rf_ScalarLogical(FALSE),
                    Rf_ScalarInteger(100),
                    Rf_ScalarInteger(PROFBUFSIZ as c_int),
                    r_string("elapsed"),
                ])
            };

            unsafe {
                crate::mainutils::essentials::do_Rprof(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    args,
                    R_NilValue(),
                );
            }
            with_profiling_state(|state| {
                assert_eq!(state.profiling, 1);
                assert_eq!(state.mem_profiling, 0);
                assert!(state.profile_outfile >= 0);
            });

            unsafe {
                let stop_args = pairlist(&[r_string("")]);
                crate::mainutils::essentials::do_Rprof(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    stop_args,
                    R_NilValue(),
                );
            }
            with_profiling_state(|state| {
                assert_eq!(state.profiling, 0);
                assert_eq!(state.profile_outfile, -1);
            });

            let contents = fs::read_to_string(&path).expect("profile file");
            assert!(contents.contains("sample.interval=20000"));
            let _ = fs::remove_file(path);
        });
    }

    #[test]
    fn public_rprofmem_wrapper_writes_session_memory_sample() {
        let session = RSession::new();
        session.with_protected(|| {
            let path = unique_profile_path("rport-rprofmem");
            let args = unsafe { pairlist(&[r_string(&path), Rf_ScalarLogical(FALSE)]) };

            unsafe {
                crate::mainutils::essentials::do_Rprofmem(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    args,
                    R_NilValue(),
                );
            }
            with_profiling_state(|state| {
                assert_eq!(state.profiling, 1);
                assert_eq!(state.mem_profiling, 1);
                assert!(state.profile_outfile >= 0);
            });

            unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ with_arena(|arena| {
                arena.alloc_vector(SEXPTYPE::INTSXP, 128);
            }) };
            R_WriteProfile(0);

            unsafe {
                let stop_args = pairlist(&[r_string("")]);
                crate::mainutils::essentials::do_Rprofmem(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    stop_args,
                    R_NilValue(),
                );
            }
            with_profiling_state(|state| {
                assert_eq!(state.profiling, 0);
                assert_eq!(state.mem_profiling, 0);
                assert_eq!(state.profile_outfile, -1);
            });

            let contents = fs::read_to_string(&path).expect("memory profile file");
            assert!(contents.contains("memory profiling: sample.interval=20000"));
            let sample_line = contents
                .lines()
                .find(|line| line.starts_with(':'))
                .expect("memory profile sample line");
            let fields: Vec<u64> = sample_line
                .trim_matches(':')
                .split(':')
                .take(4)
                .map(|field| field.parse::<u64>().expect("numeric memory field"))
                .collect();
            assert_eq!(fields.len(), 4);
            assert!(fields[0] > 0);
            assert!(fields[1] >= fields[0]);
            assert!(fields[2] > 0);
            let _ = fs::remove_file(path);
        });
    }
}
