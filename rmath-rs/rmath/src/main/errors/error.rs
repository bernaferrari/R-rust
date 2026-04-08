#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Error system: Rf_error, errorcall, check1arg, R_SignalError, REprintf, etc.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::sync::atomic::Ordering;

use crate::main::coerce::coerceVector;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals;

use super::{
    BUFSIZE, IN_ERROR, LONGWARN, R_COLLECT_WARNINGS, R_SHOW_ERROR_CALLS, R_SHOW_ERROR_MESSAGES,
    R_WARNINGS,
};

use super::format::{
    CHAR_local, ScalarLogical, asLogical, checkArity, findCall, format_varargs, getCurrentCall,
    isNull, isString, isValidString, translateChar,
};

use super::warning::PrintWarnings;

// ---------------------------------------------------------------------------
// C library bindings
// ---------------------------------------------------------------------------

unsafe extern "C" {
    #[link_name = "vsnprintf"]
    fn vsnprintf_c(
        buf: *mut c_char,
        size: usize,
        format: *const c_char,
        ap: *mut std::os::raw::c_void,
    ) -> c_int;
}

// ---------------------------------------------------------------------------
// Core error functions
// ---------------------------------------------------------------------------

/// Internal verrorcall_dflt -- the real error handler.
///
/// Ported from R's `verrorcall_dflt()` in errors.c.
pub(crate) fn verrorcall_dflt(call: SEXP, format: *const c_char, ap: *mut std::os::raw::c_void) {
    unsafe {
        use super::R_Expressions_keep;

        // Check for recursive error
        let in_err = IN_ERROR.fetch_add(1, Ordering::Relaxed);
        if in_err > 0 {
            if in_err >= 3 {
                eprint!("Error during wrapup: ");
                if !format.is_null() {
                    let mut buf = vec![0u8; BUFSIZE + 1];
                    if ap.is_null() {
                        let src = CStr::from_ptr(format).to_bytes();
                        let len = src.len().min(BUFSIZE);
                        ptr::copy_nonoverlapping(format as *const u8, buf.as_mut_ptr(), len);
                        buf[len] = 0;
                    } else {
                        vsnprintf_c(buf.as_mut_ptr() as *mut c_char, BUFSIZE, format, ap);
                        buf[BUFSIZE] = 0;
                    }
                    let msg = CStr::from_ptr(buf.as_ptr() as *const c_char)
                        .to_str()
                        .unwrap_or("");
                    eprintln!("{}", msg);
                } else {
                    eprintln!();
                }
            }
            R_COLLECT_WARNINGS.store(0, Ordering::Relaxed);
            R_WARNINGS.store(ptr::null_mut(), Ordering::Relaxed);
            eprintln!(
                "Error: no more error handlers available (recursive errors?); invoking 'abort' restart"
            );
            R_Expressions_keep();
            super::jump_to_top_ex(0, 0, 0, 0, 0);
            return;
        }

        let old_in_err = in_err;
        IN_ERROR.store(1, Ordering::Relaxed);

        // Format the variadic message
        let tmp_str = format_varargs(format, ap);

        // Build the full error message
        let mut err_msg = String::new();

        if !call.is_null() && !isNull(call) != 0 {
            let dcall = "<call>";

            if 7 + dcall.len() + 3 + tmp_str.len() < BUFSIZE {
                err_msg.push_str("Error in ");
                err_msg.push_str(dcall);
                err_msg.push_str(" : ");

                let msg_first_line = tmp_str
                    .find('\n')
                    .map(|i| &tmp_str[..i])
                    .unwrap_or(&tmp_str);
                if 14 + dcall.len() + msg_first_line.len() > LONGWARN {
                    err_msg.push_str("\n  ");
                }
                err_msg.push_str(&tmp_str);
            } else {
                err_msg.push_str("Error: ");
                err_msg.push_str(&tmp_str);
            }
        } else {
            err_msg.push_str("Error: ");
            err_msg.push_str(&tmp_str);
        }

        if !err_msg.ends_with('\n') {
            err_msg.push('\n');
        }

        // Show error call trace if configured
        if R_SHOW_ERROR_CALLS.load(Ordering::Relaxed) && !call.is_null() && !isNull(call) != 0 {
            let tr = super::warning::R_ConciseTraceback(call, 0);
            if !tr.is_empty() && err_msg.len() + tr.len() + 10 < BUFSIZE {
                err_msg.push_str("Calls: ");
                err_msg.push_str(&tr);
                err_msg.push('\n');
            }
        }

        // Write to thread-local errbuf
        super::R_SetErrmessage(&err_msg);

        // Print the error message
        if R_SHOW_ERROR_MESSAGES.load(Ordering::Relaxed) {
            eprint!("{}", super::R_GetErrorBuf());
        }

        // Print deferred warnings if any
        if R_SHOW_ERROR_MESSAGES.load(Ordering::Relaxed)
            && R_COLLECT_WARNINGS.load(Ordering::Relaxed) > 0
        {
            eprint!("In addition: ");
            PrintWarnings();
        }

        IN_ERROR.store(old_in_err, Ordering::Relaxed);
        std::panic::panic_any(RError {
            message: super::R_GetErrorBuf(),
        });
    } // unsafe
}

/// Report an error with a call.
pub unsafe fn errorcall(call: SEXP, format: *const c_char) {
    verrorcall_dflt(call, format, ptr::null_mut());
}

/// Check that the first argument's name matches the expected formal parameter.
pub fn check1arg(arg: SEXP, call: SEXP, formal: *const c_char) {
    unsafe {
        use crate::sexp::accessors::{CHAR as CHAR_ACC, PRINTNAME, TAG};
        let tag = TAG(arg);
        if tag.is_null() || tag == globals::R_NilValue() {
            return;
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() {
            return;
        }
        let supplied = CHAR_ACC(pname);
        if supplied.is_null() {
            return;
        }
        let supplied_str = CStr::from_ptr(supplied).to_str().unwrap_or("");
        let formal_str = if formal.is_null() {
            ""
        } else {
            CStr::from_ptr(formal).to_str().unwrap_or("")
        };
        let ns = supplied_str.len();
        if ns > formal_str.len() || !formal_str.starts_with(supplied_str) {
            let msg = format!(
                "supplied argument name '{}' does not match '{}'",
                supplied_str, formal_str
            );
            let c_msg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");
            errorcall(call, c_msg.as_ptr());
        }
    }
}

/// Report a formatted error with one string argument.
pub fn Rf_errorcall1(call: SEXP, format: *const c_char, arg: *const c_char) {
    unsafe {
        let msg = if arg.is_null() {
            ""
        } else {
            CStr::from_ptr(arg).to_str().unwrap_or("")
        };
        let formatted = format!(
            "{}{}",
            if format.is_null() {
                ""
            } else {
                CStr::from_ptr(format).to_str().unwrap_or("")
            },
            msg
        );
        verrorcall_dflt(
            call,
            std::ffi::CString::new(formatted).expect("CString::new failed: contains null byte").as_ptr(),
            ptr::null_mut(),
        );
    }
}

/// Report a formatted error with call, using printf-style formatting.
pub fn Rf_errorcall_fmt(call: SEXP, format: *const c_char, args: &[&CStr]) {
    unsafe {
        if format.is_null() {
            verrorcall_dflt(call, b"\0".as_ptr() as *const c_char, ptr::null_mut());
            return;
        }
        let fmt = CStr::from_ptr(format).to_str().unwrap_or("");
        let mut result = fmt.to_string();
        for arg_cstr in args {
            let arg_str = arg_cstr.to_str().unwrap_or("");
            if let Some(pos) = result.find("%s") {
                result = format!("{}{}{}", &result[..pos], arg_str, &result[pos + 2..]);
            } else if let Some(pos) = result.find("%d") {
                result = format!("{}{}{}", &result[..pos], arg_str, &result[pos + 2..]);
            } else {
                break;
            }
        }
        let c_result = std::ffi::CString::new(result).expect("CString::new failed: contains null byte");
        verrorcall_dflt(call, c_result.as_ptr(), ptr::null_mut());
    }
}

/// Report an error with a call and pre-formatted message buffer.
pub unsafe fn errorcall_cpy(call: SEXP, format: *const c_char) {
    unsafe {
        let mut buf = vec![0u8; BUFSIZE + 1];
        if !format.is_null() {
            let len = CStr::from_ptr(format).to_bytes().len().min(BUFSIZE - 1);
            ptr::copy_nonoverlapping(format as *const u8, buf.as_mut_ptr(), len);
            buf[len] = 0;
        } else {
            buf[0] = 0;
        }
        errorcall(call, buf.as_ptr() as *const c_char);
    }
}

/// Report an error (without call).
pub unsafe fn Rf_error(format: *const c_char) {
    unsafe {
        let call = getCurrentCall();
        errorcall(call, format);
    }
}

/// Report a formatted error (without call), with one string argument.
pub fn Rf_error1(format: *const c_char, arg: *const c_char) {
    let call = getCurrentCall();
    Rf_errorcall1(call, format, arg);
}

/// Unimplemented error -- for functions that haven't been ported yet.
pub fn Rf_error_unimplemented(name: &str) {
    let msg = format!("function '{}' is not yet implemented", name);
    super::R_SetErrmessage(&msg);
    std::panic::panic_any(RError { message: msg });
}

/// UNIMPLEMENTED -- called from C when a feature is not yet ported.
pub unsafe fn UNIMPLEMENTED(s: *const c_char) {
    unsafe {
        let name = if s.is_null() {
            "unknown"
        } else {
            CStr::from_ptr(s).to_str().unwrap_or("unknown")
        };
        let msg = format!("unimplemented feature in {}", name);
        let c_msg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");
        let call = getCurrentCall();
        errorcall(call, c_msg.as_ptr());
    }
}

/// WrongArgCount -- incorrect number of arguments error.
pub unsafe fn WrongArgCount(s: *const c_char) {
    unsafe {
        let name = if s.is_null() {
            "unknown"
        } else {
            CStr::from_ptr(s).to_str().unwrap_or("unknown")
        };
        let msg = format!("incorrect number of arguments to \"{}\"", name);
        let c_msg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");
        let call = getCurrentCall();
        errorcall(call, c_msg.as_ptr());
    }
}

/// Rf_checkArityCall -- check argument count matches the primitive's arity.
pub unsafe fn Rf_checkArityCall(op: SEXP, args: SEXP, call: SEXP) {
    unsafe {
        let arity = crate::main::names::PRIMARITY(op);
        if arity < 0 {
            return;
        }
        let mut n: c_int = 0;
        let mut p = args;
        while !p.is_null() && p != globals::R_NilValue() {
            n += 1;
            p = CDR(p);
        }
        if n != arity {
            let name = crate::main::names::getPRIMNAME(op);
            let name_str = if name.is_null() {
                "unknown"
            } else {
                std::ffi::CStr::from_ptr(name).to_str().unwrap_or("unknown")
            };
            let is_internal = crate::main::names::PRIMINTERNAL(op);
            let prefix = if is_internal != 0 { ".Internal(" } else { "'" };
            let suffix = if is_internal != 0 { ")" } else { "'" };
            let msg = if n == 1 {
                format!(
                    "{} argument passed to {}{}{} which requires {}",
                    n, prefix, name_str, suffix, arity
                )
            } else {
                format!(
                    "{} arguments passed to {}{}{} which requires {}",
                    n, prefix, name_str, suffix, arity
                )
            };
            let c_msg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");
            errorcall(call, c_msg.as_ptr());
        }
    }
}

// ---------------------------------------------------------------------------
// REprintf
// ---------------------------------------------------------------------------

/// REprintf -- R's error printf (prints to stderr).
#[unsafe(no_mangle)]
pub unsafe fn REprintf(format: *const c_char) {
    unsafe {
        if format.is_null() {
            return;
        }
        let msg = CStr::from_ptr(format).to_str().unwrap_or("");
        eprint!("{}", msg);
    }
}

// ---------------------------------------------------------------------------
// Stack overflow and interrupt checking
// ---------------------------------------------------------------------------

/// Signal a C stack overflow.
pub unsafe fn R_SignalCStackOverflow(usage: isize) {
    unsafe {
        let cond = super::conditions::R_makeCStackOverflowError(globals::R_NilValue(), usage);
        if !cond.is_null() {
            super::conditions::R_signalErrorConditionEx(cond, globals::R_NilValue(), 1);
        } else {
            let msg = format!("C stack usage {} is too close to the limit", usage);
            super::R_SetErrmessage(&msg);
            std::panic::panic_any(RError { message: msg });
        }
    }
}

/// Check for stack overflow.
pub unsafe fn R_CheckStack() {
    unsafe {
        let depth = globals::R_EvalDepth();
        let limit = globals::R_EvalDepthLimit();
        if depth >= limit {
            R_SignalCStackOverflow(depth as isize);
        }
    }
}

/// Check for stack overflow with extra space.
pub unsafe fn R_CheckStack2(_extra: usize) {
    unsafe {
        R_CheckStack();
    }
}

/// Check for user interrupts.
#[unsafe(no_mangle)]
pub unsafe fn R_CheckUserInterrupt() {
    unsafe {
        use super::R_INTERRUPTS_PENDING;
        use super::R_INTERRUPTS_SUSPENDED;
        R_CheckStack();
        if R_INTERRUPTS_SUSPENDED.load(Ordering::Relaxed) {
            return;
        }
        if R_INTERRUPTS_PENDING.load(Ordering::Relaxed) {
            onintr();
        }
    }
}

// ---------------------------------------------------------------------------
// Jump to top level (longjmp replacement)
// ---------------------------------------------------------------------------

/// Jump to the top-level context.
pub unsafe fn jump_to_top_ex(
    _swap: c_int,
    _eval: c_int,
    _print: c_int,
    _reset: c_int,
    _skip: c_int,
) {
    if _print != 0 && R_COLLECT_WARNINGS.load(Ordering::Relaxed) > 0 {
        PrintWarnings();
    }

    IN_ERROR.store(0, Ordering::Relaxed);
    std::panic::panic_any(RError {
        message: "jump_to_top".to_string(),
    });
}

/// Handle interrupt signal.
pub unsafe fn onintr() {
    unsafe {
        jump_to_top_ex(1, 1, 0, 0, 0);
    }
}

/// Handle interrupt signal without resume option.
pub unsafe fn onintrNoResume() {
    unsafe {
        jump_to_top_ex(0, 1, 0, 0, 0);
    }
}

/// jump_to_toplevel -- jump to top level without traceback, user error handler.
pub unsafe fn jump_to_toplevel() {
    unsafe {
        jump_to_top_ex(0, 0, 1, 1, 1);
    }
}

// ---------------------------------------------------------------------------
// R_MissingArgError
// ---------------------------------------------------------------------------

/// R_MissingArgError_c -- report a missing argument error.
pub unsafe fn R_MissingArgError_c(
    arg: *const c_char,
    call: SEXP,
    subclass: *const c_char,
) {
    unsafe {
        let arg_str = if arg.is_null() {
            "argument"
        } else {
            CStr::from_ptr(arg).to_str().unwrap_or("argument")
        };
        let sub = if subclass.is_null() {
            ""
        } else {
            CStr::from_ptr(subclass).to_str().unwrap_or("")
        };
        let msg = if sub.is_empty() {
            format!("argument \"{}\" is missing, with no default", arg_str)
        } else {
            format!("argument \"{}\" is missing, with no default", arg_str)
        };
        let c_msg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");
        errorcall(call, c_msg.as_ptr());
    }
}

/// R_MissingArgError -- report a missing argument error from a symbol.
pub unsafe fn R_MissingArgError(symbol: SEXP, call: SEXP, subclass: *const c_char) {
    unsafe {
        let arg = if symbol.is_null() || TYPEOF(symbol) != SEXPTYPE::SYMSXP.0 {
            "argument"
        } else {
            let name = CHAR_local(PRINTNAME(symbol));
            CStr::from_ptr(name).to_str().unwrap_or("argument")
        };
        R_MissingArgError_c(
            std::ffi::CString::new(arg).expect("CString::new failed: contains null byte").as_ptr(),
            call,
            subclass,
        );
    }
}

// ---------------------------------------------------------------------------
// do_stop
// ---------------------------------------------------------------------------

/// do_stop -- R's stop() function.
pub unsafe fn do_stop(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let mut c_call: SEXP = ptr::null_mut();
        if asLogical(CAR(args)) != 0 {
            c_call = findCall();
        }
        let args = CDR(args);

        if !isNull(CAR(args)) != 0 {
            SETCAR(args, coerceVector(CAR(args), SEXPTYPE::STRSXP.0));
            if isValidString(CAR(args)) == 0 {
                let c_msg = std::ffi::CString::new(" [invalid string in stop(.)]").expect("CString::new failed: contains null byte");
                errorcall(c_call, c_msg.as_ptr());
            }
            let msg = translateChar(STRING_ELT(CAR(args), 0));
            let msg_str = CStr::from_ptr(msg).to_str().unwrap_or("");
            let c_msg = std::ffi::CString::new(msg_str).expect("CString::new failed: contains null byte");
            errorcall(c_call, c_msg.as_ptr());
            ptr::null_mut()
        } else {
            errorcall(c_call, b"\0".as_ptr() as *const c_char);
            ptr::null_mut()
        }
    }
}

// ---------------------------------------------------------------------------
// do_geterrmessage / do_seterrmessage
// ---------------------------------------------------------------------------

/// do_geterrmessage -- geterrmessage().
pub unsafe fn do_geterrmessage(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let msg = super::R_GetErrorBuf();
        Rf_mkString(msg.as_ptr() as *const c_char)
    }
}

/// do_seterrmessage -- seterrmessage().
pub unsafe fn do_seterrmessage(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let msg = CAR(args);
        if isString(msg) == 0 || LENGTH(msg) != 1 {
            errorcall(
                call,
                b"error message must be a character string\x00".as_ptr() as *const c_char,
            );
        }
        let s = CHAR_local(STRING_ELT(msg, 0));
        super::R_SetErrmessage_c(s);
        globals::R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_interruptsSuspended
// ---------------------------------------------------------------------------

/// do_interruptsSuspended -- get/set interrupts suspended flag.
pub unsafe fn do_interruptsSuspended(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        use super::R_INTERRUPTS_SUSPENDED;
        let orig = R_INTERRUPTS_SUSPENDED.load(Ordering::Relaxed);
        if !args.is_null() && isNull(args) == 0 {
            let val = asLogical(CAR(args));
            R_INTERRUPTS_SUSPENDED.store(val != 0, Ordering::Relaxed);
        }
        ScalarLogical(orig as c_int)
    }
}

// ---------------------------------------------------------------------------
// ErrorMessage -- database lookup
// ---------------------------------------------------------------------------

/// ErrorMessage -- look up an error message from the database and call errorcall.
pub unsafe fn ErrorMessage(call: SEXP, which_error: c_int, format: *const c_char) {
    unsafe {
        use super::error_codes;
        let messages = [
            "invalid number of arguments",
            "invalid argument type",
            "time-series/vector length mismatch",
            "incompatible arguments",
            "unimplemented feature in %s",
            "unknown error (report this!)",
        ];

        let idx = if which_error >= 0 && (which_error as usize) < messages.len() {
            which_error as usize
        } else {
            messages.len() - 1
        };

        let msg = if which_error == error_codes::ERROR_UNIMPLEMENTED && !format.is_null() {
            let arg = CStr::from_ptr(format).to_str().unwrap_or("unknown");
            format!("unimplemented feature in {}", arg)
        } else {
            messages[idx].to_string()
        };

        let c_msg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");
        errorcall(call, c_msg.as_ptr());
    }
}

// ---------------------------------------------------------------------------
// R_SignalError (compatibility stub)
// ---------------------------------------------------------------------------

/// R_SignalError -- signal an error condition.
pub unsafe fn R_SignalError(call: SEXP, format: *const c_char) {
    unsafe {
        errorcall(call, format);
    }
}
