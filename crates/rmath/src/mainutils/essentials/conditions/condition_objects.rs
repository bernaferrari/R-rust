use super::*;

// ---------------------------------------------------------------------------
// Complete error system: condition handling
// ---------------------------------------------------------------------------

/// R's `conditionMessage(cond)` — get message from condition object.
pub unsafe fn do_conditionMessage(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let cond = CAR(args);
        if cond.is_null() || cond == R_NilValue() {
            return Rf_mkString(c"".as_ptr());
        }
        // Stock conditionMessage.condition is `c$message`: the element of
        // the condition list whose name is "message" (not an attribute).
        if let Some(msg) =
            crate::mainutils::essentials::tables::list_element_by_name(cond, "message")
        {
            if !msg.is_null() && msg != R_NilValue() && TYPEOF(msg) == SEXPTYPE::STRSXP {
                return msg;
            }
        }
        // Fall back to the explicit "message" attribute for non-list
        // condition representations.
        let msg_sym = Rf_install(c"message".as_ptr());
        let msg = crate::sexp::attrib_core::getAttrib(cond, msg_sym);
        if !msg.is_null() && msg != R_NilValue() && TYPEOF(msg) == SEXPTYPE::STRSXP {
            return msg;
        }
        Rf_mkString(c"".as_ptr())
    }
}

/// R's `conditionCall(cond)` — get call from condition object.
pub unsafe fn do_conditionCall(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let cond = CAR(args);
        if cond.is_null() || cond == R_NilValue() {
            return R_NilValue();
        }
        // Upstream conditionCall.default reads the `call` field and keeps
        // it only when it is a language object:
        // `if (is.null(c <- cond$call) || !is.language(c)) NULL else c`.
        let call = crate::mainutils::essentials::tables::list_element_by_name(cond, "call")
            .unwrap_or(R_NilValue());
        let t = TYPEOF(call);
        let is_language = t == SEXPTYPE::LANGSXP.as_c_int()
            || t == SEXPTYPE::SYMSXP.as_c_int()
            || t == SEXPTYPE::EXPRSXP.as_c_int();
        if !call.is_null() && call != R_NilValue() && is_language {
            return call;
        }
        R_NilValue()
    }
}

/// R's `simpleError(message, call)` — create a simple error condition.
pub unsafe fn do_simpleError(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { simple_constructor(args, &["simpleError", "error", "condition"]) }
}

/// Own selections before coercion, dispatch, or a constructor can reenter R.
unsafe fn simple_constructor(args: SEXP, classes: &[&str]) -> SEXP {
    use crate::sexp::{
        object::{SexpError, SexpResult},
        owner::{OwnerToken, with_runtime},
    };
    let result = (|| -> SexpResult<_> {
        let owner = unsafe { OwnerToken::current()? }
            .weak_owner()
            .ok_or(SexpError::RootUnavailable)?;
        with_runtime(&owner, |access| -> SexpResult<_> {
            let args = access.domain().wrap(args)?.into_owned()?;
            let domain = access.domain();
            let message = if args.is_nil() {
                domain.nil()
            } else {
                args.try_car()?.into_owned()?
            };
            let tail = if args.is_nil() {
                domain.nil()
            } else {
                args.try_cdr()?.into_owned()?
            };
            let call = if tail.is_nil() {
                domain.nil()
            } else {
                tail.try_car()?.into_owned()?
            };
            let base = access.with_native(|original| {
                let value = unsafe { crate::sexp::globals::R_BaseEnv() };
                original.require_active()?;
                original.sexp(value)?.into_owned()
            })?;
            let function = access.with_native(|original| {
                let value = unsafe { Rf_install(c"as.character".as_ptr()) };
                original.require_active()?;
                original.sexp(value)?.into_owned()
            })?;
            let allocator = access.allocator(&domain)?;
            // Evaluated promises preserve language values as data during the
            // generic call, and keep the original selected message rooted.
            let argument = allocator.evaluated_promise(&message, &base)?;
            let arguments = allocator.pairlist_cell(&argument, &domain.nil(), &domain.nil())?;
            let coercion = allocator.call(&function, &arguments)?;
            let converted = access.with_native(|original| {
                let value = unsafe { crate::eval::eval::Rf_eval(coercion.as_raw(), base.as_raw()) };
                original.require_active()?;
                original.sexp(value)?.into_owned()
            })?;
            let names = access.with_native(|original| {
                let value = unsafe { crate::sexp::attrib_core::R_NamesSymbol() };
                original.require_active()?;
                original.sexp(value)?.into_owned()
            })?;
            let class = access.with_native(|original| {
                let value = unsafe { crate::sexp::attrib_core::R_ClassSymbol() };
                original.require_active()?;
                original.sexp(value)?.into_owned()
            })?;
            super::simple_owned::build(access, &converted, &call, &names, &class, classes)
        })?
    })();
    result
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        .as_raw()
}

/// R's `errorCondition(message, ..., class = NULL, call = NULL)`.
///
/// Upstream base defines this as an R-level closure:
/// `structure(list(message = as.character(message), call = call, ...),
/// class = c(class, "error", "condition"))`. The port implements it as a
/// builtin; arguments arrive already evaluated with their tags, so the
/// message/class/call formals are picked out by tag (the first untagged
/// value is the message). zeallot's error paths build their conditions
/// through this.
pub unsafe fn do_errorCondition(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { error_or_warning_condition(args, "error") }
}

/// R's `warningCondition(message, ..., class = NULL, call = NULL)` —
/// see [`do_errorCondition`].
pub unsafe fn do_warningCondition(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { error_or_warning_condition(args, "warning") }
}

unsafe fn error_or_warning_condition(args: SEXP, kind: &str) -> SEXP {
    unsafe {
        let mut message: SEXP = R_NilValue();
        let mut class_arg: SEXP = R_NilValue();
        let mut call_arg: SEXP = R_NilValue();
        // Upstream keeps the `...` fields after message/call:
        // `structure(list(message = ..., call = call, ...), class = ...)`
        // — errorCondition("m", code = 42L) exposes `condition$code`.
        let mut extras: Vec<(String, SEXP)> = Vec::new();
        let mut cur = args;
        while !cur.is_null() && cur != R_NilValue() {
            let tag = crate::sexp::accessors::TAG(cur);
            let value = crate::sexp::accessors::CAR(cur);
            let name = if tag.is_null() || tag == R_NilValue() {
                String::new()
            } else {
                symbol_name(tag).unwrap_or_default()
            };
            if name == "class" {
                class_arg = value;
            } else if name == "call" {
                call_arg = value;
            } else if (name == "message" || name.is_empty()) && message == R_NilValue() {
                message = value;
            } else {
                extras.push((name, value));
            }
            cur = crate::sexp::accessors::CDR(cur);
        }
        let _extras_guards: Vec<_> = extras.iter().map(|(_, v)| protect(*v)).collect();

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, (2 + extras.len()) as i64);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let message_str = if message.is_null() || message == R_NilValue() {
            crate::mainutils::coerce::coerceVector(R_NilValue(), SEXPTYPE::STRSXP.as_c_int())
        } else {
            crate::mainutils::coerce::coerceVector(message, SEXPTYPE::STRSXP.as_c_int())
        };
        let _message_guard = protect(message_str);
        SET_VECTOR_ELT(result, 0, message_str);
        SET_VECTOR_ELT(result, 1, call_arg);
        for (i, (_, value)) in extras.iter().enumerate() {
            SET_VECTOR_ELT(result, (2 + i) as i64, *value);
        }

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, (2 + extras.len()) as i64);
        if !names.is_null() {
            let mut fields: Vec<&str> = vec!["message", "call"];
            fields.extend(extras.iter().map(|(name, _)| name.as_str()));
            for (i, n) in fields.iter().enumerate() {
                let cstr = CString::new(*n).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                if !charsxp.is_null() {
                    crate::sexp::accessors::SET_STRING_ELT(names, i as R_xlen_t, charsxp);
                }
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
        }

        let extra_classes = if class_arg.is_null()
            || class_arg == R_NilValue()
            || crate::sexp::accessors::TYPEOF(class_arg) != SEXPTYPE::STRSXP
        {
            0
        } else {
            crate::sexp::accessors::LENGTH(class_arg)
        };
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, extra_classes as i64 + 2);
        if !class.is_null() {
            let mut slot = 0usize;
            for i in 0..extra_classes as isize {
                let src = crate::sexp::accessors::STRING_ELT(class_arg, i as i64);
                if !src.is_null() {
                    crate::sexp::accessors::SET_STRING_ELT(class, slot as R_xlen_t, src);
                    slot += 1;
                }
            }
            for name in [kind, "condition"] {
                let cstr = CString::new(name).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                if !charsxp.is_null() {
                    crate::sexp::accessors::SET_STRING_ELT(class, slot as R_xlen_t, charsxp);
                    slot += 1;
                }
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"class".as_ptr()), class);
        }
        result
    }
}

/// R's `simpleWarning(message, call)` — create a simple warning condition.
pub unsafe fn do_simpleWarning(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { simple_constructor(args, &["simpleWarning", "warning", "condition"]) }
}

pub(super) unsafe fn native_simple_condition(args: SEXP) -> SEXP {
    unsafe { simple_constructor(args, &["simpleCondition", "condition"]) }
}
