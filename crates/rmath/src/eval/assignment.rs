#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Assignment operations — ports R's assignment handling from eval.c.
//!
//! Handles `<-`, `<<-`, and `=` assignment operators.

use crate::sexp::accessors::ENCLOS;
use crate::sexp::accessors::{
    CADDR, CADR, CAR, CDDR, CDR, CHAR, INTEGER_ELT, LOGICAL_ELT, NAMED, PRINTNAME, REAL_ELT,
    SET_INTEGER_ELT, SET_LOGICAL_ELT, SET_REAL_ELT, SETTAG, STRING_ELT, TAG, TYPEOF, XLENGTH,
};

use crate::sexp::envir::Environment;
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::object::{SessionNodeFactory, Sexp};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::eval::Rf_eval;

fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RSignal::Error {
        message: msg.to_string(),
    })
}

/// Own the previous attribution and original runtime while a complex source
/// assignment executes. Restoration does not depend on ambient availability.
struct SourceAssignCall {
    previous: crate::sexp::instance::RuntimeValue,
    _call: Sexp<'static>,
    owner: crate::sexp::owner::OwnerPin,
}

impl SourceAssignCall {
    unsafe fn enter(call: SEXP) -> Self {
        let instance = crate::sexp::instance::with_required_current_instance(|instance| instance);
        let owner = unsafe { (*instance).runtime_owner.clone() }
            .unwrap_or_else(|| error("source assignment requires an original managed owner"))
            .pin()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let current = unsafe { crate::sexp::instance::RuntimeValue::from_raw_in(instance, call) };
        let call = current
            .owned()
            .unwrap_or_else(|| error("source assignment requires an initialized call"));
        let previous =
            unsafe { std::mem::replace(&mut (*instance).eval_state.current_expr, current) };
        Self {
            owner,
            previous,
            _call: call,
        }
    }
}

impl Drop for SourceAssignCall {
    fn drop(&mut self) {
        // Revocation rejects new work, but the original strong pin permits this
        // local owning-field cleanup even if the session was closed or dropped.
        unsafe {
            (*self.owner.as_ptr()).eval_state.current_expr = std::mem::take(&mut self.previous);
        }
    }
}

// ---------------------------------------------------------------------------
// do_set — handle assignment operators (<-, <<-, =)
// ---------------------------------------------------------------------------

/// Snapshot source arguments before the RHS can run collecting callbacks.
/// These are actual owning expressions, separate from the evaluated values.
struct FlatReplacement {
    function: Sexp<'static>,
    object: Sexp<'static>,
    extras: Vec<(Sexp<'static>, Sexp<'static>)>,
}

impl FlatReplacement {
    fn capture(lhs: &Sexp<'static>) -> crate::sexp::object::SexpResult<Option<Self>> {
        if lhs.typeof_() != SEXPTYPE::LANGSXP {
            return Ok(None);
        }
        let first = lhs.try_cdr()?;
        if first.is_nil() {
            return Ok(None);
        }
        let object = first.try_car()?.into_owned()?;
        if !object.is_symbol() {
            return Ok(None);
        }
        let function = lhs.try_car()?.into_owned()?;
        let mut extras = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut current = first.try_cdr()?;
        while !current.is_nil() {
            let node = current
                .allocation()?
                .link()
                .ok_or(crate::sexp::object::SexpError::StaleAllocation)?;
            seen.try_reserve(1)
                .map_err(|_| crate::sexp::object::SexpError::AllocationFailed {
                    object: "replacement source arguments",
                })?;
            if !seen.insert(node) {
                return Err(crate::sexp::object::SexpError::EvaluationFailed {
                    message: "cyclic replacement source arguments".into(),
                });
            }
            extras.try_reserve(1).map_err(|_| {
                crate::sexp::object::SexpError::AllocationFailed {
                    object: "replacement source arguments",
                }
            })?;
            extras.push((
                current.try_car()?.into_owned()?,
                current.try_tag()?.into_owned()?,
            ));
            current = current.try_cdr()?;
        }
        Ok(Some(Self {
            function,
            object,
            extras,
        }))
    }
}

/// Resolve a setter once, keeping its execution identity separate from the
/// source call exposed to the setter. Known primitive fast paths remain local.
fn apply_custom_replacement(
    syntax: &FlatReplacement,
    setter: &Sexp<'static>,
    object: &Sexp<'static>,
    rhs: &Sexp<'static>,
    rhs_expression: &Sexp<'static>,
    environment: &Sexp<'static>,
) -> crate::sexp::object::SexpResult<Option<Sexp<'static>>> {
    use crate::sexp::{
        object::SexpError,
        owner::{StoredOwner, with_runtime},
    };
    let owner = StoredOwner::from_value(environment)?
        .managed()
        .ok_or(SexpError::RootUnavailable)?;
    with_runtime(&owner, |access| {
        let function = access.with_native(|owner| {
            let function = if setter.is_symbol() {
                unsafe { crate::sexp::envir::find_fun_result(setter.clone(), environment.clone()) }
                    .map_err(|message| SexpError::EvaluationFailed { message })?
            } else {
                let result = unsafe { Rf_eval(setter.as_raw(), environment.as_raw()) };
                unsafe { Sexp::from_raw(result) }
            };
            let Some(function) = function else {
                return Err(SexpError::EvaluationFailed {
                    message: format!(
                        "could not find function \"{}\"",
                        unsafe { symbol_name(setter.as_raw()) }.unwrap_or_default(),
                    ),
                });
            };
            if function.is_closure() {
                return Ok(Some(owner.sexp(function.as_raw())?.into_owned()?));
            }
            if !function.is_primitive() {
                return Err(SexpError::EvaluationFailed {
                    message: "attempt to apply non-function".into(),
                });
            }
            // Immortal primitive projections need not belong to this session's
            // heap. Snapshot only their immutable dispatch identity before
            // allocation, then publish a fresh primitive in the original heap.
            let kind = function.typeof_();
            let descriptor =
                crate::eval::primitive::PrimitiveDescriptor::from_sexp(function.clone());
            let canonical_kind = descriptor.as_ref().is_some_and(|descriptor| {
                crate::eval::primitive::fun_tab_descriptor(descriptor.table_index).is_some_and(
                    |entry| crate::eval::primitive::primitive_kind_for_eval(entry.eval) == kind,
                )
            });
            let name = descriptor
                .map(|descriptor| descriptor.name.to_owned())
                .or_else(|| crate::eval::primitive::portable_primitive_name(function))
                .ok_or_else(|| SexpError::EvaluationFailed {
                    message: "replacement primitive has no dispatch identity".into(),
                })?;
            if canonical_kind
                && setter.is_symbol()
                && unsafe { symbol_name(setter.as_raw()) }.as_deref() == Some(name.as_str())
                && matches!(
                    name.as_str(),
                    "[<-"
                        | "[[<-"
                        | "$<-"
                        | "@<-"
                        | "names<-"
                        | "dim<-"
                        | "tsp<-"
                        | "length<-"
                        | "levels<-"
                        | "storage.mode<-"
                        | "mode<-"
                        | "dimnames<-"
                )
            {
                // These paths apply the selected primitive directly below;
                // they never resolve the setter symbol a second time.
                return Ok(None);
            }
            let primitive = unsafe { crate::eval::primitive::make_primitive_binding(&name, kind) };
            Ok(Some(owner.sexp(primitive)?.into_owned()?))
        })?;
        let Some(function) = function else {
            return Ok(None);
        };
        let domain = access.domain();
        let allocator = access.allocator(&domain)?;
        let (temporary, value_tag) = access.with_native(|owner| {
            Ok((
                owner
                    .sexp(unsafe { Rf_install(c"*tmp*".as_ptr()) })?
                    .into_owned()?,
                owner
                    .sexp(unsafe { Rf_install(c"value".as_ptr()) })?
                    .into_owned()?,
            ))
        })?;
        // GNU replacement promises retain syntax independently of PRVALUE.
        let value =
            allocator.evaluated_promise_with_expression(rhs_expression, environment, rhs)?;
        let mut source_arguments = allocator.pairlist_cell(&value, &domain.nil(), &value_tag)?;
        for (expression, tag) in syntax.extras.iter().rev() {
            source_arguments = allocator.pairlist_cell(expression, &source_arguments, tag)?;
        }
        access.require_active()?;
        unsafe {
            crate::sexp::accessors::SET_NAMED(object.as_raw(), 2);
            // The assignment still owns and returns the original RHS after
            // the setter returns; its formal must not mutate that value.
            crate::sexp::accessors::SET_NAMED(rhs.as_raw(), 2);
        }
        let target =
            allocator.evaluated_promise_with_expression(&temporary, environment, object)?;
        // GNU's replacement call substitutes an unnamed *tmp* argument,
        // even when the original getter's first argument was named.
        // applyClosure creates owning lazy promises from these expressions;
        // leaving extras as syntax also preserves its expansion of `...`.
        let arguments = allocator.pairlist_cell(&target, &source_arguments, &domain.nil())?;
        source_arguments = allocator.pairlist_cell(&temporary, &source_arguments, &domain.nil())?;
        let call = allocator.call(setter, &source_arguments)?;
        access.with_native(|owner| {
            let result = if function.is_closure() {
                let result = unsafe {
                    crate::eval::closure::applyClosure(
                        call.as_raw(),
                        function.as_raw(),
                        arguments.as_raw(),
                        environment.as_raw(),
                        domain.nil().as_raw(),
                        0,
                    )
                };
                owner.sexp(result)?.into_owned()?
            } else {
                let apply = if function.typeof_() == SEXPTYPE::SPECIALSXP {
                    crate::eval::apply::apply_special_safe
                } else {
                    crate::eval::apply::apply_builtin_safe
                };
                apply(
                    function.clone(),
                    call.clone(),
                    arguments.clone(),
                    environment.clone(),
                )
                .map_err(|message| SexpError::EvaluationFailed { message })?
                .into_owned()?
            };
            Ok(Some(result))
        })
    })?
}

/// Handle assignment: lhs <- rhs, lhs <<- rhs, lhs = rhs.
/// Matches C's `do_set()` in eval.c line 3565.
///
/// `op` carries PRIMVAL: 1 or 3 for `<-`/`=`, 2 for `<<-`.
pub unsafe fn do_set(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args == R_NilValue() || CDR(args) == R_NilValue() || CDDR(args) != R_NilValue() {
            error("wrong argument count for assignment");
        }

        let lhs = CAR(args);
        let primval = crate::mainutils::relop::PRIMVAL(op);

        match TYPEOF(lhs) {
            t if t == SEXPTYPE::STRSXP => {
                let sym = crate::mainutils::subset::installTrChar(STRING_ELT(lhs, 0));
                let rhs = Rf_eval(CADR(args), rho);
                let _rhs_guard = protect(rhs);
                assign_to_symbol(sym, rhs, primval, rho);
                rhs
            }
            t if t == SEXPTYPE::SYMSXP => {
                let rhs_expr = CADR(args);
                let rhs = Rf_eval(rhs_expr, rho);
                let _rhs_guard = protect(rhs);
                // `b <- a` and `c <- b <- a` share one value, so the object
                // must be NAMED 2 and `[[<-` duplicates. `a <- f()` is a fresh
                // call and stays at 1 (`named(m)` after `m <- matrix()` is 1).
                let rhs_head = if TYPEOF(rhs_expr) == SEXPTYPE::LANGSXP {
                    CAR(rhs_expr)
                } else {
                    R_NilValue()
                };
                let already_bound = TYPEOF(rhs_expr) == SEXPTYPE::SYMSXP
                    || (TYPEOF(rhs_head) == SEXPTYPE::SYMSXP
                        && matches!(
                            symbol_name(rhs_head).as_deref(),
                            Some("<-") | Some("=") | Some("<<-")
                        ));
                if already_bound && NAMED(rhs) == 1 {
                    crate::sexp::accessors::SET_NAMED(rhs, 2);
                }
                assign_to_symbol(lhs, rhs, primval, rho);
                rhs
            }
            t if t == SEXPTYPE::LANGSXP => {
                super::runtime::set_visible(FALSE);
                return applydefine(call, op, args, rho);
            }
            _ => {
                error("invalid (do_set) left-hand side to assignment");
            }
        }
    }
}

unsafe fn assign_to_symbol(sym: SEXP, value: SEXP, primval: i32, rho: SEXP) {
    unsafe {
        bind_assignment(sym, value, primval, rho);
        super::runtime::set_visible(FALSE);
    }
}

unsafe fn bind_assignment(sym: SEXP, value: SEXP, primval: i32, rho: SEXP) {
    let target_env = if primval == 2 {
        unsafe { ENCLOS(rho) }
    } else {
        rho
    };
    let (Some(sym), Some(value), Some(target_env)) = (
        unsafe { Sexp::from_raw(sym) },
        unsafe { Sexp::from_raw(value) },
        unsafe { Sexp::from_raw(target_env) },
    ) else {
        return;
    };
    let Ok(env) = Environment::new(target_env) else {
        return;
    };

    if primval == 2 {
        unsafe { env.set(sym, value) };
    } else if let Err(err) = unsafe { env.define(sym, value) } {
        error(&format!("failed to assign binding: {err}"));
    }
}

// ---------------------------------------------------------------------------
// evalseq — evaluate a sequence with assignment
// ---------------------------------------------------------------------------

/// Evaluate a sequence of expressions (for use in multi-expression bodies).
///
/// This is the equivalent of R's `evalseq()` in eval.c.
pub unsafe fn evalseq(expr: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }

        let mut result = R_NilValue();
        let mut current = expr;
        while !current.is_null() && current != R_NilValue() {
            result = Rf_eval(CAR(current), rho);
            current = CDR(current);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// applydefine — handle complex assignment (a[b] <- value)
// ---------------------------------------------------------------------------

/// Handle complex/subscript assignment (e.g., x[1] <- 5, x$name <- val,
/// x[i][j] <- val for nested cases).
///
/// Uses `evalseq` from missing.rs to recursively evaluate the LHS chain
/// for nested assignments, then walks back up applying replacement functions.
///
/// Ported from applydefine() in eval.c:3367.
pub unsafe fn applydefine(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let _pin = owner
            .pin()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let factory = owner.node_factory();
        let own = |raw| {
            factory
                .wrap(raw)
                .and_then(Sexp::into_owned)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        };
        let _call = own(call);
        let arguments = own(args);
        let environment = own(rho);
        let lhs = arguments
            .try_car()
            .and_then(Sexp::into_owned)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let rhs_expression = arguments
            .try_cdr()
            .and_then(|cell| cell.try_car())
            .and_then(Sexp::into_owned)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let flat = FlatReplacement::capture(&lhs)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let expr = lhs.as_raw();
        if expr.is_null() || expr == R_NilValue() {
            error("invalid complex assignment");
        }

        let rhs_owned = own(Rf_eval(rhs_expression.as_raw(), environment.as_raw()));
        owner
            .require_active()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let rhs = rhs_owned.as_raw();
        // GNU begincontext(CTXT_CCODE) happens after the RHS eval, so a
        // direct `[[<-` in the right-hand side keeps its own call.
        let _source_call = SourceAssignCall::enter(call);

        let primval = crate::mainutils::relop::PRIMVAL(op);
        let forcelocal = if primval == 1 || primval == 3 { 1 } else { 0 };

        if TYPEOF(CADR(expr)) == SEXPTYPE::LANGSXP {
            // GNU applydefine: evalseq the first argument, then
            // `*tmp*` replacement calls while the base is still a call.
            // Keep the cached RHS distinct from the expression observed by
            // setter promises. Outward setters receive GNU's *vtmp* code.
            crate::sexp::accessors::SET_NAMED(rhs, 2);
            let tmp_owned = own(Rf_install(c"*tmp*".as_ptr()));
            let modified_expression = own(Rf_install(c"*vtmp*".as_ptr()));
            let tmp_sym = tmp_owned.as_raw();
            let restore_tmp =
                restore_tmp::RestoreTmp::capture(environment.clone(), tmp_owned.clone())
                    .unwrap_or_else(|error| {
                        std::panic::panic_any(crate::sexp::context::RSignal::Error {
                            message: error.to_string(),
                        })
                    });
            restore_tmp
                .run(|| {
                    let mut lhs_expr = expr;
                    let mut chain = crate::eval::missing::evalseq(CADR(lhs_expr), rho, forcelocal);
                    let _chain_guard = protect(chain);
                    let mut current_rhs = rhs_owned.clone();
                    let mut current_expression = rhs_expression.clone();
                    while TYPEOF(CADR(lhs_expr)) == SEXPTYPE::LANGSXP {
                        let assign_fn = replacement_fun_head(CAR(lhs_expr));
                        if assign_fn == R_NilValue() {
                            break;
                        }
                        crate::sexp::envir::defineVar(tmp_sym, CAR(chain), rho);
                        let repl = replace_tmp_call(
                            &own(assign_fn),
                            &tmp_owned,
                            &own(CDDR(lhs_expr)),
                            &current_rhs,
                            &current_expression,
                            &environment,
                        )
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                        current_rhs = own(crate::eval::eval::Rf_eval(repl.as_raw(), rho));
                        owner.require_active().unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        });
                        current_expression = modified_expression.clone();
                        let next = CDR(chain);
                        if !next.is_null()
                            && next != R_NilValue()
                            && TYPEOF(next) == SEXPTYPE::LISTSXP
                        {
                            chain = next;
                        }
                        lhs_expr = CADR(lhs_expr);
                    }
                    if TYPEOF(lhs_expr) == SEXPTYPE::LANGSXP {
                        let assign_fn = replacement_fun_head(CAR(lhs_expr));
                        if assign_fn != R_NilValue() {
                            crate::sexp::envir::defineVar(tmp_sym, CAR(chain), rho);
                            let repl = replace_tmp_call(
                                &own(assign_fn),
                                &tmp_owned,
                                &own(CDDR(lhs_expr)),
                                &current_rhs,
                                &current_expression,
                                &environment,
                            )
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                            current_rhs = own(crate::eval::eval::Rf_eval(repl.as_raw(), rho));
                            owner.require_active().unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                        }
                    }

                    let var_sym = CADR(lhs_expr);
                    if !var_sym.is_null() && TYPEOF(var_sym) == SEXPTYPE::SYMSXP {
                        bind_assignment(var_sym, current_rhs.as_raw(), primval, rho);
                    }
                })
                .unwrap_or_else(|error| {
                    std::panic::panic_any(crate::sexp::context::RSignal::Error {
                        message: error.to_string(),
                    })
                });
            super::runtime::set_visible(FALSE);
            rhs
        } else {
            // Simple single-level assignment: x[i] <- val
            let lhs = expr;
            let func_sym = flat
                .as_ref()
                .map_or_else(|| CAR(lhs), |syntax| syntax.function.as_raw());

            let assign_fn_owned = own(replacement_fun_head(func_sym));
            let assign_fn = assign_fn_owned.as_raw();

            if assign_fn == R_NilValue() {
                return rhs;
            }

            let object_expression = flat
                .as_ref()
                .map_or_else(|| CADR(lhs), |syntax| syntax.object.as_raw());
            let target_env = if forcelocal == 0 && TYPEOF(object_expression) == SEXPTYPE::SYMSXP {
                let enc = crate::sexp::accessors::ENCLOS(rho);
                if enc.is_null() || enc == R_NilValue() {
                    rho
                } else {
                    enc
                }
            } else {
                rho
            };
            let target_owned = own(Rf_eval(object_expression, target_env));
            owner
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let target_expr = target_owned.as_raw();
            let var_sym_early = object_expression;
            let mut target_expr = target_expr;
            if TYPEOF(var_sym_early) == SEXPTYPE::SYMSXP
                && crate::sexp::envir::binding_is_locked_raw(rho, var_sym_early)
            {
                target_expr = crate::mainutils::duplicate::shallow_duplicate(target_expr);
                let _locked_dup = protect(target_expr);
            }

            let _target_guard = protect(target_expr);

            if let Some(syntax) = &flat {
                let target = own(target_expr);
                if let Some(result) = apply_custom_replacement(
                    syntax,
                    &assign_fn_owned,
                    &target,
                    &rhs_owned,
                    &rhs_expression,
                    &environment,
                )
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                {
                    bind_assignment(
                        syntax.object.as_raw(),
                        result.as_raw(),
                        primval,
                        environment.as_raw(),
                    );
                    owner
                        .require_active()
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    super::runtime::set_visible(FALSE);
                    return rhs_owned.as_raw();
                }
            }

            if symbol_name(func_sym).as_deref() == Some("$")
                && TYPEOF(target_expr) == SEXPTYPE::ENVSXP
                && let Some(field_sym) = dollar_field_symbol(CAR(CDDR(lhs)))
            {
                crate::sexp::envir::defineVar(field_sym, rhs, target_expr);
                super::runtime::set_visible(FALSE);
                return rhs;
            }

            let call_args = CDDR(lhs);
            let raw_subscript = matches!(symbol_name(func_sym).as_deref(), Some("@") | Some("$"));
            let factory = SessionNodeFactory::new(
                crate::sexp::owner::OwnerToken::current()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
            );
            // Keep the evaluated subscript chain owned throughout conversion,
            // replacement-call allocation and the final writeback.
            let slot_subs = if raw_subscript {
                None
            } else {
                let target = factory
                    .wrap(target_expr)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let mut target_links = super::dispatch::NamedArguments::new();
                target_links.retain(&target);
                let call_args = factory
                    .wrap(call_args)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let environment = factory
                    .wrap(rho)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                Some(super::dispatch::evalListKeepMissing(call_args, environment))
            };
            let evaluated_subs = slot_subs.as_ref().map_or(call_args, Sexp::as_raw);
            if slot_subs.is_some() {
                let mut cell = evaluated_subs;
                while !cell.is_null() && cell != R_NilValue() {
                    let sub = CAR(cell);
                    if !sub.is_null() && TYPEOF(sub) == SEXPTYPE::SYMSXP {
                        let name = crate::sexp::accessors::PRINTNAME(sub);
                        if !name.is_null() && name != R_NilValue() {
                            let value = factory
                                .wrap(crate::sexp::constructors::Rf_mkString(
                                    crate::sexp::accessors::CHAR(name),
                                ))
                                .unwrap_or_else(|error| {
                                    crate::sexp::context::r_error(error.to_string())
                                });
                            crate::sexp::accessors::SETCAR(cell, value.as_raw());
                        }
                    }
                    cell = CDR(cell);
                }
            }

            let result = if symbol_name(func_sym).as_deref() == Some("[")
                && let Some(result) = try_simple_vector_subassign(target_expr, evaluated_subs, rhs)
            {
                result
            } else {
                let arg_list = build_replacement_args(target_expr, evaluated_subs, rhs);
                let _arg_list_guard = protect(arg_list);
                let repl_call = crate::sexp::constructors::Rf_cons(assign_fn, arg_list);
                if !repl_call.is_null() {
                    crate::sexp::accessors::SET_TYPEOF(repl_call, SEXPTYPE::LANGSXP.as_c_int());
                }
                let _repl_call_guard = protect(repl_call);

                apply_replacement_call(assign_fn, repl_call, arg_list, rho)
            };
            let _result_guard = protect(result);

            let var_sym = CADR(lhs);
            if TYPEOF(var_sym) == SEXPTYPE::SYMSXP {
                bind_assignment(var_sym, result, primval, rho);
            }

            super::runtime::set_visible(FALSE);
            rhs
        }
    }
}

unsafe fn symbol_name(symbol: SEXP) -> Option<String> {
    unsafe {
        if symbol.is_null() || TYPEOF(symbol) != SEXPTYPE::SYMSXP {
            return None;
        }
        let printname = PRINTNAME(symbol);
        if printname.is_null() {
            return None;
        }
        let chars = CHAR(printname);
        if chars.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(chars)
            .to_str()
            .ok()
            .map(str::to_owned)
    }
}

unsafe fn dollar_field_symbol(field: SEXP) -> Option<SEXP> {
    unsafe {
        if field.is_null() || field == R_NilValue() {
            return None;
        }
        if TYPEOF(field) == SEXPTYPE::SYMSXP {
            return Some(field);
        }
        if TYPEOF(field) == SEXPTYPE::STRSXP && XLENGTH(field) > 0 {
            return Some(crate::mainutils::subset::installTrChar(STRING_ELT(
                field, 0,
            )));
        }
        None
    }
}

unsafe fn build_replacement_args(target: SEXP, subs: SEXP, value: SEXP) -> SEXP {
    unsafe {
        let wrap_lang = |x: SEXP| match TYPEOF(x) {
            t if t == SEXPTYPE::LANGSXP || t == SEXPTYPE::SYMSXP || t == SEXPTYPE::EXPRSXP => {
                crate::sexp::memory_ext::R_mkEVPROMISE(R_NilValue(), x)
            }
            _ => x,
        };
        let value = wrap_lang(value);
        let target = wrap_lang(target);
        let mut tail = crate::sexp::constructors::Rf_cons(value, R_NilValue());
        SETTAG(tail, crate::sexp::symbol::Rf_install(c"value".as_ptr()));
        let mut guards = vec![protect(tail)];
        let mut sub_args = Vec::new();

        let mut current = subs;
        while current != R_NilValue() && !current.is_null() {
            sub_args.push((CAR(current), TAG(current)));
            current = CDR(current);
        }
        for (arg, tag) in sub_args.into_iter().rev() {
            let cell = crate::sexp::constructors::Rf_cons(arg, tail);
            if !tag.is_null() {
                SETTAG(cell, tag);
            }
            tail = cell;
            guards.push(protect(tail));
        }
        crate::sexp::constructors::Rf_cons(target, tail)
    }
}

unsafe fn apply_replacement_call(assign_fn: SEXP, call: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(assign_fn) != SEXPTYPE::SYMSXP {
            return Rf_eval(call, rho);
        }
        let name = crate::sexp::accessors::CHAR(crate::sexp::accessors::PRINTNAME(assign_fn));
        if name.is_null() {
            return Rf_eval(call, rho);
        }
        let Ok(name) = std::ffi::CStr::from_ptr(name).to_str() else {
            return Rf_eval(call, rho);
        };

        match name {
            "[<-" => crate::mainutils::subset::do_subassign(call, assign_fn, args, rho),
            "[[<-" => crate::mainutils::subset::do_subassign2(call, assign_fn, args, rho),

            "$<-" => crate::mainutils::essentials::do_dollar_set(call, assign_fn, args, rho),
            "@<-" => crate::mainutils::essentials::do_at_set(call, assign_fn, args, rho),
            "names<-" => crate::mainutils::essentials::do_names_set(call, assign_fn, args, rho),
            "dim<-" => crate::mainutils::essentials::do_dim_set(call, assign_fn, args, rho),
            "tsp<-" => crate::mainutils::essentials::do_tsp_set(call, assign_fn, args, rho),
            "length<-" => crate::mainutils::essentials::do_length_set(call, assign_fn, args, rho),
            "levels<-" => crate::mainutils::essentials::do_levels_set(call, assign_fn, args, rho),
            "storage.mode<-" | "mode<-" => {
                crate::mainutils::essentials::do_storage_mode_set(call, assign_fn, args, rho)
            }

            "dimnames<-" => {
                crate::mainutils::essentials::do_dimnames_set(call, assign_fn, args, rho)
            }
            _ => Rf_eval(call, rho),
        }
    }
}

unsafe fn try_simple_vector_subassign(target: SEXP, subs: SEXP, value: SEXP) -> Option<SEXP> {
    unsafe {
        if subs == R_NilValue() || subs.is_null() || CDR(subs) != R_NilValue() {
            return None;
        }
        if !matches!(
            SEXPTYPE(TYPEOF(target)),
            SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
        ) || !matches!(
            SEXPTYPE(TYPEOF(value)),
            SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
        ) || XLENGTH(value) < 1
        {
            return None;
        }

        let index = scalar_positive_index(CAR(subs))?;
        if index >= XLENGTH(target) {
            return None;
        }

        // GNU do_subassign_dflt duplicates when MAYBE_SHARED (NAMED >= 2).
        // This AST scalar shortcut used to mutate in place and alias every
        // binding of the same vector (`y <- x; x[1] <- 9L`). Fall through
        // so the default path can shallow-duplicate first.
        if NAMED(target) >= 2 {
            return None;
        }

        match TYPEOF(target) {
            t if t == SEXPTYPE::REALSXP => {
                let replacement = match TYPEOF(value) {
                    vt if vt == SEXPTYPE::REALSXP => REAL_ELT(value, 0),
                    vt if vt == SEXPTYPE::INTSXP || vt == SEXPTYPE::LGLSXP => {
                        let v = if vt == SEXPTYPE::INTSXP {
                            INTEGER_ELT(value, 0)
                        } else {
                            LOGICAL_ELT(value, 0)
                        };
                        if v == crate::sexp::ffi::NA_INTEGER {
                            crate::sexp::ffi::NA_REAL
                        } else {
                            v as f64
                        }
                    }
                    _ => return None,
                };
                SET_REAL_ELT(target, index as i32, replacement);
                Some(target)
            }
            t if t == SEXPTYPE::INTSXP => {
                let replacement = match TYPEOF(value) {
                    vt if vt == SEXPTYPE::INTSXP => INTEGER_ELT(value, 0),
                    vt if vt == SEXPTYPE::LGLSXP => LOGICAL_ELT(value, 0),
                    _ => return None,
                };
                SET_INTEGER_ELT(target, index as i32, replacement);
                Some(target)
            }
            t if t == SEXPTYPE::LGLSXP => {
                let replacement = match TYPEOF(value) {
                    vt if vt == SEXPTYPE::LGLSXP => LOGICAL_ELT(value, 0),
                    _ => return None,
                };
                SET_LOGICAL_ELT(target, index as i32, replacement);
                Some(target)
            }
            _ => None,
        }
    }
}

unsafe fn scalar_positive_index(index: SEXP) -> Option<crate::sexp::ffi::R_xlen_t> {
    unsafe {
        if index.is_null()
            || !matches!(
                SEXPTYPE(TYPEOF(index)),
                SEXPTYPE::INTSXP | SEXPTYPE::REALSXP
            )
            || XLENGTH(index) != 1
        {
            return None;
        }
        let raw = match TYPEOF(index) {
            t if t == SEXPTYPE::INTSXP => INTEGER_ELT(index, 0) as crate::sexp::ffi::R_xlen_t,
            t if t == SEXPTYPE::REALSXP => {
                let value = REAL_ELT(index, 0);
                if !value.is_finite() || value.fract() != 0.0 {
                    return None;
                }
                value as crate::sexp::ffi::R_xlen_t
            }
            _ => return None,
        };
        if raw < 1 { None } else { Some(raw - 1) }
    }
}

/// Cached replacement values retain their original promise code separately.
fn replace_tmp_call(
    assign_fn: &Sexp<'static>,
    tmp_sym: &Sexp<'static>,
    rest: &Sexp<'static>,
    rhs: &Sexp<'static>,
    expression: &Sexp<'static>,
    environment: &Sexp<'static>,
) -> crate::sexp::object::SexpResult<Sexp<'static>> {
    use crate::sexp::{
        object::SexpError,
        owner::{StoredOwner, with_runtime},
    };
    let original = StoredOwner::from_value(environment)?
        .managed()
        .ok_or(SexpError::RootUnavailable)?;
    with_runtime(&original, |access| {
        // Capture each actual edge before any node allocation can reenter R.
        let mut current = rest.clone();
        let mut cells = Vec::new();
        let mut seen = std::collections::HashSet::new();
        while !current.is_nil() {
            let identity = current
                .allocation()?
                .link()
                .ok_or(SexpError::StaleAllocation)?;
            seen.try_reserve(1)
                .map_err(|_| SexpError::AllocationFailed {
                    object: "replacement syntax arguments",
                })?;
            if !seen.insert(identity) {
                return Err(SexpError::EvaluationFailed {
                    message: "cyclic replacement syntax arguments".into(),
                });
            }
            cells
                .try_reserve(1)
                .map_err(|_| SexpError::AllocationFailed {
                    object: "replacement syntax arguments",
                })?;
            cells.push((
                current.try_car()?.into_owned()?,
                current.try_tag()?.into_owned()?,
            ));
            current = current.try_cdr()?.into_owned()?;
        }
        let domain = access.domain();
        let allocator = access.allocator(&domain)?;
        let value_tag = access.with_native(|owner| {
            owner
                .sexp(unsafe { Rf_install(c"value".as_ptr()) })?
                .into_owned()
        })?;
        let value = allocator.evaluated_promise_with_expression(expression, environment, rhs)?;
        let mut tail = allocator.pairlist_cell(&value, &domain.nil(), &value_tag)?;
        for (argument, tag) in cells.iter().rev() {
            tail = allocator.pairlist_cell(argument, &tail, tag)?;
        }
        tail = allocator.pairlist_cell(tmp_sym, &tail, &domain.nil())?;
        allocator.call(assign_fn, &tail)
    })?
}

/// Convert `f` or `pkg::f` / `pkg:::f` to the assignment function head.
unsafe fn replacement_fun_head(fun: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(fun) == SEXPTYPE::SYMSXP {
            return get_assign_fcn_sym(fun);
        }
        if TYPEOF(fun) == SEXPTYPE::LANGSXP {
            let op = CAR(fun);
            let op_name = symbol_name(op);
            if matches!(op_name.as_deref(), Some("::") | Some(":::"))
                && TYPEOF(CADDR(fun)) == SEXPTYPE::SYMSXP
            {
                let assign = get_assign_fcn_sym(CADDR(fun));
                if assign != R_NilValue() {
                    let call = crate::sexp::constructors::Rf_lang3(op, CADR(fun), assign);
                    return call;
                }
            }
        }
        R_NilValue()
    }
}

/// Convert a function symbol to its assignment variant: `[` -> `[<-`, `$` -> `$<-`.
fn get_assign_fcn_sym(sym: SEXP) -> SEXP {
    unsafe {
        let name = crate::sexp::accessors::CHAR(crate::sexp::accessors::PRINTNAME(sym));
        if name.is_null() {
            return R_NilValue();
        }
        let Ok(s) = std::ffi::CStr::from_ptr(name).to_str() else {
            return R_NilValue();
        };
        let assign_name = format!("{}<-", s);
        let c_name = std::ffi::CString::new(assign_name)
            .expect("assignment symbol derived from a CStr cannot contain NUL");
        crate::sexp::symbol::Rf_install(c_name.as_ptr())
    }
}

#[path = "assignment/restore_tmp.rs"]
mod restore_tmp;

/// Passive cleanup adapters retain the exact original allocation. These short
/// field operations never enter R, adopt TLS authority, or span a callback.
fn restore_tmp_barrier(
    owner: &crate::sexp::owner::OwnerPin,
    parent: &Sexp<'_>,
    child: &Sexp<'_>,
) -> crate::sexp::object::SexpResult<()> {
    if unsafe {
        crate::sexp::gengc::write_barrier_in(owner.as_ptr(), parent.as_raw(), child.as_raw())
    } {
        Ok(())
    } else {
        Err(crate::sexp::object::SexpError::AllocationFailed {
            object: "temporary binding write barrier",
        })
    }
}

fn restore_tmp_remove_metadata(
    owner: &crate::sexp::owner::OwnerPin,
    environment: &Sexp<'_>,
    symbol: &Sexp<'_>,
) {
    let key = (environment.as_raw().addr(), symbol.as_raw().addr());
    unsafe {
        (*owner.as_ptr()).active_bindings.remove(&key);
        (*owner.as_ptr()).locked_bindings.remove(&key);
    }
}

fn restore_tmp_locked(
    pin: &crate::sexp::owner::OwnerPin,
    environment: &Sexp<'_>,
    symbol: &Sexp<'_>,
) -> bool {
    let key = (environment.as_raw().addr(), symbol.as_raw().addr());
    unsafe { (*pin.as_ptr()).locked_bindings.contains(&key) }
}

fn restore_tmp_active(
    pin: &crate::sexp::owner::OwnerPin,
    environment: &Sexp<'_>,
    symbol: &Sexp<'_>,
) -> bool {
    let key = (environment.as_raw().addr(), symbol.as_raw().addr());
    unsafe { (*pin.as_ptr()).active_bindings.contains_key(&key) }
}

fn restore_tmp_read(
    authority: &crate::sexp::owner::StoredOwner<'_>,
    pin: &crate::sexp::owner::OwnerPin,
    environment: &Sexp<'static>,
    symbol: &Sexp<'static>,
) -> crate::sexp::object::SexpResult<Sexp<'static>> {
    authority.require_active()?;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::envir::find_var_in_frame_result(environment.clone(), symbol.clone())
    }));
    pin.require_live()?;
    let result = match outcome {
        Ok(result) => result,
        Err(payload) => std::panic::resume_unwind(payload),
    };
    authority.require_active()?;
    result
        .map_err(|message| crate::sexp::object::SexpError::EvaluationFailed { message })?
        .ok_or(crate::sexp::object::SexpError::StaleAllocation)?
        .into_owned()
}

#[cfg(test)]
#[path = "assignment/restore_tmp_tests.rs"]
mod restore_tmp_tests;

#[cfg(test)]
mod owned_source_assignment_tests {
    use super::*;
    use crate::sexp::accessors::SETCAR;
    use crate::sexp::{instance::RuntimeValue, owner::OwnerToken, session::RSession};
    use std::{cell::Cell, rc::Rc};

    fn source_value(session: &RSession, script: &str) -> Sexp<'static> {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let expression = owner
            .with_arena(|arena| crate::eval::parser::parse(script, arena, factory.domain()))
            .unwrap()
            .unwrap();
        unsafe {
            factory.wrap(Rf_eval(
                expression.as_raw(),
                session.global_env().unwrap().as_raw(),
            ))
        }
        .unwrap()
        .into_owned()
        .unwrap()
    }

    #[test]
    fn owned_source_custom_replacement_preserves_lazy_argument_expression() {
        let session = RSession::new_for_gc_tests();
        let value = source_value(
            &session,
            "{x<-1L;`stamp<-`<-function(x,label,value){attr(x,'expression')<-substitute(label);x};stamp(x,unbound_label)<-7L;identical(attr(x,'expression'),quote(unbound_label))}",
        );
        assert_eq!(value.logical_elt(0), Some(1));
    }

    #[test]
    fn owned_source_custom_replacement_discards_original_object_tag_like_gnu() {
        let session = RSession::new_for_gc_tests();
        let value = source_value(
            &session,
            "{x<-1L;`stamp<-`<-function(label,object,value){list(label=label,object=object)};stamp(object=x,'mark')<-7L;identical(x,list(label=1L,object='mark'))}",
        );
        assert_eq!(value.logical_elt(0), Some(1));
    }

    #[test]
    fn owned_source_custom_replacement_preserves_object_and_rhs_source() {
        let session = RSession::new_for_gc_tests();
        let value = source_value(
            &session,
            "{x<-1L;`stamp<-`<-function(x,value){attr(x,'object')<-substitute(x);attr(x,'rhs')<-substitute(value);x};stamp(x)<-quote(y);identical(attr(x,'object'),quote(`*tmp*`))&&identical(attr(x,'rhs'),quote(quote(y)))}",
        );
        assert_eq!(value.logical_elt(0), Some(1));
    }

    #[test]
    fn owned_source_custom_replacement_exposes_original_syntax_call() {
        let session = RSession::new_for_gc_tests();
        let value = source_value(
            &session,
            "{sys.call<-function(which=0L).Internal(sys.call(which));x<-1L;`stamp<-`<-function(x,label,value){attr(x,'observed')<-sys.call();x};stamp(x,unbound_label)<-{1L;7L};observed<-attr(x,'observed');identical(observed[[1L]],quote(`stamp<-`))&&identical(observed[[2L]],quote(`*tmp*`))&&identical(observed[[3L]],quote(unbound_label))&&identical(typeof(observed[[4L]]),'promise')}",
        );
        assert_eq!(value.logical_elt(0), Some(1));
    }

    #[test]
    fn owned_source_custom_replacement_resolves_active_primitive_once() {
        for script in [
            "{n<-0L;x<-c(1L,2L);makeActiveBinding('stamp<-',function(){n<<-n+1L;if(n==1L)`[<-` else `[[<-`},globalenv());stamp(x,1L)<-9L;identical(n,1L)&&identical(x,c(9L,2L))}",
            "{n<-0L;x<-1L;makeActiveBinding('stamp<-',function(){n<<-n+1L;if(n==1L)`attr<-` else `[<-`},globalenv());stamp(x,'mark')<-7L;identical(n,1L)&&identical(attr(x,'mark'),7L)}",
            "{n<-0L;x<-list();makeActiveBinding('stamp<-',function(){n<<-n+1L;`$<-`},globalenv());stamp(x,unbound_label)<-7L;identical(n,1L)&&identical(x$unbound_label,7L)}",
            "{n<-0L;x<-1L;makeActiveBinding('stamp<-',function(){n<<-n+1L;if(n==1L)0L else function(x,value)value},globalenv());failed<-FALSE;tryCatch(stamp(x)<-9L,error=function(e){failed<<-TRUE});identical(failed,TRUE)&&identical(n,1L)&&identical(x,1L)}",
        ] {
            let session = RSession::new_for_gc_tests();
            assert_eq!(
                source_value(&session, script).logical_elt(0),
                Some(1),
                "{script}"
            );
        }
    }

    #[test]
    fn owned_source_custom_replacement_preserves_sharing_and_error_isolation() {
        for script in [
            "{x<-c(1L,2L);y<-x;`stamp<-`<-function(x,value){x[1L]<-value;x};stamp(x)<-9L;identical(y,c(1L,2L))&&identical(x,c(9L,2L))}",
            "{x<-c(1L,2L);`stamp<-`<-function(x,value){x[1L]<-value;stop('abort setter')};tryCatch(stamp(x)<-9L,error=function(e)NULL);identical(x,c(1L,2L))}",
            "{x<-1L;rhs<-7L;`stamp<-`<-function(x,value){attr(value,'modified')<-TRUE;value};result<-withVisible(stamp(x)<-rhs);identical(result$value,rhs)&&is.null(attributes(rhs))&&!result$visible&&identical(attr(x,'modified'),TRUE)}",
            "{x<-1L;rhs<-c(7L,8L);`stamp<-`<-function(x,value){value[1L]<-9L;value};result<-withVisible(stamp(x)<-rhs);identical(result$value,rhs)&&identical(rhs,c(7L,8L))&&!result$visible&&identical(x,c(9L,8L))}",
        ] {
            let session = RSession::new_for_gc_tests();
            assert_eq!(
                source_value(&session, script).logical_elt(0),
                Some(1),
                "{script}"
            );
        }
    }

    #[test]
    fn owned_source_custom_replacement_returns_invisible_rhs_after_ordered_evaluation() {
        let session = RSession::new_for_gc_tests();
        source_value(
            &session,
            "{trace<-0L;x<-1L;`stamp<-`<-function(x,label,value){attr(x,label)<-value;x}}",
        );
        let value = source_value(
            &session,
            "stamp(x,{trace<-trace*10L+2L;'mark'})<-{trace<-trace*10L+1L;quote(retained_rhs)}",
        );
        assert!(value.is_symbol());
        assert_eq!(unsafe { crate::sexp::globals::R_Visible() }, FALSE);
        assert_eq!(source_value(&session, "trace").integer_elt(0), Some(12));
        assert_eq!(
            source_value(&session, "identical(attr(x,'mark'),quote(retained_rhs))").logical_elt(0),
            Some(1)
        );
    }

    #[test]
    fn owned_source_custom_replacement_keeps_detached_source_promises_through_gc() {
        let session = RSession::new_for_gc_tests();
        source_value(
            &session,
            "{x<-1L;`stamp<-`<-function(x,label,value){gc();attr(x,'object')<-substitute(x);attr(x,'label')<-substitute(label);attr(x,'rhs')<-substitute(value);x}}",
        );
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let expression = owner
            .with_arena(|arena| {
                crate::eval::parser::parse(
                    "stamp(x,unbound_label)<-{gc();quote(retained_rhs)}",
                    arena,
                    factory.domain(),
                )
            })
            .unwrap()
            .unwrap();
        let source_arguments = expression.try_cdr().unwrap().into_owned().unwrap();
        let lhs_arguments = source_arguments
            .try_car()
            .unwrap()
            .try_cdr()
            .unwrap()
            .into_owned()
            .unwrap();
        let extra_arguments = lhs_arguments.try_cdr().unwrap().into_owned().unwrap();
        let rhs_cell = source_arguments.try_cdr().unwrap().into_owned().unwrap();
        let called = Rc::new(Cell::new(false));
        let observed = called.clone();
        session.with_active_in(|instance| unsafe {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if observed.replace(true) {
                    return;
                }
                (*instance).memory_state.gc_force_gap = 0;
                (*instance).eval_state.current_expr = RuntimeValue::empty();
                SETCAR(lhs_arguments.as_raw(), R_NilValue());
                SETCAR(extra_arguments.as_raw(), R_NilValue());
                SETCAR(rhs_cell.as_raw(), R_NilValue());
                crate::sexp::gengc::full_gc_in(instance);
            }));
            // The explicit RHS collection happens after applydefine captures
            // the source arguments. The setter collects again after the
            // initialized source-expression promises have been published.
        });
        let value = unsafe {
            factory.wrap(Rf_eval(
                expression.as_raw(),
                session.global_env().unwrap().as_raw(),
            ))
        }
        .unwrap();
        assert!(
            called.get(),
            "must detach the original source during a collecting callback"
        );
        assert!(value.is_symbol());
        assert_eq!(unsafe { crate::sexp::globals::R_Visible() }, FALSE);
        let checked = source_value(
            &session,
            "identical(attr(x,'object'),quote(`*tmp*`))&&identical(attr(x,'label'),quote(unbound_label))&&identical(attr(x,'rhs'),quote({gc();quote(retained_rhs)}))",
        );
        assert_eq!(checked.logical_elt(0), Some(1));
    }

    #[test]
    fn owned_source_attribution_restores_after_collecting_callback_and_unwind() {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let owner = OwnerToken::from_raw(instance);
            let factory = owner.node_factory();
            let previous = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
                .unwrap();
            let current = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
                .unwrap();
            let old_node = crate::sexp::memory::checked_projection(previous.as_raw())
                .unwrap()
                .1;
            let new_node = crate::sexp::memory::checked_projection(current.as_raw())
                .unwrap()
                .1;
            let previous_pointer = previous.as_raw();
            (*instance).eval_state.current_expr =
                RuntimeValue::from_raw_in(instance, previous_pointer);
            drop(previous);
            let call = current.as_raw();
            let observed = Rc::new(Cell::new(false));
            let called = observed.clone();
            let old_observed = old_node.clone();
            let new_observed = new_node.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if called.replace(true) {
                    return;
                }
                (*instance).eval_state.current_expr = RuntimeValue::empty();
                crate::sexp::gengc::full_gc_in(instance);
                assert!(
                    old_observed.is_live(),
                    "previous attribution must be owned by restoration guard"
                );
                assert!(
                    new_observed.is_live(),
                    "original source input must survive field replacement"
                );
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = SourceAssignCall::enter(call);
                drop(current);
                let _allocated = factory
                    .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, 9)))
                    .unwrap();
                crate::sexp::context::r_error("unwind source assignment");
            }));
            (*instance).memory_state.gc_force_gap = 0;
            assert!(result.is_err());
            assert!(observed.get());
            assert_eq!(
                (*instance).eval_state.current_expr.as_raw(),
                previous_pointer
            );
            owner.full_gc().unwrap();
            assert!(old_node.is_live());
            assert!(!new_node.is_live());
            (*instance).eval_state.current_expr = RuntimeValue::empty();
            owner.full_gc().unwrap();
            assert!(!old_node.is_live());
        });
    }

    #[test]
    fn owned_source_attribution_cleanup_keeps_dropped_original_runtime_alive() {
        let session = RSession::new_for_gc_tests();
        let (guard, old_node, new_node) = session.with_active_in(|instance| unsafe {
            let owner = OwnerToken::from_raw(instance);
            let factory = owner.node_factory();
            let previous = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
                .unwrap();
            let current = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
                .unwrap();
            let old_node = crate::sexp::memory::checked_projection(previous.as_raw())
                .unwrap()
                .1;
            let new_node = crate::sexp::memory::checked_projection(current.as_raw())
                .unwrap()
                .1;
            (*instance).eval_state.current_expr =
                RuntimeValue::from_raw_in(instance, previous.as_raw());
            let guard = SourceAssignCall::enter(current.as_raw());
            drop(previous);
            drop(current);
            owner.full_gc().unwrap();
            (guard, old_node, new_node)
        });
        drop(session);
        assert!(guard.owner.require_live().is_err());
        assert!(old_node.is_live());
        assert!(new_node.is_live());
        drop(guard);
        assert!(!old_node.is_live());
        assert!(!new_node.is_live());
    }
}
