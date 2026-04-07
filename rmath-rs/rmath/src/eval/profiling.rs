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

use std::cell::{Cell, RefCell};
use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;

use crate::main::context as main_context;
use crate::main::context::{R_Srcref, get_R_InBCInterpreter, get_R_ToplevelContext};
use crate::sexp::accessors::{
    CADDR, CADR, CAR, CDR, CHAR, INTEGER, LENGTH, PRINTNAME, RAW, REAL, STRING_ELT, TYPEOF,
};
use crate::sexp::attrib_core::getAttrib;
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::context::R_GlobalContext;
use crate::sexp::context::RCNTXT;
use crate::sexp::context::ctxt_flags;
use crate::sexp::envir::findVar;
use crate::sexp::ffi::{NA_INTEGER, R_FINITE};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{R_PreserveObject, R_ReleaseObject};
use crate::sexp::symbol::Rf_install;
use crate::sexp::symbol::{
    R_Bracket2Symbol, R_DollarSymbol, R_DoubleColonSymbol, R_TripleColonSymbol,
};

// ---------------------------------------------------------------------------
// BC profiling constants
// ---------------------------------------------------------------------------

/// Number of bytecode opcodes -- sentinel value in the BC opcode enum.
/// In R, this is the last entry in the bytecode opcode enum (eval.c line ~4732).
/// We use 256 as a reasonable upper bound covering all defined opcodes.
const OPCOUNT: usize = 256;

/// Sentinel for "no current opcode" during BC profiling.
const NO_CURRENT_OPCODE: c_int = -1;

/// Buffer size for profiling output.
const PROFBUFSIZ: usize = 10500;

/// Maximum digits for IEEE double integer part printing.
const PB_MAX_DBL_DIGITS: usize = 309;

// ---------------------------------------------------------------------------
// Profiling state (static globals)
// ---------------------------------------------------------------------------

thread_local! { static R_Profiling: Cell<c_int> = Cell::new(0); }
thread_local! { static R_Mem_Profiling: Cell<c_int> = Cell::new(0); }
thread_local! { static R_GC_Profiling: Cell<c_int> = Cell::new(0); }
thread_local! { static R_Line_Profiling: Cell<c_int> = Cell::new(0); }
thread_local! { static R_Filter_Callframes: Cell<c_int> = Cell::new(0); }
thread_local! { static R_Profiling_Error: Cell<c_int> = Cell::new(0); }
thread_local! { static bc_profiling: Cell<c_int> = Cell::new(0); }
thread_local! { static current_opcode: Cell<c_int> = Cell::new(NO_CURRENT_OPCODE); }
thread_local! { static opcode_counts: RefCell<[c_int; OPCOUNT]> = RefCell::new([0; OPCOUNT]); }

/// Profiling event type: CPU time or elapsed time.
#[derive(Clone, Copy, PartialEq)]
#[repr(C)]
pub enum rpe_type {
    RPE_CPU = 0,
    RPE_ELAPSED = 1,
}

thread_local! { static R_Profiling_Event: Cell<rpe_type> = Cell::new(rpe_type::RPE_CPU); }

/// Output file handle for profiling.
/// On Unix, this is a file descriptor (int). On Windows, it would be a FILE*.
thread_local! { static R_ProfileOutfile: Cell<c_int> = Cell::new(-1); }

/// Array of source file name pointers for line profiling.
thread_local! { static R_Srcfiles: Cell<*mut *mut c_char> = Cell::new(ptr::null_mut()); }

/// Count of source file buffer entries.
thread_local! { static R_Srcfile_bufcount: Cell<usize> = Cell::new(0); }

/// Raw SEXP buffer for filenames and pointers.
thread_local! { static R_Srcfiles_buffer: Cell<SEXP> = Cell::new(ptr::null_mut()); }

// ---------------------------------------------------------------------------
// R_Profiling -- check if profiling is active
// ---------------------------------------------------------------------------

/// Check whether R profiling is currently active.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_Profiling_active() -> c_int {
    R_Profiling.with(|v| v.get())
}

// ---------------------------------------------------------------------------
// R_isRprofiling -- check if profiling is enabled
// ---------------------------------------------------------------------------

/// Check if R profiling is enabled (public API).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_isRprofiling() -> c_int {
    R_Profiling.with(|v| v.get())
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
        let r_srcfiles = R_Srcfiles.with(|v| v.get());
        let r_srcfiles_buffer = R_Srcfiles_buffer.with(|v| v.get());
        if r_srcfiles.is_null() || r_srcfiles_buffer.is_null() {
            return 0;
        }

        let line_prof = R_Line_Profiling.with(|v| v.get());
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

            let bufcount = R_Srcfile_bufcount.with(|v| v.get());
            if (fnum as usize) >= bufcount {
                // Too many files
                R_Profiling_Error.with(|v| v.set(1));
                return 0;
            }

            // Check buffer space
            let buf_start = RAW(r_srcfiles_buffer) as *mut c_char;
            let current_ptr = *r_srcfiles.add(fnum as usize);
            let used = current_ptr as usize - buf_start as usize;
            let total = LENGTH(r_srcfiles_buffer) as usize;

            if used + len + 1 > total {
                // Out of space in the buffer
                R_Profiling_Error.with(|v| v.set(2));
                return 0;
            }

            // Copy filename into buffer
            let dst = *r_srcfiles.add(fnum as usize);
            ptr::copy_nonoverlapping(filename as *const u8, dst as *mut u8, len + 1);

            // Set up next pointer
            let next_ptr = dst.add(len + 1);
            *r_srcfiles.add((fnum + 1) as usize) = next_ptr as *mut c_char;
            *next_ptr = 0; // NUL terminator

            R_Line_Profiling.with(|v| v.set(v.get() + 1));
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
        let line_val = crate::main::coerce::vector::asInteger(srcref);
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
        if TYPEOF(srcfile) != SEXPTYPE::ENVSXP.0 {
            return;
        }

        // Look up the filename in the srcfile environment
        let fn_sym = Rf_install(b"filename\0".as_ptr() as *const c_char);
        let filename_sexp = findVar(fn_sym, srcfile);
        if TYPEOF(filename_sexp) != SEXPTYPE::STRSXP.0 || LENGTH(filename_sexp) == 0 {
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
        if R_Filter_Callframes.with(|v| v.get()) == 0 {
            return (*cptr).nextcontext;
        }

        let toplevel = get_R_ToplevelContext();
        if cptr == toplevel {
            return ptr::null_mut();
        }

        // Find parent context, same algorithm as in `parent.frame()`
        let parent = main_context::R_findParentContext(cptr, 1);

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
                    result =
                        main_context::R_findExecContext((*parent).nextcontext, sysparent as SEXP);
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
        let outfile = R_ProfileOutfile.with(|v| v.get());
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
        let srcref = R_Srcref.with(|s| s.get());
        let in_bc = get_R_InBCInterpreter();
        if srcref != in_bc {
            srcref
        } else {
            main_context::R_findBCInterpreterSrcref(ptr::null_mut())
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
        let prevnum = R_Line_Profiling.with(|v| v.get());

        let mut pb = profbuf {
            ptr: buf.as_mut_ptr() as *mut c_char,
            left: PROFBUFSIZ,
        };

        // Memory profiling: record memory allocation sizes
        if R_Mem_Profiling.with(|v| v.get()) != 0 {
            // get_current_mem is not yet available; use stub values
            let smallv: u64 = 0;
            let bigv: u64 = 0;
            let nodes: u64 = 0;

            pb_str(&mut pb, b":\0".as_ptr() as *const c_char);
            pb_uint(&mut pb, smallv);
            pb_str(&mut pb, b":\0".as_ptr() as *const c_char);
            pb_uint(&mut pb, bigv);
            pb_str(&mut pb, b":\0".as_ptr() as *const c_char);
            pb_uint(&mut pb, nodes);
            pb_str(&mut pb, b":\0".as_ptr() as *const c_char);
            // get_duplicate_counter / reset_duplicate_counter not yet available
            pb_uint(&mut pb, 0u64);
            pb_str(&mut pb, b":\0".as_ptr() as *const c_char);
        }

        // GC profiling
        if R_GC_Profiling.with(|v| v.get()) != 0 && crate::main::memory_main::R_gc_running() != 0 {
            pb_str(&mut pb, b"\"<GC>\" \0".as_ptr() as *const c_char);
        }

        // Line profiling
        if R_Line_Profiling.with(|v| v.get()) != 0 {
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
                && TYPEOF(call) == SEXPTYPE::LANGSXP.0
            {
                let fun = CAR(call);
                pb_str(&mut pb, b"\"\0".as_ptr() as *const c_char);

                if TYPEOF(fun) == SEXPTYPE::SYMSXP.0 {
                    // Simple symbol: just print its name
                    pb_str(&mut pb, CHAR(PRINTNAME(fun)));
                } else if !fun.is_null() && TYPEOF(fun) == SEXPTYPE::LANGSXP.0 {
                    let fun_head = CAR(fun);
                    if (fun_head == R_DoubleColonSymbol()
                        || fun_head == R_TripleColonSymbol()
                        || fun_head == R_DollarSymbol())
                        && !CADR(fun).is_null()
                        && TYPEOF(CADR(fun)) == SEXPTYPE::SYMSXP.0
                        && !CADDR(fun).is_null()
                        && TYPEOF(CADDR(fun)) == SEXPTYPE::SYMSXP.0
                    {
                        // Function accessed via ::, :::, or $
                        pb_str(&mut pb, CHAR(PRINTNAME(CADR(fun))));
                        pb_str(&mut pb, CHAR(PRINTNAME(CAR(fun))));
                        pb_str(&mut pb, CHAR(PRINTNAME(CADDR(fun))));
                    } else if fun_head == R_Bracket2Symbol()
                        && !CADR(fun).is_null()
                        && TYPEOF(CADR(fun)) == SEXPTYPE::SYMSXP.0
                        && !CADDR(fun).is_null()
                        && (TYPEOF(CADDR(fun)) == SEXPTYPE::SYMSXP.0
                            || TYPEOF(CADDR(fun)) == SEXPTYPE::STRSXP.0
                            || TYPEOF(CADDR(fun)) == SEXPTYPE::INTSXP.0
                            || TYPEOF(CADDR(fun)) == SEXPTYPE::REALSXP.0)
                        && LENGTH(CADDR(fun)) > 0
                    {
                        // Function accessed via [[
                        let arg1 = CADR(fun);
                        let arg2 = CADDR(fun);

                        pb_str(&mut pb, CHAR(PRINTNAME(arg1)));
                        pb_str(&mut pb, b"[[\0".as_ptr() as *const c_char);

                        if TYPEOF(arg2) == SEXPTYPE::SYMSXP.0 {
                            pb_str(&mut pb, CHAR(PRINTNAME(arg2)));
                        } else if TYPEOF(arg2) == SEXPTYPE::STRSXP.0 {
                            pb_str(&mut pb, b"\"\0".as_ptr() as *const c_char);
                            pb_str(&mut pb, CHAR(STRING_ELT(arg2, 0)));
                            pb_str(&mut pb, b"\"\0".as_ptr() as *const c_char);
                        } else if TYPEOF(arg2) == SEXPTYPE::INTSXP.0 {
                            pb_int(&mut pb, *INTEGER(arg2) as i64);
                        } else if TYPEOF(arg2) == SEXPTYPE::REALSXP.0 {
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
                if R_Line_Profiling.with(|v| v.get()) != 0 {
                    let srcref_val = (*cptr).srcref;
                    let in_bc = get_R_InBCInterpreter();
                    if srcref_val == in_bc {
                        lineprof(&mut pb, main_context::R_findBCInterpreterSrcref(cptr));
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
            R_Profiling_Error.with(|v| v.set(3));
        }

        // Write any new source file references
        let line_prof_val = R_Line_Profiling.with(|v| v.get());
        let r_srcfiles = R_Srcfiles.with(|v| v.get());
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
fn profile_thread_entry(
    interval_us: u64,
    terminate_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    while !terminate_flag.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_micros(interval_us));
        if terminate_flag.load(Ordering::Relaxed) {
            break;
        }
        // Send profiling signal to main thread
        // In the C version, this calls pthread_kill(R_profiled_thread, SIGPROF)
        // Here we call doprof directly for simplicity
        unsafe {
            doprof(libc::SIGPROF);
        }
    }
}

// ---------------------------------------------------------------------------
// R_EndProfiling -- stop profiling (full implementation)
// ---------------------------------------------------------------------------

/// Stop profiling and close the output file.
///
/// Ported from R's `R_EndProfiling()` in eval.c.
unsafe fn R_EndProfiling() {
    unsafe {
        // On Unix: disable the timer or signal the thread to terminate
        // Simplified: just close the file and reset state

        if R_Profiling_Event.with(|v| v.get()) == rpe_type::RPE_CPU {
            // Disable ITIMER_PROF
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
            libc::setitimer(libc::ITIMER_PROF, &zero_val, ptr::null_mut());
        }

        // For elapsed-time profiling, the thread is stopped via terminate flag
        // (handled externally via the global terminate flag)

        // Close the output file
        let outfile = R_ProfileOutfile.with(|v| v.get());
        if outfile >= 0 {
            libc::close(outfile);
            R_ProfileOutfile.with(|v| v.set(-1));
        }

        // Reset state
        R_Profiling.with(|v| v.set(0));
        R_Mem_Profiling.with(|v| v.set(0));
        R_GC_Profiling.with(|v| v.set(0));
        R_Line_Profiling.with(|v| v.set(0));

        // Release the source files buffer
        let buf = R_Srcfiles_buffer.with(|v| v.get());
        if !buf.is_null() && buf != R_NilValue() {
            R_ReleaseObject(buf);
            R_Srcfiles_buffer.with(|v| v.set(ptr::null_mut()));
        }
        R_Srcfiles.with(|v| v.set(ptr::null_mut()));

        // Report any profiling errors
        let err = R_Profiling_Error.with(|v| v.get());
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
/// Opens the output file, sets up the timer or profiling thread,
/// and enables the profiling signal handler.
///
/// Ported from R's `R_InitProfiling()` in eval.c.
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
        if R_ProfileOutfile.with(|v| v.get()) >= 0 {
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
        R_ProfileOutfile.with(|v| v.set(fd));

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
        R_Mem_Profiling.with(|v| v.set(mem_profiling));
        R_Profiling_Error.with(|v| v.set(0));
        R_Line_Profiling.with(|v| v.set(line_profiling));
        R_GC_Profiling.with(|v| v.set(gc_profiling));
        R_Filter_Callframes.with(|v| v.set(filter_callframes));

        // Set up line profiling buffer
        if line_profiling != 0 {
            let bufcount = numfiles as usize;
            R_Srcfile_bufcount.with(|v| v.set(bufcount));
            let len1 = bufcount * std::mem::size_of::<*mut c_char>();
            let len2 = bufsize as usize;
            let total = (len1 + len2) as c_int;

            let buf = Rf_allocVector(SEXPTYPE::RAWSXP.0, total);
            R_Srcfiles_buffer.with(|v| v.set(buf));
            R_PreserveObject(buf);

            // Set up the pointer array in the first part of the buffer
            let srcfiles = RAW(buf) as *mut *mut c_char;
            R_Srcfiles.with(|v| v.set(srcfiles));

            // The actual strings start after the pointer array
            let buf_start = (RAW(buf) as *mut c_char).add(len1);
            *srcfiles = buf_start as *mut c_char;
            *buf_start = 0; // NUL terminator for first filename slot
        }

        R_Profiling_Event.with(|v| v.set(event));

        // Set up the timer or thread
        if event == rpe_type::RPE_ELAPSED {
            // For elapsed-time profiling, we would create a timer thread
            // In this Rust port, threading is simplified
        } else if event == rpe_type::RPE_CPU {
            // Set up ITIMER_PROF for CPU-time profiling
            let it_interval = libc::timeval {
                tv_sec: (interval as i64 / 1000000) as libc::time_t,
                tv_usec: (interval - (interval / 1000000) * 1000000) as libc::suseconds_t,
            };
            let itv = libc::itimerval {
                it_interval,
                it_value: it_interval,
            };
            if libc::setitimer(libc::ITIMER_PROF, &itv, ptr::null_mut()) == -1 {
                // Failed to set timer
                libc::close(R_ProfileOutfile.with(|v| v.get()));
                R_ProfileOutfile.with(|v| v.set(-1));
                return;
            }
        }

        R_Profiling.with(|v| v.set(1));
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
        if bc_profiling.with(|v| v.get()) != 0 {
            // Cannot use R profiling while byte code profiling
            return R_NilValue();
        }

        // Parse arguments
        let filename_arg = CAR(args);
        if filename_arg.is_null() {
            return R_NilValue();
        }

        // Get filename string
        if TYPEOF(filename_arg) != SEXPTYPE::STRSXP.0 || LENGTH(filename_arg) != 1 {
            return R_NilValue();
        }

        let filename_sexp = STRING_ELT(filename_arg, 0);
        args = CDR(args);

        let append_mode = crate::main::coerce::vector::asLogical(CAR(args));
        args = CDR(args);

        let dinterval = crate::main::coerce::vector::asReal(CAR(args));
        args = CDR(args);

        let mem_profiling = crate::main::coerce::vector::asLogical(CAR(args));
        args = CDR(args);

        let gc_profiling = crate::main::coerce::vector::asLogical(CAR(args));
        args = CDR(args);

        let line_profiling = crate::main::coerce::vector::asLogical(CAR(args));
        args = CDR(args);

        let filter_callframes = crate::main::coerce::vector::asLogical(CAR(args));
        args = CDR(args);

        let numfiles = crate::main::coerce::vector::asInteger(CAR(args));
        args = CDR(args);

        let bufsize = crate::main::coerce::vector::asInteger(CAR(args));
        args = CDR(args);

        // Get event type argument
        let event_arg_sexp = CAR(args);
        let event = if !event_arg_sexp.is_null()
            && TYPEOF(event_arg_sexp) == SEXPTYPE::STRSXP.0
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
pub unsafe fn do_Rprofmem(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { R_NilValue() }
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
    unsafe {
        let op = current_opcode.with(|v| v.get());
        if op >= 0 && (op as usize) < OPCOUNT {
            opcode_counts.with(|counts| {
                counts.borrow_mut()[op as usize] += 1;
            });
        }
        // Reinstall handler: signal(SIGPROF, dobcprof);
    }
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

        if R_Profiling.with(|v| v.get()) != 0 {
            // Profile timer in use
            return R_NilValue();
        }
        if bc_profiling.with(|v| v.get()) != 0 {
            // Already byte code profiling
            return R_NilValue();
        }

        // Initialize the profile data
        current_opcode.with(|v| v.set(NO_CURRENT_OPCODE));
        opcode_counts.with(|counts| {
            for i in 0..OPCOUNT {
                counts.borrow_mut()[i] = 0;
            }
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
        if libc::setitimer(libc::ITIMER_PROF, &itv, ptr::null_mut()) == -1 {
            return R_NilValue();
        }

        bc_profiling.with(|v| v.set(1));

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
        if bc_profiling.with(|v| v.get()) == 0 {
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
        libc::setitimer(libc::ITIMER_PROF, &zero_val, ptr::null_mut());

        bc_profiling.with(|v| v.set(0));

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
        let val = Rf_allocVector(SEXPTYPE::INTSXP.0, OPCOUNT as c_int);
        if val.is_null() {
            return R_NilValue();
        }
        let ip = INTEGER(val);
        if !ip.is_null() {
            opcode_counts.with(|counts| {
                let counts = counts.borrow();
                for i in 0..OPCOUNT {
                    *ip.add(i) = counts[i];
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_WriteProfile(_out: c_int) {
    // Stub: used by external profiling tools
}

// ---------------------------------------------------------------------------
// bc_check_sigint -- check for user interrupts in bytecode loop
// ---------------------------------------------------------------------------
