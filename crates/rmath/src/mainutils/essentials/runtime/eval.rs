//! `eval`, `substitute`, `quote`, `parse` plus source-parsing helpers.

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use crate::mainutils::essentials::*;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADDR, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT,
    LENGTH, LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_NAMED,
    SET_OBJECT, SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF,
    VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
#[allow(unused_imports)]
use crate::sexp::context::RError;
#[allow(unused_imports)]
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
#[allow(unused_imports)]
use crate::sexp::globals::{R_MissingArg, R_NilValue};
#[allow(unused_imports)]
use crate::sexp::protect::protect;
#[allow(unused_imports)]
use crate::sexp::symbol::Rf_install;

#[cfg(test)]
mod eval_expression_tests {
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn owned_eval_expression_null_and_visibility_match_gnu() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let environment = session.global_env().unwrap().into_owned().unwrap();
        // Pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5.
        for (script, visible) in [
            ("eval(expression())", TRUE),
            ("eval(expression(1L,NULL))", TRUE),
            ("eval(expression(invisible(1L),NULL))", TRUE),
            ("eval(expression(1L,invisible(NULL)))", FALSE),
        ] {
            let expression = owner
                .with_arena(|arena| crate::eval::parser::parse(script, arena, factory.domain()))
                .unwrap()
                .unwrap();
            unsafe {
                let value = owner
                    .sexp(crate::eval::eval::Rf_eval(
                        expression.as_raw(),
                        environment.as_raw(),
                    ))
                    .unwrap()
                    .into_owned()
                    .unwrap();
                assert!(value.is_nil(), "source {script}");
                assert_eq!(
                    crate::sexp::globals::R_Visible(),
                    visible,
                    "source {script}"
                );
                let code = owner
                    .sexp(
                        crate::eval::bc_compile::compile_expr(
                            expression.as_raw(),
                            environment.as_raw(),
                        )
                        .expect("eval call must compile"),
                    )
                    .unwrap()
                    .into_owned()
                    .unwrap();
                SET_VECTOR_ELT(
                    crate::eval::bc_eval::BCODE_CONSTS(code.as_raw()),
                    0,
                    R_NilValue(),
                );
                let value = owner
                    .sexp(crate::eval::bc_eval::bcEval(
                        code.as_raw(),
                        environment.as_raw(),
                    ))
                    .unwrap()
                    .into_owned()
                    .unwrap();
                assert!(value.is_nil(), "source-erased private bytecode {script}");
                assert_eq!(
                    crate::sexp::globals::R_Visible(),
                    visible,
                    "private {script}"
                );
            }
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn owned_eval_expression_gnu_fixture_preserves_null_and_visibility() {
        let mut session = RSession::new_for_gc_tests();
        let bytes = include_bytes!(
            "../../../../../r-embed/tests/fixtures/gnu-bytecode-eval-expression/eval.rds"
        );
        let raw = bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let (result, _, _) = session
            .eval_code_with_output_capture(&format!("f<-unserialize(as.raw(c({raw})));NULL"));
        assert!(result.unwrap().is_nil());
        unsafe {
            let owner = session.owner_token().unwrap();
            let function = owner
                .sexp(crate::sexp::envir::R_findVar(
                    Rf_install(c"f".as_ptr()),
                    session.global_env().unwrap().as_raw(),
                ))
                .unwrap()
                .into_owned()
                .unwrap();
            let body = owner
                .sexp(crate::sexp::accessors::BODY(function.as_raw()))
                .unwrap()
                .into_owned()
                .unwrap();
            assert!(crate::eval::bc_eval::BCODE_IS_GNU(body.as_raw()));
            let instructions = owner
                .sexp(VECTOR_ELT(body.as_raw(), 0))
                .unwrap()
                .into_owned()
                .unwrap();
            let words = (0..instructions.len())
                .map(|index| instructions.integer_elt(index).unwrap())
                .collect::<Vec<_>>();
            let constants = owner
                .sexp(crate::eval::bc_eval::BCODE_CONSTS(body.as_raw()))
                .unwrap()
                .into_owned()
                .unwrap();
            assert!(
                crate::eval::bytecode::validate_gnu_adapter_with_constants(
                    &words,
                    constants.as_raw()
                )
                .unwrap()
            );
            // GNU constant zero is also the CALL's syntax operand; retain it.
            // Invoke the verified tagged GNU body directly: bcEval dispatches
            // to the hard-fail adapter and has no retained-source fallback.
            let quote = owner
                .with_arena(|arena| {
                    crate::eval::parser::parse(
                        "quote(return(7L))",
                        arena,
                        owner.node_factory().domain(),
                    )
                })
                .unwrap()
                .unwrap();
            let environment = session.global_env().unwrap().into_owned().unwrap();
            let argument = owner
                .sexp(crate::eval::eval::Rf_eval(
                    quote.as_raw(),
                    environment.as_raw(),
                ))
                .unwrap()
                .into_owned()
                .unwrap();
            crate::sexp::envir::defineVar(
                Rf_install(c"e".as_ptr()),
                argument.as_raw(),
                environment.as_raw(),
            );
            let value = owner
                .sexp(crate::eval::bc_eval::bcEval(
                    body.as_raw(),
                    environment.as_raw(),
                ))
                .unwrap()
                .into_owned()
                .unwrap();
            assert_eq!(
                value.integer_elt(0),
                Some(7),
                "direct supported GNU bytecode backend"
            );
        }
        for (script, expected, visible) in [
            ("f(expression())", None, TRUE),
            ("f(expression(1L,NULL))", None, TRUE),
            ("f(expression(invisible(1L),NULL))", None, TRUE),
            ("f(expression(1L,invisible(NULL)))", None, FALSE),
            ("f(quote(return(7L)))", Some(7), TRUE),
            ("f(quote(return(invisible(7L))))", Some(7), FALSE),
            ("f(expression(return(7L),9L))", Some(7), TRUE),
        ] {
            let (result, _, _) = session.eval_code_with_output_capture(script);
            let result = result.unwrap();
            if let Some(expected) = expected {
                assert_eq!(result.integer_elt(0), Some(expected), "{script}");
            } else {
                assert!(result.is_nil(), "{script}");
            }
            assert_eq!(crate::sexp::globals::R_Visible(), visible, "{script}");
        }
    }

    #[test]
    fn owned_eval_environment_conversion_matches_gnu() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let environment = session.global_env().unwrap().into_owned().unwrap();
        // GNU's public eval wrapper contributes a frame for negative numeric
        // envir. This primitive-only fixture installs that wrapper explicitly.
        let wrapper = owner.with_arena(|arena| {
            crate::eval::parser::parse(
                "eval.gnu<-function(expr,envir,enclos=baseenv()).Internal(eval(expr,envir,enclos))",
                arena,factory.domain(),
            )
        }).unwrap().unwrap();
        unsafe {
            let _ = crate::eval::eval::Rf_eval(wrapper.as_raw(), environment.as_raw());
        }
        for (script, expected) in [
            ("eval(quote(x),list(x=7L))", 7),
            ("eval(quote(x),pairlist(x=7L))", 7),
            ("{x<-7L;eval(quote(x),0L)}", 7),
            ("{x<-7L;eval(quote(x),0)}", 7),
            ("{x<-7L;eval(quote(x),NULL,environment())}", 7),
            (
                "{x<-11L;f<-function(){x<-7L;eval.gnu(quote(x),0L)};f()}",
                11,
            ),
            (
                "{x<-11L;f<-function(){x<-7L;eval.gnu(quote(x),-1L)};f()}",
                7,
            ),
        ] {
            let expression = owner
                .with_arena(|arena| crate::eval::parser::parse(script, arena, factory.domain()))
                .unwrap()
                .unwrap();
            unsafe {
                let value = owner
                    .sexp(crate::eval::eval::Rf_eval(
                        expression.as_raw(),
                        environment.as_raw(),
                    ))
                    .unwrap()
                    .into_owned()
                    .unwrap();
                assert_eq!(value.integer_elt(0), Some(expected), "{script}");
            }
        }
        for script in [
            "eval(1L,TRUE)",
            "eval(NULL,TRUE)",
            "eval(expression(),TRUE)",
            "eval(1L,c(0L,1L))",
        ] {
            let expression = owner
                .with_arena(|arena| crate::eval::parser::parse(script, arena, factory.domain()))
                .unwrap()
                .unwrap();
            let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                crate::eval::eval::Rf_eval(expression.as_raw(), environment.as_raw())
            }))
            .expect_err(script);
            assert!(
                error.downcast_ref::<RError>().is_some()
                    || matches!(
                        error.downcast_ref::<crate::sexp::context::RSignal>(),
                        Some(crate::sexp::context::RSignal::Error { .. })
                    ),
                "{script}"
            );
        }
    }

    #[test]
    fn owned_eval_return_is_local_to_exact_eval_context() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let environment = session.global_env().unwrap().into_owned().unwrap();
        for (script, expected, visible) in [
            ("eval(quote(return(7L)))", 7, TRUE),
            ("eval(quote(return(invisible(7L))))", 7, FALSE),
            ("eval(expression(return(7L),9L))", 7, TRUE),
            ("{f<-function(){eval(quote(return(7L)));9L};f()}", 9, TRUE),
            (
                "{f<-function(){eval(quote({gc();return(7L)}));9L};f()}",
                9,
                TRUE,
            ),
        ] {
            let expression = owner
                .with_arena(|arena| crate::eval::parser::parse(script, arena, factory.domain()))
                .unwrap()
                .unwrap();
            session.with_active(|| unsafe {
                let result = crate::eval::eval::eval_expr(expression.clone(), environment.clone());
                let value = result.unwrap();
                assert_eq!(value.integer_elt(0), Some(expected), "source {script}");
                assert_eq!(
                    crate::sexp::globals::R_Visible(),
                    visible,
                    "source {script}"
                );
                let code = owner
                    .sexp(
                        crate::eval::bc_compile::compile_expr(
                            expression.as_raw(),
                            environment.as_raw(),
                        )
                        .expect("eval return call must compile"),
                    )
                    .unwrap()
                    .into_owned()
                    .unwrap();
                SET_VECTOR_ELT(
                    crate::eval::bc_eval::BCODE_CONSTS(code.as_raw()),
                    0,
                    R_NilValue(),
                );
                let value = owner
                    .sexp(crate::eval::bc_eval::bcEval(
                        code.as_raw(),
                        environment.as_raw(),
                    ))
                    .unwrap()
                    .into_owned()
                    .unwrap();
                assert_eq!(value.integer_elt(0), Some(expected), "private {script}");
                assert_eq!(
                    crate::sexp::globals::R_Visible(),
                    visible,
                    "private {script}"
                );
            });
        }
    }

    #[test]
    fn owned_eval_native_direct_entry_installs_and_releases_original_transfer_scope() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let environment = session.global_env().unwrap().into_owned().unwrap();
        assert!(crate::sexp::transfer::active_owner_pin().is_err());
        for script in [
            "eval(quote(return(7L)))",
            "eval(quote(eval(quote(return(7L)))))",
        ] {
            let expression = owner
                .with_arena(|arena| crate::eval::parser::parse(script, arena, factory.domain()))
                .unwrap()
                .unwrap();
            // Deliberately use the native entry with the current managed owner
            // and no with_active scope. eval must provide its local protocol.
            let result =
                unsafe { crate::eval::eval::eval_expr(expression, environment.clone()) }.unwrap();
            assert_eq!(result.integer_elt(0), Some(7), "{script}");
            assert!(
                crate::sexp::transfer::active_owner_pin().is_err(),
                "native scope must clean up"
            );
        }
    }

    #[test]
    fn owned_eval_preserves_unmatched_original_return_ticket_after_full_gc() {
        use crate::sexp::{context::RSignal, transfer::OwnedTransfer};
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let environment = session.global_env().unwrap().into_owned().unwrap();
        let expression = owner
            .with_arena(|arena| crate::eval::parser::parse("{gc();9L}", arena, factory.domain()))
            .unwrap()
            .unwrap();
        let value = unsafe {
            owner
                .sexp(Rf_ScalarInteger(7))
                .unwrap()
                .into_owned()
                .unwrap()
        };
        let pending = std::rc::Rc::new(std::cell::RefCell::new(Some(value)));
        session.with_active(|| unsafe {
            let outer = crate::sexp::context::begin_context_guard(
                crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                    | crate::sexp::context::ctxt_flags::CTXT_RETURN,
                R_NilValue(),
                environment.as_raw(),
                environment.as_raw(),
                None,
                R_NilValue(),
                R_NilValue(),
            );
            let target = outer.context();
            let callback_value = pending.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                let Some(value) = callback_value.borrow_mut().take() else {
                    return;
                };
                let ticket = crate::sexp::context::return_transfer(target, value.as_raw());
                (*target).returnValue.replace_from_raw(R_NilValue());
                crate::sexp::gengc::full_gc();
                std::panic::panic_any(RSignal::Return(ticket));
            }));
            let args = owner
                .sexp(Rf_cons(expression.as_raw(), R_NilValue()))
                .unwrap()
                .into_owned()
                .unwrap();
            let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                do_eval(
                    R_NilValue(),
                    R_NilValue(),
                    args.as_raw(),
                    environment.as_raw(),
                )
            }))
            .expect_err("eval must rethrow a return targeting the original outer context");
            assert!(pending.borrow().is_none());
            let signal = payload
                .downcast::<RSignal>()
                .expect("original return signal");
            let RSignal::Return(ticket) = *signal else {
                panic!("original return signal");
            };
            let lease = ticket.take().unwrap();
            lease.require_live().unwrap();
            let OwnedTransfer::Return {
                target: Some(original),
                value,
            } = lease.data()
            else {
                panic!("original return target/value");
            };
            assert_eq!(original.get(), target);
            crate::sexp::gengc::full_gc();
            assert_eq!(value.integer_elt(0), Some(7));
        });
    }

    #[test]
    fn owned_eval_selected_expression_survives_detachment_and_full_gc() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let environment = session.global_env().unwrap().into_owned().unwrap();
        let source = owner
            .with_arena(|arena| {
                crate::eval::parser::parse(
                    "expression({gc();list(value=7L)})",
                    arena,
                    factory.domain(),
                )
            })
            .unwrap()
            .unwrap();
        let expression = unsafe {
            owner
                .sexp(crate::eval::eval::Rf_eval(
                    source.as_raw(),
                    environment.as_raw(),
                ))
                .unwrap()
                .into_owned()
                .unwrap()
        };
        drop(source);
        let original = owner.weak_owner().unwrap();
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let captured_count = count.clone();
        let captured_expression = expression.clone();
        session.with_active_in(|instance| unsafe {
            (*instance).error_state.current_srcref_location = Some(("outer.R".into(), 17));
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if captured_count.replace(1) == 0 {
                    SET_VECTOR_ELT(captured_expression.as_raw(), 0, R_NilValue());
                    crate::sexp::gengc::full_gc();
                }
            }));
        });
        let result = crate::sexp::owner::with_runtime(&original, |access| {
            let domain = access.domain();
            let allocator = access.allocator(&domain).unwrap();
            let args = allocator
                .pairlist_cell(&expression, &domain.nil(), &domain.nil())
                .unwrap();
            drop(expression);
            eval_owned(access, &domain.nil(), &domain.nil(), &args, &environment)
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            count.get(),
            1,
            "the selected expression must really collect"
        );
        assert_eq!(result.try_vector_elt(0).unwrap().integer_elt(0), Some(7));
        session.with_active_in(|instance| unsafe {
            assert_eq!(
                (*instance).error_state.current_srcref_location,
                Some(("outer.R".into(), 17))
            );
        });
    }

    #[test]
    fn owned_eval_revoked_runtime_restores_original_location_without_success() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let original = owner.weak_owner().unwrap();
        let pin = original.pin().unwrap();
        let factory = owner.node_factory();
        let environment = session.global_env().unwrap().into_owned().unwrap();
        let source = owner
            .with_arena(|arena| {
                crate::eval::parser::parse("expression({gc();7L})", arena, factory.domain())
            })
            .unwrap()
            .unwrap();
        let expression = unsafe {
            owner
                .sexp(crate::eval::eval::Rf_eval(
                    source.as_raw(),
                    environment.as_raw(),
                ))
                .unwrap()
                .into_owned()
                .unwrap()
        };
        let revoked = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed = revoked.clone();
        session.with_active_in(|instance| unsafe {
            (*instance).error_state.current_srcref_location = Some(("caller.R".into(), 23));
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !observed.replace(true) {
                    assert!(!(*instance).context_stack.is_empty());
                    assert!((*instance).error_state.current_srcref_location.is_none());
                    crate::sexp::instance::revoke_instance_availability(instance);
                }
            }));
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::sexp::owner::with_runtime(&original, |access| {
                    let domain = access.domain();
                    let allocator = access.allocator(&domain).unwrap();
                    let args = allocator
                        .pairlist_cell(&expression, &domain.nil(), &domain.nil())
                        .unwrap();
                    eval_owned(access, &domain.nil(), &domain.nil(), &args, &environment)
                })
            }));
            assert!(
                revoked.get(),
                "the GC callback must really revoke the runtime"
            );
            assert!(
                !matches!(outcome, Ok(Ok(Ok(_)))),
                "revoked eval cannot publish success"
            );
        });
        assert!(original.pin().is_err());
        unsafe {
            assert_eq!(
                (*pin.as_ptr()).error_state.current_srcref_location,
                Some(("caller.R".into(), 23))
            );
            assert!((*pin.as_ptr()).context_stack.is_empty());
        }
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime: eval, substitute, quote, parse
// ---------------------------------------------------------------------------

/// R's `local(expr, envir = new.env())` — evaluate `expr` in a fresh child
/// environment and return its value (eval.c `do_local`). The default
/// environment parents to the caller (`_rho`); an explicit ENVSXP `envir`
/// is used as-is (wrapped in a child so assignments stay local).
pub unsafe fn do_local(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let envir_arg = CAR(CDR(args));
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        let parent = if envir_arg.is_null() || envir_arg == R_NilValue() {
            _rho
        } else {
            envir_arg
        };
        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), parent, R_NilValue());
        if env.is_null() {
            return R_NilValue();
        }
        let _guard = protect(env);
        crate::eval::eval::Rf_eval(expr, env)
    }
}

/// Native adapter: retain all inputs before environment conversion or R callbacks.
pub unsafe fn do_eval(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::sexp::{
        object::SexpError,
        owner::{OwnerToken, with_runtime},
    };
    let result = (|| {
        let token = unsafe { OwnerToken::current()? };
        let original = token.weak_owner().ok_or(SexpError::RootUnavailable)?;
        with_runtime(&original, |access| {
            // Native direct callers can have a current managed runtime without
            // an outer session activation. Reuse the canonical original-owner
            // transfer scope so eval-local returns can publish owning tickets.
            let _transfers = crate::sexp::transfer::TransferScopeGuard::enter(original.clone())?;
            let domain = access.domain();
            let own = |raw: SEXP| {
                if raw.is_null() {
                    Ok(domain.nil())
                } else {
                    domain.wrap(raw)
                }
            };
            let call = own(call)?;
            let op = own(op)?;
            let args = own(args)?;
            let rho = own(rho)?;
            eval_owned(access, &call, &op, &args, &rho)
        })?
    })();
    result
        .unwrap_or_else(|error: SexpError| {
            let message = match error {
                SexpError::EvaluationFailed { message } => message,
                other => format!("eval failed: {other}"),
            };
            std::panic::panic_any(RError { message })
        })
        .as_raw()
}

fn invalid_eval_environment(kind: SEXPTYPE) -> crate::sexp::object::SexpError {
    // type2char returns a static language-type label and cannot invoke R.
    let label = unsafe { CStr::from_ptr(crate::mainutils::util_main::type2char(kind.as_c_int())) };
    crate::sexp::object::SexpError::EvaluationFailed {
        message: format!(
            "invalid 'envir' argument of type '{}'",
            label.to_string_lossy()
        ),
    }
}

fn eval_owned(
    access: &crate::sexp::owner::RuntimeAccess,
    call: &crate::sexp::object::Sexp<'static>,
    op: &crate::sexp::object::Sexp<'static>,
    args: &crate::sexp::object::Sexp<'static>,
    rho: &crate::sexp::object::Sexp<'static>,
) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>> {
    use crate::sexp::object::SexpError;
    let domain = access.domain();
    let _inputs = (
        domain.link(call)?,
        domain.link(op)?,
        domain.link(args)?,
        domain.link(rho)?,
    );
    // Snapshot actual argument values before allocation can detach their cells.
    let mut values = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut cell = args.clone();
    while !cell.is_nil() {
        seen.try_reserve(1)
            .map_err(|_| SexpError::AllocationFailed {
                object: "eval arguments",
            })?;
        if !seen.insert(cell.as_raw().addr()) {
            return Err(SexpError::EvaluationFailed {
                message: "cyclic eval arguments".into(),
            });
        }
        values
            .try_reserve(1)
            .map_err(|_| SexpError::AllocationFailed {
                object: "eval arguments",
            })?;
        values.push(cell.try_car()?);
        cell = cell.try_cdr()?;
    }
    let expr = values.first().cloned().unwrap_or_else(|| domain.nil());
    let mut env = values.get(1).cloned().unwrap_or_else(|| domain.missing());
    let mut encl = values.get(2).cloned().unwrap_or_else(|| domain.nil());
    let missing = domain.missing();
    let caller_call = access.with_native(|owner| {
        let context = unsafe { crate::sexp::context::R_GlobalContext() };
        if context.is_null() {
            return Ok(call.clone());
        }
        let raw = unsafe { (*context).call.as_raw() };
        if raw.is_null() || raw == domain.nil().as_raw() {
            Ok(call.clone())
        } else {
            owner.sexp(raw)?.into_owned()
        }
    })?;
    if encl.is_nil() || encl.as_raw() == missing.as_raw() {
        encl = access
            .with_native(|owner| owner.sexp(crate::eval::runtime::base_env())?.into_owned())?;
    } else if encl.typeof_() != SEXPTYPE::ENVSXP {
        return Err(SexpError::EvaluationFailed {
            message: "invalid 'enclos' argument".into(),
        });
    }
    if values.len() < 2 || env.as_raw() == missing.as_raw() {
        env = rho.clone();
    } else {
        match env.typeof_() {
            SEXPTYPE::NILSXP => env = encl.clone(),
            SEXPTYPE::ENVSXP => {}
            SEXPTYPE::LISTSXP | SEXPTYPE::VECSXP => {
                let frame = access.with_native(|owner| {
                    let raw = unsafe {
                        if env.typeof_() == SEXPTYPE::LISTSXP {
                            crate::mainutils::duplicate::Rf_duplicate(env.as_raw())
                        } else {
                            crate::eval::missing::VectorToPairListNamed(env.as_raw())
                        }
                    };
                    owner.sexp(raw)?.into_owned()
                })?;
                if env.typeof_() == SEXPTYPE::VECSXP {
                    let mut cell = frame.clone();
                    while !cell.is_nil() {
                        let value = cell.try_car()?;
                        unsafe { SET_NAMED(value.as_raw(), 2) };
                        cell = cell.try_cdr()?;
                    }
                }
                env = access.with_native(|owner| {
                    owner
                        .sexp(unsafe {
                            crate::sexp::memory_ext::NewEnvironment(
                                frame.as_raw(),
                                encl.as_raw(),
                                domain.nil().as_raw(),
                            )
                        })?
                        .into_owned()
                })?;
            }
            SEXPTYPE::INTSXP | SEXPTYPE::REALSXP => {
                if env.len() != 1 {
                    return Err(SexpError::EvaluationFailed {
                        message: "numeric 'envir' arg not of length one".into(),
                    });
                }
                let frame = access.with_native(|_| {
                    Ok(unsafe { crate::mainutils::coerce::asInteger(env.as_raw()) })
                })?;
                if frame == NA_INTEGER {
                    return Err(invalid_eval_environment(env.typeof_()));
                }
                env = access.with_native(|owner| {
                    owner
                        .sexp(unsafe {
                            crate::eval::context::R_sysframe(frame, std::ptr::null_mut())
                        })?
                        .into_owned()
                })?;
            }
            _ => {
                return Err(invalid_eval_environment(env.typeof_()));
            }
        }
    }
    let evalable = matches!(
        expr.typeof_(),
        SEXPTYPE::LANGSXP | SEXPTYPE::SYMSXP | SEXPTYPE::BCODESXP | SEXPTYPE::EXPRSXP
    );
    let _context = if evalable {
        Some(access.with_native(|_| {
            Ok(unsafe {
                crate::sexp::context::begin_context_guard(
                    crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                        | crate::sexp::context::ctxt_flags::CTXT_RETURN,
                    caller_call.as_raw(),
                    env.as_raw(),
                    rho.as_raw(),
                    None,
                    op.as_raw(),
                    args.as_raw(),
                )
            })
        })?)
    } else {
        None
    };
    let evaluated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eval_body_owned(access, &expr, &env)
    }));
    match evaluated {
        Ok(value) => value,
        Err(payload) => {
            use crate::sexp::{context::RSignal, transfer::OwnedTransfer};
            if let (Some(context), Some(RSignal::Return(ticket))) =
                (_context.as_ref(), payload.downcast_ref::<RSignal>())
            {
                let lease = ticket.resolve()?;
                lease.require_live()?;
                if matches!(lease.data(), OwnedTransfer::Return { target: Some(target), .. }
                    if target.get() == context.context())
                {
                    // Only this original context cell may consume the ticket.
                    // The taken lease retains the actual returned value until
                    // it has an independent owning result; unmatched signals
                    // keep their original ticket and continue unwinding.
                    let signal = payload.downcast::<RSignal>().expect("matched eval return");
                    let RSignal::Return(ticket) = *signal else {
                        unreachable!()
                    };
                    let lease = ticket.take()?;
                    lease.require_live()?;
                    let OwnedTransfer::Return { value, .. } = lease.data() else {
                        unreachable!()
                    };
                    let value = value.clone();
                    access.require_active()?;
                    return Ok(value);
                }
            }
            std::panic::resume_unwind(payload)
        }
    }
}

fn eval_body_owned(
    access: &crate::sexp::owner::RuntimeAccess,
    expr: &crate::sexp::object::Sexp<'static>,
    env: &crate::sexp::object::Sexp<'static>,
) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>> {
    use crate::sexp::object::SexpError;
    let domain = access.domain();
    if matches!(
        expr.typeof_(),
        SEXPTYPE::LANGSXP | SEXPTYPE::SYMSXP | SEXPTYPE::BCODESXP
    ) {
        return access.with_native(|owner| {
            owner
                .sexp(unsafe { crate::eval::eval::Rf_eval(expr.as_raw(), env.as_raw()) })?
                .into_owned()
        });
    }
    if expr.typeof_() == SEXPTYPE::EXPRSXP {
        let _location = access.with_native(|owner| {
            let instance = owner.as_ptr();
            Ok(EvalSrcrefGuard {
                instance,
                previous: unsafe { (*instance).error_state.current_srcref_location.clone() },
                _pin: owner.pin()?.ok_or(SexpError::RootUnavailable)?,
            })
        })?;
        let mut result = domain.nil();
        access.with_native(|_| {
            crate::sexp::globals::set_R_Visible(TRUE);
            Ok(())
        })?;
        for index in 0..expr.len() {
            // NULL is an evaluable element too: it replaces the prior result
            // and restores visibility. Own the selected child before srcref
            // setup or evaluation can replace its slot and collect the graph.
            let element = expr.try_vector_elt(index)?;
            access.with_native(|_| {
                crate::mainutils::srcref::set_current_srcref_location(
                    element.as_raw(),
                    expr.as_raw(),
                    index as usize,
                );
                Ok(())
            })?;
            result = access.with_native(|owner| {
                owner
                    .sexp(unsafe { crate::eval::eval::Rf_eval(element.as_raw(), env.as_raw()) })?
                    .into_owned()
            })?;
        }
        access.require_active()?;
        return Ok(result);
    }
    access.with_native(|_| {
        crate::sexp::globals::set_R_Visible(TRUE);
        Ok(())
    })?;
    Ok(expr.clone())
}

struct EvalSrcrefGuard {
    instance: *mut crate::sexp::instance::RInstance,
    previous: Option<(String, i32)>,
    _pin: crate::sexp::owner::OwnerPin,
}

impl Drop for EvalSrcrefGuard {
    fn drop(&mut self) {
        // Restore the original pinned runtime even after callback revocation.
        unsafe {
            (*self.instance).error_state.current_srcref_location = self.previous.take();
        }
    }
}

/// R's `substitute(expr, env)` — substitute symbols in expression.
pub unsafe fn do_substitute(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::coerce::do_substitute(_call, _op, args, _rho) }
}

/// R's `quote(expr)` — return expression unevaluated.
pub unsafe fn do_quote(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, NAMED, SET_NAMED};
        let mut nargs = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            nargs += 1;
            current = CDR(current);
        }
        if nargs != 1 {
            base_error(format!(
                "{nargs} arguments passed to 'quote' which requires 1"
            ));
        }
        let tag = TAG(args);
        if !tag.is_null() && tag != R_NilValue() {
            let name = if TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let printname = PRINTNAME(tag);
                if printname.is_null() {
                    String::new()
                } else {
                    let chars = CHAR(printname);
                    if chars.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(chars).to_string_lossy().into_owned()
                    }
                }
            } else {
                String::new()
            };
            if name != "expr" {
                base_error(format!(
                    "supplied argument name '{name}' does not match 'expr'"
                ));
            }
        }
        let val = CAR(args);
        if val.is_null() || val == R_NilValue() {
            return R_NilValue();
        }
        // ENSURE_NAMEDMAX — prevent modification of source code references
        if NAMED(val) < 2 {
            SET_NAMED(val, 2);
        }
        val
    }
}

/// R's `parse(text)` — parse R code strings into an expression vector.
pub unsafe fn do_parse(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let keep_source = {
            let explicit = arg_by_name_or_position(args, &["keep.source"], usize::MAX);
            let opt = if explicit != R_NilValue() {
                explicit
            } else {
                crate::mainutils::options::GetOption1(crate::sexp::symbol::Rf_install(
                    c"keep.source".as_ptr(),
                ))
            };
            !opt.is_null() && crate::mainutils::coerce::asLogical(opt) == 1
        };
        let text_arg = arg_by_name_or_position(args, &["text"], 0);
        let file_arg = arg_by_name_or_position(args, &["file"], 0);
        if text_arg.is_null() || text_arg == R_NilValue() {
            if !file_arg.is_null() && file_arg != R_NilValue() {
                let file_path = elt_to_string(file_arg, 0);
                let content = crate::mainutils::browser_files::read_text_or_host(&file_path)
                    .unwrap_or_else(|err| {
                        base_error(format!("cannot open file '{}': {}", file_path, err))
                    });
                if keep_source {
                    return parse_with_srcrefs(&content, &file_path);
                }
                return parse_source_expression_vector(&content);
            }
            return Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
        }

        let n = XLENGTH(text_arg);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
        }

        let mut source = Vec::with_capacity(n as usize);
        for i in 0..n {
            if TYPEOF(text_arg) == SEXPTYPE::STRSXP && is_string_na(text_arg, i) {
                std::panic::panic_any(RError {
                    message: "invalid 'text' argument".to_string(),
                });
            }
            let text = elt_to_string(text_arg, i);
            source.push(text);
        }
        let combined = source.join("\n");
        if keep_source {
            // Upstream parse(text=) attributes an unnamed srcfile (the
            // renderer falls back to `(from #n)` for it).
            return parse_with_srcrefs(&combined, "<text>");
        }
        parse_source_strings(&source)
    }
}

/// Parse with byte spans and attach srcrefs + srcfile (keep.source).
pub(crate) unsafe fn parse_with_srcrefs(content: &str, filename: &str) -> SEXP {
    unsafe {
        let parser_factory = crate::eval::parser::active_factory();
        let spans = crate::sexp::memory::with_arena(|arena| {
            let mut parser =
                crate::eval::parser::Parser::new(content, arena, parser_factory.clone());
            parser.set_keep_srcrefs(true);
            parser
                .parse_top_level_with_spans()
                .map_err(|e| e.to_string())
        });
        match spans {
            Ok(spans) => {
                let exprs: Vec<SEXP> = spans.iter().map(|(e, _, _)| e.clone().as_raw()).collect();
                let vec_sexp = crate::sexp::constructors::Rf_allocVector3(
                    SEXPTYPE::EXPRSXP,
                    exprs.len() as i64,
                );
                let _vg = crate::sexp::protect::protect(vec_sexp);
                for (i, &e) in exprs.iter().enumerate() {
                    crate::sexp::accessors::SET_VECTOR_ELT(vec_sexp, i as i64, e);
                }
                crate::mainutils::srcref::attach_srcrefs_with_spans(
                    &spans
                        .iter()
                        .map(|(e, start, end)| (e.clone().as_raw(), *start, *end))
                        .collect::<Vec<_>>(),
                    content,
                    filename,
                    vec_sexp,
                );
                vec_sexp
            }
            Err(msg) => {
                std::panic::panic_any(RError { message: msg });
            }
        }
    }
}

unsafe fn parse_source_strings(source: &[String]) -> SEXP {
    let combined = source.join("\n");
    unsafe { parse_source_expression_vector(&combined) }
}

pub(crate) unsafe fn parse_source_expression_vector(source: &str) -> SEXP {
    unsafe {
        let parser_factory = crate::eval::parser::active_factory();
        let parsed = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse_expressions_strict(source, arena, parser_factory.clone())
                .map_err(|err| err.to_string())
        })
        .unwrap_or_else(|message| std::panic::panic_any(RError { message }));

        let result = Rf_allocVector3(SEXPTYPE::EXPRSXP, parsed.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, value) in parsed.iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, value.clone().as_raw());
        }
        result
    }
}

unsafe fn d_symbol_name(sym: SEXP) -> String {
    unsafe {
        if TYPEOF(sym) != SEXPTYPE::SYMSXP {
            return String::new();
        }
        let pn = PRINTNAME(sym);
        if pn.is_null() {
            return String::new();
        }
        CStr::from_ptr(CHAR(pn)).to_string_lossy().into_owned()
    }
}

unsafe fn d_numeric(x: SEXP) -> Option<f64> {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::REALSXP && XLENGTH(x) == 1 {
            Some(*REAL(x))
        } else if TYPEOF(x) == SEXPTYPE::INTSXP && XLENGTH(x) == 1 {
            Some(*INTEGER(x) as f64)
        } else {
            None
        }
    }
}

unsafe fn d_diff(expr: SEXP, var: &str) -> SEXP {
    unsafe {
        if TYPEOF(expr) == SEXPTYPE::SYMSXP {
            return if d_symbol_name(expr) == var {
                Rf_ScalarInteger(1)
            } else {
                Rf_ScalarInteger(0)
            };
        }
        if d_numeric(expr).is_some() {
            return Rf_ScalarInteger(0);
        }
        if TYPEOF(expr) != SEXPTYPE::LANGSXP {
            return Rf_ScalarInteger(0);
        }
        let op = CAR(expr);
        let name = d_symbol_name(op);
        if name == "^" {
            let base = CAR(CDR(expr));
            let exp = CAR(CDR(CDR(expr)));
            if d_symbol_name(base) == var {
                if let Some(n) = d_numeric(exp) {
                    if (n - 1.0).abs() < 1e-15 {
                        return Rf_ScalarInteger(1);
                    }
                    let n_s = Rf_ScalarReal(n);
                    if (n - 2.0).abs() < 1e-15 {
                        return crate::sexp::constructors::Rf_lang3(
                            Rf_install(c"*".as_ptr()),
                            n_s,
                            base,
                        );
                    }
                    let nm1 = Rf_ScalarReal(n - 1.0);
                    let pow =
                        crate::sexp::constructors::Rf_lang3(Rf_install(c"^".as_ptr()), base, nm1);
                    return crate::sexp::constructors::Rf_lang3(
                        Rf_install(c"*".as_ptr()),
                        n_s,
                        pow,
                    );
                }
            }
        }
        if name == "+" || name == "-" {
            let a = d_diff(CAR(CDR(expr)), var);
            let b = d_diff(CAR(CDR(CDR(expr))), var);
            if let Some(bv) = d_numeric(b) {
                if bv == 0.0 {
                    return a;
                }
            }
            if name == "+" {
                if let Some(av) = d_numeric(a) {
                    if av == 0.0 {
                        return b;
                    }
                }
            }
            return crate::sexp::constructors::Rf_lang3(op, a, b);
        }
        if name == "*" {
            let a = CAR(CDR(expr));
            let b = CAR(CDR(CDR(expr)));
            if d_numeric(a).is_some() {
                let db = d_diff(b, var);
                if d_numeric(db) == Some(1.0) {
                    return a;
                }
                if d_numeric(db) == Some(0.0) {
                    return Rf_ScalarInteger(0);
                }
                return crate::sexp::constructors::Rf_lang3(op, a, db);
            }
            if d_numeric(b).is_some() {
                let da = d_diff(a, var);
                if d_numeric(da) == Some(1.0) {
                    return b;
                }
                if d_numeric(da) == Some(0.0) {
                    return Rf_ScalarInteger(0);
                }
                return crate::sexp::constructors::Rf_lang3(op, da, b);
            }
        }
        if name == "sin" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"cos".as_ptr()), arg);
            }
        }
        if name == "exp" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"exp".as_ptr()), arg);
            }
        }
        if name == "log" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"/".as_ptr()),
                    Rf_ScalarReal(1.0),
                    arg,
                );
            }
        }
        if name == "cos" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let s = crate::sexp::constructors::Rf_lang2(Rf_install(c"sin".as_ptr()), arg);
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"-".as_ptr()), s);
            }
        }
        if name == "sqrt" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let pow = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"^".as_ptr()),
                    arg,
                    Rf_ScalarReal(-0.5),
                );
                return crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"*".as_ptr()),
                    Rf_ScalarReal(0.5),
                    pow,
                );
            }
        }
        if name == "tan" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let c = crate::sexp::constructors::Rf_lang2(Rf_install(c"cos".as_ptr()), arg);
                let c2 = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"^".as_ptr()),
                    c,
                    Rf_ScalarReal(2.0),
                );
                return crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"/".as_ptr()),
                    Rf_ScalarReal(1.0),
                    c2,
                );
            }
        }
        if name == "asin" || name == "acos" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let x2 = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"^".as_ptr()),
                    arg,
                    Rf_ScalarReal(2.0),
                );
                let inner = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"-".as_ptr()),
                    Rf_ScalarReal(1.0),
                    x2,
                );
                let s = crate::sexp::constructors::Rf_lang2(Rf_install(c"sqrt".as_ptr()), inner);
                let rec = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"/".as_ptr()),
                    Rf_ScalarReal(1.0),
                    s,
                );
                return if name == "acos" {
                    crate::sexp::constructors::Rf_lang2(Rf_install(c"-".as_ptr()), rec)
                } else {
                    rec
                };
            }
        }
        if name == "sinh" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"cosh".as_ptr()), arg);
            }
        }
        if name == "cosh" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"sinh".as_ptr()), arg);
            }
        }
        if name == "atan" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let x2 = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"^".as_ptr()),
                    arg,
                    Rf_ScalarReal(2.0),
                );
                let den = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"+".as_ptr()),
                    Rf_ScalarReal(1.0),
                    x2,
                );
                return crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"/".as_ptr()),
                    Rf_ScalarReal(1.0),
                    den,
                );
            }
        }
        if name == "tanh" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let c = crate::sexp::constructors::Rf_lang2(Rf_install(c"cosh".as_ptr()), arg);
                let c2 = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"^".as_ptr()),
                    c,
                    Rf_ScalarReal(2.0),
                );
                return crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"/".as_ptr()),
                    Rf_ScalarReal(1.0),
                    c2,
                );
            }
        }
        if name == "gamma" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let g = crate::sexp::constructors::Rf_lang2(Rf_install(c"gamma".as_ptr()), arg);
                let dg = crate::sexp::constructors::Rf_lang2(Rf_install(c"digamma".as_ptr()), arg);
                return crate::sexp::constructors::Rf_lang3(Rf_install(c"*".as_ptr()), g, dg);
            }
        }
        if name == "lgamma" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"digamma".as_ptr()), arg);
            }
        }
        if name == "digamma" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"trigamma".as_ptr()), arg);
            }
        }
        if name == "trigamma" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"psigamma".as_ptr()),
                    arg,
                    Rf_ScalarInteger(2),
                );
            }
        }
        if name == "expm1" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                return crate::sexp::constructors::Rf_lang2(Rf_install(c"exp".as_ptr()), arg);
            }
        }
        if name == "log1p" {
            let arg = CAR(CDR(expr));
            if d_symbol_name(arg) == var {
                let den = crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"+".as_ptr()),
                    Rf_ScalarReal(1.0),
                    arg,
                );
                return crate::sexp::constructors::Rf_lang3(
                    Rf_install(c"/".as_ptr()),
                    Rf_ScalarReal(1.0),
                    den,
                );
            }
        }
        if TYPEOF(expr) == SEXPTYPE::LANGSXP && !name.is_empty() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                &format!("Function '{name}' is not in the derivatives table"),
            );
        }
        Rf_ScalarInteger(0)
    }
}

/// GNU `D(expr, name)`.
pub unsafe fn do_D(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut expr = CAR(args);
        if TYPEOF(expr) == SEXPTYPE::EXPRSXP && XLENGTH(expr) >= 1 {
            expr = VECTOR_ELT(expr, 0);
        }
        let name_s = CAR(CDR(args));
        let var = if TYPEOF(name_s) == SEXPTYPE::STRSXP {
            CStr::from_ptr(CHAR(STRING_ELT(name_s, 0)))
                .to_string_lossy()
                .into_owned()
        } else {
            d_symbol_name(name_s)
        };
        d_diff(expr, &var)
    }
}

/// GNU `deriv(~expr, name)` as an evaluable expression.
pub unsafe fn do_deriv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut expr = CAR(args);
        if TYPEOF(expr) == SEXPTYPE::LANGSXP && d_symbol_name(CAR(expr)) == "~" {
            expr = CAR(CDR(expr));
        }
        if TYPEOF(expr) == SEXPTYPE::EXPRSXP && XLENGTH(expr) >= 1 {
            expr = VECTOR_ELT(expr, 0);
        }
        let name_s = CAR(CDR(args));
        let var = if TYPEOF(name_s) == SEXPTYPE::STRSXP {
            CStr::from_ptr(CHAR(STRING_ELT(name_s, 0)))
                .to_string_lossy()
                .into_owned()
        } else {
            d_symbol_name(name_s)
        };
        let d = d_diff(expr, &var);
        let _d = protect(d);
        let e_txt = crate::mainutils::deparse::deparse1line(expr, false);
        let _et = protect(e_txt);
        let d_txt = crate::mainutils::deparse::deparse1line(d, false);
        let _dt = protect(d_txt);
        let e_s = CStr::from_ptr(CHAR(STRING_ELT(e_txt, 0))).to_string_lossy();
        let d_s = CStr::from_ptr(CHAR(STRING_ELT(d_txt, 0))).to_string_lossy();
        let src = format!("{{ .value <- {e_s}; attr(.value, \"gradient\") <- {d_s}; .value }}");
        parse_source_expression_vector(&src)
    }
}

/// GNU `deriv3(~expr, name)` with hessian.
pub unsafe fn do_deriv3(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut expr = CAR(args);
        if TYPEOF(expr) == SEXPTYPE::LANGSXP && d_symbol_name(CAR(expr)) == "~" {
            expr = CAR(CDR(expr));
        }
        if TYPEOF(expr) == SEXPTYPE::EXPRSXP && XLENGTH(expr) >= 1 {
            expr = VECTOR_ELT(expr, 0);
        }
        let name_s = CAR(CDR(args));
        let var = if TYPEOF(name_s) == SEXPTYPE::STRSXP {
            CStr::from_ptr(CHAR(STRING_ELT(name_s, 0)))
                .to_string_lossy()
                .into_owned()
        } else {
            d_symbol_name(name_s)
        };
        let d = d_diff(expr, &var);
        let _d = protect(d);
        let h = d_diff(d, &var);
        let _h = protect(h);
        let e_txt = crate::mainutils::deparse::deparse1line(expr, false);
        let _et = protect(e_txt);
        let d_txt = crate::mainutils::deparse::deparse1line(d, false);
        let _dt = protect(d_txt);
        let h_txt = crate::mainutils::deparse::deparse1line(h, false);
        let _ht = protect(h_txt);
        let e_s = CStr::from_ptr(CHAR(STRING_ELT(e_txt, 0))).to_string_lossy();
        let d_s = CStr::from_ptr(CHAR(STRING_ELT(d_txt, 0))).to_string_lossy();
        let h_s = CStr::from_ptr(CHAR(STRING_ELT(h_txt, 0))).to_string_lossy();
        let src = format!(
            "{{ .value <- {e_s}; attr(.value, \"gradient\") <- {d_s}; attr(.value, \"hessian\") <- {h_s}; .value }}"
        );
        parse_source_expression_vector(&src)
    }
}
