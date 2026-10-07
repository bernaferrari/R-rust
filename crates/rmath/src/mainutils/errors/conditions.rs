#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Condition handling: handler entries, condition signaling, condition
//! constructors (R_make*/R_signal*), tryCatch support, and initialization.

use super::helpers::translateChar;
use super::*;

// ---------------------------------------------------------------------------
// Condition handling infrastructure
// ---------------------------------------------------------------------------

/// Handler entry structure (mirrors R's mkHandlerEntry).
pub fn mkHandlerEntry(
    klass: SEXP,
    parentenv: SEXP,
    handler: SEXP,
    target: SEXP,
    result: SEXP,
    calling: c_int,
) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let factory = owner.node_factory();
        let inputs = [klass, parentenv, handler, target, result].map(|pointer| {
            factory
                .wrap(pointer)
                .and_then(|value| value.into_owned())
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        });
        let entry = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 5)))
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut entry = crate::sexp::object::SexpMut::try_from_checked(entry)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        for (index, value) in inputs.into_iter().enumerate() {
            entry
                .try_set_vector_elt(index as _, value)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        SETLEVELS(entry.as_raw(), calling);
        entry.as_raw()
    }
}

/// IS_CALLING_ENTRY macro.
#[inline]
pub unsafe fn IS_CALLING_ENTRY(e: SEXP) -> c_int {
    unsafe { LEVELS(e) }
}

/// ENTRY_CLASS macro.
#[inline]
pub unsafe fn ENTRY_CLASS(e: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(e, 0) }
}

/// ENTRY_HANDLER macro.
#[inline]
pub unsafe fn ENTRY_HANDLER(e: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(e, 2) }
}

/// ENTRY_TARGET_ENVIR macro.
#[inline]
pub unsafe fn ENTRY_TARGET_ENVIR(e: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(e, 3) }
}

/// ENTRY_RETURN_RESULT macro.
#[inline]
pub unsafe fn ENTRY_RETURN_RESULT(e: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(e, 4) }
}

/// CLEAR_ENTRY_CALLING_ENVIR macro.
#[inline]
pub unsafe fn CLEAR_ENTRY_CALLING_ENVIR(e: SEXP) {
    unsafe {
        SET_VECTOR_ELT(e, 1, globals::R_NilValue());
    }
}

/// CLEAR_ENTRY_TARGET_ENVIR macro.
#[inline]
pub unsafe fn CLEAR_ENTRY_TARGET_ENVIR(e: SEXP) {
    unsafe {
        SET_VECTOR_ELT(e, 3, globals::R_NilValue());
    }
}

/// RESULT_SIZE for handler results.
pub const RESULT_SIZE: usize = 4;

/// do_addCondHands — add condition handlers to the stack.
pub unsafe fn do_addCondHands(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let _pin = owner
            .pin()
            .unwrap()
            .expect("condition handlers require a managed runtime");
        let factory = owner.node_factory();
        let own = |pointer| {
            factory
                .wrap(pointer)
                .and_then(|value| value.into_owned())
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        };
        let arguments = own(args);
        let classes = own(CAR(arguments.as_raw()));
        let mut rest = CDR(arguments.as_raw());
        let handlers = own(CAR(rest));
        rest = CDR(rest);
        let parentenv = own(CAR(rest));
        rest = CDR(rest);
        let target = own(CAR(rest));
        rest = CDR(rest);
        let calling_value = own(CAR(rest));
        let oldstack = crate::sexp::instance::with_required_current_instance(|instance| {
            (*instance).error_state.handler_stack.owned()
        })
        .unwrap_or_else(|| factory.nil().into_owned().unwrap());
        let n = if classes.is_nil() || handlers.is_nil() {
            0
        } else {
            LENGTH(handlers.as_raw())
        };
        let entries: Vec<_> = (0..n)
            .map(|i| {
                (
                    own(STRING_ELT(classes.as_raw(), i as _)),
                    own(VECTOR_ELT(handlers.as_raw(), i as _)),
                )
            })
            .collect();
        let calling = asLogical(calling_value.as_raw());
        owner
            .require_active()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if classes.is_nil() || handlers.is_nil() {
            return oldstack.as_raw();
        }
        let result = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, RESULT_SIZE as _)))
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut newstack = oldstack.clone();
        for (klass, handler) in entries.into_iter().rev() {
            let entry = own(mkHandlerEntry(
                klass.as_raw(),
                parentenv.as_raw(),
                handler.as_raw(),
                target.as_raw(),
                result.as_raw(),
                calling,
            ));
            newstack = factory
                .pairlist_cell(&entry, &newstack, &factory.nil())
                .and_then(|value| value.into_owned())
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        set_handler_stack(newstack.as_raw());
        oldstack.as_raw()
    }
}

/// do_resetCondHands — reset condition handlers to a previous state.
pub unsafe fn do_resetCondHands(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let old = CAR(args);
        set_handler_stack(old);
        globals::R_NilValue()
    }
}

// ---------------------------------------------------------------------------

// Condition signaling
// ---------------------------------------------------------------------------

unsafe fn findConditionHandler(cond: SEXP) -> SEXP {
    unsafe {
        let classes = getAttrib(cond, R_ClassSymbol());
        if TYPEOF(classes) != SEXPTYPE::STRSXP {
            return globals::R_NilValue();
        }
        let n_classes = LENGTH(classes);
        let mut list = handler_stack();
        while !list.is_null() && list != globals::R_NilValue() {
            let entry = CAR(list);
            let entry_class = ENTRY_CLASS(entry);
            if !entry_class.is_null() {
                let entry_bytes = CHAR(entry_class);
                if !entry_bytes.is_null() {
                    let entry_str = CStr::from_ptr(entry_bytes).to_bytes();
                    for i in 0..n_classes {
                        let cls = STRING_ELT(classes, i as R_xlen_t);
                        if !cls.is_null() {
                            let cls_bytes = CHAR(cls);
                            if !cls_bytes.is_null() {
                                let cls_str = CStr::from_ptr(cls_bytes).to_bytes();
                                if entry_str == cls_str {
                                    return list;
                                }
                            }
                        }
                    }
                }
            }
            list = CDR(list);
        }
        globals::R_NilValue()
    }
}

/// do_signalCondition — signal a condition through the handler stack.
pub unsafe fn do_signalCondition(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        signal_condition_object(CAR(args), CADR(args), CADDR(args));
        globals::R_NilValue()
    }
}

/// Shared public/native dispatch: stack order chooses the first matching entry,
/// and that entry plus newer handlers are removed before invoking its callback.
/// Restore the original owner's stack on normal return and every unwind path.
pub(crate) unsafe fn signal_condition_object(cond: SEXP, msg: SEXP, ecall: SEXP) -> bool {
    let condition = unsafe { crate::sexp::context::own_control_value(cond) };
    let message = unsafe { crate::sexp::context::own_control_value(msg) };
    let call = unsafe { crate::sexp::context::own_control_value(ecall) };
    super::native::with_preserved_handler_stack(|| unsafe {
        let mut called = false;
        let mut list = findConditionHandler(condition.as_raw());
        while !list.is_null() && list != globals::R_NilValue() {
            let entry = crate::sexp::context::own_control_value(CAR(list));
            set_handler_stack(CDR(list));
            if IS_CALLING_ENTRY(entry.as_raw()) != 0 {
                let h = ENTRY_HANDLER(entry.as_raw());
                if h == globals::R_RestartToken() {
                    let msg = message.as_raw();
                    let msgstr = if TYPEOF(msg) == SEXPTYPE::STRSXP && LENGTH(msg) > 0 {
                        let c = translateChar(STRING_ELT(msg, 0));
                        CStr::from_ptr(c).to_str().unwrap_or("error")
                    } else {
                        "error message not a string"
                    };
                    let cmsg = std::ffi::CString::new(msgstr).unwrap_or_default();
                    verrorcall_dflt(call.as_raw(), cmsg.as_ptr(), ptr::null_mut());
                } else if !super::native::dispatch_calling_handler(h, condition.as_raw()) {
                    let hcall = Rf_lang2(h, condition.as_raw());
                    let _hcall_guard = protect(hcall);
                    let _ = crate::eval::eval::Rf_eval(hcall, globals::R_GlobalEnv());
                }
                called = true;
            } else {
                gotoExitingHandler(condition.as_raw(), call.as_raw(), entry.as_raw());
            }
            list = findConditionHandler(condition.as_raw());
        }
        called
    })
}

/// do_dfltWarn — default warning handler.
pub unsafe fn do_dfltWarn(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if TYPEOF(CAR(args)) != SEXPTYPE::STRSXP || LENGTH(CAR(args)) != 1 {
            errorcall(call, b"bad error message\x00".as_ptr() as *const c_char);
        }
        let msg = translateChar(STRING_ELT(CAR(args), 0));
        let ecall = CADR(args);
        vwarningcall_dflt(ecall, msg, ptr::null_mut());
        globals::R_NilValue()
    }
}

/// do_dfltStop — default error handler.
pub unsafe fn do_dfltStop(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if TYPEOF(CAR(args)) != SEXPTYPE::STRSXP || LENGTH(CAR(args)) != 1 {
            errorcall(call, b"bad error message\x00".as_ptr() as *const c_char);
        }
        let msg = translateChar(STRING_ELT(CAR(args), 0));
        let message = CStr::from_ptr(msg).to_str().unwrap_or("").to_string();
        errorcall_str(globals::R_NilValue(), &message)
    }
}

// ---------------------------------------------------------------------------
// Condition creation helpers
// ---------------------------------------------------------------------------

/// R_makeErrorCondition — create an error condition object.
pub unsafe fn R_makeErrorCondition(
    call: SEXP,
    classname: *const c_char,
    subclassname: *const c_char,
    nextra: c_int,
    format: *const c_char,
) -> SEXP {
    unsafe {
        let class = if classname.is_null() {
            ""
        } else {
            CStr::from_ptr(classname).to_str().unwrap_or("")
        };
        let sub = if subclassname.is_null() {
            ""
        } else {
            CStr::from_ptr(subclassname).to_str().unwrap_or("")
        };
        let fmt = if format.is_null() {
            ""
        } else {
            CStr::from_ptr(format).to_str().unwrap_or("")
        };

        make_condition(call, class, sub, nextra, fmt, "error")
    }
}

/// The raw attribute bridge remains here until graph edges use checked links.
/// Text and vector construction use bounded Rust inputs and owning roots.
pub(super) unsafe fn make_condition(
    call: SEXP,
    class: &str,
    subclass: &str,
    extra: c_int,
    message: &str,
    category: &str,
) -> SEXP {
    use crate::sexp::object::{SessionNodeFactory, SexpError, SexpMut, SexpResult};

    let build = || -> SexpResult<crate::sexp::object::Sexp<'_>> {
        // SAFETY: the translated condition entry retains the active owner.
        let factory =
            SessionNodeFactory::new(unsafe { crate::sexp::owner::OwnerToken::current() }?);
        let call = if call.is_null() {
            factory.nil()
        } else {
            factory.wrap(call)?
        };
        let length = extra.checked_add(2).filter(|length| *length >= 2).ok_or(
            SexpError::AllocationFailed {
                object: "condition",
            },
        )?;
        let condition =
            factory.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, length.into())))?;
        let mut condition = SexpMut::try_from_checked(condition)?;
        let message = factory.strings(&[message])?;
        condition.try_set_vector_elt(0, message)?;
        condition.try_set_vector_elt(1, call)?;
        let names =
            factory.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, length.into())))?;
        let mut names = SexpMut::try_from_checked(names)?;
        names.try_set_string_elt(0, factory.character("message")?)?;
        names.try_set_string_elt(1, factory.character("call")?)?;
        // SAFETY: both objects remain checked and rooted through allocation.
        unsafe { setAttrib_wrap(condition.as_raw(), R_NamesSymbol(), names.as_raw()) };
        let classes = if subclass.is_empty() {
            factory.strings(&[class, category, "condition"])?
        } else {
            factory.strings(&[subclass, class, category, "condition"])?
        };
        // SAFETY: both objects remain checked and rooted through allocation.
        unsafe { setAttrib_wrap(condition.as_raw(), R_ClassSymbol(), classes.as_raw()) };
        Ok(condition.freeze())
    };
    build()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        .as_raw()
}

/// R_signalErrorCondition — signal an error condition.
pub unsafe fn R_signalErrorCondition(cond: SEXP, call: SEXP) {
    unsafe {
        // Extract message from condition and call errorcall_dflt
        if TYPEOF(cond) != SEXPTYPE::VECSXP || LENGTH(cond) == 0 {
            errorcall(
                call,
                b"condition object must be a VECSXP of length at least one\x00".as_ptr()
                    as *const c_char,
            );
        }
        let elt = VECTOR_ELT(cond, 0);
        if TYPEOF(elt) != SEXPTYPE::STRSXP || LENGTH(elt) != 1 {
            errorcall(
                call,
                b"first element of condition object must be a scalar string\x00".as_ptr()
                    as *const c_char,
            );
        }
        // GNU R_signalErrorCondition keeps `cond` as the signaled object so
        // tryCatch handlers receive objectNotFoundError rather than a
        // reconstructed simpleError.
        crate::sexp::instance::with_required_current_instance(|inst| unsafe {
            (*inst).error_state.signalled_condition =
                crate::sexp::instance::RuntimeValue::from_raw_in(inst, cond);
        });
        let msg = translateChar(STRING_ELT(elt, 0));
        errorcall(call, msg);
    }
}

/// R_signalErrorConditionEx — signal an error condition with exitOnly flag.
pub unsafe fn R_signalErrorConditionEx(cond: SEXP, call: SEXP, exitOnly: c_int) {
    unsafe {
        R_signalErrorCondition(cond, call);
    }
}

/// GNU `R_ObjectNotFoundError(sym, call, mode)`.
pub unsafe fn R_ObjectNotFoundError(sym: SEXP, call: SEXP, mode: Option<&str>) -> ! {
    unsafe {
        let pname = PRINTNAME(sym);
        let name = if pname.is_null() {
            String::from("???")
        } else {
            let chars = CHAR(pname);
            if chars.is_null() {
                String::from("???")
            } else {
                CStr::from_ptr(chars)
                    .to_str()
                    .map(str::to_string)
                    .unwrap_or_else(|_| String::from("???"))
            }
        };
        let msg = match mode {
            None => format!("object '{name}' not found"),
            Some(mode) => format!("object '{name}' of mode '{mode}' was not found"),
        };
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
        let call = if call.is_null() {
            crate::sexp::globals::R_NilValue()
        } else {
            call
        };
        let _call_guard = protect(call);
        let cond = R_makeErrorCondition(
            call,
            c"objectNotFoundError".as_ptr(),
            std::ptr::null(),
            2,
            c_msg.as_ptr(),
        );
        let _cond_guard = protect(cond);
        R_setConditionField(cond, 2, c"name".as_ptr(), sym);
        let mode_name = mode.unwrap_or("any");
        let mode_cstr = std::ffi::CString::new(mode_name).unwrap_or_default();
        let mode_sexp = Rf_mkString(mode_cstr.as_ptr());
        let _mode_guard = protect(mode_sexp);
        R_setConditionField(cond, 3, c"mode".as_ptr(), mode_sexp);
        R_signalErrorCondition(cond, call);
        unreachable!("R_signalErrorCondition does not return")
    }
}

/// GNU `R_FunctionNotFoundError(sym, call)`.
pub unsafe fn R_FunctionNotFoundError(sym: SEXP, call: SEXP) -> ! {
    unsafe {
        let pname = PRINTNAME(sym);
        let name = if pname.is_null() {
            String::from("???")
        } else {
            let chars = CHAR(pname);
            if chars.is_null() {
                String::from("???")
            } else {
                CStr::from_ptr(chars)
                    .to_str()
                    .map(str::to_string)
                    .unwrap_or_else(|_| String::from("???"))
            }
        };
        let c_msg = std::ffi::CString::new(format!("could not find function \"{name}\""))
            .unwrap_or_default();
        let call = if call.is_null() {
            crate::sexp::globals::R_NilValue()
        } else {
            call
        };
        let _call_guard = protect(call);
        let cond = R_makeErrorCondition(
            call,
            c"objectNotFoundError".as_ptr(),
            c"functionNotFoundError".as_ptr(),
            2,
            c_msg.as_ptr(),
        );
        let _cond_guard = protect(cond);
        R_setConditionField(cond, 2, c"name".as_ptr(), sym);
        let mode_sexp = Rf_mkString(c"function".as_ptr());
        let _mode_guard = protect(mode_sexp);
        R_setConditionField(cond, 3, c"mode".as_ptr(), mode_sexp);
        R_signalErrorCondition(cond, call);
        unreachable!("R_signalErrorCondition does not return")
    }
}

/// R_setConditionField — set a field in a condition object.
pub unsafe fn R_setConditionField(cond: SEXP, idx: R_xlen_t, name: *const c_char, val: SEXP) {
    unsafe {
        if TYPEOF(cond) != SEXPTYPE::VECSXP {
            return;
        }
        let len = XLENGTH(cond);
        if idx < 0 || idx >= len {
            return;
        }
        SET_VECTOR_ELT(cond, idx, val);
        let names = getAttrib_wrap(cond, R_NamesSymbol());
        if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) == len {
            SET_STRING_ELT(names, idx, Rf_mkChar(name));
        }
    }
}

// ---------------------------------------------------------------------------
// tryCatch support (simplified)
// ---------------------------------------------------------------------------

// Resolve outside the transfer store borrow and retain the canonical owning
// payload through result publication. Unmatched transfers preserve their ticket.
fn take_matching_exiting_handler(
    payload: Box<dyn std::any::Any + Send>,
) -> Result<crate::sexp::transfer::TransferLease, Box<dyn std::any::Any + Send>> {
    use crate::sexp::{context::RSignal, transfer::OwnedTransfer};
    if let Some(RSignal::ExitingHandler(ticket)) = payload.downcast_ref::<RSignal>() {
        let lease = ticket
            .resolve()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        lease
            .require_live()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let OwnedTransfer::ExitingHandler { target_env, .. } = lease.data() else {
            crate::sexp::context::r_error("invalid exiting-handler transfer")
        };
        if crate::sexp::context::context_env_exists(target_env.as_raw()) {
            let signal = payload
                .downcast::<RSignal>()
                .expect("resolved exiting handler");
            let RSignal::ExitingHandler(ticket) = *signal else {
                unreachable!("resolved exiting handler")
            };
            return Ok(ticket
                .take()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())));
        }
    }
    Err(payload)
}

fn is_nonlocal_transfer(payload: &(dyn std::any::Any + Send)) -> bool {
    use crate::sexp::context::RSignal;
    matches!(
        payload.downcast_ref::<RSignal>(),
        Some(
            RSignal::Return(_)
                | RSignal::Jump(_)
                | RSignal::ExitingHandler(_)
                | RSignal::Restart(_)
                | RSignal::Break
                | RSignal::Next
                | RSignal::Abort
        )
    )
}

fn exiting_handler_result(lease: &crate::sexp::transfer::TransferLease) -> SEXP {
    lease
        .require_live()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let crate::sexp::transfer::OwnedTransfer::ExitingHandler { result, .. } = lease.data() else {
        crate::sexp::context::r_error("invalid exiting-handler transfer")
    };
    result.as_raw()
}

/// Native error catcher. Callback data must remain live for this dynamic scope.
/// Rust callbacks declare C-unwind so evaluation can use nonlocal transfers.
pub unsafe fn R_tryCatchError(
    body: Option<super::NativeBody>,
    bdata: *mut c_void,
    handler: Option<super::NativeHandler>,
    hdata: *mut c_void,
) -> SEXP {
    let factory = unsafe { crate::sexp::owner::OwnerToken::current() }
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
        .node_factory();
    let classes = factory
        .strings(&["error"])
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    unsafe {
        super::native::catch_native(
            body,
            bdata,
            classes.as_raw(),
            handler,
            hdata,
            None,
            ptr::null_mut(),
        )
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// R_InitConditions — initialize error/warning condition objects.
pub unsafe fn R_InitConditions() {
    unsafe {
        // Create and preserve condition objects for stack overflow errors
        let protect_so = R_makeErrorCondition(
            globals::R_NilValue(),
            b"stackOverflowError\x00".as_ptr() as *const c_char,
            b"protectStackOverflowError\x00".as_ptr() as *const c_char,
            0,
            b"protect(): protection stack overflow\x00".as_ptr() as *const c_char,
        );
        crate::sexp::protect::R_PreserveObject(protect_so);

        let expr_so = R_makeErrorCondition(
            globals::R_NilValue(),
            b"stackOverflowError\x00".as_ptr() as *const c_char,
            b"expressionStackOverflowError\x00".as_ptr() as *const c_char,
            0,
            b"evaluation nested too deeply: infinite recursion / options(expressions=)?\x00"
                .as_ptr() as *const c_char,
        );
        crate::sexp::protect::R_PreserveObject(expr_so);

        let node_so = R_makeErrorCondition(
            globals::R_NilValue(),
            b"stackOverflowError\x00".as_ptr() as *const c_char,
            b"nodeStackOverflowError\x00".as_ptr() as *const c_char,
            0,
            b"node stack overflow\x00".as_ptr() as *const c_char,
        );
        crate::sexp::protect::R_PreserveObject(node_so);
    }
}

/// R_MissingArgError_c — report a missing argument error.
/// Matches C's `void R_MissingArgError_c(const char* arg, SEXP call, const char* subclass)`
pub unsafe fn R_MissingArgError_c(arg: *const c_char, call: SEXP, subclass: *const c_char) {
    unsafe {
        let arg_str = if arg.is_null() {
            ""
        } else {
            CStr::from_ptr(arg).to_str().unwrap_or("")
        };
        let _call_guard = protect(call);
        let msg = if !arg_str.is_empty() {
            format!("argument \"{}\" is missing, with no default", arg_str)
        } else {
            "argument is missing, with no default".to_string()
        };
        let c_msg = std::ffi::CString::new(msg.clone()).unwrap_or_default();
        let cond = R_makeErrorCondition(
            call,
            b"missingArgError\0".as_ptr() as *const c_char,
            subclass,
            0,
            c_msg.as_ptr(),
        );
        let _cond_guard = protect(cond);
        R_signalErrorCondition(cond, call);
    }
}

/// R_MissingArgError — report a missing argument error from a symbol.
/// Matches C's `void R_MissingArgError(SEXP symbol, SEXP call, const char* subclass)`
pub unsafe fn R_MissingArgError(symbol: SEXP, call: SEXP, subclass: *const c_char) {
    unsafe {
        let arg = if symbol.is_null() || TYPEOF(symbol) != SEXPTYPE::SYMSXP {
            b"\0".as_ptr() as *const c_char
        } else {
            let name = CHAR_local(PRINTNAME(symbol));
            if name.is_null() {
                b"\0".as_ptr() as *const c_char
            } else {
                name
            }
        };
        R_MissingArgError_c(arg, call, subclass);
    }
}

/// R_signalWarningCondition — signal a warning condition object.
/// Matches C's `void R_signalWarningCondition(SEXP cond)`.
pub unsafe fn R_signalWarningCondition(cond: SEXP) {
    unsafe {
        if cond.is_null() || TYPEOF(cond) != SEXPTYPE::VECSXP || LENGTH(cond) < 1 {
            return;
        }
        let elt = VECTOR_ELT(cond, 0);
        if TYPEOF(elt) != SEXPTYPE::STRSXP || LENGTH(elt) != 1 {
            return;
        }
        let msg = translateChar(STRING_ELT(elt, 0));
        let call = if LENGTH(cond) > 1 {
            VECTOR_ELT(cond, 1)
        } else {
            ptr::null_mut()
        };
        warningcall(call, msg);
    }
}

/// Apply the default warning policy to an already-signaled condition.
///
/// This is the post-handler half of `R_signalWarningCondition`: callers that
/// preserve a concrete warning subclass signal it first, then collect/print it
/// here only when no handler muffled the warning.
pub(crate) unsafe fn warning_condition_default(cond: SEXP) {
    unsafe {
        if cond.is_null() || TYPEOF(cond) != SEXPTYPE::VECSXP || LENGTH(cond) < 1 {
            return;
        }
        let elt = VECTOR_ELT(cond, 0);
        if TYPEOF(elt) != SEXPTYPE::STRSXP || LENGTH(elt) != 1 {
            return;
        }
        let msg = translateChar(STRING_ELT(elt, 0));
        let call = if LENGTH(cond) > 1 {
            VECTOR_ELT(cond, 1)
        } else {
            ptr::null_mut()
        };
        vwarningcall_dflt(call, msg, ptr::null_mut());
    }
}

/// R_makeWarningCondition — create a warning condition object.
/// Matches C's `SEXP R_makeWarningCondition(SEXP call, const char *classname,
/// const char *subclassname, int nextra, const char *format, ...)`
pub unsafe fn R_makeWarningCondition(
    call: SEXP,
    classname: *const c_char,
    subclassname: *const c_char,
    nextra: c_int,
    format: *const c_char,
) -> SEXP {
    unsafe {
        let class = if classname.is_null() {
            "simpleWarning"
        } else {
            CStr::from_ptr(classname)
                .to_str()
                .unwrap_or("simpleWarning")
        };
        let sub = if subclassname.is_null() {
            ""
        } else {
            CStr::from_ptr(subclassname).to_str().unwrap_or("")
        };
        let fmt = if format.is_null() {
            ""
        } else {
            CStr::from_ptr(format).to_str().unwrap_or("")
        };

        make_condition(call, class, sub, nextra, fmt, "warning")
    }
}

/// Message text for a partial-match warning operand: PRINTNAME for a
/// symbol, translateChar for a CHARSXP string (upstream errors.c passes
/// these straight into the format string).
unsafe fn partial_match_text(x: SEXP) -> String {
    unsafe {
        if !x.is_null() && TYPEOF(x) == SEXPTYPE::SYMSXP {
            CStr::from_ptr(CHAR_local(PRINTNAME(x)))
                .to_string_lossy()
                .into_owned()
        } else if !x.is_null() {
            CStr::from_ptr(translateChar(x))
                .to_string_lossy()
                .into_owned()
        } else {
            "?".to_string()
        }
    }
}

/// R_makePartialMatchWarningCondition — create a partial match warning condition.
/// Matches C's `SEXP R_makePartialMatchWarningCondition(SEXP call, SEXP input, SEXP target)`
/// where input/target are symbols or CHARSXP strings.
pub unsafe fn R_makePartialMatchWarningCondition(call: SEXP, input: SEXP, target: SEXP) -> SEXP {
    unsafe {
        let msg = format!(
            "partial match of '{}' to '{}'",
            partial_match_text(input),
            partial_match_text(target),
        );
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        let cond = R_makeWarningCondition(
            call,
            b"partialMatchWarning\0".as_ptr() as *const c_char,
            ptr::null(),
            2,
            c_msg.as_ptr(),
        );
        let _cond_guard = protect(cond);
        R_setConditionField(
            cond,
            2,
            b"input\0".as_ptr() as *const c_char,
            if !input.is_null() && TYPEOF(input) == SEXPTYPE::SYMSXP {
                input
            } else {
                Rf_ScalarString(input)
            },
        );
        R_setConditionField(
            cond,
            3,
            b"target\0".as_ptr() as *const c_char,
            if !target.is_null() && TYPEOF(target) == SEXPTYPE::SYMSXP {
                target
            } else {
                Rf_ScalarString(target)
            },
        );
        // ideally we would want the function/object in a field also
        cond
    }
}

/// R_makePartialArgumentMatchWarningCondition — create a partial argument
/// match warning condition (supplied argument tag vs function formal).
/// Matches C's `SEXP R_makePartialArgumentMatchWarningCondition(SEXP call,
/// SEXP argument, SEXP formal)`
pub unsafe fn R_makePartialArgumentMatchWarningCondition(
    call: SEXP,
    argument: SEXP,
    formal: SEXP,
) -> SEXP {
    unsafe {
        let msg = format!(
            "partial argument match of '{}' to '{}'",
            partial_match_text(argument),
            partial_match_text(formal),
        );
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        let cond = R_makeWarningCondition(
            call,
            b"partialMatchWarning\0".as_ptr() as *const c_char,
            b"partialArgumentMatchWarning\0".as_ptr() as *const c_char,
            2,
            c_msg.as_ptr(),
        );
        let _cond_guard = protect(cond);
        R_setConditionField(cond, 2, b"argument\0".as_ptr() as *const c_char, argument);
        R_setConditionField(cond, 3, b"formal\0".as_ptr() as *const c_char, formal);
        // ideally we would want the function/object in a field also
        cond
    }
}

/// R_makeNotSubsettableError — create a "not subsettable" error condition.
/// Matches C's `SEXP R_makeNotSubsettableError(SEXP x, SEXP call)`
pub unsafe fn R_makeNotSubsettableError(x: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let class_str = if !x.is_null() {
            let klass = getAttrib_wrap(x, R_ClassSymbol());
            if !klass.is_null() && TYPEOF(klass) == SEXPTYPE::STRSXP && LENGTH(klass) >= 1 {
                let s = CHAR_local(STRING_ELT(klass, 0));
                CStr::from_ptr(s).to_str().unwrap_or("object")
            } else {
                "object"
            }
        } else {
            "object"
        };
        let msg = format!("object of type '{}' is not subsettable", class_str);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"notSubsettableError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeMissingSubscriptError — create a missing subscript error condition.
/// Matches C's `SEXP R_makeMissingSubscriptError(SEXP x, SEXP call)`
pub unsafe fn R_makeMissingSubscriptError(x: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let class_str = if !x.is_null() {
            let klass = getAttrib_wrap(x, R_ClassSymbol());
            if !klass.is_null() && TYPEOF(klass) == SEXPTYPE::STRSXP && LENGTH(klass) >= 1 {
                let s = CHAR_local(STRING_ELT(klass, 0));
                CStr::from_ptr(s).to_str().unwrap_or("object")
            } else {
                "object"
            }
        } else {
            "object"
        };
        let msg = format!("subscript out of bounds for {}", class_str);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"missingSubscriptError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeMissingSubscriptError1 — create a missing subscript error condition (no x).
/// Matches C's `SEXP R_makeMissingSubscriptError1(SEXP call)`
pub unsafe fn R_makeMissingSubscriptError1(call: SEXP) -> SEXP {
    unsafe {
        let msg = "subscript out of bounds";
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"missingSubscriptError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}
/// `x[[]]` / `x[[]] <-`: class `MissingSubscriptError`, message
/// `"missing subscript"`, with `call` and `object` fields.
pub unsafe fn R_MissingSubscriptError(x: SEXP, call: SEXP) -> ! {
    unsafe {
        let c_msg = std::ffi::CString::new("missing subscript").unwrap_or_default();
        let cond = R_makeErrorCondition(
            call,
            b"MissingSubscriptError\0".as_ptr() as *const c_char,
            std::ptr::null(),
            1,
            c_msg.as_ptr(),
        );
        let _guard = protect(cond);
        R_setConditionField(cond, 2, b"object\0".as_ptr() as *const c_char, x);
        R_signalErrorCondition(cond, call);
        unreachable!()
    }
}

/// R_makeOutOfBoundsError — create an out-of-bounds error condition.
/// Matches C's `SEXP R_makeOutOfBoundsError(SEXP x, int subscript, SEXP sindex, SEXP call)`
pub unsafe fn R_makeOutOfBoundsError(x: SEXP, subscript: c_int, sindex: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let idx_str = if !sindex.is_null() && TYPEOF(sindex) == SEXPTYPE::REALSXP {
            format!("{}", *REAL(sindex))
        } else if !sindex.is_null() && TYPEOF(sindex) == SEXPTYPE::INTSXP {
            format!("{}", *INTEGER(sindex))
        } else {
            format!("{}", subscript)
        };
        let msg = format!("subscript out of bounds (index {} too large)", idx_str);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"outOfBoundsError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeCStackOverflowError — create a C stack overflow error condition.
/// Matches C's `SEXP R_makeCStackOverflowError(SEXP call, intptr_t usage)`
pub unsafe fn R_makeCStackOverflowError(call: SEXP, usage: isize) -> SEXP {
    unsafe {
        let msg = format!("C stack usage {} is too close to the limit", usage);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"stackOverflowError\0".as_ptr() as *const c_char,
            b"cStackOverflowError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_getProtectStackOverflowError — get the preserved protect stack overflow condition.
pub unsafe fn R_getProtectStackOverflowError() -> SEXP {
    unsafe {
        // Would return a preserved condition; for now return nil
        globals::R_NilValue()
    }
}

/// R_getExpressionStackOverflowError — get the preserved expression stack overflow condition.
pub unsafe fn R_getExpressionStackOverflowError() -> SEXP {
    unsafe {
        // Would return a preserved condition; for now return nil
        globals::R_NilValue()
    }
}

/// R_getNodeStackOverflowError — get the preserved node stack overflow condition.
pub unsafe fn R_getNodeStackOverflowError() -> SEXP {
    unsafe {
        // Would return a preserved condition; for now return nil
        globals::R_NilValue()
    }
}

/// Native tryCatch with GNU class filtering and finalization arguments.
/// # Safety
/// Callback data remains valid until the scope ends; callback result projections
/// belong to the live active runtime. Callbacks that evaluate R permit unwinding.
pub unsafe fn R_tryCatch(
    body: Option<super::NativeBody>,
    bdata: *mut c_void,
    classes: SEXP,
    handler: Option<super::NativeHandler>,
    hdata: *mut c_void,
    finally: Option<super::NativeFinally>,
    fdata: *mut c_void,
) -> SEXP {
    unsafe { super::native::catch_native(body, bdata, classes, handler, hdata, finally, fdata) }
}

/// Native calling error handler. Callback data follows the same scoped contract.
pub unsafe fn R_withCallingErrorHandler(
    body: Option<super::NativeBody>,
    bdata: *mut c_void,
    handler: Option<super::NativeHandler>,
    hdata: *mut c_void,
) -> SEXP {
    unsafe { super::native::calling_native(body, bdata, handler, hdata) }
}

#[cfg(test)]
mod condition_construction_tests {
    use super::*;
    use crate::sexp::{object::SessionNodeFactory, session::RSession};
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn owned_exiting_handler_result_survives_detachment_collection_and_rethrow() {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let factory = SessionNodeFactory::new(owner);
            let nil = globals::R_NilValue();
            let environment = factory
                .wrap(crate::sexp::memory_ext::NewEnvironment(
                    nil,
                    nil,
                    globals::R_GlobalEnv(),
                ))
                .unwrap();
            let result = factory.wrap(Rf_allocVector(SEXPTYPE::VECSXP, 3)).unwrap();
            let condition = factory
                .wrap(crate::sexp::constructors::Rf_ScalarInteger(91))
                .unwrap();
            let entry = factory
                .wrap(mkHandlerEntry(
                    nil,
                    nil,
                    nil,
                    environment.as_raw(),
                    result.as_raw(),
                    0,
                ))
                .unwrap();
            let result_node = crate::sexp::memory::checked_projection(result.as_raw())
                .unwrap()
                .1;
            let environment_node = crate::sexp::memory::checked_projection(environment.as_raw())
                .unwrap()
                .1;
            let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                gotoExitingHandler(condition.as_raw(), nil, entry.as_raw())
            }))
            .expect_err("exiting handler must unwind");
            drop(entry);
            drop(result);
            drop(condition);
            drop(environment);
            (*instance).error_state.handler_stack =
                crate::sexp::instance::RuntimeValue::from_raw_in(instance, nil);
            (*instance).context_stack.clear();
            owner.full_gc().unwrap();
            assert!(result_node.is_live());
            assert!(environment_node.is_live());
            let payload = match take_matching_exiting_handler(payload) {
                Err(payload) => payload,
                Ok(_) => panic!("no matching context must preserve the transfer"),
            };
            let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                std::panic::resume_unwind(payload)
            }))
            .expect_err("unmatched ticket must keep unwinding");
            owner.full_gc().unwrap();
            let crate::sexp::context::RSignal::ExitingHandler(ticket) = payload
                .downcast_ref::<crate::sexp::context::RSignal>()
                .unwrap()
            else {
                panic!("same exiting-handler signal required")
            };
            let snapshot = ticket.resolve().unwrap();
            let crate::sexp::transfer::OwnedTransfer::ExitingHandler { target_env, .. } =
                snapshot.data()
            else {
                panic!("same exiting-handler transfer required")
            };
            let context = crate::sexp::context::Rf_begincontext_in(
                instance,
                crate::sexp::context::ctxt_flags::CTXT_FUNCTION,
                nil,
                target_env.as_raw(),
                nil,
                None,
                nil,
                nil,
            );
            drop(snapshot);
            let lease = take_matching_exiting_handler(payload).unwrap();
            // Even a matched handler's context can disappear before publication.
            crate::sexp::context::Rf_endcontext_in(instance, context);
            owner.full_gc().unwrap();
            let result = factory.wrap(exiting_handler_result(&lease)).unwrap();
            assert_eq!(
                result.try_vector_elt(0).unwrap().integer_elt(0).unwrap(),
                91
            );
            drop(result);
            drop(lease);
            owner.full_gc().unwrap();
            assert!(!result_node.is_live());
            assert!(!environment_node.is_live());
        });
    }

    #[test]
    fn bounded_condition_text_survives_each_allocation_and_reentrant_gc() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let call = session.global_env().unwrap();
        let notifications = Rc::new(Cell::new(0));
        let observed = notifications.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            crate::sexp::gengc::full_gc();
        }));
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        });
        let before = crate::sexp::protect::R_ProtectCount();
        // Each view ends before another non-NUL byte: C string scanning is invalid.
        let text = String::from("__bounded message!!");
        let message = &text[2..17];
        for (category, subclass) in [("error", "specificError"), ("warning", "")] {
            let condition = factory
                .wrap(unsafe {
                    make_condition(call.as_raw(), "customClass", subclass, 2, message, category)
                })
                .unwrap();
            crate::sexp::gengc::full_gc();
            assert_eq!(condition.len(), 4);
            assert_eq!(
                condition
                    .try_vector_elt(0)
                    .unwrap()
                    .try_string_value_elt(0)
                    .unwrap(),
                Some(message.to_owned())
            );
            assert_eq!(condition.try_vector_elt(1).unwrap().as_raw(), call.as_raw());
            let names = factory
                .wrap(unsafe { getAttrib_wrap(condition.as_raw(), R_NamesSymbol()) })
                .unwrap();
            assert_eq!(
                names.try_string_value_elt(0).unwrap(),
                Some("message".to_owned())
            );
            assert_eq!(
                names.try_string_value_elt(1).unwrap(),
                Some("call".to_owned())
            );
            let classes = factory
                .wrap(unsafe { getAttrib_wrap(condition.as_raw(), R_ClassSymbol()) })
                .unwrap();
            let mut expected = Vec::new();
            if !subclass.is_empty() {
                expected.push(subclass);
            }
            expected.extend(["customClass", category, "condition"]);
            assert_eq!(classes.len() as usize, expected.len());
            for (index, expected) in expected.into_iter().enumerate() {
                assert_eq!(
                    classes.try_string_value_elt(index as _).unwrap(),
                    Some(expected.to_owned())
                );
            }
        }
        session.with_active_in(|instance| unsafe { (*instance).memory_state.gc_force_gap = 0 });
        assert!(notifications.get() >= 10);
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
    }
}
