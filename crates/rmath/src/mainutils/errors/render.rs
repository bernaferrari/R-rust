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
pub(super) fn getCurrentCall() -> SEXP {
    current_call(false)
}

/// Public stop/warning stand in for GNU closures. Their caller excludes the
/// bytecode expression marker, but includes native tryCatch's synthetic call.
/// Implicit errors retain that marker through getCurrentCall().
pub(crate) fn condition_caller_call() -> SEXP {
    current_call(true)
}

fn current_call(skip_expression_contexts: bool) -> SEXP {
    unsafe {
        // A context counts as carrying a call only when it holds a real
        fn usable_call(call: SEXP) -> SEXP {
            {
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
        let mut c = &*ctx;
        // GNU getCurrentCall: if the top frame is CTXT_BUILTIN, walk past it
        // once. Ops (`>`, `+`) are builtins, so warning() from DispatchGroup
        // stores R_NilValue and PrintWarnings omits the `In x > y :` prefix.
        if (c.callflag & crate::sexp::context::ctxt_flags::CTXT_BUILTIN) != 0 {
            if c.nextcontext.is_null() {
                return globals::R_NilValue();
            }
            c = &*c.nextcontext;
        }
        // Skip promise-evaluation contexts (bare CTXT_RETURN, null call)
        // so errors raised during a forced argument keep the attribution
        // of the enclosing R call.
        while (c.callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION) == 0
            && !c.nextcontext.is_null()
            && (usable_call(c.call.as_raw()) == globals::R_NilValue()
                || (skip_expression_contexts
                    && (c.callflag
                        & (crate::sexp::context::ctxt_flags::CTXT_CCODE
                            | crate::sexp::context::ctxt_flags::CTXT_BUILTIN))
                        != 0))
        {
            c = &*c.nextcontext;
        }
        usable_call(c.call.as_raw())
    }
}

/// Public accessor mirroring upstream `getCurrentCall()` (errors.c): the call
/// of the innermost context on the context stack (skipping a CTXT_BUILTIN
/// top frame). Used by interpreter raise sites to attribute errors to the
/// enclosing R call, like upstream `R_MissingArgError`/`error()`.
pub fn R_getCurrentCall() -> SEXP {
    getCurrentCall()
}

/// findCall: find the function context's call for error reporting.
pub(crate) fn findCall() -> SEXP {
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
                return if ctx_ref.call.as_raw().is_null() {
                    globals::R_NilValue()
                } else {
                    ctx_ref.call.as_raw()
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
    owner: crate::sexp::owner::OwnerPin,
    old_in_error: i32,
    old_expressions: c_int,
}

impl Drop for RestoreInError {
    fn drop(&mut self) {
        // Restore the original physical runtime even after callback revocation.
        unsafe {
            (*self.owner.as_ptr()).error_state.in_error = self.old_in_error;
            (*self.owner.as_ptr()).error_state.expressions = self.old_expressions;
        }
    }
}

impl RestoreInError {
    fn require_active(&self) {
        unsafe { crate::sexp::owner::OwnerToken::from_raw(self.owner.as_ptr()) }
            .require_active()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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
pub(super) fn show_error_locations_enabled() -> bool {
    let opt_owner = crate::sexp::context::own_control_value(
        crate::mainutils::options::existing_option("show.error.locations"),
    );
    let opt = opt_owner.as_raw();
    if opt.is_null() || isNull(opt) != 0 || XLENGTH(opt) != 1 {
        return false;
    }
    if crate::mainutils::coerce::asLogical(opt) == 1 {
        return true;
    }
    if TYPEOF(opt) == SEXPTYPE::STRSXP {
        let tag = STRING_ELT(opt, 0);
        // All prefixes of "top" are ASCII in the supported encodings.
        return b"top".starts_with(&crate::mainutils::printutils::charsxp_bytes(tag));
    }
    false
}

/// Default error renderer for a C-style message (`ap` is an optional `va_list`).
///
/// # Safety
///
/// `format` must be null or point to a NUL-terminated string valid for reads
/// for the duration of the call, and `ap` must be null or a live `va_list`
/// whose arguments match `format`.
pub(super) unsafe fn verrorcall_dflt(call: SEXP, format: *const c_char, ap: *mut c_void) {
    unsafe {
        use super::byte_buffer::Snapshot;
        // Copy literal native bytes before touching error state: format can
        // itself be the pointer returned by R_curErrorBuf.
        let raw_input = if ap.is_null() {
            Snapshot::raw(|i| {
                if format.is_null() {
                    None
                } else {
                    Some(*format.cast::<u8>().add(i))
                }
            })
        } else {
            let formatted = format_varargs(format, ap);
            Snapshot::raw(|i| formatted.as_bytes().get(i).copied())
        };
        verrorcall_bytes(call, raw_input);
    }
}

pub(super) fn verrorcall_bytes(call: SEXP, raw_input: super::byte_buffer::Snapshot) {
    unsafe {
        use super::byte_buffer::Snapshot;
        let owner = super::state::error_scope_pin();
        let old_in_err = in_error();
        if old_in_err > 0 {
            // fail-safe handler for recursive errors
            if old_in_err >= 3 {
                eprint!("Error during wrapup: ");
                use std::io::Write;
                let mut stderr = std::io::stderr().lock();
                let _ = stderr.write_all(raw_input.as_bytes());
                let _ = stderr.write_all(b"\n");
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
            owner,
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
        set_error_call_less(!has_call);
        record_error_call(call, false);
        let head_len = if has_call {
            b"Error in ".len()
        } else {
            b"Error: ".len()
        };
        let tmp_cap = (warn_len + 1).saturating_sub(head_len).saturating_sub(1);
        let utf8 = crate::mainutils::platform::locale_is_utf8();
        let tmp = raw_input.clipped(tmp_cap, utf8);

        // errors.c:804-819 — resolve the `(from <loc>)` marker before
        // rendering: gated by show.error.locations. A top-level
        // expression parsed with keep.source=TRUE carries an srcref:
        // GetSrcLoc then renders `(from <file>#<line>)`. Expressions
        // without srcrefs fall back to the script-loop position
        // (`(from #n)` — upstream's unnamed-srcfile shape); 0 = none.
        let location_mark = if show_error_locations_enabled() {
            match crate::mainutils::srcref::current_srcref_location() {
                Some((file, line)) if !file.is_empty() && line > 0 => {
                    format!(" (from {file}#{line})")
                }
                _ => {
                    let n = toplevel_expr_no();
                    if n > 0 {
                        format!(" (from #{n})")
                    } else {
                        String::new()
                    }
                }
            }
        } else {
            String::new()
        };
        _guard.require_active();
        let dcall = if has_call {
            // Error with call — "Error in <call> : <message>"
            // Upstream errors.c verrorcall_dflt deparses the call with
            // deparse1s(); reuse the faithful port instead of a placeholder.
            let dcall_raw = crate::mainutils::deparse::deparse1s(call);
            _guard.require_active();
            let dcall = if dcall_raw.is_null() || dcall_raw == globals::R_NilValue() {
                Snapshot::raw(|i| b"<call>".get(i).copied())
            } else {
                let dcall_owner = crate::sexp::context::own_control_value(dcall_raw);
                if TYPEOF(dcall_raw) != SEXPTYPE::STRSXP || XLENGTH(dcall_raw) == 0 {
                    Snapshot::raw(|i| b"<call>".get(i).copied())
                } else {
                    let character = crate::sexp::context::own_control_value(STRING_ELT(
                        dcall_owner.as_raw(),
                        0,
                    ));
                    let pointer = translateChar(character.as_raw());
                    _guard.require_active();
                    if pointer.is_null() {
                        Snapshot::raw(|i| b"<call>".get(i).copied())
                    } else {
                        Snapshot::raw(|i| Some(*pointer.cast::<u8>().add(i)))
                    }
                }
            };

            Some(dcall)
        } else {
            None
        };
        let (mut err_msg, body_range) =
            Snapshot::render_error(&tmp, dcall.as_ref(), location_mark.as_bytes(), utf8);
        if r_show_error_calls() && has_call {
            let trace = R_ConciseTraceback(call, 0);
            _guard.require_active();
            err_msg.append_trace(trace.as_bytes());
        }

        // Payload contract: the RError message is the BARE message (what
        // condition objects / tryCatch handlers see, matching upstream where
        // "Error in <call> :" attribution is added only by top-level error
        // printing). Strip any prefix that a raise site baked into its text so
        // the payload stays clean; the rendered errbuf above keeps the full
        // attribution for stderr.
        let payload_message = Snapshot::host_text(tmp.bare_bytes());
        let cache_owner = crate::sexp::owner::OwnerToken::from_raw(_guard.owner.as_ptr())
            .weak_owner()
            .expect("managed renderer owner");
        let err_msg = err_msg.normalized(utf8);
        let cached_message = super::RenderedMessage::copy(&cache_owner, &payload_message)
            .ok()
            .or_else(|| {
                // A full arena must still report the original error. Reuse
                // the bare body when it survived the fixed-buffer render.
                let range = body_range?;
                (err_msg.as_bytes().get(range.clone())? == tmp.bare_bytes())
                    .then_some(super::RenderedMessage::InBuffer(range))
            });

        _guard.require_active();
        super::state::set_rendered_error_buffer(err_msg, cached_message);

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
            use std::io::Write;
            let snapshot = super::state::error_buffer_snapshot();
            let _ = std::io::stderr().lock().write_all(snapshot.as_bytes());
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

pub(super) fn findSimpleErrorHandler() -> SEXP {
    let mut list = handler_stack();
    while !list.is_null() && list != globals::R_NilValue() {
        let entry = CAR(list);
        let class_ptr = char_cstring(ENTRY_CLASS(entry));
        if !class_ptr.is_none() {
            let class_str = class_ptr.as_deref().unwrap_or_default().to_bytes();
            if class_str == b"simpleError" || class_str == b"error" || class_str == b"condition" {
                return list;
            }
        }
        list = CDR(list);
    }
    globals::R_NilValue()
}

pub(super) fn gotoExitingHandler(cond: SEXP, call: SEXP, entry: SEXP) {
    let rho = ENTRY_TARGET_ENVIR(entry);
    let result = ENTRY_RETURN_RESULT(entry);
    SET_VECTOR_ELT(result, 0, cond);
    SET_VECTOR_ELT(result, 1, call);
    SET_VECTOR_ELT(result, 2, ENTRY_HANDLER(entry));
    let transfer = crate::sexp::context::exiting_handler_transfer(rho, result);
    std::panic::panic_any(crate::sexp::context::RSignal::ExitingHandler(transfer));
}

/// Walk the handler stack for a simple error carrying `format`.
///
/// # Safety
///
/// `format` must be null or point to a NUL-terminated string valid for reads
/// for the duration of the call.
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
                let handler = crate::sexp::context::own_control_value(ENTRY_HANDLER(entry));
                let condition =
                    crate::sexp::context::own_control_value(super::conditions::make_condition(
                        call,
                        "simpleError",
                        "",
                        0,
                        &localbuf,
                        "error",
                    ));
                if super::native::dispatch_calling_handler(handler.as_raw(), condition.as_raw()) {
                    list = findSimpleErrorHandler();
                    continue;
                }
                let hooksym = crate::sexp::symbol::install_str(".handleSimpleError");
                let msg_cstr = std::ffi::CString::new(localbuf.as_str()).unwrap_or_default();
                let msg_sexp = Rf_mkString(msg_cstr.as_ptr());
                let _msg_guard = protect(msg_sexp);
                // GNU .handleSimpleError takes (handler, message, call).
                // The originating call is condition data, not an expression
                // to evaluate while invoking the calling handler.
                let quoted_call = Rf_lang2(crate::sexp::symbol::install_str("quote"), call);
                let _call_guard = protect(quoted_call);
                let hcall = Rf_lang4(hooksym, handler.as_raw(), msg_sexp, quoted_call);
                let _hcall_guard = protect(hcall);
                let _ = crate::eval::eval::Rf_eval(hcall, globals::R_BaseEnv());
            } else {
                let entry = crate::sexp::context::own_control_value(entry);
                let condition =
                    crate::sexp::context::own_control_value(super::conditions::make_condition(
                        call,
                        "simpleError",
                        "",
                        0,
                        &localbuf,
                        "error",
                    ));
                // GNU records the unformatted message when an exiting error
                // handler takes over. Calling handlers still see the prior
                // buffer; signaling a supplied condition does not replace it.
                R_SetErrmessage(&localbuf);
                gotoExitingHandler(condition.as_raw(), call, entry.as_raw());
            }
            list = findSimpleErrorHandler();
        }
    }
}

/// Borrow the NUL-terminated prefix of a byte buffer as a C string
/// (empty when the buffer holds no NUL).
pub fn cstr_until_nul(bytes: &[u8]) -> &CStr {
    CStr::from_bytes_until_nul(bytes).unwrap_or_default()
}

/// Borrow the NUL-terminated prefix of a `c_char` buffer as a C string
/// (empty when the buffer holds no NUL).
pub fn c_chars_until_nul(chars: &[c_char]) -> &CStr {
    // SAFETY: `c_char` and `u8` have identical size and alignment, so the
    // live slice may be viewed as bytes for the same lifetime.
    let bytes = unsafe { std::slice::from_raw_parts(chars.as_ptr().cast::<u8>(), chars.len()) };
    cstr_until_nul(bytes)
}

/// Report an error with a call.
///
/// This is the equivalent of R's `errorcall()`.
/// In C this is variadic: `void errorcall(SEXP call, const char *format, ...)`.
/// In Rust, the format string should be a pre-formatted message (no % placeholders).
/// For formatted errors, use `Rf_errorcall1()` or pre-format before calling.
///
/// It does not return — it panics with an RError payload.
/// Rust callers should use [`errorcall_cstr`] / [`errorcall_str`].
///
/// # Safety
///
/// `format` must be null or point to a NUL-terminated string valid for reads
/// for the duration of the call.
pub unsafe fn errorcall(call: SEXP, format: *const c_char) {
    // SAFETY: forwarded under this function's `# Safety` contract.
    unsafe { raise_error(call, format, true) };
}

/// Safe core of [`errorcall`]: report an error with a call from a C string.
pub fn errorcall_cstr(call: SEXP, message: &CStr) {
    // SAFETY: `message` is a live NUL-terminated string for the whole call.
    unsafe { raise_error(call, message.as_ptr(), true) };
}

/// Raise an error whose call was inferred from the current context, as in
/// GNU error()/stop(). tryCatch may substitute its frame for this call.
///
/// # Safety
///
/// `format` must be null or point to a NUL-terminated string valid for reads
/// for the duration of the call.
pub(crate) unsafe fn implicit_errorcall(call: SEXP, format: *const c_char) {
    // SAFETY: forwarded under this function's `# Safety` contract.
    unsafe { raise_error(call, format, false) };
}

/// Safe core of [`implicit_errorcall`].
pub(crate) fn implicit_errorcall_cstr(call: SEXP, message: &CStr) {
    // SAFETY: `message` is a live NUL-terminated string for the whole call.
    unsafe { raise_error(call, message.as_ptr(), false) };
}

/// # Safety
///
/// `format` must be null or point to a NUL-terminated string that stays
/// valid for reads until this call unwinds.
unsafe fn raise_error(call: SEXP, format: *const c_char, explicit: bool) {
    let _call = protect(if call.is_null() {
        globals::R_NilValue()
    } else {
        call
    });
    // SAFETY: `format` is valid under this function's `# Safety` contract.
    unsafe { vsignalError(call, format) };
    // A calling handler can raise a replacement error. Record only after
    // it returns, so the original applied call cannot override that error.
    record_error_call(call, explicit);
    // SAFETY: as above; a null `ap` means `format` is the final message.
    unsafe { verrorcall_dflt(call, format, ptr::null_mut()) };
}

pub(crate) fn implicit_errorcall_str(call: SEXP, message: &str) -> ! {
    let c_msg = std::ffi::CString::new(message).unwrap_or_default();
    implicit_errorcall_cstr(call, &c_msg);
    unreachable!("implicit_errorcall never returns");
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
    errorcall_cstr(call, &c_msg);
    unreachable!("errorcall never returns: verrorcall_dflt panics with RError");
}

/// Report an error like upstream `.Internal(.dfltStop(message, call))`:
/// render "Error in <call> : <message>" (or "Error: <message>" for a
/// null call) and panic with the bare-message `RError` payload WITHOUT
/// walking the handler stack. Upstream `stop(<condition>)` signals the
/// condition first (`.signalCondition`); the handler search already
/// happened at signal time, so the default error must not repeat it.
pub fn dflt_stop_str(call: SEXP, message: &str) -> ! {
    {
        let c_msg = std::ffi::CString::new(message).unwrap_or_default();
        // SAFETY: `c_msg` is a live NUL-terminated message and `ap` is null.
        unsafe { verrorcall_dflt(call, c_msg.as_ptr(), ptr::null_mut()) };
        unreachable!("verrorcall_dflt panics with RError");
    }
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
    let original = crate::sexp::instance::current_instance_ptr();
    // Retain physical bytes, but never use this lease to reactivate a revoked
    // owner or attribute its failure to a replacement runtime.
    let _pin = original
        .and_then(|instance| unsafe { crate::sexp::context::pin_context_owner_in(instance) });
    let availability =
        original.map(|instance| unsafe { crate::sexp::instance::instance_liveness(instance) });
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(payload) => {
            if original.is_none()
                || original != crate::sexp::instance::current_instance_ptr()
                || !availability.as_ref().is_some_and(|owner| owner.is_live())
            {
                std::panic::resume_unwind(payload);
            }
            if let Some(err) = payload.downcast_ref::<RError>() {
                let message = err.message.as_str();
                if !error_was_last_rendered(message) {
                    // Diverges: renders "Error in <call> : <message>" and
                    // panics with the bare-message payload.
                    errorcall_str(call, message);
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
pub fn Rf_errorcall1(call: SEXP, format: &CStr, arg: &CStr) {
    let msg = arg.to_str().unwrap_or("");
    let formatted = format!("{}{}", format.to_str().unwrap_or(""), msg);
    errorcall_str(call, &formatted);
}

/// Report a formatted error with call, using printf-style formatting.
/// This is a Rust-native helper that supports simple format strings.
pub fn Rf_errorcall_fmt(call: SEXP, format: &CStr, args: &[&CStr]) {
    {
        let fmt = format.to_str().unwrap_or("");
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
        errorcall_cstr(call, &c_result);
    }
}

/// Report an error with a call and pre-formatted message buffer.
/// Matches C's `errorcall_cpy()` — copies all data before doing anything else.
pub fn errorcall_cpy(call: SEXP, format: &CStr) {
    let mut buf = vec![0u8; BUFSIZE + 1];
    let bytes = format.to_bytes();
    let len = bytes.len().min(BUFSIZE - 1);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    let message = CStr::from_bytes_until_nul(&buf).unwrap_or_default();
    errorcall_cstr(call, message);
}

/// Report an error (without call).
///
/// This is the equivalent of R's `Rf_error()`.
/// The format string should be a pre-formatted message.
/// It does not return — it panics with an RError payload.
/// Rust callers should use [`Rf_error_cstr`].
///
/// # Safety
///
/// `format` must be null or point to a NUL-terminated string valid for reads
/// for the duration of the call.
pub unsafe fn Rf_error(format: *const c_char) {
    let call = getCurrentCall();
    // Rf_error in C is variadic: void error(const char *format, ...)
    // In Rust, callers should pass pre-formatted strings.
    // SAFETY: forwarded under this function's `# Safety` contract.
    unsafe { implicit_errorcall(call, format) };
}

/// Safe core of [`Rf_error`]: report an error (without call) from a C string.
pub fn Rf_error_cstr(message: &CStr) {
    let call = getCurrentCall();
    implicit_errorcall_cstr(call, message);
}

/// Report a formatted error (without call), with one string argument.
/// Equivalent to C's `error("%s", msg)`.
pub fn Rf_error1(format: &CStr, arg: &CStr) {
    let call = getCurrentCall();
    let fmt = format.to_str().unwrap_or("");
    let arg = arg.to_str().unwrap_or("");
    implicit_errorcall_str(call, &format!("{fmt}{arg}"));
}

/// Unimplemented error — for functions that haven't been ported yet.
pub fn Rf_error_unimplemented(name: &str) {
    let msg = format!("function '{}' is not yet implemented", name);
    R_SetErrmessage(&msg);
    std::panic::panic_any(RError { message: msg });
}

/// UNIMPLEMENTED — called from C when a feature is not yet ported.
/// Matches C: `void UNIMPLEMENTED(const char *s) { error("unimplemented feature in %s", s); }`
pub fn UNIMPLEMENTED(s: &CStr) {
    let name = s.to_str().unwrap_or("unknown");
    let msg = format!("unimplemented feature in {}", name);
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    let call = getCurrentCall();
    implicit_errorcall_cstr(call, &c_msg);
}

/// WrongArgCount — incorrect number of arguments error.
/// Matches C: `void WrongArgCount(const char *s) { error("incorrect number of arguments to \"%s\"", s); }`
pub fn WrongArgCount(s: &CStr) {
    let name = s.to_str().unwrap_or("unknown");
    let msg = format!("incorrect number of arguments to \"{}\"", name);
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    let call = getCurrentCall();
    implicit_errorcall_cstr(call, &c_msg);
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
/// Default warning renderer for a C-style message (`ap` is an optional `va_list`).
///
/// # Safety
///
/// `format` must be null or point to a NUL-terminated string valid for reads
/// for the duration of the call, and `ap` must be null or a live `va_list`
/// whose arguments match `format`.
pub(super) unsafe fn vwarningcall_dflt(call: SEXP, format: *const c_char, ap: *mut c_void) {
    let pin = super::state::error_scope_pin();
    let pointer = pin.as_ptr();
    let owner = unsafe { crate::sexp::owner::OwnerToken::from_raw(pointer) };
    let affiliation = owner.weak_owner().expect("managed warning owner");
    // The incoming call may be rooted solely by a mutable runtime override.
    // Retain the actual value before even the first option-symbol allocation.
    let call_owned = collection_checked({
        owner
            .sexp(if call.is_null() {
                globals::R_NilValue()
            } else {
                call
            })
            .and_then(crate::sexp::object::Sexp::into_owned)
    });
    let call = call_owned.as_raw();
    collection_checked(crate::sexp::owner::with_runtime(
        &affiliation,
        |access| unsafe {
            // Guard against recursive warnings
            if in_warning() != 0 {
                return;
            }

            // Check for warning.expression option
            let expression = collection_checked(access.with_native(|_| {
                access
                    .domain()
                    .wrap(GetOption1(crate::sexp::symbol::install_str(
                        "warning.expression",
                    )))
            }));
            if !expression.is_nil() {
                if !matches!(expression.typeof_(), SEXPTYPE::LANGSXP | SEXPTYPE::EXPRSXP) {
                    crate::sexp::context::r_error("invalid option \"warning.expression\"");
                }
                // Snapshot the actual nearest function environment before evaluation
                // can replace contexts or mutate the option holding the expression.
                let environment = (*pointer)
                    .context_stack
                    .iter()
                    .rev()
                    .find_map(|context| {
                        let context = &*context.get();
                        (context.callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION != 0)
                            .then(|| context.cloenv.owned())
                            .flatten()
                    })
                    .unwrap_or_else(|| {
                        collection_checked(access.domain().wrap((*pointer).global_env))
                    });
                collection_checked(access.require_active());
                let _result = collection_checked(access.with_native(|_| {
                    access.domain().wrap(crate::eval::eval::eval_keep_vis(
                        expression.as_raw(),
                        environment.as_raw(),
                    ))
                }));
                return;
            }

            // Get warn level
            let mut w = collection_checked(access.with_native(|_| {
                let option = access
                    .domain()
                    .wrap(GetOption1(crate::sexp::symbol::install_str("warn")))?;
                Ok(crate::mainutils::coerce::asInteger(option.as_raw()))
            }));
            if w == crate::sexp::ffi::NA_INTEGER {
                w = 0;
            }
            if w <= 0 && immediate_warning() {
                w = 1;
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

            // GNU permits a warning.expression to remove itself and signal a
            // nested warning. Guard only the default rendering/collection path.
            (*pointer).error_state.in_warning = 1;
            let _cleanup = WarningCollectionScope { owner: pin };

            // Format the variadic message into a string
            // SAFETY: `format`/`ap` are valid under this function's contract.
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
                (*pointer).error_state.in_warning = 0;
                let full_msg = format!("(converted from warning) {}", fmt_str);
                let c_msg = std::ffi::CString::new(full_msg).unwrap_or_default();
                errorcall_cstr(call, &c_msg);
            } else if w == 1 || immediate_warning() {
                // Print warnings immediately
                let dcall = if !call.is_null() && isNull(call) == 0 {
                    // errors.c:496 deparses with deparse1s()
                    let text = warning_dcall(call);
                    collection_checked(access.require_active());
                    text
                } else {
                    String::new()
                };

                let mut out = String::new();
                if dcall.is_empty() {
                    out.push_str("Warning:");
                } else {
                    out.push_str("Warning in ");
                    out.push_str(&dcall);
                    out.push_str(" :");
                    let msg_first_line = fmt_str
                        .find('\n')
                        .map(|i| &fmt_str[..i])
                        .unwrap_or(&fmt_str);
                    if 18 + dcall.len() + msg_first_line.len() > LONGWARN {
                        out.push('\n');
                        out.push(' ');
                    }
                }
                out.push(' ');
                out.push_str(&fmt_str);
                out.push('\n');
                if r_show_warn_calls() && !call.is_null() && isNull(call) == 0 {
                    let tr = collection_checked(access.with_native(|_| {
                        let sigsym = crate::sexp::symbol::install_str(".signalSimpleWarning");
                        access.require_active()?;
                        Ok(R_ConciseTraceback(
                            call,
                            if SYMVALUE(sigsym) != globals::R_UnboundValue() {
                                1
                            } else {
                                0
                            },
                        ))
                    }));
                    if !tr.is_empty() {
                        out.push_str("Calls: ");
                        out.push_str(&tr);
                        out.push('\n');
                    }
                }
                collection_checked(access.require_active());
                crate::sexp::output::capture_stderr(&out);
            } else {
                // Collection retains actual vector/name/call leases across
                // traceback and character allocation callbacks.
                collect_warning_owned(call, &fmt_str);
            }

            collection_checked(access.require_active());
        },
    ));
}

struct WarningCollectionScope {
    owner: crate::sexp::owner::OwnerPin,
}
impl Drop for WarningCollectionScope {
    fn drop(&mut self) {
        unsafe {
            (*self.owner.as_ptr()).error_state.in_warning = 0;
        }
    }
}

fn collection_checked<T>(result: crate::sexp::object::SexpResult<T>) -> T {
    result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

fn collect_warning_owned(call: SEXP, message: &str) {
    let pin = super::state::error_scope_pin();
    let pointer = pin.as_ptr();
    let owner = unsafe { crate::sexp::owner::OwnerToken::from_raw(pointer) };
    let affiliation = owner.weak_owner().expect("managed warning owner");
    let _cleanup = WarningCollectionScope { owner: pin };
    collection_checked(crate::sexp::owner::with_runtime(&affiliation, |access| {
        let domain = access.domain();
        let call = if call.is_null() {
            domain.nil()
        } else {
            collection_checked(domain.wrap(call))
        };
        if unsafe { (*pointer).error_state.collect_warnings } == 0 {
            {
                setup_warnings();
            }
        }
        collection_checked(access.require_active());
        let (count, capacity, warnings) = unsafe {
            (
                (*pointer).error_state.collect_warnings,
                (*pointer).error_state.nwarnings,
                (*pointer).error_state.warnings.owned(),
            )
        };
        if count < 0 {
            crate::sexp::context::r_error("invalid collected warning count");
        }
        if count >= capacity {
            return;
        }
        let Some(warnings) = warnings else {
            return;
        };
        if warnings.typeof_() != SEXPTYPE::VECSXP {
            return;
        }
        let attributes = collection_checked(warnings.try_attrib());
        let names = if attributes.is_nil() {
            None
        } else {
            let candidate = collection_checked(attributes.try_car());
            (candidate.typeof_() == SEXPTYPE::STRSXP).then_some(candidate)
        };
        let mut text = message.to_owned();
        if r_show_warn_calls() && !call.is_nil() {
            let trace = collection_checked(
                access.with_native(|_| Ok(R_ConciseTraceback(call.as_raw(), 0))),
            );
            if !trace.is_empty() && text.len() + trace.len() + 8 < BUFSIZE {
                text.push_str("\nCalls: ");
                text.push_str(&trace);
            }
        }
        let allocator = collection_checked(access.allocator(&domain));
        let character = collection_checked(allocator.character(&text));
        let mut vector =
            collection_checked(crate::sexp::object::SexpMut::try_from_checked(warnings));
        collection_checked(vector.try_set_vector_elt(count as i64, call));
        if let Some(names) = names {
            let mut names =
                collection_checked(crate::sexp::object::SexpMut::try_from_checked(names));
            collection_checked(names.try_set_string_elt(count as i64, character));
        }
        let warnings = vector.freeze();
        collection_checked(access.require_active());
        // A callback may clear or replace the live field. Publish the complete
        // original snapshot and its matching count together after callbacks.
        unsafe {
            (*pointer).error_state.warnings =
                crate::sexp::instance::RuntimeValue::from_owned(warnings);
            (*pointer).error_state.collect_warnings = count + 1;
        }
    }));
}

/// Setup the warnings collection vector with both actual allocations owned
/// before name-symbol installation can allocate or trigger notification.
pub(super) fn setup_warnings() {
    let pin = super::state::error_scope_pin();
    let pointer = pin.as_ptr();
    let owner = unsafe { crate::sexp::owner::OwnerToken::from_raw(pointer) };
    let affiliation = owner.weak_owner().expect("managed warning owner");
    collection_checked(crate::sexp::owner::with_runtime(&affiliation, |access| {
        let capacity = unsafe { (*pointer).error_state.nwarnings };
        if capacity < 0 {
            crate::sexp::context::r_error("invalid warning collection capacity");
        }
        let domain = access.domain();
        let allocator = collection_checked(access.allocator(&domain));
        let warnings = collection_checked(
            allocator.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, capacity as i64))),
        );
        let names = collection_checked(
            allocator.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, capacity as i64))),
        );
        collection_checked(access.with_native(|_| {
            let names_symbol = R_NamesSymbol();
            access.require_active()?;
            setAttrib_wrap(warnings.as_raw(), names_symbol, names.as_raw());

            Ok(())
        }));
        collection_checked(access.require_active());
        unsafe {
            (*pointer).error_state.warnings =
                crate::sexp::instance::RuntimeValue::from_owned(warnings);
            (*pointer).error_state.collect_warnings = 0;
        }
    }));
}

/// Select override precedence before a boundary can allocate or collect.
pub(super) fn effective_warning_call(call: SEXP) -> SEXP {
    // A base-constructor override owns attribution for the whole handler body.
    let override_call = super::warning_call_override();
    if !override_call.is_null() {
        override_call
    } else {
        call
    }
}

/// Issue a warning with call.
///
/// This is the equivalent of R's `warningcall()`.
/// Unlike errors, warnings do not terminate execution.
/// Rust callers should use [`warningcall_cstr`].
///
/// # Safety
///
/// `format` must be null or point to a NUL-terminated string valid for reads
/// for the duration of the call.
pub unsafe fn warningcall(call: SEXP, format: *const c_char) {
    // SAFETY: forwarded under this function's `# Safety` contract.
    unsafe { vsignalWarning(effective_warning_call(call), format) };
}

/// Safe core of [`warningcall`]: issue a warning with call from a C string.
pub fn warningcall_cstr(call: SEXP, message: &CStr) {
    // SAFETY: `message` is a live NUL-terminated string for the whole call.
    unsafe { vsignalWarning(effective_warning_call(call), message.as_ptr()) };
}

pub(crate) fn mark_calling_handlers_signaled() {
    super::state::with_error_state(|state| state.calling_handlers_signaled = true);
}

fn calling_handlers_already_signaled() -> bool {
    super::state::with_error_state(|state| state.calling_handlers_signaled)
}

fn clear_calling_handlers_signaled() {
    super::state::with_error_state(|state| state.calling_handlers_signaled = false);
}

/// Signal a simple warning carrying `format` to calling handlers, then
/// fall back to the default warning action.
///
/// # Safety
///
/// `format` must be null or point to a NUL-terminated string valid for reads
/// for the duration of the call.
pub(super) unsafe fn vsignalWarning(call: SEXP, format: *const c_char) {
    unsafe {
        // Calling handlers first (upstream signals the simpleWarning
        // through R_HandlerStack before any default emission): a handler
        // that invokes the dynamically scoped muffleWarning restart
        // suppresses print/collection entirely (e.g. muffling the
        // invalid-factor-level warning from `[<-.factor`).
        // `do_warning` (the R-level builtin) already signaled its calling
        // handlers with the full muffle contract BEFORE routing here for
        // collection — skip the re-signal so handlers fire exactly once.
        if calling_handlers_already_signaled() {
            clear_calling_handlers_signaled();
            vwarningcall_dflt(call, format, ptr::null_mut());
            return;
        }
        {
            let msg = if format.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(format)
                    .to_string_lossy()
                    .into_owned()
            };
            let cond = crate::mainutils::essentials::simple_warning_condition(&msg, call);
            let _cond_guard = protect(cond);
            let muffled = crate::mainutils::essentials::signal_calling_warning_condition(
                cond,
                crate::sexp::globals::R_BaseEnv(),
            );
            if muffled {
                return;
            }
            // tryCatch(warning=) without an error handler consumes the warning
            // before warn >= 2 turns it into an error. assertWarning registers
            // both, so it still sees the default action.
            if crate::mainutils::essentials::try_catch_wants_warning() {
                std::panic::panic_any(crate::sexp::context::RSignal::Warning { message: msg });
            }
        }
        let hooksym = crate::sexp::symbol::install_str(".signalSimpleWarning");
        // A freshly interned port symbol carries a NULL value slot (C uses
        // R_UnboundValue), so treat NULL as unbound too: otherwise every
        // warning would take the hook path and be silently dropped.
        let hook = SYMVALUE(hooksym);
        if !hook.is_null() && hook != globals::R_UnboundValue() {
            let msg = if format.is_null() {
                mk_string("")
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
pub fn warningcall_immediate(call: SEXP, message: &CStr) {
    let prev = immediate_warning();
    set_immediate_warning(true);
    // SAFETY: `message` is a live NUL-terminated string and `ap` is null.
    unsafe { vwarningcall_dflt(call, message.as_ptr(), ptr::null_mut()) };
    set_immediate_warning(prev);
}

/// Issue a warning (without call).
/// Rust callers should use [`Rf_warning_cstr`].
///
/// # Safety
///
/// `format` must be null or point to a NUL-terminated string valid for reads
/// for the duration of the call.
pub unsafe fn Rf_warning(format: *const c_char) {
    let call = getCurrentCall();
    // SAFETY: forwarded under this function's `# Safety` contract.
    unsafe { warningcall(call, format) };
}

/// Safe core of [`Rf_warning`]: issue a warning (without call) from a C string.
pub fn Rf_warning_cstr(message: &CStr) {
    let call = getCurrentCall();
    warningcall_cstr(call, message);
}

/// Issue an immediate warning (without call).
pub fn Rf_warning_immediate(message: &CStr) {
    let call = getCurrentCall();
    warningcall_immediate(call, message);
}

/// Issue a formatted warning with call (Rust helper).
/// Equivalent to C's `warningcall(call, "%s", msg)`.
pub fn Rf_warningcall1(call: SEXP, msg: &CStr) {
    let msg_str = msg.to_str().unwrap_or("");
    let c_msg = std::ffi::CString::new(msg_str).unwrap_or_default();
    warningcall_cstr(call, &c_msg);
}

/// Issue a formatted warning without call (Rust helper).
/// Equivalent to C's `warning("%s", msg)`.
pub fn Rf_warning1(msg: &CStr) {
    let call = getCurrentCall();
    Rf_warningcall1(call, msg);
}

/// Own the previous attribution and original runtime until scope cleanup.
pub struct MathlibWarningCallGuard {
    owner: crate::sexp::owner::OwnerPin,
    previous: Option<crate::sexp::instance::RuntimeValue>,
}

impl Drop for MathlibWarningCallGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            unsafe {
                (*self.owner.as_ptr()).error_state.mathlib_warning_call = previous;
            }
        }
    }
}

/// Scope nmath warning attribution to the original owning call.
pub fn mathlib_warning_call_guard(call: SEXP) -> MathlibWarningCallGuard {
    let owner = super::state::error_scope_pin();
    let next = unsafe { crate::sexp::instance::RuntimeValue::from_raw_in(owner.as_ptr(), call) };
    let previous = unsafe {
        std::mem::replace(
            &mut (*owner.as_ptr()).error_state.mathlib_warning_call,
            next,
        )
    };
    MathlibWarningCallGuard {
        owner,
        previous: Some(previous),
    }
}

pub fn mathlib_warning_call() -> SEXP {
    super::state::with_error_state(|state| state.mathlib_warning_call.as_raw())
}

// ---------------------------------------------------------------------------
// Message functions (R's message())
// ---------------------------------------------------------------------------

/// Issue a message (R's message()).
/// Messages are printed to stdout (via Rprintf in C, println! here).
/// Unlike errors/warnings, messages do not terminate or indicate problems.
///
/// Ported from R's `Rf_message()` concept.
pub fn Rf_message(format: Option<&CStr>) {
    let Some(format) = format else {
        println!();
        return;
    };
    let msg = format.to_str().unwrap_or("");
    // Strip trailing newline if present (C version does this)
    let msg = msg.trim_end_matches('\n');
    println!("{}", msg);
}

/// Issue a message with call.
/// Ported from R's message handling in errors.c.
pub fn messagecall(call: SEXP, format: Option<&CStr>) {
    {
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
pub fn Rf_message_append(format: Option<&CStr>, append: c_int) {
    let Some(format) = format else {
        if append == 0 {
            println!();
        }
        return;
    };
    let msg = format.to_str().unwrap_or("");
    let msg = msg.trim_end_matches('\n');
    if append == 0 {
        println!("{}", msg);
    } else {
        print!("{}", msg);
    }
}

/// do_message — R's message() builtin.
/// Ported from errors.c.
pub fn do_message(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    checkArity(op, args);

    let mut c_call: SEXP = ptr::null_mut();
    if asLogical(CAR(args)) != 0 {
        c_call = findCall();
    }
    let mut args = CDR(args);

    let append = asLogical(CAR(args));
    args = CDR(args);

    if isNull(CAR(args)) == 0 {
        SETCAR(args, coerceVector(CAR(args), SEXPTYPE::STRSXP.as_c_int()));
        if isValidString(CAR(args)) != 0 {
            let msg = translateChar(STRING_ELT(CAR(args), 0));
            // SAFETY: `translateChar` returns null or a NUL-terminated string
            // kept alive by the protected argument for this call.
            let msg = (!msg.is_null()).then(|| unsafe { CStr::from_ptr(msg) });
            Rf_message_append(msg, append);
        }
    }

    globals::R_NilValue()
}
