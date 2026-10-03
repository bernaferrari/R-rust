//! Application of closures, specials, and builtins.

use std::os::raw::c_int;

use crate::sexp::accessors::{CAR, CDR, CHAR, CLOENV, FORMALS, PRINTNAME, TAG, TYPEOF};

use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;

use crate::sexp::memory_ext::vmaxget;
use crate::sexp::object::{SessionNodeFactory, Sexp};

use super::attrib_core::{R_ClassSymbol, getAttrib, isObject};
use super::eval::eval_safe;
use super::primitive::{
    PrimitiveDescriptor, get_primfun, internal_result_invisible, primitive_controls_visibility,
};

/// Safe closure application.
pub(crate) fn apply_closure_safe<'a>(
    fun: Sexp<'a>,
    call: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    let frame = PrimitiveCall::new(fun, call, args, rho)?;
    let raw_result = unsafe {
        super::closure::applyClosure(
            frame.call.as_raw(),
            frame.fun.as_raw(),
            frame.args.as_raw(),
            frame.rho.as_raw(),
            frame.factory.nil().as_raw(),
            TRUE,
        )
    };
    frame
        .factory
        .wrap(raw_result)
        .map_err(|error| error.to_string())
}

fn call_head_name(call: Sexp<'_>) -> String {
    unsafe {
        let fun_sym = crate::sexp::accessors::CAR(call.as_raw());
        let pname = crate::sexp::accessors::PRINTNAME(fun_sym);
        if pname.is_null() {
            return String::new();
        }
        let s = crate::sexp::accessors::CHAR(pname);
        if s.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(s)
                .to_str()
                .map(str::to_string)
                .unwrap_or_default()
        }
    }
}

fn primitive_call_name(
    primitive: Option<PrimitiveDescriptor<'_>>,
    fun: Sexp<'_>,
    call: Sexp<'_>,
) -> String {
    // Prefer identity carried by the value, including portable helpers with
    // session-local negative offsets. Call spelling is only a legacy fallback.
    if let Some(primitive) = primitive {
        if !primitive.name.is_empty() && primitive.name != "unknown" {
            return primitive.name.to_string();
        }
    }
    if let Some(name) = super::primitive::portable_primitive_name(fun) {
        return name;
    }
    if let Some(name) = namespace_lookup_name(call.clone()) {
        return name;
    }
    call_head_name(call)
}

/// If `call` is a `pkg::name` / `pkg:::name` lookup, return `name`.
fn namespace_lookup_name(call: Sexp<'_>) -> Option<String> {
    unsafe {
        let head = crate::sexp::accessors::CAR(call.as_raw());
        if crate::sexp::ffi::SEXPTYPE::LANGSXP != crate::sexp::accessors::TYPEOF(head) {
            return None;
        }
        let op = crate::sexp::accessors::CAR(head);
        let op_name = crate::sexp::accessors::PRINTNAME(op);
        if op_name.is_null() {
            return None;
        }
        let op_chars = crate::sexp::accessors::CHAR(op_name);
        if op_chars.is_null() {
            return None;
        }
        let op_str = std::ffi::CStr::from_ptr(op_chars).to_str().unwrap_or("");
        if op_str != "::" && op_str != ":::" {
            return None;
        }
        // `::` call shape is `(:: pkg name)`: CAR(head) is the `::`
        // symbol, CADR(head) is the package, CADDR(head) is the name.
        let name_sym = crate::sexp::accessors::CAR(crate::sexp::accessors::CDR(
            crate::sexp::accessors::CDR(head),
        ));
        if name_sym.is_null() {
            return None;
        }
        let pname = crate::sexp::accessors::PRINTNAME(name_sym);
        if pname.is_null() {
            return None;
        }
        let chars = crate::sexp::accessors::CHAR(pname);
        if chars.is_null() {
            return None;
        }
        let name = std::ffi::CStr::from_ptr(chars).to_str().unwrap_or("");
        if name.is_empty() {
            return None;
        }
        Some(name.to_string())
    }
}

/// Whether a builtin must receive the source call's unevaluated arguments.
///
/// Bytecode callers need the same name resolution as the normal application
/// path. In particular, a primitive may not expose a usable `PRIMNAME`, while
/// its call head still identifies namespace operators such as `::`.
pub(crate) fn builtin_requires_raw_args(fun: Sexp<'_>, call: Sexp<'_>) -> bool {
    let primitive = PrimitiveDescriptor::from_sexp(fun.clone());
    let op_name = primitive_call_name(primitive, fun, call);
    super::builtin::unevaluated_builtin_handler(&op_name).is_some()
}

/// Safe special form application.
pub(crate) fn apply_special_safe<'a>(
    fun: Sexp<'a>,
    call: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    let frame = PrimitiveCall::new(fun, call, args, rho)?;
    let fun = frame.fun.clone();
    let call = frame.call.clone();
    let args = frame.args.clone();
    let rho = frame.rho.clone();
    let _vmax = unsafe { vmaxget() };
    let primitive = PrimitiveDescriptor::from_sexp(fun.clone());
    let flag = descriptor_print_flag(primitive.as_ref());
    let op_name = primitive_call_name(primitive.clone(), fun.clone(), call.clone());
    set_visibility_for_print_flag(flag);

    let tmp = if let Some(primfun) = primitive.and_then(|primitive| primitive.fun) {
        crate::mainutils::errors::attribute_handler_errors(call.clone().as_raw(), || unsafe {
            primfun(call.as_raw(), fun.as_raw(), args.as_raw(), rho.as_raw())
        })
    } else {
        crate::mainutils::errors::attribute_handler_errors(call.clone().as_raw(), || unsafe {
            super::special::do_special_dispatch(
                call.as_raw(),
                fun.as_raw(),
                args.as_raw(),
                rho.as_raw(),
            )
        })
    };

    finish_application(
        frame.factory.wrap(tmp).map_err(|error| error.to_string())?,
        flag,
        &op_name,
        VisibilityRestore::UnlessPrimitiveControlsIt,
    )
}

#[derive(Clone)]
struct PrimitiveCall<'a> {
    fun: Sexp<'a>,
    call: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
    factory: SessionNodeFactory<'a>,
}

impl<'a> PrimitiveCall<'a> {
    fn new(fun: Sexp<'a>, call: Sexp<'a>, args: Sexp<'a>, rho: Sexp<'a>) -> Result<Self, String> {
        let checked_factory = rho
            .node_factory()
            .or_else(|_| fun.node_factory())
            .or_else(|_| call.node_factory())
            .or_else(|_| args.node_factory());
        let factory = if let Ok(factory) = checked_factory {
            factory
        } else {
            // SAFETY: raw evaluator entry points retain the active owner for
            // the application lifetime. Normalize their internal raw views
            // into counted handles before any provider or primitive runs.
            SessionNodeFactory::new(
                unsafe { crate::sexp::owner::OwnerToken::current() }
                    .map_err(|error| error.to_string())?,
            )
        };
        factory
            .require_active()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            fun: factory
                .wrap(fun.as_raw())
                .map_err(|error| error.to_string())?,
            call: factory
                .wrap(call.as_raw())
                .map_err(|error| error.to_string())?,
            args: factory
                .wrap(args.as_raw())
                .map_err(|error| error.to_string())?,
            rho: factory
                .wrap(rho.as_raw())
                .map_err(|error| error.to_string())?,
            factory,
        })
    }

    fn eval_args(&self) -> Sexp<'a> {
        super::dispatch::evalList(
            self.args.clone(),
            self.rho.clone(),
            Some(self.call.clone()),
            -1,
        )
    }
}

#[derive(Clone, Copy)]
enum VisibilityRestore {
    Always,
    UnlessPrimitiveControlsIt,
}

fn set_visibility_for_print_flag(flag: c_int) {
    super::runtime::set_visible_for_print_flag(flag);
}

fn descriptor_print_flag(primitive: Option<&PrimitiveDescriptor<'_>>) -> c_int {
    // No FunTab entry: the handler owns R_Visible, same as PRIMPRINT >= 2.
    // Flag 0 would force the result visible after show / disassemble.
    primitive.map(|primitive| primitive.print_flag).unwrap_or(2)
}

fn finish_application<'a>(
    result: Sexp<'a>,
    flag: c_int,
    op_name: &str,
    restore: VisibilityRestore,
) -> Result<Sexp<'a>, String> {
    let should_restore = match restore {
        VisibilityRestore::Always => flag < 2,
        VisibilityRestore::UnlessPrimitiveControlsIt => {
            flag < 2
                && !primitive_controls_visibility(op_name)
                && !internal_result_invisible(op_name)
        }
    };
    // GNU eval.c overwrites R_Visible from PRIMPRINT when flag < 2, even
    // if the handler or a dispatched method (cat()) cleared it. `[` must
    // still auto-print NULL. Visibility-controlling names are excluded above.
    if should_restore {
        set_visibility_for_print_flag(flag);
    }
    Ok(result)
}

/// Safe builtin function application.
pub(crate) fn apply_builtin_safe<'a>(
    fun: Sexp<'a>,
    call: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    let frame = PrimitiveCall::new(fun, call, args, rho)?;
    let _vmax = unsafe { vmaxget() };
    let primitive = PrimitiveDescriptor::from_sexp(frame.fun.clone());
    let flag = descriptor_print_flag(primitive.as_ref());
    set_visibility_for_print_flag(flag);

    let op_name = primitive_call_name(primitive, frame.fun.clone(), frame.call.clone());

    if let Some((result, restore)) = apply_unevaluated_builtin(frame.clone(), &op_name) {
        return finish_application(
            frame
                .factory
                .wrap(result)
                .map_err(|error| error.to_string())?,
            flag,
            &op_name,
            restore,
        );
    }

    let evaled_args = frame.eval_args();
    let result = apply_evaluated_builtin(frame.clone(), &op_name, evaled_args.as_raw());
    finish_application(
        frame
            .factory
            .wrap(result)
            .map_err(|error| error.to_string())?,
        flag,
        &op_name,
        VisibilityRestore::UnlessPrimitiveControlsIt,
    )
}

/// Apply a builtin to already-evaluated argument values.
///
/// GNU SETTER_CALL / GETTER_CALL push evaluated lhs/rhs. Feeding those
/// through `apply_builtin_safe` would `evalList` them again and CALL a
/// LANGSXP value (`body(f) <- quote(standardGeneric("norm"))`).
pub(crate) fn apply_builtin_values_safe<'a>(
    fun: Sexp<'a>,
    call: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    let frame = PrimitiveCall::new(fun, call, args, rho)?;
    let _vmax = unsafe { vmaxget() };
    let primitive = PrimitiveDescriptor::from_sexp(frame.fun.clone());
    let flag = descriptor_print_flag(primitive.as_ref());
    // Already-forced arguments carry visibility (cat, message, ...elt).
    // A print-flag reset here makes withVisible report TRUE and
    // capture.output prints NULL.

    let op_name = primitive_call_name(primitive, frame.fun.clone(), frame.call.clone());
    let evaled_args = frame.args.clone();
    let result = apply_evaluated_builtin(frame.clone(), &op_name, evaled_args.as_raw());
    finish_application(
        frame
            .factory
            .wrap(result)
            .map_err(|error| error.to_string())?,
        flag,
        &op_name,
        VisibilityRestore::UnlessPrimitiveControlsIt,
    )
}

fn apply_unevaluated_builtin<'a>(
    frame: PrimitiveCall<'a>,
    op_name: &str,
) -> Option<(SEXP, VisibilityRestore)> {
    let builtin = super::builtin::unevaluated_builtin_handler(op_name)?;
    unsafe {
        check_prototype_first_arg(
            op_name,
            frame.args.clone().as_raw(),
            frame.call.clone().as_raw(),
        );
    }

    let result = crate::mainutils::errors::attribute_handler_errors(
        frame.call.clone().as_raw(),
        || unsafe {
            (builtin.handler)(
                frame.call.as_raw(),
                frame.fun.as_raw(),
                frame.args.as_raw(),
                frame.rho.as_raw(),
            )
        },
    );
    let restore = if builtin.restore_visibility_always {
        VisibilityRestore::Always
    } else {
        VisibilityRestore::UnlessPrimitiveControlsIt
    };
    Some((result, restore))
}

fn apply_evaluated_builtin<'a>(frame: PrimitiveCall<'a>, op_name: &str, evaled_args: SEXP) -> SEXP {
    let fun = frame.fun;
    let call = frame.call;
    let args = frame.args;
    let rho = frame.rho;
    if let Some(handler) = super::builtin::evaluated_builtin_handler(op_name) {
        unsafe {
            check_prototype_first_arg(op_name, evaled_args, call.clone().as_raw());
        }
        let result =
            crate::mainutils::errors::attribute_handler_errors(call.clone().as_raw(), || unsafe {
                handler(call.as_raw(), fun.as_raw(), evaled_args, rho.as_raw())
            });
        if internal_result_invisible(op_name) {
            super::runtime::set_visible(crate::sexp::ffi::FALSE);
        }
        return result;
    }

    // Try S3/S4 dispatch for primitive names that are not handled directly.
    if let Some(s3_result) = try_s3_dispatch(
        op_name,
        fun.clone(),
        call.clone(),
        args.clone(),
        rho.clone(),
        evaled_args,
    ) {
        s3_result
    } else if let Some(s4_result) = try_s4_dispatch(
        op_name,
        fun.clone(),
        call.clone(),
        args,
        rho.clone(),
        evaled_args,
    ) {
        s4_result
    } else if let Some(primfun) = unsafe { get_primfun(fun.clone().as_raw()) } {
        crate::mainutils::errors::attribute_handler_errors(call.clone().as_raw(), || unsafe {
            primfun(call.as_raw(), fun.as_raw(), evaled_args, rho.as_raw())
        })
    } else {
        std::panic::panic_any(crate::sexp::context::RError {
            message: format!("builtin function '{op_name}' is not implemented"),
        });
    }
}
unsafe fn check_prototype_first_arg(op_name: &str, evaled_args: SEXP, call: SEXP) {
    unsafe {
        match first_prototype_formal(op_name) {
            None => {}
            Some(None) => {
                if !evaled_args.is_null() && evaled_args != R_NilValue() {
                    let n = crate::sexp::constructors::Rf_length(evaled_args);
                    if n > 0 {
                        crate::mainutils::errors::errorcall_str(
                            call,
                            &format!(
                                "{n} argument{} passed to '{op_name}' which requires 0",
                                if n == 1 { "" } else { "s" }
                            ),
                        );
                    }
                }
            }
            Some(Some(formal)) if formal != "..." => {
                if evaled_args.is_null() || evaled_args == R_NilValue() {
                    return;
                }
                let Ok(cname) = std::ffi::CString::new(formal) else {
                    return;
                };
                crate::mainutils::seq::check1arg(evaled_args, call, cname.as_ptr());
            }
            Some(Some(_)) => {}
        }
    }
}

unsafe fn first_prototype_formal(op_name: &str) -> Option<Option<String>> {
    unsafe {
        let base = crate::sexp::globals::R_BaseEnv();
        let Ok(name_c) = std::ffi::CString::new(op_name) else {
            return None;
        };
        let symbol = crate::sexp::symbol::Rf_install(name_c.as_ptr());
        for registry in [".GenericArgsEnv", ".ArgsEnv"] {
            let Ok(reg_c) = std::ffi::CString::new(registry) else {
                continue;
            };
            let env = crate::sexp::envir::R_findVarInFrame(
                base,
                crate::sexp::symbol::Rf_install(reg_c.as_ptr()),
            );
            if env.is_null() || TYPEOF(env) != SEXPTYPE::ENVSXP {
                continue;
            }
            let mut proto = crate::sexp::envir::R_findVarInFrame(env, symbol);
            if proto.is_null() || proto == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            if TYPEOF(proto) == SEXPTYPE::PROMSXP {
                proto = crate::sexp::envir::forcePromise(proto);
            }
            if TYPEOF(proto) != SEXPTYPE::CLOSXP {
                continue;
            }
            let formals = FORMALS(proto);
            if formals.is_null() || formals == R_NilValue() {
                return Some(None);
            }
            // GNU Rf_check1arg is only used by one-argument primitives.
            // Multi-arg prototypes such as seq.int(from, to, ...) must
            // accept `seq.int(to=3, from=1)` via matchArgs.
            if CDR(formals) != R_NilValue() && !CDR(formals).is_null() {
                return None;
            }
            let tag = TAG(formals);
            if tag.is_null() || tag == R_NilValue() {
                return Some(None);
            }
            let pname = PRINTNAME(tag);
            if pname.is_null() {
                return Some(None);
            }
            let bytes = CHAR(pname);
            if bytes.is_null() {
                return Some(None);
            }
            return std::ffi::CStr::from_ptr(bytes)
                .to_str()
                .ok()
                .map(|s| Some(s.to_string()));
        }
        None
    }
}

// ---------------------------------------------------------------------------
// S3 Dispatch — method dispatch based on class attribute
// ---------------------------------------------------------------------------

/// Try to dispatch to an S3 method for a generic function.
///
/// If the first argument has a class attribute, look for `generic.class` method.
/// For example, if calling `print(x)` where `class(x) == "data.frame"`,
/// look for `print.data.frame` function.
fn try_s3_dispatch<'a>(
    op_name: &str,
    fun: Sexp<'a>,
    call: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
    evaled_args: SEXP,
) -> Option<SEXP> {
    unsafe {
        // Skip S3 dispatch for operators and special forms
        if op_name.starts_with(|c: char| !c.is_alphanumeric()) {
            return None;
        }
        if !crate::sexp::init::is_internal_generic_name(op_name) {
            return None;
        }

        // Skip if already a method call (contains a dot like "print.default")
        if op_name.contains('.') {
            return None;
        }

        // Get the first argument from evaled_args
        if evaled_args.is_null() || evaled_args == R_NilValue() {
            return None;
        }
        let first_arg = CAR(evaled_args);
        if first_arg.is_null() || first_arg == R_NilValue() {
            return None;
        }

        // Check if the object has a class attribute
        if isObject(first_arg) == FALSE {
            return None;
        }

        let klass = getAttrib(first_arg, R_ClassSymbol());
        if klass.is_null() || klass == R_NilValue() || TYPEOF(klass) != SEXPTYPE::STRSXP {
            return None;
        }

        let defrho = if TYPEOF(fun.clone().as_raw()) == SEXPTYPE::CLOSXP {
            CLOENV(fun.as_raw())
        } else {
            rho.clone().as_raw()
        };
        let method_match = crate::mainutils::objects::lookup_s3_method_for_classes(
            op_name,
            klass,
            rho.clone().as_raw(),
            rho.clone().as_raw(),
            defrho,
            false,
        )?;

        let method_val = method_match.method;
        let method_type = TYPEOF(method_val);
        if method_type == SEXPTYPE::CLOSXP {
            return Some(super::closure::applyClosure(
                call.as_raw(),
                method_val,
                evaled_args,
                rho.as_raw(),
                R_NilValue(),
                TRUE,
            ));
        }

        if method_type == SEXPTYPE::BUILTINSXP || method_type == SEXPTYPE::SPECIALSXP {
            if let Some(primfun) = get_primfun(method_val) {
                return Some(primfun(
                    call.as_raw(),
                    method_val,
                    evaled_args,
                    rho.as_raw(),
                ));
            }
        }

        None
    }
}

// ---------------------------------------------------------------------------
// S4 Dispatch — method dispatch for S4 formal classes
// ---------------------------------------------------------------------------

/// Try to dispatch to an S4 method.
///
/// S4 dispatch checks for formal class definitions and uses method dispatch.
/// This is a simplified implementation that falls back to S3 semantics.
fn try_s4_dispatch<'a>(
    _op_name: &str,
    _fun: Sexp<'a>,
    _call: Sexp<'a>,
    _args: Sexp<'a>,
    _rho: Sexp<'a>,
    _evaled_args: SEXP,
) -> Option<SEXP> {
    // S4 dispatch requires the methods package and formal class definitions.
    // For now, return None to fall through to the default behavior.
    // A full S4 implementation would:
    // 1. Check if the object has an S4 class (inherits from a formal class)
    // 2. Look up the method in the methods namespace
    // 3. Dispatch to the appropriate method
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::parser;
    use crate::sexp::envir::defineVar;
    use crate::sexp::session::RSession;
    use crate::sexp::symbol::Rf_install;

    fn parse_source<'session>(
        session: &'session RSession,
        source: &str,
    ) -> Result<Sexp<'session>, parser::ParseError> {
        let factory = crate::sexp::object::SessionNodeFactory::new(session.owner_token().unwrap());
        session.with_active_in(|owner| unsafe {
            crate::sexp::memory::with_arena_in(owner, |arena| parser::parse(source, arena, factory))
        })
    }

    #[test]
    fn unknown_builtin_reports_error_instead_of_null() {
        let _session = RSession::new();
        unsafe {
            let sym = Rf_install(c"not_ported_builtin".as_ptr());
            let prim = crate::eval::primitive::make_primitive_binding(
                "not_ported_builtin",
                SEXPTYPE::BUILTINSXP,
            );
            defineVar(sym, prim, crate::eval::runtime::global_env());

            let expr = parse_source(&_session, "not_ported_builtin()").expect("parse call");
            let env = _session.global_env().expect("checked global environment");
            let err =
                eval_safe(expr, env).expect_err("unknown builtin should not evaluate to NULL");

            assert!(err.contains("builtin function 'not_ported_builtin' is not implemented"));
        }
    }

    #[test]
    fn aliased_special_dispatches_through_bound_value() {
        let _session = RSession::new();
        unsafe {
            let env = _session.global_env().expect("checked global environment");

            // h <- `[` binds the subset primitive under an unrelated name;
            // h(x, 2) must dispatch on that bound value (upstream dispatches
            // the primitive's funtab entry), not on the call-head name.
            let binding = parse_source(&_session, "h <- `[`").expect("parse alias binding");
            eval_safe(binding, env.clone()).expect("bind alias");

            let call = parse_source(&_session, "h(c(10, 20, 30), 2)").expect("parse aliased call");
            let result = eval_safe(call, env).expect("aliased subset call dispatches");

            assert_eq!(*crate::sexp::accessors::REAL(result.as_raw()), 20.0);
        }
    }
}
