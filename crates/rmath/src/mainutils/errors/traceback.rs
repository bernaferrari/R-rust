#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Traceback support and source-reference helpers.

use super::*;

// ---------------------------------------------------------------------------
// Traceback support
// ---------------------------------------------------------------------------

struct TraceFrame {
    // Retain the original context cell, even if a callback removes it from the
    // stack. Each selected value separately owns its exact allocation.
    _context: std::rc::Rc<std::cell::UnsafeCell<crate::sexp::context::RCNTXT>>,
    call: Option<crate::sexp::object::Sexp<'static>>,
    srcref: Option<crate::sexp::object::Sexp<'static>>,
}

unsafe fn traceback_frames(
    instance: *mut crate::sexp::instance::RInstance,
    skip: c_int,
) -> Vec<TraceFrame> {
    unsafe {
        let mut frames = Vec::new();
        let mut remaining = skip;
        let mut context = crate::sexp::context::R_GlobalContext_in(instance);
        while !context.is_null() {
            let cell =
                crate::sexp::context::retain_context_in(instance, context).unwrap_or_else(|| {
                    crate::sexp::context::r_error(
                        "traceback context no longer belongs to its owner",
                    )
                });
            // Field snapshots finish before duplicate/setAttrib can invoke R.
            let (flag, next, call, srcref) = {
                let context = &*cell.get();
                (
                    context.callflag,
                    context.nextcontext,
                    context.call.owned(),
                    context.srcref.owned(),
                )
            };
            if flag == crate::sexp::context::ctxt_flags::CTXT_TOPLEVEL {
                break;
            }
            if flag
                & (crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                    | crate::sexp::context::ctxt_flags::CTXT_BUILTIN)
                != 0
            {
                if remaining > 0 {
                    remaining -= 1;
                } else {
                    frames.try_reserve(1).unwrap_or_else(|_| {
                        crate::sexp::context::r_error("cannot reserve traceback frames")
                    });
                    frames.push(TraceFrame {
                        _context: cell,
                        call,
                        srcref,
                    });
                }
            }
            context = next;
        }
        frames
    }
}

unsafe fn own_trace_value(
    instance: *mut crate::sexp::instance::RInstance,
    value: SEXP,
) -> Option<crate::sexp::object::Sexp<'static>> {
    if value.is_null() {
        return None;
    }
    let owner = unsafe { crate::sexp::owner::OwnerToken::from_raw(instance) };
    Some(
        owner
            .sexp(value)
            .and_then(|value| value.into_owned())
            .unwrap_or_else(|error| {
                crate::sexp::context::r_error(format!("invalid traceback value: {error}"))
            }),
    )
}

fn trace_projection(value: Option<&crate::sexp::object::Sexp<'static>>) -> SEXP {
    value.map_or(ptr::null_mut(), |value| value.as_raw())
}

/// R_GetTracebackOnly — return traceback without deparsing calls.
/// Ported from errors.c R_GetTracebackOnly().
pub unsafe fn R_GetTracebackOnly(skip: c_int) -> SEXP {
    unsafe {
        let instance = crate::sexp::instance::with_required_current_instance(|instance| instance);
        let pin = crate::sexp::context::pin_context_owner_in(instance);
        // Snapshot the selected original cells and values before the first
        // allocation. A collecting callback can change or drain the stack.
        let frames = traceback_frames(instance, skip);
        let count = c_int::try_from(frames.len())
            .unwrap_or_else(|_| crate::sexp::context::r_error("too many traceback frames"));
        let result = own_trace_value(instance, Rf_allocList(count));
        crate::sexp::context::require_context_owner_live(pin.as_ref());
        let mut cell = trace_projection(result.as_ref());
        for frame in frames {
            let call =
                crate::mainutils::duplicate::Rf_duplicate(trace_projection(frame.call.as_ref()));
            crate::sexp::context::require_context_owner_live(pin.as_ref());
            let call = own_trace_value(instance, call);
            let source = trace_projection(frame.srcref.as_ref());
            if !source.is_null() && source != globals::R_NilValue() {
                let symbol = crate::sexp::symbol::Rf_install(c"srcref".as_ptr());
                crate::sexp::context::require_context_owner_live(pin.as_ref());
                let source = crate::mainutils::duplicate::Rf_duplicate(source);
                crate::sexp::context::require_context_owner_live(pin.as_ref());
                let source = own_trace_value(instance, source);
                crate::sexp::attrib_core::setAttrib(
                    trace_projection(call.as_ref()),
                    symbol,
                    trace_projection(source.as_ref()),
                );
                crate::sexp::context::require_context_owner_live(pin.as_ref());
            }
            if !cell.is_null() {
                SETCAR(cell, trace_projection(call.as_ref()));
            }
            cell = CDR(cell);
        }
        trace_projection(result.as_ref())
    }
}
pub unsafe fn save_error_traceback() {
    unsafe {
        let trace = R_GetTracebackOnly(0);
        if trace.is_null()
            || trace == globals::R_NilValue()
            || crate::sexp::accessors::LENGTH(trace) == 0
        {
            return;
        }
        let _g = crate::sexp::protect::protect(trace);
        let mut cell = trace;
        while !cell.is_null() && cell != globals::R_NilValue() {
            let call = CAR(cell);
            if !call.is_null() && call != globals::R_NilValue() {
                SETCAR(cell, crate::mainutils::duplicate::duplicate(call));
            }
            cell = CDR(cell);
        }
        let symbol = crate::sexp::symbol::Rf_install(c".Traceback".as_ptr());
        crate::sexp::envir::defineVar(symbol, trace, crate::eval::runtime::base_env());
    }
}

/// R_ConciseTraceback — return a concise call chain as a string.
/// Ported from errors.c R_ConciseTraceback().
pub unsafe fn R_ConciseTraceback(call: SEXP, skip: c_int) -> String {
    unsafe {
        let instance = crate::sexp::instance::with_required_current_instance(|instance| instance);
        let _pin = crate::sexp::context::pin_context_owner_in(instance);
        let frames = traceback_frames(instance, skip);
        let mut buf = String::new();
        let mut ncalls: c_int = 0;
        let mut too_many = false;
        let mut top = String::new();
        for frame in frames {
            let call = trace_projection(frame.call.as_ref());
            let fun = if !call.is_null() {
                CAR(call)
            } else {
                ptr::null_mut()
            };
            let this = if !fun.is_null() && TYPEOF(fun) == SEXPTYPE::SYMSXP {
                let name = CHAR_local(PRINTNAME(fun));
                if name.is_null() {
                    "<Anonymous>".to_string()
                } else {
                    CStr::from_ptr(name)
                        .to_str()
                        .unwrap_or("<Anonymous>")
                        .to_string()
                }
            } else {
                "<Anonymous>".to_string()
            };
            if matches!(
                this.as_str(),
                "stop" | "warning" | "suppressWarnings" | ".signalSimpleWarning"
            ) {
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
                    buf = this;
                }
            }
        }
        if too_many && top.len() < 50 {
            buf = format!("{} {}", top, buf);
        }
        buf
    }
}

/// do_traceback — traceback().
pub unsafe fn do_traceback(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let skip = crate::mainutils::coerce::asInteger(CAR(args));
        if skip == crate::sexp::ffi::NA_INTEGER || skip < 0 {
            errorcall(call, b"invalid 'skip' value\x00".as_ptr() as *const c_char);
        }
        R_GetTracebackOnly(skip)
    }
}

// ---------------------------------------------------------------------------
// R_GetCurrentSrcref (simplified)
// ---------------------------------------------------------------------------

/// R_GetCurrentSrcref — get the current source reference.
pub unsafe fn R_GetCurrentSrcref(skip: c_int) -> SEXP {
    unsafe {
        // Simplified: no source references in Rust port yet
        globals::R_NilValue()
    }
}

/// R_GetSrcFilename — get source filename from a srcref.
pub unsafe fn R_GetSrcFilename(_srcref: SEXP) -> SEXP {
    unsafe { Rf_mkString(b"\x00".as_ptr() as *const c_char) }
}

#[cfg(test)]
mod owning_context_tests {
    use super::*;

    #[test]
    fn owned_traceback_snapshots_survive_context_replacement_teardown_and_collecting_duplicates() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let nil = globals::R_NilValue();
            let first = crate::sexp::constructors::Rf_lang2(
                crate::sexp::symbol::Rf_install(c"trace_first".as_ptr()),
                nil,
            );
            let first = owner.sexp(first).unwrap().into_owned().unwrap();
            let second = crate::sexp::constructors::Rf_lang2(
                crate::sexp::symbol::Rf_install(c"trace_second".as_ptr()),
                nil,
            );
            let second = owner.sexp(second).unwrap().into_owned().unwrap();
            let old = crate::sexp::context::Rf_begincontext_in(
                instance,
                crate::sexp::context::ctxt_flags::CTXT_FUNCTION,
                first.as_raw(),
                nil,
                nil,
                None,
                nil,
                nil,
            );
            let recent = crate::sexp::context::Rf_begincontext_in(
                instance,
                crate::sexp::context::ctxt_flags::CTXT_FUNCTION,
                second.as_raw(),
                nil,
                nil,
                None,
                nil,
                nil,
            );
            (*recent)
                .srcref
                .replace_from_raw_in(instance, crate::sexp::constructors::Rf_ScalarInteger(71));
            let old_cell = crate::sexp::context::retain_context_in(instance, old).unwrap();
            let recent_cell = crate::sexp::context::retain_context_in(instance, recent).unwrap();
            drop(first);
            drop(second);
            let invoked = std::rc::Rc::new(std::cell::Cell::new(false));
            let observed = invoked.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if observed.replace(true) {
                    return;
                }
                (*old_cell.get()).call.replace_from_raw_in(instance, nil);
                (*recent_cell.get()).call.replace_from_raw_in(instance, nil);
                (*recent_cell.get())
                    .srcref
                    .replace_from_raw_in(instance, nil);
                (*instance).context_stack.clear();
                crate::sexp::gengc::full_gc_in(instance);
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let trace = R_GetTracebackOnly(0);
            let trace = owner.sexp(trace).unwrap();
            assert!(invoked.get());
            assert_eq!(LENGTH(trace.as_raw()), 2);
            let recent_call = CAR(trace.as_raw());
            let old_call = CAR(CDR(trace.as_raw()));
            assert_eq!(
                CStr::from_ptr(CHAR_local(PRINTNAME(CAR(recent_call)))).to_bytes(),
                b"trace_second"
            );
            assert_eq!(
                CStr::from_ptr(CHAR_local(PRINTNAME(CAR(old_call)))).to_bytes(),
                b"trace_first"
            );
            let source = crate::sexp::attrib_core::getAttrib(
                recent_call,
                crate::sexp::symbol::Rf_install(c"srcref".as_ptr()),
            );
            assert_eq!(owner.sexp(source).unwrap().integer_elt(0), Some(71));
            owner.full_gc().unwrap();
            assert_eq!(LENGTH(trace.as_raw()), 2);
            assert!(crate::sexp::context::R_GlobalContext_in(instance).is_null());
        });
    }
}
