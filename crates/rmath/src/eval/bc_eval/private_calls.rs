//! Private (owner-scoped) call/replacement evaluation helpers — extracted verbatim from eval/bc_eval.rs

use super::*;

/// Build calls from actual owning values. Computed symbols and language objects
/// are data; initialized promises prevent the native evaluator interpreting them
/// again. Syntax-special calls retain their original argument expressions.
pub(crate) fn private_call_owned(
    function: &Sexp<'static>,
    arguments: &[Sexp<'static>],
    tags: &[Option<Sexp<'static>>],
    environment: &Sexp<'static>,
    syntax: bool,
    source: Option<&Sexp<'static>>,
) -> Sexp<'static> {
    let owner = crate::sexp::owner::StoredOwner::from_value(environment)
        .and_then(|owner| {
            owner
                .managed()
                .ok_or(crate::sexp::object::SexpError::RootUnavailable)
        })
        .unwrap_or_else(|error| bc_error(error.to_string()));
    crate::sexp::owner::with_runtime(&owner, |access| {
        let domain = access.domain();
        let allocator = access.allocator(&domain)?;
        let mut expressions = Vec::new();
        if let Some(source) = source {
            let mut cell = source.try_cdr()?;
            for _ in arguments {
                expressions.push(cell.try_car()?);
                cell = cell.try_cdr()?;
            }
        }
        let mut rest = domain.nil();
        for (index, argument) in arguments.iter().enumerate() {
            let value = if syntax {
                argument.clone()
            } else if argument.typeof_() == SEXPTYPE::PROMSXP
                || argument.as_raw() == domain.missing().as_raw()
            {
                argument.clone()
            } else {
                let expression = expressions
                    .get(arguments.len() - 1 - index)
                    .unwrap_or(argument);
                allocator.evaluated_promise_with_expression(expression, environment, argument)?
            };
            rest = allocator.pairlist_cell(
                &value,
                &rest,
                tags.get(index)
                    .and_then(Option::as_ref)
                    .unwrap_or(&domain.nil()),
            )?;
        }
        allocator.call(function, &rest)
    })
    .and_then(|result| result)
    .unwrap_or_else(|error| bc_error(error.to_string()))
}

/// Execute cached arguments while exposing the original call to the callee.
pub(crate) unsafe fn eval_source_call(
    execution: &Sexp<'static>,
    function: &Sexp<'static>,
    source: &Sexp<'static>,
    environment: &Sexp<'static>,
    stack: &R_bcstack_t,
    loops: &[LoopContext],
) -> Result<SEXP, LoopJump> {
    if source.typeof_() != SEXPTYPE::LANGSXP {
        bc_error("CALL source constant is not a language object");
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        with_stack_rooted(stack, execution.as_raw(), || {
            let selected = if function.typeof_() == SEXPTYPE::SYMSXP {
                own_operand(crate::sexp::envir::findFun(
                    function.as_raw(),
                    environment.as_raw(),
                ))
            } else {
                function.clone()
            };
            let apply = match selected.typeof_() {
                SEXPTYPE::CLOSXP => super::super::apply::apply_closure_safe,
                SEXPTYPE::SPECIALSXP => super::super::apply::apply_special_safe,
                SEXPTYPE::BUILTINSXP => super::super::apply::apply_builtin_safe,
                _ => bc_error("attempt to apply non-function"),
            };
            apply(
                selected,
                source.clone(),
                own_operand(CDR(execution.as_raw())),
                environment.clone(),
            )
            .unwrap_or_else(|error| bc_error(error))
            .as_raw()
        })
    }));
    match result {
        Ok(value) => Ok(value),
        Err(payload) => Err(recover_loop_jump(payload, loops)),
    }
}

/// The setter executes with cached target/RHS promises while its function
/// context exposes GNU's original syntax call. Actual owning trees keep both
/// views alive across allocation, dispatch, full GC, and unwinding.
pub(crate) fn private_replacement_owned(
    setter: &Sexp<'static>,
    arguments: &[Sexp<'static>],
    tags: &[Option<Sexp<'static>>],
    rhs_expression: &Sexp<'static>,
    environment: &Sexp<'static>,
    loops: &[LoopContext],
) -> Result<Sexp<'static>, LoopJump> {
    private_target_call_owned(
        setter,
        arguments,
        tags,
        Some(rhs_expression),
        environment,
        loops,
    )
}

/// Calls a getter or setter with separately owning observable syntax and
/// cached execution arguments. A getter has no value argument.
pub(crate) fn private_target_call_owned(
    setter: &Sexp<'static>,
    arguments: &[Sexp<'static>],
    tags: &[Option<Sexp<'static>>],
    rhs_expression: Option<&Sexp<'static>>,
    environment: &Sexp<'static>,
    loops: &[LoopContext],
) -> Result<Sexp<'static>, LoopJump> {
    use crate::sexp::{
        object::SexpError,
        owner::{StoredOwner, with_runtime},
    };
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let owner = StoredOwner::from_value(environment)
            .and_then(|owner| owner.managed().ok_or(SexpError::RootUnavailable))
            .unwrap_or_else(|error| bc_error(error.to_string()));
        with_runtime(&owner, |access| {
            let domain = access.domain();
            let allocator = access.allocator(&domain)?;
            let (temporary, value_tag) = access.with_native(|owner| {
                Ok((
                    owner
                        .sexp(unsafe { crate::sexp::symbol::Rf_install(c"*tmp*".as_ptr()) })?
                        .into_owned()?,
                    owner
                        .sexp(unsafe { crate::sexp::symbol::Rf_install(c"value".as_ptr()) })?
                        .into_owned()?,
                ))
            })?;
            let rhs = rhs_expression
                .map(|_| {
                    arguments.first().ok_or(SexpError::AllocationFailed {
                        object: "replacement RHS",
                    })
                })
                .transpose()?;
            let object = arguments.last().ok_or(SexpError::AllocationFailed {
                object: "replacement target",
            })?;
            let mut tail = if let Some(expression) = rhs_expression {
                let value = allocator.evaluated_promise_with_expression(
                    expression,
                    environment,
                    rhs.expect("RHS presence follows expression"),
                )?;
                allocator.pairlist_cell(&value, &domain.nil(), &value_tag)?
            } else {
                domain.nil()
            };
            for index in usize::from(rhs_expression.is_some())..arguments.len() - 1 {
                tail = allocator.pairlist_cell(
                    &arguments[index],
                    &tail,
                    tags.get(index)
                        .and_then(Option::as_ref)
                        .unwrap_or(&domain.nil()),
                )?;
            }
            access.require_active()?;
            unsafe {
                if let Some(rhs) = rhs {
                    crate::sexp::accessors::SET_NAMED(rhs.as_raw(), 2);
                }
                crate::sexp::accessors::SET_NAMED(object.as_raw(), 2);
            }
            let syntax_arguments = allocator.pairlist_cell(&temporary, &tail, &domain.nil())?;
            let call = allocator.call(setter, &syntax_arguments)?;
            if setter.typeof_() == SEXPTYPE::LANGSXP {
                if !is_replacement_function_head(setter) {
                    return Err(SexpError::EvaluationFailed {
                        message: "invalid replacement function head".into(),
                    });
                }
                let restore_temporary = crate::eval::assignment::restore_tmp::RestoreTmp::capture(
                    environment.clone(),
                    temporary.clone(),
                )?;
                return restore_temporary.run(|| {
                    access.with_native(|owner| {
                        unsafe {
                            crate::sexp::envir::defineVar(
                                temporary.as_raw(),
                                object.as_raw(),
                                environment.as_raw(),
                            );
                        }
                        access.require_active()?;
                        let result = unsafe {
                            super::super::eval::Rf_eval(call.as_raw(), environment.as_raw())
                        };
                        access.require_active()?;
                        owner.sexp(result)?.into_owned()
                    })
                })?;
            }
            let target =
                allocator.evaluated_promise_with_expression(&temporary, environment, object)?;
            let execution_arguments = allocator.pairlist_cell(&target, &tail, &domain.nil())?;
            access.with_native(|owner| {
                let function = unsafe {
                    crate::sexp::envir::find_fun_result(setter.clone(), environment.clone())
                }
                .map_err(|message| SexpError::EvaluationFailed { message })?
                .ok_or_else(|| SexpError::EvaluationFailed {
                    message: "could not find replacement function".into(),
                })?;
                access.require_active()?;
                if function.is_closure() {
                    let result = unsafe {
                        super::super::closure::applyClosure(
                            call.as_raw(),
                            function.as_raw(),
                            execution_arguments.as_raw(),
                            environment.as_raw(),
                            domain.nil().as_raw(),
                            0,
                        )
                    };
                    owner.sexp(result)?.into_owned()
                } else {
                    let apply = if function.typeof_() == SEXPTYPE::SPECIALSXP {
                        super::super::apply::apply_special_safe
                    } else {
                        super::super::apply::apply_builtin_safe
                    };
                    apply(
                        function.clone(),
                        call.clone(),
                        execution_arguments.clone(),
                        environment.clone(),
                    )
                    .map_err(|message| SexpError::EvaluationFailed { message })?
                    .into_owned()
                }
            })
        })
        .and_then(|result| result)
        .unwrap_or_else(|error| bc_error(error.to_string()))
    }));
    match attempt {
        Ok(result) => Ok(result),
        Err(payload) => Err(recover_loop_jump(payload, loops)),
    }
}

/// Own the selected root/child through the complete getter-inner-outer chain.
/// No temporary binding exposes a partial update or reselects a mutated root.
pub(crate) fn private_replacement_path_owned(
    setter: &Sexp<'static>,
    root: &Sexp<'static>,
    field: &Sexp<'static>,
    superassign: bool,
    arguments: &mut Vec<Sexp<'static>>,
    tags: &mut Vec<Option<Sexp<'static>>>,
    rhs_expression: &Sexp<'static>,
    environment: &Sexp<'static>,
    loops: &[LoopContext],
) -> Result<Sexp<'static>, LoopJump> {
    use crate::sexp::{
        object::SexpError,
        owner::{StoredOwner, with_runtime},
    };
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let owner = StoredOwner::from_value(environment)
            .and_then(|owner| owner.managed().ok_or(SexpError::RootUnavailable))
            .unwrap_or_else(|error| bc_error(error.to_string()));
        with_runtime(&owner, |access| {
            let lookup = access.with_native(|owner| {
                if superassign {
                    owner
                        .sexp(unsafe { ENCLOS(environment.as_raw()) })?
                        .into_owned()
                } else {
                    Ok(environment.clone())
                }
            })?;
            let object = access.with_native(|owner| {
                owner
                    .sexp(unsafe { super::super::eval::Rf_eval(root.as_raw(), lookup.as_raw()) })?
                    .into_owned()
            })?;
            access.require_active()?;
            unsafe {
                crate::sexp::accessors::SET_NAMED(object.as_raw(), 2);
            }
            let (getter, outer_setter, modified_expression) = if field.is_nil() {
                (None, None, None)
            } else {
                access.with_native(|owner| {
                    Ok((
                        Some(
                            owner
                                .sexp(unsafe { crate::sexp::symbol::Rf_install(c"$".as_ptr()) })?
                                .into_owned()?,
                        ),
                        Some(
                            owner
                                .sexp(unsafe { crate::sexp::symbol::Rf_install(c"$<-".as_ptr()) })?
                                .into_owned()?,
                        ),
                        Some(
                            owner
                                .sexp(unsafe {
                                    crate::sexp::symbol::Rf_install(c"*vtmp*".as_ptr())
                                })?
                                .into_owned()?,
                        ),
                    ))
                })?
            };
            let child = if let Some(getter) = &getter {
                match private_target_call_owned(
                    getter,
                    &[field.clone(), object.clone()],
                    &[None, None],
                    None,
                    environment,
                    loops,
                ) {
                    Ok(value) => value,
                    Err(jump) => return Ok(Err(jump)),
                }
            } else {
                object.clone()
            };
            arguments
                .try_reserve(1)
                .map_err(|_| SexpError::AllocationFailed {
                    object: "replacement child",
                })?;
            tags.try_reserve(1)
                .map_err(|_| SexpError::AllocationFailed {
                    object: "replacement child tag",
                })?;
            arguments.push(child);
            tags.push(None);
            let changed = match private_replacement_owned(
                setter,
                arguments,
                tags,
                rhs_expression,
                environment,
                loops,
            ) {
                Ok(value) => value,
                Err(jump) => return Ok(Err(jump)),
            };
            let result = if let Some(outer_setter) = &outer_setter {
                match private_replacement_owned(
                    outer_setter,
                    &[changed, field.clone(), object],
                    &[None, None, None],
                    modified_expression.as_ref().expect("dollar RHS expression"),
                    environment,
                    loops,
                ) {
                    Ok(value) => value,
                    Err(jump) => return Ok(Err(jump)),
                }
            } else {
                changed
            };
            access.with_native(|_| {
                unsafe {
                    if superassign {
                        setVar(root.as_raw(), result.as_raw(), lookup.as_raw());
                    } else {
                        defineVar(root.as_raw(), result.as_raw(), environment.as_raw());
                    }
                }
                Ok(())
            })?;
            Ok(Ok(result))
        })
        .and_then(|result| result)
        .unwrap_or_else(|error| bc_error(error.to_string()))
    }));
    match attempt {
        Ok(result) => result,
        Err(payload) => Err(recover_loop_jump(payload, loops)),
    }
}

/// Whether a bytecode replacement head is a symbol or the narrow explicit
/// namespace form supported by the replacement-chain adapter.
pub(crate) fn is_replacement_function_head(value: &Sexp<'static>) -> bool {
    if value.typeof_() == SEXPTYPE::SYMSXP {
        return true;
    }
    if value.typeof_() != SEXPTYPE::LANGSXP {
        return false;
    }
    unsafe {
        let Some(operator) = crate::eval::bc_compile::symbol_name_from_sexp(
            crate::sexp::accessors::CAR(value.as_raw()),
        ) else {
            return false;
        };
        if !matches!(operator.as_str(), "::" | ":::") {
            return false;
        }
        let package_cell = crate::sexp::accessors::CDR(value.as_raw());
        if package_cell.is_null() || package_cell == crate::sexp::globals::R_NilValue() {
            return false;
        }
        let member_cell = crate::sexp::accessors::CDR(package_cell);
        if member_cell.is_null()
            || member_cell == crate::sexp::globals::R_NilValue()
            || crate::sexp::accessors::CDR(member_cell) != crate::sexp::globals::R_NilValue()
        {
            return false;
        }
        let package_tag = crate::sexp::accessors::TAG(package_cell);
        let member_tag = crate::sexp::accessors::TAG(member_cell);
        (package_tag.is_null() || package_tag == crate::sexp::globals::R_NilValue())
            && (member_tag.is_null() || member_tag == crate::sexp::globals::R_NilValue())
            && crate::sexp::accessors::TYPEOF(crate::sexp::accessors::CAR(package_cell))
                == SEXPTYPE::SYMSXP
            && crate::sexp::accessors::TYPEOF(crate::sexp::accessors::CAR(member_cell))
                == SEXPTYPE::SYMSXP
    }
}

/// Each level retains original extra syntax/tags. Getter and setter calls
/// deliberately construct their own promises: GNU can reevaluate an index.
pub(crate) struct ReplacementLevel {
    pub(crate) getter: Sexp<'static>,
    pub(crate) setter: Sexp<'static>,
    pub(crate) extras: Vec<(Sexp<'static>, Sexp<'static>)>,
}

impl ReplacementLevel {
    fn arguments(
        &self,
        object: &Sexp<'static>,
        value: Option<&Sexp<'static>>,
    ) -> (Vec<Sexp<'static>>, Vec<Option<Sexp<'static>>>) {
        let count = self
            .extras
            .len()
            .checked_add(1 + usize::from(value.is_some()))
            .unwrap_or_else(|| bc_error("replacement level argument count overflow"));
        let mut arguments = Vec::new();
        let mut tags = Vec::new();
        arguments
            .try_reserve(count)
            .unwrap_or_else(|_| bc_error("cannot allocate replacement level arguments"));
        tags.try_reserve(count)
            .unwrap_or_else(|_| bc_error("cannot allocate replacement level tags"));
        if let Some(value) = value {
            arguments.push(value.clone());
            tags.push(None);
        }
        for (value, tag) in self.extras.iter().rev() {
            arguments.push(value.clone());
            tags.push((!tag.is_nil()).then(|| tag.clone()));
        }
        arguments.push(object.clone());
        tags.push(None);
        (arguments, tags)
    }
}

pub(crate) fn private_replacement_chain_owned(
    root: &Sexp<'static>,
    superassign: bool,
    levels: &[ReplacementLevel],
    rhs: &Sexp<'static>,
    expression: &Sexp<'static>,
    environment: &Sexp<'static>,
    loops: &[LoopContext],
) -> Result<Sexp<'static>, LoopJump> {
    use crate::sexp::{
        object::SexpError,
        owner::{StoredOwner, with_runtime},
    };
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let owner = StoredOwner::from_value(environment)
            .and_then(|owner| owner.managed().ok_or(SexpError::RootUnavailable))
            .unwrap_or_else(|error| bc_error(error.to_string()));
        with_runtime(&owner, |access| {
            let lookup = access.with_native(|owner| {
                if superassign {
                    owner
                        .sexp(unsafe { ENCLOS(environment.as_raw()) })?
                        .into_owned()
                } else {
                    Ok(environment.clone())
                }
            })?;
            let object = access.with_native(|owner| {
                owner
                    .sexp(unsafe { super::super::eval::Rf_eval(root.as_raw(), lookup.as_raw()) })?
                    .into_owned()
            })?;
            access.require_active()?;
            unsafe {
                crate::sexp::accessors::SET_NAMED(object.as_raw(), 2);
            }
            let modified_expression = access.with_native(|owner| {
                owner
                    .sexp(unsafe { crate::sexp::symbol::Rf_install(c"*vtmp*".as_ptr()) })?
                    .into_owned()
            })?;
            let mut targets = Vec::new();
            targets
                .try_reserve(levels.len())
                .map_err(|_| SexpError::AllocationFailed {
                    object: "replacement target continuations",
                })?;
            targets.push(object);
            for level in &levels[..levels.len() - 1] {
                let (arguments, tags) = level.arguments(targets.last().expect("root target"), None);
                let child = match private_target_call_owned(
                    &level.getter,
                    &arguments,
                    &tags,
                    None,
                    environment,
                    loops,
                ) {
                    Ok(value) => value,
                    Err(jump) => return Ok(Err(jump)),
                };
                access.require_active()?;
                unsafe {
                    crate::sexp::accessors::SET_NAMED(child.as_raw(), 2);
                }
                targets.push(child);
            }
            let mut changed = rhs.clone();
            for (index, level) in levels.iter().enumerate().rev() {
                let (arguments, tags) = level.arguments(&targets[index], Some(&changed));
                let code = if index + 1 == levels.len() {
                    expression
                } else {
                    &modified_expression
                };
                changed = match private_replacement_owned(
                    &level.setter,
                    &arguments,
                    &tags,
                    code,
                    environment,
                    loops,
                ) {
                    Ok(value) => value,
                    Err(jump) => return Ok(Err(jump)),
                };
            }
            access.with_native(|_| {
                unsafe {
                    if superassign {
                        setVar(root.as_raw(), changed.as_raw(), lookup.as_raw());
                    } else {
                        defineVar(root.as_raw(), changed.as_raw(), environment.as_raw());
                    }
                }
                Ok(())
            })?;
            Ok(Ok(changed))
        })
        .and_then(|result| result)
        .unwrap_or_else(|error| bc_error(error.to_string()))
    }));
    match attempt {
        Ok(result) => result,
        Err(payload) => Err(recover_loop_jump(payload, loops)),
    }
}

pub(crate) fn private_argument_tags(
    pending: &mut Vec<(usize, Sexp<'static>)>,
    top: usize,
    count: usize,
) -> Vec<Option<Sexp<'static>>> {
    let mut tags = Vec::new();
    tags.try_reserve(count)
        .unwrap_or_else(|_| bc_error("cannot allocate bytecode argument tags"));
    tags.resize_with(count, || None);
    let bottom = top
        .checked_sub(count)
        .unwrap_or_else(|| bc_error("argument tag stack underflow"));
    let mut remaining = Vec::new();
    remaining
        .try_reserve(pending.len())
        .unwrap_or_else(|_| bc_error("cannot retain bytecode outer argument tags"));
    for (depth, tag) in std::mem::take(pending) {
        if depth < bottom {
            remaining.push((depth, tag));
        } else if depth < top {
            tags[top - 1 - depth] = Some(tag);
        }
    }
    *pending = remaining;
    tags
}

pub(crate) unsafe fn stack_top_checked(stack: &R_bcstack_t, context: &str) -> SEXP {
    unsafe {
        let value = stack.top();
        if value.is_null() {
            bc_error(format!("{context} bytecode stack underflow"));
        }
        value
    }
}

pub(crate) unsafe fn stack_at_checked(stack: &R_bcstack_t, index: usize, context: &str) -> SEXP {
    unsafe {
        if index >= stack.depth() {
            bc_error(format!("{context} bytecode stack slot {index} is missing"));
        }
        stack.at(index)
    }
}

/// The operand stack already owns its live values. Retain only the actual
/// extra native value while translated operations can invoke R callbacks.
pub(crate) unsafe fn with_stack_rooted<F, R>(_stack: &R_bcstack_t, extra: SEXP, f: F) -> R
where
    F: FnOnce() -> R,
{
    let owner = unsafe { crate::sexp::owner::OwnerToken::current() }
        .unwrap_or_else(|error| bc_error(error.to_string()));
    let weak = owner
        .weak_owner()
        .unwrap_or_else(|| bc_error("bytecode callback requires a runtime owner"));
    let pin = weak
        .pin()
        .unwrap_or_else(|error| bc_error(error.to_string()));
    let _extra = if extra.is_null() {
        None
    } else {
        Some(unsafe { own_operand(extra) })
    };
    let result = f();
    pin.require_live()
        .unwrap_or_else(|error| bc_error(error.to_string()));
    result
}

pub(crate) unsafe fn bind_for_element(
    stack: &R_bcstack_t,
    sequence: SEXP,
    symbol: SEXP,
    index: c_int,
    rho: SEXP,
) {
    unsafe {
        with_stack_rooted(stack, symbol, || {
            let value = for_sequence_element(sequence, index);
            defineVar(symbol, value, rho);
        });
    }
}
