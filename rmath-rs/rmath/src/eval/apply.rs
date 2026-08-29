//! Application of closures, specials, and builtins.

use std::os::raw::c_int;

use crate::sexp::accessors::{CAR, CDR, CLOENV, PRINTNAME, TYPEOF};
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory::RArena;
use crate::sexp::memory_ext::vmaxget;
use crate::sexp::object::Sexp;

use super::attrib_core::{R_ClassSymbol, getAttrib, isObject};
use super::eval::eval_safe;
use super::primitive::{PrimitiveDescriptor, get_primfun, primitive_controls_visibility};

/// Safe closure application.
pub(crate) fn apply_closure_safe<'a>(
    fun: Sexp<'a>,
    call: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    let raw_result = unsafe {
        super::closure::applyClosure(
            call.as_raw(),
            fun.as_raw(),
            args.as_raw(),
            rho.as_raw(),
            R_NilValue(),
            TRUE,
        )
    };
    Ok(unsafe { Sexp::from_raw_unchecked(raw_result) })
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

fn primitive_call_name(primitive: Option<PrimitiveDescriptor<'_>>, call: Sexp<'_>) -> String {
    primitive
        .map(|primitive| primitive.name.to_string())
        .unwrap_or_else(|| call_head_name(call))
}

/// Safe special form application.
pub(crate) fn apply_special_safe<'a>(
    fun: Sexp<'a>,
    call: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    let _vmax = unsafe { vmaxget() };
    let primitive = PrimitiveDescriptor::from_sexp(fun);
    let flag = primitive.map(|primitive| primitive.print_flag).unwrap_or(0);
    let op_name = primitive_call_name(primitive, call);
    set_visibility_for_print_flag(flag);

    let tmp = if let Some(primfun) = primitive.and_then(|primitive| primitive.fun) {
        crate::mainutils::errors::attribute_handler_errors(call.as_raw(), || unsafe {
            primfun(call.as_raw(), fun.as_raw(), args.as_raw(), rho.as_raw())
        })
    } else {
        crate::mainutils::errors::attribute_handler_errors(call.as_raw(), || unsafe {
            super::special::do_special_dispatch(
                call.as_raw(),
                fun.as_raw(),
                args.as_raw(),
                rho.as_raw(),
            )
        })
    };

    finish_application(
        tmp,
        flag,
        &op_name,
        VisibilityRestore::UnlessPrimitiveControlsIt,
    )
}

#[derive(Clone, Copy)]
struct PrimitiveCall<'a> {
    fun: Sexp<'a>,
    call: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
}

impl<'a> PrimitiveCall<'a> {
    fn eval_args(self) -> SEXP {
        unsafe {
            super::dispatch::evalList(
                self.args.as_raw(),
                self.rho.as_raw(),
                self.call.as_raw(),
                -1,
            )
        }
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

fn finish_application<'a>(
    result: SEXP,
    flag: c_int,
    op_name: &str,
    restore: VisibilityRestore,
) -> Result<Sexp<'a>, String> {
    let should_restore = match restore {
        VisibilityRestore::Always => flag < 2,
        VisibilityRestore::UnlessPrimitiveControlsIt => {
            flag < 2 && !primitive_controls_visibility(op_name)
        }
    };
    if should_restore {
        set_visibility_for_print_flag(flag);
    }
    Ok(unsafe { Sexp::from_raw_unchecked(result) })
}

/// Safe builtin function application.
pub(crate) fn apply_builtin_safe<'a>(
    fun: Sexp<'a>,
    call: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    let _vmax = unsafe { vmaxget() };
    let primitive = PrimitiveDescriptor::from_sexp(fun);
    let flag = primitive.map(|primitive| primitive.print_flag).unwrap_or(0);
    set_visibility_for_print_flag(flag);

    let frame = PrimitiveCall {
        fun,
        call,
        args,
        rho,
    };
    let op_name = primitive_call_name(primitive, call);

    if let Some((result, restore)) = apply_unevaluated_builtin(frame, &op_name) {
        return finish_application(result, flag, &op_name, restore);
    }

    let evaled_args = frame.eval_args();
    let result = apply_evaluated_builtin(frame, &op_name, evaled_args);
    finish_application(
        result,
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
    let result =
        crate::mainutils::errors::attribute_handler_errors(frame.call.as_raw(), || unsafe {
            (builtin.handler)(
                frame.call.as_raw(),
                frame.fun.as_raw(),
                frame.args.as_raw(),
                frame.rho.as_raw(),
            )
        });
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
        return crate::mainutils::errors::attribute_handler_errors(call.as_raw(), || unsafe {
            handler(call.as_raw(), fun.as_raw(), evaled_args, rho.as_raw())
        });
    }

    // Try S3/S4 dispatch for primitive names that are not handled directly.
    if let Some(s3_result) = try_s3_dispatch(op_name, fun, call, args, rho, evaled_args) {
        s3_result
    } else if let Some(s4_result) = try_s4_dispatch(op_name, fun, call, args, rho, evaled_args) {
        s4_result
    } else if let Some(primfun) = unsafe { get_primfun(fun.as_raw()) } {
        crate::mainutils::errors::attribute_handler_errors(call.as_raw(), || unsafe {
            primfun(call.as_raw(), fun.as_raw(), evaled_args, rho.as_raw())
        })
    } else {
        std::panic::panic_any(crate::sexp::context::RError {
            message: format!("builtin function '{op_name}' is not implemented"),
        });
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

        let defrho = if TYPEOF(fun.as_raw()) == SEXPTYPE::CLOSXP {
            CLOENV(fun.as_raw())
        } else {
            rho.as_raw()
        };
        let method_match = crate::mainutils::objects::lookup_s3_method_for_classes(
            op_name,
            klass,
            rho.as_raw(),
            rho.as_raw(),
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

// ---------------------------------------------------------------------------
// Real parent.frame() implementation
// ---------------------------------------------------------------------------

/// Walk up the call stack to find the parent frame environment.
///
/// In R, parent.frame(n) returns the environment n frames up the call stack.
/// This implementation uses the RCNTXT context stack.
fn do_parent_frame_impl(n: c_int, rho: SEXP) -> SEXP {
    unsafe {
        if n <= 0 {
            return rho;
        }

        // Walk up the context stack to find the parent environment
        let ctx = super::runtime::global_context();
        if ctx.is_null() {
            return super::runtime::global_env();
        }

        // Use the R_findParentContext helper
        let parent_ctx = super::context::R_findParentContext(ctx, n);
        if !parent_ctx.is_null() && !(*parent_ctx).cloenv.is_null() {
            return (*parent_ctx).cloenv;
        }

        // Fallback: walk environment chain using Sexp enclos()
        let env = Sexp::from_raw_unchecked(rho);
        let mut current = env;
        for _ in 0..n {
            match current.try_enclos() {
                Ok(enclos) if enclos.is_environment() => current = enclos,
                _ => return super::runtime::global_env(),
            }
        }
        current.as_raw()
    }
}

// ---------------------------------------------------------------------------
// Real source() implementation — parse and evaluate R source files
// ---------------------------------------------------------------------------

/// Parse and evaluate an R source file.
///
/// This reads the file, parses it using the R parser, and evaluates
/// each expression in the given environment.
fn do_source_impl(file_path: &str, rho: SEXP) -> Result<SEXP, String> {
    unsafe {
        // Read the file
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| format!("cannot open file '{}': {}", file_path, e))?;

        // Parse the file contents
        let mut arena = RArena::new();
        let parsed = super::parser::parse(&content, &mut arena)
            .map_err(|e| format!("parse error in '{}': {}", file_path, e))?;

        // Evaluate each expression in the parsed program
        // The parser returns a pairlist of expressions (or a single expression)
        let env = Sexp::from_raw_unchecked(rho);

        // If it's a pairlist (LANGSXP with CDR), evaluate each element
        if !parsed.is_null() && TYPEOF(parsed) == SEXPTYPE::LANGSXP {
            // Check if this is a "{" block - evaluate each element
            let head = CAR(parsed);
            if !head.is_null() && TYPEOF(head) == SEXPTYPE::SYMSXP {
                let sym_name = PRINTNAME(head);
                if !sym_name.is_null() {
                    let name_str = crate::sexp::accessors::CHAR(sym_name);
                    if !name_str.is_null() {
                        if std::ffi::CStr::from_ptr(name_str).to_str() == Ok("{") {
                            // It's a block — evaluate each sub-expression
                            let mut current = CDR(parsed);
                            let mut last_result = R_NilValue();
                            while !current.is_null() && current != R_NilValue() {
                                let expr = CAR(current);
                                if !expr.is_null() {
                                    let sexp_expr = Sexp::from_raw_unchecked(expr);
                                    last_result = eval_safe(sexp_expr, env)
                                        .map_err(|e| format!("error in '{}': {}", file_path, e))?
                                        .as_raw();
                                }
                                current = CDR(current);
                            }
                            return Ok(last_result);
                        }
                    }
                }
            }
        }

        // Single expression or non-block
        let sexp_expr = Sexp::from_raw_unchecked(parsed);
        let result =
            eval_safe(sexp_expr, env).map_err(|e| format!("error in '{}': {}", file_path, e))?;

        Ok(result.as_raw())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::parser;
    use crate::sexp::envir::defineVar;
    use crate::sexp::session::RSession;
    use crate::sexp::symbol::Rf_install;

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

            let mut arena = RArena::new();
            let expr = parser::parse("not_ported_builtin()", &mut arena).expect("parse call");
            let env = Sexp::from_raw_unchecked(crate::eval::runtime::global_env());
            let err = eval_safe(Sexp::from_raw_unchecked(expr), env)
                .expect_err("unknown builtin should not evaluate to NULL");

            assert!(err.contains("builtin function 'not_ported_builtin' is not implemented"));
        }
    }

    #[test]
    fn aliased_special_dispatches_through_bound_value() {
        let _session = RSession::new();
        unsafe {
            let mut arena = RArena::new();
            let env = Sexp::from_raw_unchecked(crate::eval::runtime::global_env());

            // h <- `[` binds the subset primitive under an unrelated name;
            // h(x, 2) must dispatch on that bound value (upstream dispatches
            // the primitive's funtab entry), not on the call-head name.
            let binding = parser::parse("h <- `[`", &mut arena).expect("parse alias binding");
            eval_safe(Sexp::from_raw_unchecked(binding), env).expect("bind alias");

            let call =
                parser::parse("h(c(10, 20, 30), 2)", &mut arena).expect("parse aliased call");
            let result = eval_safe(Sexp::from_raw_unchecked(call), env)
                .expect("aliased subset call dispatches");

            assert_eq!(*crate::sexp::accessors::REAL(result.as_raw()), 20.0);
        }
    }
}
