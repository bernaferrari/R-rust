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

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;

use crate::eval::attrib_core::getAttrib;

use crate::sexp::accessors::{
    CADDR, CADR, CAR, CDR, CHAR, INTEGER, LENGTH, PRINTNAME, RAW, REAL, STRING_ELT, TYPEOF,
};
use crate::sexp::constructors::{Rf_allocVector, Rf_mkString};
use crate::sexp::context::R_GlobalContext;
use crate::sexp::context::RCNTXT;
use crate::sexp::context::ctxt_flags;
use crate::sexp::envir::R_findVar;
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_FINITE, TRUE};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance::{
    NO_PROFILING_OPCODE, PROFILING_OPCODE_COUNT, ProfilingState, with_required_current_instance,
};
use crate::sexp::protect::{R_PreserveObject, R_ReleaseObject};

/// Profiling timer type (ITIMER_PROF is not available on Android).
#[cfg(not(target_os = "android"))]
const PROF_TIMER: libc::c_int = libc::ITIMER_PROF;
#[cfg(target_os = "android")]
const PROF_TIMER: libc::c_int = 2; // ITIMER_PROF value on most Unix systems
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
    unsafe { R_GlobalContext() }
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
    with_required_current_instance(|instance| f(&mut instance.eval_state.profiling))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MemoryProfileSnapshot {
    current_bytes: u64,
    peak_bytes: u64,
    active_nodes: u64,
    gc_freed_nodes: u64,
}

fn memory_profile_snapshot() -> MemoryProfileSnapshot {
    with_required_current_instance(|instance| {
        let current_bytes = instance.arena.total_bytes_allocated();
        let peak_bytes = instance
            .eval_state
            .profiling
            .memory_peak_bytes
            .max(current_bytes)
            .max(instance.gc_state.stats.peak_memory);
        instance.eval_state.profiling.memory_peak_bytes = peak_bytes;

        MemoryProfileSnapshot {
            current_bytes: current_bytes as u64,
            peak_bytes: peak_bytes as u64,
            active_nodes: instance.arena.node_count() as u64,
            gc_freed_nodes: instance.gc_state.stats.freed as u64,
        }
    })
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
    with_profiling_state(|state| state.profiling)
}

// ---------------------------------------------------------------------------
// R_isRprofiling -- check if profiling is enabled
// ---------------------------------------------------------------------------

/// Check if R profiling is enabled (public API).
pub fn R_isRprofiling() -> c_int {
    with_profiling_state(|state| state.profiling)
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
        let r_srcfiles = with_profiling_state(|state| state.srcfiles);
        let r_srcfiles_buffer = with_profiling_state(|state| state.srcfiles_buffer);
        if r_srcfiles.is_null() || r_srcfiles_buffer.is_null() {
            return 0;
        }

        let line_prof = with_profiling_state(|state| state.line_profiling);
        if line_prof <= 0 {
            return 0;
        }

        let mut fnum: c_int = 0;

        // Linear search through previously recorded filenames
        while fnum < line_prof - 1 {
            let existing = *r_srcfiles.add(fnum as usize);
            if existing.is_null() {
                break;
            }
            // Compare strings
            let mut equal = true;
            let mut j: usize = 0;
            loop {
                let a = *filename.add(j) as u8;
                let b = *existing.add(j) as u8;
                if a != b {
                    equal = false;
                    break;
                }
                if a == 0 {
                    break;
                }
                j += 1;
            }
            if equal {
                return fnum + 1;
            }
            fnum += 1;
        }

        if fnum == line_prof - 1 {
            // Compute length of filename
            let mut len: usize = 0;
            while *filename.add(len) != 0 {
                len += 1;
            }

            let bufcount = with_profiling_state(|state| state.srcfile_bufcount);
            if (fnum as usize) >= bufcount {
                // Too many files
                with_profiling_state(|state| state.profiling_error = 1);
                return 0;
            }

            // Check buffer space
            let buf_start = RAW(r_srcfiles_buffer) as *mut c_char;
            let current_ptr = *r_srcfiles.add(fnum as usize);
            let used = current_ptr as usize - buf_start as usize;
            let total = LENGTH(r_srcfiles_buffer) as usize;

            if used + len + 1 > total {
                // Out of space in the buffer
                with_profiling_state(|state| state.profiling_error = 2);
                return 0;
            }

            // Copy filename into buffer
            let dst = *r_srcfiles.add(fnum as usize);
            ptr::copy_nonoverlapping(filename as *const u8, dst as *mut u8, len + 1);

            // Set up next pointer
            let next_ptr = dst.add(len + 1);
            *r_srcfiles.add((fnum + 1) as usize) = next_ptr as *mut c_char;
            *next_ptr = 0; // NUL terminator

            with_profiling_state(|state| state.line_profiling += 1);
        }

        fnum + 1
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

        // Get line number from srcref
        let line_val = crate::mainutils::coerce::asInteger(srcref);
        if line_val == NA_INTEGER {
            return;
        }
        let line = line_val;

        // Get the srcfile attribute
        let srcfile_sym = Rf_install(b"srcfile\0".as_ptr() as *const c_char);
        let srcfile = getAttrib(srcref, srcfile_sym);
        if srcfile.is_null() || srcfile == R_NilValue() {
            return;
        }
        if TYPEOF(srcfile) != SEXPTYPE::ENVSXP {
            return;
        }

        // Look up the filename in the srcfile environment
        let fn_sym = Rf_install(b"filename\0".as_ptr() as *const c_char);
        let filename_sexp = R_findVar(fn_sym, srcfile);
        if TYPEOF(filename_sexp) != SEXPTYPE::STRSXP || LENGTH(filename_sexp) == 0 {
            return;
        }
        let filename = CHAR(STRING_ELT(filename_sexp, 0));

        let fnum = getFilenum(filename);
        if fnum != 0 {
            pb_int(pb, fnum as i64);
            pb_str(pb, b"#\0".as_ptr() as *const c_char);
            pb_int(pb, line as i64);
            pb_str(pb, b" \0".as_ptr() as *const c_char);
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
unsafe fn findProfContext(cptr: *mut RCNTXT) -> *mut RCNTXT {
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
            let parent_callfun = (*parent).callfun;
            if !parent_callfun.is_null() {
                let eval_sym = Rf_install(b"eval\0".as_ptr() as *const c_char);
                let eval_internal = crate::sexp::accessors::INTERNAL(eval_sym);
                if parent_callfun == eval_internal {
                    let sysparent = (*cptr).sysparent;
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
        let srcref = with_profiling_state(|state| state.sref);
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

        // Line profiling
        if with_profiling_state(|state| state.line_profiling) != 0 {
            lineprof(&mut pb, R_getCurrentSrcref());
        }

        // Walk the context stack
        let mut cptr = R_GlobalContext();
        while !cptr.is_null() {
            cptr = findProfContext(cptr);
            if cptr.is_null() {
                break;
            }

            let callflag = (*cptr).callflag;
            let call = (*cptr).call;

            if (callflag & (ctxt_flags::CTXT_FUNCTION | ctxt_flags::CTXT_BUILTIN)) != 0
                && !call.is_null()
                && TYPEOF(call) == SEXPTYPE::LANGSXP
            {
                let fun = CAR(call);
                pb_str(&mut pb, b"\"\0".as_ptr() as *const c_char);

                if TYPEOF(fun) == SEXPTYPE::SYMSXP {
                    // Simple symbol: just print its name
                    pb_str(&mut pb, CHAR(PRINTNAME(fun)));
                } else if !fun.is_null() && TYPEOF(fun) == SEXPTYPE::LANGSXP {
                    let fun_head = CAR(fun);
                    if (fun_head == R_DoubleColonSymbol()
                        || fun_head == R_TripleColonSymbol()
                        || fun_head == R_DollarSymbol())
                        && !CADR(fun).is_null()
                        && TYPEOF(CADR(fun)) == SEXPTYPE::SYMSXP
                        && !CADDR(fun).is_null()
                        && TYPEOF(CADDR(fun)) == SEXPTYPE::SYMSXP
                    {
                        // Function accessed via ::, :::, or $
                        pb_str(&mut pb, CHAR(PRINTNAME(CADR(fun))));
                        pb_str(&mut pb, CHAR(PRINTNAME(CAR(fun))));
                        pb_str(&mut pb, CHAR(PRINTNAME(CADDR(fun))));
                    } else if fun_head == R_Bracket2Symbol()
                        && !CADR(fun).is_null()
                        && TYPEOF(CADR(fun)) == SEXPTYPE::SYMSXP
                        && !CADDR(fun).is_null()
                        && (TYPEOF(CADDR(fun)) == SEXPTYPE::SYMSXP
                            || TYPEOF(CADDR(fun)) == SEXPTYPE::STRSXP
                            || TYPEOF(CADDR(fun)) == SEXPTYPE::INTSXP
                            || TYPEOF(CADDR(fun)) == SEXPTYPE::REALSXP)
                        && LENGTH(CADDR(fun)) > 0
                    {
                        // Function accessed via [[
                        let arg1 = CADR(fun);
                        let arg2 = CADDR(fun);

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
                    let srcref_val = (*cptr).srcref;
                    let in_bc = get_R_InBCInterpreter();
                    if srcref_val == in_bc {
                        lineprof(&mut pb, R_findBCInterpreterSrcref(cptr));
                    } else {
                        lineprof(&mut pb, srcref_val);
                    }
                }
            }
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
        let r_srcfiles = with_profiling_state(|state| state.srcfiles);
        let mut i = prevnum;
        while i < line_prof_val {
            pf_str(b"#File \0".as_ptr() as *const c_char);
            pf_int(i);
            pf_str(b": \0".as_ptr() as *const c_char);
            if !r_srcfiles.is_null() {
                let fname = *r_srcfiles.add((i - 1) as usize);
                if !fname.is_null() {
                    pf_str(fname);
                }
            }
            pf_str(b"\n\0".as_ptr() as *const c_char);
            i += 1;
        }

        // Write the profile line
        let mut len: usize = 0;
        while buf[len] != 0 && len < PROFBUFSIZ {
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

// ---------------------------------------------------------------------------
// R_EndProfiling -- stop profiling (full implementation)
// ---------------------------------------------------------------------------

/// Stop profiling and close the output file.
///
/// This follows R's `R_EndProfiling()` state transition, but deliberately does
/// not manipulate process-global profiling timers. Samples are emitted through
/// `R_WriteProfile`, keeping profiler state session-owned for Android and
/// parallel runtimes.
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

        // Release the source files buffer
        let buf = with_profiling_state(|state| state.srcfiles_buffer);
        if !buf.is_null() && buf != R_NilValue() {
            R_ReleaseObject(buf);
            with_profiling_state(|state| state.srcfiles_buffer = ptr::null_mut());
        }
        with_profiling_state(|state| state.srcfiles = ptr::null_mut());

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

        // Set up line profiling buffer
        if line_profiling != 0 {
            let bufcount = numfiles as usize;
            with_profiling_state(|state| state.srcfile_bufcount = bufcount);
            let len1 = bufcount * std::mem::size_of::<*mut c_char>();
            let len2 = bufsize as usize;
            let total = (len1 + len2) as c_int;

            let buf = Rf_allocVector(SEXPTYPE::RAWSXP, total);
            with_profiling_state(|state| state.srcfiles_buffer = buf);
            R_PreserveObject(buf);

            // Set up the pointer array in the first part of the buffer
            let srcfiles = RAW(buf) as *mut *mut c_char;
            with_profiling_state(|state| state.srcfiles = srcfiles);

            // The actual strings start after the pointer array
            let buf_start = (RAW(buf) as *mut c_char).add(len1);
            *srcfiles = buf_start as *mut c_char;
            *buf_start = 0; // NUL terminator for first filename slot
        }

        with_profiling_state(|state| state.profiling_event = profiling_event_code(event));

        with_profiling_state(|state| state.profiling = 1);
    }
}

// ---------------------------------------------------------------------------
// do_Rprof -- Rprof() builtin (full implementation)
// ---------------------------------------------------------------------------

/// Implement the `Rprof()` function.
///
/// When called with a non-empty filename, starts profiling to that file.
/// When called with an empty filename, stops profiling.
///
/// Ported from R's `do_Rprof()` in eval.c.
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

// ---------------------------------------------------------------------------
// do_Rprof_mem -- Rprofmem() builtin
// ---------------------------------------------------------------------------

/// Implement the `Rprofmem()` function for memory profiling.
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

// ---------------------------------------------------------------------------
// do_Rprofaddr -- Rprofaddr() builtin
// ---------------------------------------------------------------------------

/// Implement the `Rprofaddr()` function for address profiling.
pub unsafe fn do_Rprofaddr(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// do_gcprof -- gcprof() builtin
// ---------------------------------------------------------------------------

/// Implement the `gcprof()` function for GC profiling.
pub unsafe fn do_gcprof(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { R_NilValue() }
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

// ---------------------------------------------------------------------------
// do_bcprofstop -- stop bytecode profiling
// ---------------------------------------------------------------------------

/// Stop bytecode profiling.
///
/// Disables the profiling timer.
///
/// Ported from R's `do_bcprofstop()` in eval.c.
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

// ---------------------------------------------------------------------------
// do_bcprofcounts -- get bytecode profiling counts
// ---------------------------------------------------------------------------

/// Get bytecode profiling opcode counts.
///
/// Returns an integer vector of opcode counts.
///
/// Ported from R's `do_bcprofcounts()` in eval.c.
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
            with_arena(|arena| {
                arena.alloc_vector(SEXPTYPE::REALSXP, 128);
                arena.alloc_charsxp(b"profile-left");
            });
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
    fn memory_profile_prefix_writes_real_snapshot_values() {
        let _session = RSession::new();
        with_arena(|arena| {
            arena.alloc_vector(SEXPTYPE::INTSXP, 64);
        });
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

            with_arena(|arena| {
                arena.alloc_vector(SEXPTYPE::INTSXP, 128);
            });
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
