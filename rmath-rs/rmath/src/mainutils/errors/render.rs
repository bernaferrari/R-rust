#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Error/warning/message rendering: current-call lookup, verrorcall and
//! vwarningcall default paths, and the Rf_error/Rf_warning entry points.

use super::helpers::translateChar;
use super::*;

// ---------------------------------------------------------------------------
// getCurrentCall (simplified)
// ---------------------------------------------------------------------------

/// Get the current call from the context stack.
/// In C this walks R_GlobalContext; here we use the thread-local context.
pub(super) unsafe fn getCurrentCall() -> SEXP {
    unsafe {
        // A context counts as carrying a call only when it holds a real
        fn usable_call(call: SEXP) -> SEXP {
            unsafe {
                if call.is_null()
                    || call == globals::R_NilValue()
                    || TYPEOF(call) == SEXPTYPE::NILSXP
                {
                    globals::R_NilValue()
                } else {
                    call
                }
            }
        }

        let ctx = crate::sexp::context::R_GlobalContext();
        if ctx.is_null() {
            return globals::R_NilValue();
        }
        let c = &*ctx;
        // Skip CTXT_BUILTIN contexts
        if (c.callflag & crate::sexp::context::ctxt_flags::CTXT_BUILTIN) != 0
            && !c.nextcontext.is_null()
        {
            let next = &*c.nextcontext;
            return usable_call(next.call);
        }
        usable_call(c.call)
    }
}

/// Public accessor mirroring upstream `getCurrentCall()` (errors.c): the call
/// of the innermost context on the context stack (skipping a CTXT_BUILTIN
/// top frame). Used by interpreter raise sites to attribute errors to the
/// enclosing R call, like upstream `R_MissingArgError`/`error()`.
pub unsafe fn R_getCurrentCall() -> SEXP {
    unsafe { getCurrentCall() }
}

/// findCall: find the function context's call for error reporting.
pub(super) unsafe fn findCall() -> SEXP {
    unsafe {
        let ctx = crate::sexp::context::R_GlobalContext();
        if ctx.is_null() {
            return globals::R_NilValue();
        }
        let mut c = (*ctx).nextcontext;
        while !c.is_null() {
            let ctx_ref = &*c;
            if ctx_ref.callflag == crate::sexp::context::ctxt_flags::CTXT_TOPLEVEL {
                break;
            }
            if (ctx_ref.callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION) != 0 {
                return if ctx_ref.call.is_null() {
                    globals::R_NilValue()
                } else {
                    ctx_ref.call
                };
            }
            c = ctx_ref.nextcontext;
        }
        globals::R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Core error functions
// ---------------------------------------------------------------------------

/// Internal verrorcall_dflt — the real error handler.
///
/// This formats the error message into errbuf, prints it if allowed,
/// and then panics with RError to unwind the stack.
///
/// Ported from R's `verrorcall_dflt()` in errors.c.
///
/// The `ap` parameter is a `*mut c_void` that should be cast to `va_list`.
/// When called from Rust code, ap is typically null and the format string
/// is already the final message.
/// Drop guard that mirrors C's `restore_inError` callback.
/// Guaranteed to run even if the panic is caught mid-stack, because
/// Drop runs during unwinding before catch_unwind handlers.
pub(super) struct RestoreInError {
    old_in_error: i32,
    old_expressions: c_int,
}

impl Drop for RestoreInError {
    fn drop(&mut self) {
        set_in_error(self.old_in_error);
        R_SetExpressions(self.old_expressions);
    }
}

/// Strip a baked-in "Error in <call> : " / "Error: " rendering prefix from a
/// message so condition payloads carry the bare message, as upstream does:
/// the prefix belongs to top-level stderr rendering only.
pub(super) fn strip_call_prefix(message: &str) -> String {
    if let Some(rest) = message.strip_prefix("Error in ") {
        // Find the " : " separator; upstream renders "<call> : <message>".
        if let Some(pos) = rest.find(" : ") {
            let candidate = &rest[pos + 3..];
            if !candidate.is_empty() {
                return candidate.to_string();
            }
        }
    }
    if let Some(rest) = message.strip_prefix("Error: ") {
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    message.to_string()
}

/// `show.error.locations` gate (errors.c:803-808): a length-1 option whose
/// value is logically TRUE, or a string partially matching "top", enables
/// the `(from ...)` location marker. Other strings ("bottom" included —
/// upstream dropped bottom-position support) and NA leave it off; numeric
/// values convert to logical.
pub(super) unsafe fn show_error_locations_enabled() -> bool {
    unsafe {
        let opt = GetOption1(Rf_install(
            b"show.error.locations\0".as_ptr() as *const c_char
        ));
        if opt.is_null() || isNull(opt) != 0 || XLENGTH(opt) != 1 {
            return false;
        }
        if crate::mainutils::coerce::asLogical(opt) == 1 {
            return true;
        }
        if TYPEOF(opt) == SEXPTYPE::STRSXP {
            let pattern = Rf_mkString(b"top\0".as_ptr() as *const c_char);
            let _pattern_guard = protect(pattern);
            return crate::mainutils::match_mod::pmatch(pattern, opt, 0) != 0;
        }
        false
    }
}

pub(super) unsafe fn verrorcall_dflt(call: SEXP, format: *const c_char, ap: *mut c_void) {
    unsafe {
        let old_in_err = in_error();
        if old_in_err > 0 {
            // fail-safe handler for recursive errors
            if old_in_err >= 3 {
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
            // Clean up warnings
            set_collect_warnings(0);
            set_warnings_ptr(ptr::null_mut());
            eprintln!(
                "Error: no more error handlers available (recursive errors?); invoking 'abort' restart"
            );
            R_Expressions_keep();
            jump_to_top_ex(0, 0, 0, 0, 0);
            return;
        }

        // Push a Drop guard — equivalent to C's begincontext + cend = &restore_inError.
        // This guarantees IN_ERROR and R_Expressions are restored even if the panic
        // is caught by an intermediate catch_unwind frame.
        let _guard = RestoreInError {
            old_in_error: old_in_err,
            old_expressions: R_Expressions(),
        };
        set_in_error(1);

        // Format the variadic message.  Like errors.c:790-817, the message
        // is capped by the warning length: with a call at
        // min(BUFSIZE, R_WarnLength) + 1 - strlen("Error in ") bytes for
        // Rvsnprintf (i.e. warn_len - 9 characters), without a call at
        // warn_len - 7 ("Error: ").
        let warn_len = BUFSIZE.min(r_warn_length().max(0) as usize);
        let has_call = !call.is_null() && isNull(call) == 0;
        let head_len = if has_call {
            b"Error in ".len()
        } else {
            b"Error: ".len()
        };
        let tmp_cap = (warn_len + 1).saturating_sub(head_len).saturating_sub(1);
        let mut tmp_str = format_varargs(format, ap);
        truncate_bytes(&mut tmp_str, tmp_cap);

        // errors.c:804-819 — resolve the `(from <loc>)` marker before
        // rendering: gated by show.error.locations, pointing at the 1-based
        // index of the top-level expression being evaluated. The port parses
        // scripts without srcrefs, so the script-loop position stands in for
        // upstream's srcref-derived `#line` (GetSrcLoc with an unnamed
        // srcfile renders exactly this `(from #n)` shape); 0 = no location.
        let location_no = if show_error_locations_enabled() {
            toplevel_expr_no()
        } else {
            0
        };
        let location_mark = if location_no > 0 {
            format!(" (from #{location_no})")
        } else {
            String::new()
        };
        // ERRBUFCAT only concatenates while the total stays under BUFSIZE.
        let errcat = |buf: &mut String, s: &str| {
            if buf.len() + s.len() < BUFSIZE {
                buf.push_str(s);
            }
        };

        // Build the full error message and write to errbuf via R_SetErrmessage
        let mut err_msg = String::new();

        if has_call {
            // Error with call — "Error in <call> : <message>"
            // Upstream errors.c verrorcall_dflt deparses the call with
            // deparse1s(); reuse the faithful port instead of a placeholder.
            let dcall_sexp = crate::mainutils::deparse::deparse1s(call);
            let dcall: String = if dcall_sexp.is_null()
                || dcall_sexp == globals::R_NilValue()
                || TYPEOF(dcall_sexp) != SEXPTYPE::STRSXP
                || XLENGTH(dcall_sexp) == 0
            {
                "<call>".to_string()
            } else {
                let cs = STRING_ELT(dcall_sexp, 0);
                if cs.is_null() {
                    "<call>".to_string()
                } else {
                    let cptr = translateChar(cs);
                    if cptr.is_null() {
                        "<call>".to_string()
                    } else {
                        std::ffi::CStr::from_ptr(cptr)
                            .to_string_lossy()
                            .into_owned()
                    }
                }
            };

            // errors.c:818 — the buffer-fit test is strlen("Error in ") +
            // strlen("\n  ") + strlen(tmp) < BUFSIZE; the deparsed call
            // participates only in the LONGWARN wrap decision below.
            if head_len + b"\n  ".len() + tmp_str.len() < BUFSIZE {
                errcat(&mut err_msg, "Error in ");
                errcat(&mut err_msg, &dcall);
                // errors.c:815-819/828-830 — the "(from <loc>)" marker sits
                // between the deparsed call and the " : " separator.
                errcat(&mut err_msg, &location_mark);
                errcat(&mut err_msg, " : ");

                // Check if first line is too long
                // (14 + strlen(dcall) + msgline1 > LONGWARN).
                let msg_first_line = tmp_str
                    .find('\n')
                    .map(|i| &tmp_str[..i])
                    .unwrap_or(&tmp_str);
                if 14 + dcall.len() + msg_first_line.len() > LONGWARN {
                    errcat(&mut err_msg, "\n  ");
                }
                errcat(&mut err_msg, &tmp_str);
            } else {
                // Fallback: just "Error: <message>"
                errcat(&mut err_msg, "Error: ");
                errcat(&mut err_msg, &tmp_str);
            }
        } else {
            // Error without call — "Error: <message>". Upstream only decorates
            // the "Error in <call>" form (a top-level stop() carries no call),
            // so here the location marker appends to the rendered line
            // instead, keeping every top-level error line locatable.
            errcat(&mut err_msg, "Error: ");
            errcat(&mut err_msg, &tmp_str);
            errcat(&mut err_msg, &location_mark);
        }

        // Approximate truncation detection (errors.c:855-863): with a
        // single-byte locale (R_MB_CUR_MAX == 1) this can only trigger when
        // the buffer already overflowed past BUFSIZE - 1.
        let nc = err_msg.len();
        if nc > BUFSIZE - 1 {
            let end = (nc + 1).min(BUFSIZE + 1 - 4);
            truncate_bytes(&mut err_msg, end - 1);
            err_msg.push_str("...\n");
        } else {
            // Ensure newline termination
            if !err_msg.ends_with('\n') {
                err_msg.push('\n');
            }

            // Show error call trace if configured (errors.c:870-882:
            // nc_tr + nc + strlen("Calls:") + 2 < BUFSIZE + 1).
            if r_show_error_calls() && has_call {
                let tr = R_ConciseTraceback(call, 0);
                if !tr.is_empty() && tr.len() + err_msg.len() + b"Calls:".len() + 2 < BUFSIZE + 1 {
                    err_msg.push_str("Calls: ");
                    err_msg.push_str(&tr);
                    err_msg.push('\n');
                }
            }
        }

        // Payload contract: the RError message is the BARE message (what
        // condition objects / tryCatch handlers see, matching upstream where
        // "Error in <call> :" attribution is added only by top-level error
        // printing). Strip any prefix that a raise site baked into its text so
        // the payload stays clean; the rendered errbuf above keeps the full
        // attribution for stderr.
        let payload_message = strip_call_prefix(&tmp_str);

        // Write to thread-local errbuf via R_SetErrmessage
        R_SetErrmessage(&err_msg);

        // Record that this exact error message was rendered into the error
        // buffer so the top-level renderer (and the builtin-dispatch
        // attribution wrapper) trust the buffer for this error only —
        // renders from previously caught errors must not leak into later
        // results (upstream: caught errors never reach this printer).
        set_last_rendered_message(Some(payload_message.clone()));

        // Emission contract, exactly once:
        // - When no output capture is active (standalone embedding), write
        //   the rendered text to process stderr here, like Rscript.
        // - When the session is capturing output, do NOT emit here. The
        //   error may still be caught by tryCatch up-stack (upstream prints
        //   nothing for caught errors); the top-level embedding layer emits
        //   the rendered error buffer text once, and only when the error
        //   actually escapes the script. Emitting into the captured-stderr
        //   channel here would leak caught errors into successful results.
        if r_show_error_messages() && !crate::sexp::output::is_capturing() {
            eprint!("{}", R_GetErrorBuf());
        }

        // Deferred warnings follow the same rule (upstream prints them only
        // for errors that reach top-level printing).
        if r_show_error_messages() && collect_warnings() > 0 && !crate::sexp::output::is_capturing()
        {
            eprint!("In addition: ");
            PrintWarnings();
        }

        // The Drop guard (_guard) will restore IN_ERROR and R_Expressions
        // automatically.
        std::panic::panic_any(RError {
            message: payload_message,
        });
    }
}

/// Truncate `s` to at most `limit` bytes without splitting a UTF-8
/// character (the intent of errors.c's mbcsTruncateToValid).
pub(super) fn truncate_bytes(s: &mut String, limit: usize) {
    if s.len() <= limit {
        return;
    }
    let mut end = limit;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
}

// ---------------------------------------------------------------------------
// Handler dispatch for tryCatch/withCallingHandlers support
// ---------------------------------------------------------------------------

pub(super) unsafe fn findSimpleErrorHandler() -> SEXP {
    unsafe {
        let mut list = handler_stack();
        while !list.is_null() && list != globals::R_NilValue() {
            let entry = CAR(list);
            let class_ptr = CHAR(ENTRY_CLASS(entry));
            if !class_ptr.is_null() {
                let class_str = CStr::from_ptr(class_ptr).to_bytes();
                if class_str == b"simpleError" || class_str == b"error" || class_str == b"condition"
                {
                    return list;
                }
            }
            list = CDR(list);
        }
        globals::R_NilValue()
    }
}

pub(super) unsafe fn gotoExitingHandler(cond: SEXP, call: SEXP, entry: SEXP) {
    unsafe {
        let rho = ENTRY_TARGET_ENVIR(entry);
        let result = ENTRY_RETURN_RESULT(entry);
        SET_VECTOR_ELT(result, 0, cond);
        SET_VECTOR_ELT(result, 1, call);
        SET_VECTOR_ELT(result, 2, ENTRY_HANDLER(entry));
        std::panic::panic_any(crate::sexp::context::RSignal::ExitingHandler {
            target_env: rho,
            result,
        });
    }
}

pub(super) unsafe fn vsignalError(call: SEXP, format: *const c_char) {
    unsafe {
        let localbuf = if format.is_null() {
            String::new()
        } else {
            CStr::from_ptr(format).to_str().unwrap_or("").to_string()
        };

        let mut list = findSimpleErrorHandler();
        while !list.is_null() && list != globals::R_NilValue() {
            let entry = CAR(list);
            set_handler_stack(CDR(list));
            if IS_CALLING_ENTRY(entry) != 0 {
                if ENTRY_HANDLER(entry) == globals::R_RestartToken() {
                    break;
                }
                let hooksym = Rf_install(b".handleSimpleError\0".as_ptr() as *const c_char);
                let msg_cstr = std::ffi::CString::new(localbuf.as_str()).unwrap_or_default();
                let msg_sexp = Rf_mkString(msg_cstr.as_ptr());
                let _msg_guard = protect(msg_sexp);
                let handler = ENTRY_HANDLER(entry);
                let inner = Rf_lang2(handler, msg_sexp);
                let _inner_guard = protect(inner);
                let hcall = Rf_lang3(hooksym, inner, call);
                let _hcall_guard = protect(hcall);
                let _ = crate::eval::eval::Rf_eval(hcall, globals::R_BaseEnv());
            } else {
                gotoExitingHandler(globals::R_NilValue(), call, entry);
            }
            list = findSimpleErrorHandler();
        }
    }
}

/// Report an error with a call.
///
/// This is the equivalent of R's `errorcall()`.
/// In C this is variadic: `void errorcall(SEXP call, const char *format, ...)`.
/// In Rust, the format string should be a pre-formatted message (no % placeholders).
/// For formatted errors, use `Rf_errorcall1()` or pre-format before calling.
///
/// It does not return — it panics with an RError payload.
pub fn errorcall(call: SEXP, format: *const c_char) {
    unsafe {
        vsignalError(call, format);
        verrorcall_dflt(call, format, ptr::null_mut());
    }
}

/// Report an error with a call, from a Rust `&str` message.
///
/// This is the Rust-native equivalent of upstream `errorcall(call, "%s", msg)`
/// used by the interpreter and builtin handlers: it renders
/// "Error in <call> : <message>" into the error buffer (attributing the call
/// exactly like stock R) and panics with a bare-message `RError` payload.
/// Pass a null call for upstream `call. = FALSE` semantics ("Error: <message>").
pub fn errorcall_str(call: SEXP, message: &str) -> ! {
    let c_msg = std::ffi::CString::new(message).unwrap_or_default();
    errorcall(call, c_msg.as_ptr());
    unreachable!("errorcall never returns: verrorcall_dflt panics with RError");
}

/// Run a builtin/special handler call, attributing unattributed errors to
/// the R call being applied.
///
/// Upstream builtin handlers receive `call` and raise `errorcall(call, ...)`;
/// most ported handlers predate that convention and panic with a bare
/// `RError`. This wrapper mirrors the upstream convention at the dispatch
/// boundary: if the handler panics with an error that has not already been
/// rendered (and thus attributed) by a raise site, the error is re-raised
/// through `errorcall_str` with the applied call, so top-level rendering
/// shows "Error in <call> : <message>" exactly like stock R.
pub(crate) fn attribute_handler_errors<F>(call: SEXP, f: F) -> SEXP
where
    F: FnOnce() -> SEXP,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(payload) => {
            if let Some(err) = payload.downcast_ref::<RError>() {
                let message = err.message.clone();
                if !error_was_last_rendered(&message) {
                    // Diverges: renders "Error in <call> : <message>" and
                    // panics with the bare-message payload.
                    errorcall_str(call, &message);
                }
            }
            // Already attributed at the raise site (or not an RError):
            // continue unwinding with the original payload untouched.
            std::panic::resume_unwind(payload)
        }
    }
}

/// Report a formatted error with one string argument.
/// Equivalent to C's `errorcall(call, "%s", msg)`.
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
            std::ffi::CString::new(formatted)
                .unwrap_or_default()
                .as_ptr(),
            ptr::null_mut(),
        );
    }
}

/// Report a formatted error with call, using printf-style formatting.
/// This is a Rust-native helper that supports simple format strings.
pub fn Rf_errorcall_fmt(call: SEXP, format: *const c_char, args: &[&CStr]) {
    unsafe {
        if format.is_null() {
            verrorcall_dflt(call, b"\0".as_ptr() as *const c_char, ptr::null_mut());
            return;
        }
        let fmt = CStr::from_ptr(format).to_str().unwrap_or("");
        // Simple format expansion: replace %s with args in order
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
        let c_result = std::ffi::CString::new(result).unwrap_or_default();
        verrorcall_dflt(call, c_result.as_ptr(), ptr::null_mut());
    }
}

/// Report an error with a call and pre-formatted message buffer.
/// Matches C's `errorcall_cpy()` — copies all data before doing anything else.
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
///
/// This is the equivalent of R's `Rf_error()`.
/// The format string should be a pre-formatted message.
/// It does not return — it panics with an RError payload.
pub unsafe fn Rf_error(format: *const c_char) {
    unsafe {
        let call = getCurrentCall();
        // Rf_error in C is variadic: void error(const char *format, ...)
        // In Rust, callers should pass pre-formatted strings.
        errorcall(call, format);
    }
}

/// Report a formatted error (without call), with one string argument.
/// Equivalent to C's `error("%s", msg)`.
pub unsafe fn Rf_error1(format: *const c_char, arg: *const c_char) {
    unsafe {
        let call = getCurrentCall();
        Rf_errorcall1(call, format, arg);
    }
}

/// Unimplemented error — for functions that haven't been ported yet.
pub fn Rf_error_unimplemented(name: &str) {
    let msg = format!("function '{}' is not yet implemented", name);
    R_SetErrmessage(&msg);
    std::panic::panic_any(RError { message: msg });
}

/// UNIMPLEMENTED — called from C when a feature is not yet ported.
/// Matches C: `void UNIMPLEMENTED(const char *s) { error("unimplemented feature in %s", s); }`
pub unsafe fn UNIMPLEMENTED(s: *const c_char) {
    unsafe {
        let name = if s.is_null() {
            "unknown"
        } else {
            CStr::from_ptr(s).to_str().unwrap_or("unknown")
        };
        let msg = format!("unimplemented feature in {}", name);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
        let call = getCurrentCall();
        errorcall(call, c_msg.as_ptr());
    }
}

/// WrongArgCount — incorrect number of arguments error.
/// Matches C: `void WrongArgCount(const char *s) { error("incorrect number of arguments to \"%s\"", s); }`
pub unsafe fn WrongArgCount(s: *const c_char) {
    unsafe {
        let name = if s.is_null() {
            "unknown"
        } else {
            CStr::from_ptr(s).to_str().unwrap_or("unknown")
        };
        let msg = format!("incorrect number of arguments to \"{}\"", name);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
        let call = getCurrentCall();
        errorcall(call, c_msg.as_ptr());
    }
}

// ---------------------------------------------------------------------------
// Warning functions
// ---------------------------------------------------------------------------

/// Internal vwarningcall_dflt — the real warning handler.
///
/// Ported from R's `vwarningcall_dflt()` in errors.c.
/// Handles three modes based on `warn` option:
/// - w < 0: ignore
/// - w == 0: collect warnings for later display
/// - w == 1: print immediately
/// - w >= 2: convert to error
pub(super) unsafe fn vwarningcall_dflt(call: SEXP, format: *const c_char, ap: *mut c_void) {
    unsafe {
        // Guard against recursive warnings
        if in_warning() != 0 {
            return;
        }

        // Check for warning.expression option
        let s = GetOption1(Rf_install(b"warning.expression\0".as_ptr() as *const c_char));
        if !s.is_null() && isNull(s) == 0 {
            if isLanguage(s) == 0 && isExpression(s) == 0 {
                // Invalid option — fall through
            } else {
                // Would eval the expression — for now, format and print
                let msg = format_varargs(format, ap);
                eprintln!("Warning: {}", msg);
                return;
            }
        }

        // Get warn level
        let warn_sym = Rf_install(b"warn\0".as_ptr() as *const c_char);
        let w = asLogical(GetOption1(warn_sym));
        if w == crate::sexp::ffi::NA_INTEGER {
            // Set to sensible default
            if immediate_warning() {
                // w = 1 — print immediately
            } else {
                // w = 0 — default, handled below
            }
        }
        if w < 0 || in_warning() != 0 || in_error() != 0 {
            return;
        }

        // suppressWarnings(): upstream muffles through a calling-handler
        // restart so the warning never reaches collection or printing; the
        // port tracks the same with a depth counter around the expression.
        if suppress_warnings_depth() > 0 {
            return;
        }

        set_in_warning(1);

        // Format the variadic message into a string
        let (mut fmt_str, truncated) = format_varargs_to_buf(format, ap);
        if truncated {
            // Append truncation marker if room
            let trunc_msg = " [... truncated]";
            if fmt_str.len() + trunc_msg.len() < BUFSIZE {
                fmt_str.push_str(trunc_msg);
            }
        }

        if w >= 2 {
            // Convert warning to error
            set_in_warning(0);
            let full_msg = format!("(converted from warning) {}", fmt_str);
            let c_msg = std::ffi::CString::new(full_msg).unwrap_or_default();
            errorcall(call, c_msg.as_ptr());
        } else if w == 1 || immediate_warning() {
            // Print warnings immediately
            let dcall = if !call.is_null() && isNull(call) == 0 {
                // errors.c:496 deparses with deparse1s()
                warning_dcall(call)
            } else {
                String::new()
            };

            if dcall.is_empty() {
                eprint!("Warning:");
            } else {
                eprint!("Warning in {} :", dcall);
                // Check if first line fits on same line
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

            if r_show_warn_calls() && !call.is_null() && isNull(call) == 0 {
                // Respect .signalSimpleWarning hook if present by filtering the traceback accordingly
                let sigsym = Rf_install(b".signalSimpleWarning\0".as_ptr() as *const c_char);
                let tr = if SYMVALUE(sigsym) != globals::R_UnboundValue() {
                    R_ConciseTraceback(call, 1)
                } else {
                    R_ConciseTraceback(call, 0)
                };
                if !tr.is_empty() {
                    eprintln!("Calls: {}", tr);
                }
            }
        } else {
            // w == 0: collect warnings
            if collect_warnings() == 0 {
                setup_warnings();
            }
            let cw = collect_warnings();
            let nw = nwarnings();
            if cw < nw {
                // Store the warning
                let warnings_ptr = warnings_ptr();
                if !warnings_ptr.is_null() && TYPEOF(warnings_ptr) == SEXPTYPE::VECSXP {
                    SET_VECTOR_ELT(warnings_ptr, cw as R_xlen_t, call);
                    let names = CAR(ATTRIB(warnings_ptr));
                    if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP {
                        // Append traceback if requested
                        #[allow(clippy::implicit_clone)]
                        let mut msg_to_store = fmt_str.to_string();
                        if r_show_warn_calls() && !call.is_null() && isNull(call) == 0 {
                            let tr = R_ConciseTraceback(call, 0);
                            if !tr.is_empty() && msg_to_store.len() + tr.len() + 8 < BUFSIZE {
                                msg_to_store.push_str("\nCalls: ");
                                msg_to_store.push_str(&tr);
                            }
                        }
                        let c_msg = std::ffi::CString::new(msg_to_store).unwrap_or_default();
                        let ch = Rf_mkChar(c_msg.as_ptr());
                        SET_STRING_ELT(names, cw as R_xlen_t, ch);
                    }
                    increment_collect_warnings();
                }
            }
        }

        set_in_warning(0);
    }
}

/// Setup the warnings collection vector.
pub(super) unsafe fn setup_warnings() {
    unsafe {
        let nw = nwarnings();
        let w = Rf_allocVector(SEXPTYPE::VECSXP, nw);
        let names = Rf_allocVector(SEXPTYPE::STRSXP, nw);
        setAttrib_wrap(w, R_NamesSymbol(), names);
        set_warnings_ptr(w);
        set_collect_warnings(0);
    }
}

/// Issue a warning with call.
///
/// This is the equivalent of R's `warningcall()`.
/// Unlike errors, warnings do not terminate execution.
pub unsafe fn warningcall(call: SEXP, format: *const c_char) {
    unsafe {
        // A base-constructor builtin that owns the current warning-call
        // override (upstream: the closure context above its .Internal)
        // claims attribution for the whole handler body.
        let override_call = super::warning_call_override();
        let call = if override_call.is_null() {
            call
        } else {
            override_call
        };
        vsignalWarning(call, format);
    }
}

pub(super) unsafe fn vsignalWarning(call: SEXP, format: *const c_char) {
    unsafe {
        let hooksym = Rf_install(b".signalSimpleWarning\0".as_ptr() as *const c_char);
        // A freshly interned port symbol carries a NULL value slot (C uses
        // R_UnboundValue), so treat NULL as unbound too: otherwise every
        // warning would take the hook path and be silently dropped.
        let hook = SYMVALUE(hooksym);
        if !hook.is_null() && hook != globals::R_UnboundValue() {
            let msg = if format.is_null() {
                Rf_mkString(b"\0".as_ptr() as *const c_char)
            } else {
                Rf_mkString(format)
            };
            let _msg_guard = protect(msg);
            let hcall = Rf_lang3(hooksym, msg, call);
            let _hcall_guard = protect(hcall);
            let _ = crate::eval::eval::Rf_eval(hcall, globals::R_BaseEnv());
        } else {
            vwarningcall_dflt(call, format, ptr::null_mut());
        }
    }
}

/// Issue an immediate warning (bypass collection).
pub unsafe fn warningcall_immediate(call: SEXP, format: *const c_char) {
    unsafe {
        let prev = immediate_warning();
        set_immediate_warning(true);
        vwarningcall_dflt(call, format, ptr::null_mut());
        set_immediate_warning(prev);
    }
}

/// Issue a warning (without call).
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
/// Equivalent to C's `warningcall(call, "%s", msg)`.
pub unsafe fn Rf_warningcall1(call: SEXP, msg: *const c_char) {
    unsafe {
        let msg_str = if msg.is_null() {
            ""
        } else {
            CStr::from_ptr(msg).to_str().unwrap_or("")
        };
        let c_msg = std::ffi::CString::new(msg_str).unwrap_or_default();
        warningcall(call, c_msg.as_ptr());
    }
}

/// Issue a formatted warning without call (Rust helper).
/// Equivalent to C's `warning("%s", msg)`.
pub unsafe fn Rf_warning1(msg: *const c_char) {
    unsafe {
        let call = getCurrentCall();
        Rf_warningcall1(call, msg);
    }
}

thread_local! {
    static MATHLIB_WARNING_CALL: std::cell::Cell<SEXP> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// Restore the previous mathlib warning call on drop.
pub struct MathlibWarningCallGuard {
    prev: SEXP,
}

impl Drop for MathlibWarningCallGuard {
    fn drop(&mut self) {
        MATHLIB_WARNING_CALL.with(|slot| slot.set(self.prev));
    }
}

/// Scope mathlib (nmath) warning attribution to `call`.
///
/// Upstream's `warning()` resolves the call by walking out of the enclosing
/// CTXT_BUILTIN context; the port does not push builtin contexts for the
/// dpq builtins, so `dpq_evaluate` pushes the builtin's call here for the
/// duration of the nmath invocation.
pub fn mathlib_warning_call_guard(call: SEXP) -> MathlibWarningCallGuard {
    let prev = MATHLIB_WARNING_CALL.with(|slot| slot.replace(call));
    MathlibWarningCallGuard { prev }
}

/// The call mathlib warnings should attribute to, if one is in scope.
pub fn mathlib_warning_call() -> SEXP {
    MATHLIB_WARNING_CALL.with(|slot| slot.get())
}

// ---------------------------------------------------------------------------
// Message functions (R's message())
// ---------------------------------------------------------------------------

/// Issue a message (R's message()).
/// Messages are printed to stdout (via Rprintf in C, println! here).
/// Unlike errors/warnings, messages do not terminate or indicate problems.
///
/// Ported from R's `Rf_message()` concept.
pub unsafe fn Rf_message(format: *const c_char) {
    unsafe {
        if format.is_null() {
            println!();
            return;
        }
        let msg = CStr::from_ptr(format).to_str().unwrap_or("");
        // Strip trailing newline if present (C version does this)
        let msg = msg.trim_end_matches('\n');
        println!("{}", msg);
    }
}

/// Issue a message with call.
/// Ported from R's message handling in errors.c.
pub unsafe fn messagecall(call: SEXP, format: *const c_char) {
    unsafe {
        // In C, message() doesn't use the call for display,
        // but it's passed for consistency
        let _ = call; // suppress unused warning
        Rf_message(format);
    }
}

/// Issue a message with append flag.
/// When append=TRUE, the message is appended without a newline prefix.
/// When append=FALSE (default), the message starts on a new line.
///
/// This matches R's `message(..., appendLF = TRUE)` behavior.
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

/// do_message — R's message() builtin.
/// Ported from errors.c.
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
            SETCAR(args, coerceVector(CAR(args), SEXPTYPE::STRSXP.as_c_int()));
            if isValidString(CAR(args)) != 0 {
                let msg = translateChar(STRING_ELT(CAR(args), 0));
                Rf_message_append(msg, append);
            }
        }

        globals::R_NilValue()
    }
}
