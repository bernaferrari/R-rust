//! Selected upstream base-R wrappers run by the ordinary evaluator.
//! The cache owns closures; each execution owns its inputs and working values.
use crate::sexp::{
    ffi::{SEXP, SEXPTYPE},
    globals::R_BaseEnv,
    object::{Sexp, SexpError, SexpResult},
    owner::{OwnerToken, RuntimeAccess, with_runtime},
};

pub(crate) unsafe fn apply(
    name: &'static str,
    source: &str,
    args: SEXP,
    rho: SEXP,
    evaluated: bool,
) -> SEXP {
    unsafe { apply_in_environment(name, source, args, rho, evaluated, R_BaseEnv()) }
}

/// Compile a wrapper in its owning namespace, preserving lexical helper lookup.
pub(crate) unsafe fn apply_in_environment(
    name: &'static str,
    source: &str,
    args: SEXP,
    rho: SEXP,
    evaluated: bool,
    environment: SEXP,
) -> SEXP {
    let result = (|| {
        let owner = unsafe { OwnerToken::current() }?;
        let owner = owner.weak_owner().ok_or(SexpError::RootUnavailable)?;
        with_runtime(&owner, |access| {
            apply_owned(access, name, source, args, rho, evaluated, environment)
        })?
    })();
    result
        .unwrap_or_else(|error: SexpError| {
            std::panic::panic_any(crate::sexp::context::RError {
                message: error.to_string(),
            })
        })
        .as_raw()
}

fn evaluate(
    access: &RuntimeAccess,
    expression: &Sexp<'_>,
    environment: &Sexp<'_>,
) -> SexpResult<Sexp<'static>> {
    access.with_native(|owner| {
        let result =
            unsafe { crate::eval::eval::Rf_eval(expression.as_raw(), environment.as_raw()) };
        owner.sexp(result)?.into_owned()
    })
}

fn apply_owned(
    access: &RuntimeAccess,
    name: &'static str,
    source: &str,
    args: SEXP,
    rho: SEXP,
    evaluated: bool,
    environment: SEXP,
) -> SexpResult<Sexp<'static>> {
    let domain = access.domain();
    let allocator = access.allocator(&domain)?;
    let args = domain.wrap(args)?;
    let rho = domain.wrap(rho)?;
    let environment = domain.wrap(environment)?;
    let cached = access
        .with_native(|owner| {
            Ok(unsafe { (*owner.as_ptr()).base_wrappers.borrow().get(name).cloned() })
        })?
        .filter(|fun| {
            fun.try_cloenv()
                .is_ok_and(|cached_env| cached_env == environment)
        });
    let fun = match cached {
        Some(fun) => fun,
        None => {
            let parsed = access
                .with_arena(|arena| crate::eval::parser::parse(source, arena, domain.clone()))?
                .map_err(|error| SexpError::EvaluationFailed {
                    message: format!("invalid base wrapper source: {error:?}"),
                })?;
            let fun = evaluate(access, &parsed, &environment)?;
            access.with_native(|owner| {
                unsafe {
                    (*owner.as_ptr())
                        .base_wrappers
                        .borrow_mut()
                        .insert(name, fun.clone());
                }
                Ok(())
            })?;
            fun
        }
    };
    let mut call_args = args.clone();
    if evaluated {
        let mut cells = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut remaining = args;
        while remaining.typeof_() != SEXPTYPE::NILSXP {
            let link = remaining
                .allocation()?
                .link()
                .ok_or(SexpError::StaleAllocation)?;
            seen.try_reserve(1).map_err(|_| SexpError::AllocationFailed {
                object: "base wrapper argument index",
            })?;
            if !seen.insert(link) {
                return Err(SexpError::EvaluationFailed {
                    message: "cyclic base wrapper arguments".to_owned(),
                });
            }
            cells.try_reserve(1).map_err(|_| SexpError::AllocationFailed {
                object: "base wrapper argument snapshot",
            })?;
            cells.push((remaining.try_car()?, remaining.try_tag()?));
            remaining = remaining.try_cdr()?;
        }
        call_args = domain.nil();
        for (value, tag) in cells.into_iter().rev() {
            let promise = allocator.evaluated_promise(&value, &rho)?;
            call_args = allocator.pairlist_cell(&promise, &call_args, &tag)?;
        }
    }
    let call = allocator.call(&fun, &call_args)?;
    evaluate(access, &call, &rho)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn owned_base_wrapper_rejects_cyclic_evaluated_arguments() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let args = factory
            .pairlist_cell(&factory.nil(), &factory.nil(), &factory.nil())
            .unwrap();
        unsafe { crate::sexp::accessors::SETCDR(args.as_raw(), args.as_raw()) };
        let weak = owner.weak_owner().unwrap();
        let env = session.global_env().unwrap();
        let result = with_runtime(&weak, |access| {
            apply_owned(access, "owned_wrapper_cycle", "function(x) x", args.as_raw(), env.as_raw(), true, env.as_raw())
        }).unwrap();
        assert!(matches!(result, Err(SexpError::EvaluationFailed { ref message }) if message == "cyclic base wrapper arguments"));
    }

    #[test]
    fn owned_base_wrapper_survives_cache_eviction_during_collecting_call() {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            let value = factory
                .allocate(|arena| {
                    let value = arena.alloc_vector_sexp(SEXPTYPE::INTSXP, 1)?;
                    let mut value = crate::sexp::SexpMut::try_from_checked(value).ok()?;
                    value.try_set_integer_elt(0, 17).ok()?;
                    Some(value.freeze().as_raw())
                })
                .unwrap();
            let args = factory
                .pairlist_cell(&value, &factory.nil(), &factory.nil())
                .unwrap();
            let evictions = Rc::new(Cell::new(0));
            let observed = evictions.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                let cache = &(*instance).base_wrappers;
                if !cache.borrow().is_empty() {
                    cache.borrow_mut().clear();
                    observed.set(observed.get() + 1);
                }
                crate::sexp::gengc::full_gc_in(instance);
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = apply(
                "owned_wrapper_gc",
                "function(x) { gc(); x + 1 }",
                args.as_raw(),
                (*instance).global_env,
                true,
            );
            let result = factory.wrap(result).unwrap();
            assert_eq!(result.real_elt(0), Some(18.0));
            assert!(
                evictions.get() > 0,
                "the active closure must actually lose its cache entry"
            );
        });
    }

    #[test]
    fn owned_base_wrapper_releases_evicted_closure_without_preserve_root() {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let preserved = (*instance).preserve_stack.checked_entries_snapshot().len();
            apply(
                "owned_wrapper_release",
                "function() 23L",
                crate::sexp::globals::R_NilValue(),
                (*instance).global_env,
                false,
            );
            let closure = (*instance)
                .base_wrappers
                .borrow()
                .get("owned_wrapper_release")
                .unwrap()
                .allocation()
                .unwrap()
                .clone();
            assert_eq!((*instance).preserve_stack.checked_entries_snapshot().len(), preserved);
            crate::sexp::gengc::full_gc_in(instance);
            assert!(closure.is_live());
            (*instance).base_wrappers.borrow_mut().clear();
            crate::sexp::gengc::full_gc_in(instance);
            assert!(
                !closure.is_live(),
                "eviction must release the actual closure root"
            );
        });
    }
}
