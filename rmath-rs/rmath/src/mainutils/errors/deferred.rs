#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Deferred warning printing: PrintWarnings and the deferred-warnings
//! entry points.

use super::helpers::translateChar;
use super::*;

// ---------------------------------------------------------------------------
// PrintWarnings
// ---------------------------------------------------------------------------

/// Render the collected-warnings block exactly like errors.c `PrintWarnings()`
/// and consume the collection state (including the truncated `last.warning`
/// install). Returns `None` when there is nothing to print.
///
/// Callers own the emission channel: `PrintWarnings()` writes the block to
/// stderr like upstream REprintf, while the script-loop flush routes it into
/// the session output stream to keep Rscript's terminal interleaving.
///
/// Rendering (errors.c:615-673): a single warning prints
/// `Warning message:` then `In <dcall> : <msg>` (or `<msg> ` without a call);
/// two to ten print `Warning messages:` with an `N: ` prefix; longer counts
/// collapse to a summary line. `dcall` is `deparse1s()` of the stored call,
/// and a first line that would exceed LONGWARN (6/10 + dcall + msgline1)
/// wraps with `\n ` before the one-space-indented message.
pub(crate) unsafe fn take_warnings_block() -> Option<String> {
    unsafe {
        let cw = collect_warnings();
        if cw == 0 {
            return None;
        }

        if in_print_warnings() != 0 {
            set_collect_warnings(0);
            set_warnings_ptr(ptr::null_mut());
            return Some("Lost warning messages\n".to_string());
        }

        set_in_print_warnings(1);

        let warnings_ptr = warnings_ptr();
        if warnings_ptr.is_null() || TYPEOF(warnings_ptr) != SEXPTYPE::VECSXP {
            set_in_print_warnings(0);
            return None;
        }

        let names = CAR(ATTRIB(warnings_ptr));
        let msg_of = |i: R_xlen_t| -> String {
            if names.is_null() || TYPEOF(names) != SEXPTYPE::STRSXP {
                return String::new();
            }
            let msg = CHAR_local(STRING_ELT(names, i));
            CStr::from_ptr(msg).to_str().unwrap_or("").to_string()
        };

        let mut block = String::new();

        if cw == 1 {
            block.push_str("Warning message:\n");
            let call = VECTOR_ELT(warnings_ptr, 0);
            let msg = msg_of(0);
            if isNull(call) != 0 {
                // REprintf("%s \n", msg)
                block.push_str(&msg);
                block.push_str(" \n");
            } else {
                let dcall = warning_dcall(call);
                block.push_str("In ");
                block.push_str(&dcall);
                block.push_str(" :");
                let msgline1 = msg.split('\n').next().map_or(0, str::len);
                if 6 + dcall.len() + msgline1 > LONGWARN {
                    block.push_str("\n ");
                }
                block.push(' ');
                block.push_str(&msg);
                block.push('\n');
            }
        } else if cw <= 10 {
            block.push_str("Warning messages:\n");
            for i in 0..cw as R_xlen_t {
                let call = VECTOR_ELT(warnings_ptr, i);
                let msg = msg_of(i);
                if isNull(call) != 0 {
                    block.push_str(&format!("{}: {} \n", i + 1, msg));
                } else {
                    let dcall = warning_dcall(call);
                    block.push_str(&format!("{}: In {} :", i + 1, dcall));
                    let msgline1 = msg.split('\n').next().map_or(0, str::len);
                    if 10 + dcall.len() + msgline1 > LONGWARN {
                        block.push_str("\n ");
                    }
                    block.push(' ');
                    block.push_str(&msg);
                    block.push('\n');
                }
            }
        } else {
            let nw = nwarnings();
            if cw < nw {
                block.push_str(&format!(
                    "There were {} warnings (use warnings() to see them)\n",
                    cw
                ));
            } else {
                block.push_str(&format!(
                    "There were {} or more warnings (use warnings() to see the first {})\n",
                    nw, nw
                ));
            }
        }

        // Truncate and install last.warning (errors.c:685-695): exactly the
        // collected entries, not the spare-capacity collection vector.
        let sym = Rf_install(b"last.warning\0".as_ptr() as *const c_char);
        let last = Rf_allocVector(SEXPTYPE::VECSXP, cw);
        let _last_guard = protect(last);
        let last_names = Rf_allocVector(SEXPTYPE::STRSXP, cw);
        let _names_guard = protect(last_names);
        for i in 0..cw as R_xlen_t {
            SET_VECTOR_ELT(last, i, VECTOR_ELT(warnings_ptr, i));
            if names.is_null() || TYPEOF(names) != SEXPTYPE::STRSXP {
                SET_STRING_ELT(last_names, i, Rf_mkChar(b"\0".as_ptr() as *const c_char));
            } else {
                SET_STRING_ELT(last_names, i, STRING_ELT(names, i));
            }
        }
        setAttrib_wrap(last, R_NamesSymbol(), last_names);
        SET_SYMVALUE(sym, last);

        set_in_print_warnings(0);
        set_collect_warnings(0);
        set_warnings_ptr(ptr::null_mut());
        Some(block)
    }
}

/// `deparse1s()` of a stored warning call as a Rust string (errors.c uses the
/// same rendering for the `In <call> :` header). Falls back to `<call>` when
/// the deparse yields nothing usable, mirroring the error renderer above.
pub(crate) unsafe fn warning_dcall(call: SEXP) -> String {
    unsafe {
        let dcall_sexp = crate::mainutils::deparse::deparse1s(call);
        if dcall_sexp.is_null()
            || dcall_sexp == globals::R_NilValue()
            || TYPEOF(dcall_sexp) != SEXPTYPE::STRSXP
            || XLENGTH(dcall_sexp) == 0
        {
            return "<call>".to_string();
        }
        let cs = STRING_ELT(dcall_sexp, 0);
        if cs.is_null() {
            return "<call>".to_string();
        }
        let cptr = translateChar(cs);
        if cptr.is_null() {
            return "<call>".to_string();
        }
        CStr::from_ptr(cptr).to_string_lossy().into_owned()
    }
}

/// Print collected warnings to stderr — upstream's channel (REprintf).
pub unsafe fn PrintWarnings() {
    unsafe {
        if let Some(block) = take_warnings_block() {
            eprint!("{}", block);
        }
    }
}

/// Flush collected warnings at a top-level statement boundary — the port of
/// main.c's REPL loop tail (`if (R_CollectWarnings) PrintWarnings();` after
/// each evaluated expression). Upstream writes to stderr and the terminal
/// interleaves it with stdout in real time; the session model keeps one
/// output stream, so the block is appended to the interleaved stdout capture
/// (falling back to real stdout when no capture is active). Deliberately
/// bypasses `sink()` diversion — warnings are stderr in upstream.
pub unsafe fn print_warnings_at_statement_boundary() {
    unsafe {
        let Some(block) = take_warnings_block() else {
            return;
        };
        let routed = instance::with_current_instance(|inst| {
            let mut capture = inst.output_capture.borrow_mut();
            if capture.is_capturing() {
                capture.capture_stdout_bypassing_sink(&block);
                true
            } else {
                false
            }
        });
        if routed != Some(true) {
            print!("{}", block);
        }
    }
}

/// do_printDeferredWarnings — print deferred warnings.
pub unsafe fn do_printDeferredWarnings(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if r_show_error_messages() && collect_warnings() > 0 {
            PrintWarnings();
        }
        globals::R_NilValue()
    }
}

/// R_PrintDeferredWarnings — print deferred warnings.
/// Matches C's `static void R_PrintDeferredWarnings(void)`
pub unsafe fn R_PrintDeferredWarnings() {
    unsafe {
        if r_show_error_messages() && collect_warnings() > 0 {
            eprint!("In addition: ");
            PrintWarnings();
        }
    }
}
