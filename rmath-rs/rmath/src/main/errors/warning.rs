#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Warning system: Rf_warning, Rf_warningcall, vwarningcall_dflt, warning state.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::atomic::Ordering;

use crate::attrib_core::R_NamesSymbol;
use crate::main::coerce::coerceVector;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals;
use crate::sexp::symbol::Rf_install;

use super::{
    BUFSIZE, IMMEDIATE_WARNING, IN_ERROR, IN_PRINT_WARNINGS, IN_WARNING, LONGWARN,
    NO_BREAK_WARNING, R_COLLECT_WARNINGS, R_NWARNINGS, R_SHOW_WARN_CALLS, R_WARNINGS,
};

use super::format::{
    CHAR_local, GetOption1, asLogical, checkArity, findCall, format_varargs, format_varargs_to_buf,
    getCurrentCall, isExpression, isInteger, isLanguage, isNull, isValidString, translateChar,
};

// ---------------------------------------------------------------------------
// Core warning handler
// ---------------------------------------------------------------------------

/// Internal vwarningcall_dflt -- the real warning handler.
///
/// Ported from R's `vwarningcall_dflt()` in errors.c.
/// Handles three modes based on `warn` option:
/// - w < 0: ignore
/// - w == 0: collect warnings for later display
/// - w == 1: print immediately
/// - w >= 2: convert to error
pub(crate) fn vwarningcall_dflt(call: SEXP, format: *const c_char, ap: *mut c_void) {
    unsafe {
        // Guard against recursive warnings
        if IN_WARNING.load(Ordering::Relaxed) != 0 {
            return;
        }

        // Check for warning.expression option
        let s = GetOption1(Rf_install(b"warning.expression\0".as_ptr() as *const c_char));
        if !s.is_null() && !isNull(s) != 0 {
            if isLanguage(s) == 0 && isExpression(s) == 0 {
                // Invalid option -- fall through
            } else {
                let msg = format_varargs(format, ap);
                eprintln!("Warning: {}", msg);
                return;
            }
        }

        // Get warn level
        let warn_sym = Rf_install(b"warn\0".as_ptr() as *const c_char);
        let w = asLogical(GetOption1(warn_sym));
        if w == crate::sexp::ffi::NA_INTEGER {
            if IMMEDIATE_WARNING.load(Ordering::Relaxed) {
                // w = 1 -- print immediately
            } else {
                // w = 0 -- default, handled below
            }
        }

        if w < 0 || IN_WARNING.load(Ordering::Relaxed) != 0 || IN_ERROR.load(Ordering::Relaxed) != 0
        {
            return;
        }

        IN_WARNING.store(1, Ordering::Relaxed);

        // Format the variadic message into a string
        let (mut fmt_str, truncated) = format_varargs_to_buf(format, ap);
        if truncated {
            let trunc_msg = " [... truncated]";
            if fmt_str.len() + trunc_msg.len() < BUFSIZE {
                fmt_str.push_str(trunc_msg);
            }
        }

        if w >= 2 {
            // Convert warning to error
            IN_WARNING.store(0, Ordering::Relaxed);
            let full_msg = format!("(converted from warning) {}", fmt_str);
            let c_msg = std::ffi::CString::new(full_msg).expect("CString::new failed: contains null byte");
            errorcall(call, c_msg.as_ptr());
        } else if w == 1 || IMMEDIATE_WARNING.load(Ordering::Relaxed) {
            // Print warnings immediately
            let dcall = if !call.is_null() && !isNull(call) != 0 {
                "<call>"
            } else {
                ""
            };

            if dcall.is_empty() {
                eprint!("Warning:");
            } else {
                eprint!("Warning in {} :", dcall);
                let msg_first_line = fmt_str
                    .find('\n')
                    .map(|i| &fmt_str[..i])
                    .unwrap_or(&fmt_str);
                if 18 + dcall.len() + msg_first_line.len() > LONGWARN {
                    eprintln!();
                    eprint!(" ");
                }
            }
            eprintln!(" {}", fmt_str);

            if R_SHOW_WARN_CALLS.load(Ordering::Relaxed) && !call.is_null() && !isNull(call) != 0 {
                let tr = R_ConciseTraceback(call, 0);
                if !tr.is_empty() {
                    eprintln!("Calls: {}", tr);
                }
            }
        } else {
            // w == 0: collect warnings
            if R_COLLECT_WARNINGS.load(Ordering::Relaxed) == 0 {
                setup_warnings();
            }
            let cw = R_COLLECT_WARNINGS.load(Ordering::Relaxed);
            let nw = R_NWARNINGS.load(Ordering::Relaxed);
            if cw < nw {
                let warnings_ptr = R_WARNINGS.load(Ordering::Relaxed);
                if !warnings_ptr.is_null() && TYPEOF(warnings_ptr) == SEXPTYPE::VECSXP.0 {
                    SET_VECTOR_ELT(warnings_ptr, cw as R_xlen_t, call);
                    let names = CAR(ATTRIB(warnings_ptr));
                    if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP.0 {
                        let mut msg_to_store = fmt_str.to_string();
                        if R_SHOW_WARN_CALLS.load(Ordering::Relaxed)
                            && !call.is_null()
                            && !isNull(call) != 0
                        {
                            let tr = R_ConciseTraceback(call, 0);
                            if !tr.is_empty() && msg_to_store.len() + tr.len() + 8 < BUFSIZE {
                                msg_to_store.push_str("\nCalls: ");
                                msg_to_store.push_str(&tr);
                            }
                        }
                        let c_msg = std::ffi::CString::new(msg_to_store).expect("CString::new failed: contains null byte");
                        let ch = Rf_mkChar(c_msg.as_ptr());
                        SET_STRING_ELT(names, cw as R_xlen_t, ch);
                    }
                    R_COLLECT_WARNINGS.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        IN_WARNING.store(0, Ordering::Relaxed);
    } // unsafe
}

/// Setup the warnings collection vector.
pub(crate) fn setup_warnings() {
    unsafe {
        let nw = R_NWARNINGS.load(Ordering::Relaxed);
        let w = Rf_allocVector(SEXPTYPE::VECSXP.0, nw);
        let names = Rf_allocVector(SEXPTYPE::STRSXP.0, nw);
        super::format::setAttrib_wrap(w, R_NamesSymbol(), names);
        R_WARNINGS.store(w, Ordering::Relaxed);
        R_COLLECT_WARNINGS.store(0, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Public warning functions
// ---------------------------------------------------------------------------

/// Issue a warning with call.
pub unsafe fn warningcall(call: SEXP, format: *const c_char) {
    vwarningcall_dflt(call, format, ptr::null_mut());
}

/// Issue an immediate warning (bypass collection).
pub unsafe fn warningcall_immediate(call: SEXP, format: *const c_char) {
    let prev = IMMEDIATE_WARNING.load(Ordering::Relaxed);
    IMMEDIATE_WARNING.store(true, Ordering::Relaxed);
    vwarningcall_dflt(call, format, ptr::null_mut());
    IMMEDIATE_WARNING.store(prev, Ordering::Relaxed);
}

/// Issue a warning (without call).
#[unsafe(no_mangle)]
pub unsafe fn Rf_warning(format: *const c_char) {
    unsafe {
        let call = getCurrentCall();
        warningcall(call, format);
    }
}

/// Issue an immediate warning (without call).
pub unsafe fn Rf_warning_immediate(format: *const c_char) {
    unsafe {
        let call = getCurrentCall();
        warningcall_immediate(call, format);
    }
}

/// Issue a formatted warning with call (Rust helper).
pub fn Rf_warningcall1(call: SEXP, msg: *const c_char) {
    unsafe {
        let msg_str = if msg.is_null() {
            ""
        } else {
            CStr::from_ptr(msg).to_str().unwrap_or("")
        };
        let c_msg = std::ffi::CString::new(msg_str).expect("CString::new failed: contains null byte");
        warningcall(call, c_msg.as_ptr());
    }
}

/// Issue a formatted warning without call (Rust helper).
pub fn Rf_warning1(msg: *const c_char) {
    let call = getCurrentCall();
    Rf_warningcall1(call, msg);
}

// ---------------------------------------------------------------------------
// Rf_message
// ---------------------------------------------------------------------------

/// Issue a message (R's message()).
pub unsafe fn Rf_message(format: *const c_char) {
    unsafe {
        if format.is_null() {
            println!();
            return;
        }
        let msg = CStr::from_ptr(format).to_str().unwrap_or("");
        let msg = msg.trim_end_matches('\n');
        println!("{}", msg);
    }
}

/// Issue a message with call.
pub fn messagecall(call: SEXP, format: *const c_char) {
    unsafe {
        Rf_message(format);
    }
}

/// Issue a message with append flag.
pub unsafe fn Rf_message_append(format: *const c_char, append: c_int) {
    unsafe {
        if format.is_null() {
            if append == 0 {
                println!();
            }
            return;
        }
        let msg = CStr::from_ptr(format).to_str().unwrap_or("");
        let msg = msg.trim_end_matches('\n');
        if append == 0 {
            println!("{}", msg);
        } else {
            print!("{}", msg);
        }
    }
}

// ---------------------------------------------------------------------------
// do_message
// ---------------------------------------------------------------------------

/// do_message -- R's message() builtin.
pub unsafe fn do_message(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let mut c_call: SEXP = ptr::null_mut();
        if asLogical(CAR(args)) != 0 {
            c_call = findCall();
        }
        let mut args = CDR(args);

        let append = asLogical(CAR(args));
        args = CDR(args);

        if !isNull(CAR(args)) != 0 {
            SETCAR(args, coerceVector(CAR(args), SEXPTYPE::STRSXP.0));
            if isValidString(CAR(args)) != 0 {
                let msg = translateChar(STRING_ELT(CAR(args), 0));
                Rf_message_append(msg, append);
            }
        }

        globals::R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// PrintWarnings
// ---------------------------------------------------------------------------

/// Print collected warnings.
pub fn PrintWarnings() {
    unsafe {
        let cw = R_COLLECT_WARNINGS.load(Ordering::Relaxed);
        if cw == 0 {
            return;
        }

        if IN_PRINT_WARNINGS.load(Ordering::Relaxed) != 0 {
            R_COLLECT_WARNINGS.store(0, Ordering::Relaxed);
            R_WARNINGS.store(ptr::null_mut(), Ordering::Relaxed);
            eprintln!("Lost warning messages");
            return;
        }

        IN_PRINT_WARNINGS.store(1, Ordering::Relaxed);

        let warnings_ptr = R_WARNINGS.load(Ordering::Relaxed);
        if warnings_ptr.is_null() || TYPEOF(warnings_ptr) != SEXPTYPE::VECSXP.0 {
            IN_PRINT_WARNINGS.store(0, Ordering::Relaxed);
            return;
        }

        let names = CAR(ATTRIB(warnings_ptr));

        if cw == 1 {
            eprintln!("Warning message:\n");
            if !isNull(VECTOR_ELT(warnings_ptr, 0)) != 0 {
                if !names.is_null() {
                    let msg = CHAR_local(STRING_ELT(names, 0));
                    let msg_str = CStr::from_ptr(msg).to_str().unwrap_or("");
                    eprintln!(" {}\n", msg_str);
                }
            } else {
                if !names.is_null() {
                    let msg = CHAR_local(STRING_ELT(names, 0));
                    let msg_str = CStr::from_ptr(msg).to_str().unwrap_or("");
                    eprintln!(" {}\n", msg_str);
                }
            }
        } else if cw <= 10 {
            eprintln!("Warning messages:\n");
            for i in 0..cw {
                let call = VECTOR_ELT(warnings_ptr, i as R_xlen_t);
                if !names.is_null() {
                    let msg = CHAR_local(STRING_ELT(names, i as R_xlen_t));
                    let msg_str = CStr::from_ptr(msg).to_str().unwrap_or("");
                    if isNull(call) != 0 {
                        eprintln!("{}: {}\n", i + 1, msg_str);
                    } else {
                        eprintln!("{}: In <call> : {}\n", i + 1, msg_str);
                    }
                }
            }
        } else {
            let nw = R_NWARNINGS.load(Ordering::Relaxed);
            if cw < nw {
                eprintln!("There were {} warnings (use warnings() to see them)\n", cw);
            } else {
                eprintln!(
                    "There were {} or more warnings (use warnings() to see the first {})\n",
                    nw, nw
                );
            }
        }

        IN_PRINT_WARNINGS.store(0, Ordering::Relaxed);
        R_COLLECT_WARNINGS.store(0, Ordering::Relaxed);
        R_WARNINGS.store(ptr::null_mut(), Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// do_warning -- R's warning() function
// ---------------------------------------------------------------------------

/// do_warning -- R's warning() function.
pub unsafe fn do_warning(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let mut c_call: SEXP = ptr::null_mut();
        if asLogical(CAR(args)) != 0 {
            c_call = findCall();
        }
        let mut args = CDR(args);

        if asLogical(CAR(args)) != 0 {
            IMMEDIATE_WARNING.store(true, Ordering::Relaxed);
        } else {
            IMMEDIATE_WARNING.store(false, Ordering::Relaxed);
        }
        args = CDR(args);

        if asLogical(CAR(args)) != 0 {
            NO_BREAK_WARNING.store(true, Ordering::Relaxed);
        } else {
            NO_BREAK_WARNING.store(false, Ordering::Relaxed);
        }
        args = CDR(args);

        if !isNull(CAR(args)) != 0 {
            SETCAR(args, coerceVector(CAR(args), SEXPTYPE::STRSXP.0));
            if isValidString(CAR(args)) == 0 {
                let c_msg = std::ffi::CString::new(" [invalid string in warning(.)]").expect("CString::new failed: contains null byte");
                warningcall(c_call, c_msg.as_ptr());
            } else {
                let msg = translateChar(STRING_ELT(CAR(args), 0));
                let msg_str = CStr::from_ptr(msg).to_str().unwrap_or("");
                let c_msg = std::ffi::CString::new(msg_str).expect("CString::new failed: contains null byte");
                warningcall(c_call, c_msg.as_ptr());
            }
        } else {
            warningcall(c_call, b"\0".as_ptr() as *const c_char);
        }

        IMMEDIATE_WARNING.store(false, Ordering::Relaxed);
        NO_BREAK_WARNING.store(false, Ordering::Relaxed);

        globals::R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// WarningMessage -- database lookup
// ---------------------------------------------------------------------------

/// WarningMessage -- look up a warning message from the database and call warningcall.
pub unsafe fn WarningMessage(call: SEXP, which_warn: c_int, format: *const c_char) {
    unsafe {
        let messages = [
            "NAs introduced by coercion",
            "inaccurate integer conversion in coercion",
            "imaginary parts discarded in coercion",
            "unknown warning (report this!)",
        ];

        let idx = if which_warn >= 0 && (which_warn as usize) < messages.len() {
            which_warn as usize
        } else {
            messages.len() - 1
        };

        let c_msg = std::ffi::CString::new(messages[idx]).expect("CString::new failed: contains null byte");
        warningcall(call, c_msg.as_ptr());
    }
}

// ---------------------------------------------------------------------------
// R_PrintDeferredWarnings
// ---------------------------------------------------------------------------

/// R_PrintDeferredWarnings -- print deferred warnings.
pub fn R_PrintDeferredWarnings() {
    use super::R_SHOW_ERROR_MESSAGES;
    if R_SHOW_ERROR_MESSAGES.load(Ordering::Relaxed)
        && R_COLLECT_WARNINGS.load(Ordering::Relaxed) > 0
    {
        eprint!("In addition: ");
        PrintWarnings();
    }
}

// ---------------------------------------------------------------------------
// Traceback support
// ---------------------------------------------------------------------------

/// R_GetTracebackOnly -- return traceback without deparsing calls.
pub unsafe fn R_GetTracebackOnly(skip: c_int) -> SEXP {
    unsafe {
        let mut nback: c_int = 0;
        let mut ns = skip;

        let ctx = crate::sexp::context::R_GlobalContext();
        let mut c = ctx;
        while !c.is_null() {
            let ctx_ref = &*c;
            if ctx_ref.callflag == crate::sexp::context::ctxt_flags::CTXT_TOPLEVEL {
                break;
            }
            if (ctx_ref.callflag
                & (crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                    | crate::sexp::context::ctxt_flags::CTXT_BUILTIN))
                != 0
            {
                if ns > 0 {
                    ns -= 1;
                } else {
                    nback += 1;
                }
            }
            c = ctx_ref.nextcontext;
        }

        let s = Rf_allocList(nback);
        let mut t = s;
        let mut skip2 = skip;

        c = ctx;
        while !c.is_null() {
            let ctx_ref = &*c;
            if ctx_ref.callflag == crate::sexp::context::ctxt_flags::CTXT_TOPLEVEL {
                break;
            }
            if (ctx_ref.callflag
                & (crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                    | crate::sexp::context::ctxt_flags::CTXT_BUILTIN))
                != 0
            {
                if skip2 > 0 {
                    skip2 -= 1;
                } else {
                    if !t.is_null() {
                        SETCAR(t, ctx_ref.call);
                    }
                    t = CDR(t);
                }
            }
            c = ctx_ref.nextcontext;
        }

        s
    }
}

/// R_ConciseTraceback -- return a concise call chain as a string.
pub fn R_ConciseTraceback(call: SEXP, skip: c_int) -> String {
    unsafe {
        use super::R_NSHOWCALLS;
        let ctx = crate::sexp::context::R_GlobalContext();
        let mut c = ctx;
        let mut buf = String::new();
        let mut ncalls: c_int = 0;
        let mut too_many = false;
        let mut top = "";
        let mut skip_count = skip;

        while !c.is_null() {
            let ctx_ref = &*c;
            if ctx_ref.callflag == crate::sexp::context::ctxt_flags::CTXT_TOPLEVEL {
                break;
            }
            if (ctx_ref.callflag
                & (crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                    | crate::sexp::context::ctxt_flags::CTXT_BUILTIN))
                != 0
            {
                if skip_count > 0 {
                    skip_count -= 1;
                } else {
                    let fun = if !ctx_ref.call.is_null() {
                        CAR(ctx_ref.call)
                    } else {
                        ptr::null_mut()
                    };
                    let this = if !fun.is_null() && TYPEOF(fun) == SEXPTYPE::SYMSXP.0 {
                        let name = CHAR_local(PRINTNAME(fun));
                        CStr::from_ptr(name).to_str().unwrap_or("<Anonymous>")
                    } else {
                        "<Anonymous>"
                    };

                    if this == "stop" || this == "warning" || this == "suppressWarnings" {
                        buf.clear();
                        ncalls = 0;
                        too_many = false;
                    } else {
                        ncalls += 1;
                        if too_many {
                            top = this;
                        } else if buf.len() > R_NSHOWCALLS {
                            buf = format!("... {}", buf);
                            too_many = true;
                            top = this;
                        } else if !buf.is_empty() {
                            buf = format!("{} -> {}", this, buf);
                        } else {
                            buf = this.to_string();
                        }
                    }
                }
            }
            c = ctx_ref.nextcontext;
        }

        if too_many && top.len() < 50 {
            buf = format!("{} {}", top, buf);
        }

        buf
    }
}

// ---------------------------------------------------------------------------
// do_printDeferredWarnings
// ---------------------------------------------------------------------------

/// do_printDeferredWarnings -- print deferred warnings.
pub unsafe fn do_printDeferredWarnings(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        checkArity(op, args);
        R_PrintDeferredWarnings();
        globals::R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// R_GetCurrentSrcref
// ---------------------------------------------------------------------------

/// R_GetCurrentSrcref -- get the current source reference.
pub unsafe fn R_GetCurrentSrcref(skip: c_int) -> SEXP {
    unsafe {
        // Simplified: no source references in Rust port yet
        globals::R_NilValue()
    }
}

/// R_GetSrcFilename -- get source filename from a srcref.
pub unsafe fn R_GetSrcFilename(_srcref: SEXP) -> SEXP {
    unsafe { Rf_mkString(b"\x00".as_ptr() as *const c_char) }
}

// ---------------------------------------------------------------------------
// gettext/ngettext support (simplified -- no actual i18n)
// ---------------------------------------------------------------------------

/// do_gettext -- R's gettext() function (simplified, no i18n).
pub unsafe fn do_gettext(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let string = CADR(args);
        if isNull(string) != 0 || LENGTH(string) == 0 {
            return string;
        }
        string
    }
}

/// do_ngettext -- R's ngettext() function (simplified, no i18n).
pub unsafe fn do_ngettext(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let n = if isInteger(CAR(args)) != 0 && LENGTH(CAR(args)) >= 1 {
            *INTEGER(CAR(args)).offset(0)
        } else {
            crate::sexp::ffi::NA_INTEGER
        };
        let msg1 = CADR(args);
        let msg2 = CADDR(args);

        if n == crate::sexp::ffi::NA_INTEGER || n < 0 {
            errorcall(call, b"invalid 'n' argument\x00".as_ptr() as *const c_char);
        }

        if n == 1 { msg1 } else { msg2 }
    }
}

// ---------------------------------------------------------------------------
// do_bindtextdomain
// ---------------------------------------------------------------------------

/// do_bindtextdomain -- R's bindtextdomain() function (simplified, no i18n).
pub unsafe fn do_bindtextdomain(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if isNull(CAR(args)) != 0 && isNull(CADR(args)) != 0 {
            super::format::ScalarLogical(1)
        } else {
            globals::R_NilValue()
        }
    }
}

// Use safe wrapper from mod.rs for errorcall
use super::errorcall;
