//! GNU replacement values and evaluation order apply to compiled subscripts.

use super::*;
use crate::sexp::session::RSession;
use std::{cell::Cell, rc::Rc};

fn setup(session: &mut RSession, script: &str) {
    let (result, _, _) = session.eval_code_with_output_capture(script);
    drop(
        result
            .expect("real source setup must succeed")
            .into_owned()
            .unwrap(),
    );
}

fn compare_assignment(session: &mut RSession, setup_code: &str, assignment: &str, collect: bool) {
    setup(session, setup_code);
    let environment = session.global_env().unwrap().into_owned().unwrap();
    let factory = environment.node_factory().unwrap();
    let expression = session
        .owner_token()
        .unwrap()
        .with_arena(|arena| crate::eval::parser::parse(assignment, arena, factory.domain()))
        .unwrap()
        .unwrap();
    let expected = unsafe {
        own_operand(crate::eval::eval::Rf_eval(
            expression.as_raw(),
            environment.as_raw(),
        ))
    };
    let expected_visible = unsafe { crate::sexp::globals::R_Visible() };
    let symbol = unsafe { crate::sexp::symbol::Rf_install(c"x".as_ptr()) };
    let expected_object =
        unsafe { own_operand(crate::sexp::envir::R_findVar(symbol, environment.as_raw())) };
    let events_symbol = unsafe { crate::sexp::symbol::Rf_install(c"events".as_ptr()) };
    let expected_events = unsafe {
        own_operand(crate::sexp::envir::R_findVar(
            events_symbol,
            environment.as_raw(),
        ))
    };
    assert_eq!(expected_visible, 0, "GNU assignment is invisible");
    setup(session, setup_code);
    let original_rhs = if setup_code.contains("rhsValue") {
        Some(unsafe {
            own_operand(crate::sexp::envir::R_findVar(
                crate::sexp::symbol::Rf_install(c"rhsValue".as_ptr()),
                environment.as_raw(),
            ))
        })
    } else {
        None
    };
    let bytecode = unsafe {
        own_operand(
            compile_expr(expression.as_raw(), environment.as_raw())
                .expect("subassignment must compile"),
        )
    };
    let pool = unsafe { own_operand(super::super::bc_eval::BCODE_CONSTS(bytecode.as_raw())) };
    SexpMut::try_from_checked(pool.clone())
        .unwrap()
        .try_set_vector_elt(0, factory.nil())
        .unwrap();
    drop(expression);
    session.gc();
    let callbacks = Rc::new(Cell::new(0));
    if collect {
        let captured_pool = pool.clone();
        let count = callbacks.clone();
        let original = environment.runtime_owner.as_ref().unwrap().clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if count.replace(1) != 0 {
                return;
            }
            let pin = original.pin().unwrap();
            unsafe {
                (*pin.as_ptr()).memory_state.gc_force_gap = 0;
                (*pin.as_ptr()).memory_state.gc_force_wait = 0;
            }
            let factory = captured_pool.node_factory().unwrap();
            let mut mutation = SexpMut::try_from_checked(captured_pool.clone()).unwrap();
            for index in 0..captured_pool.len() {
                mutation.try_set_vector_elt(index, factory.nil()).unwrap();
            }
            drop(mutation);
            crate::sexp::gengc::full_gc();
        }));
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        });
    }
    let actual = unsafe {
        own_operand(super::super::bc_eval::bcEval(
            bytecode.as_raw(),
            environment.as_raw(),
        ))
    };
    let actual_visible = unsafe { crate::sexp::globals::R_Visible() };
    let actual_object =
        unsafe { own_operand(crate::sexp::envir::R_findVar(symbol, environment.as_raw())) };
    let actual_events = unsafe {
        own_operand(crate::sexp::envir::R_findVar(
            events_symbol,
            environment.as_raw(),
        ))
    };
    for (label, source, compiled) in [
        ("returned RHS", &expected, &actual),
        ("modified object", &expected_object, &actual_object),
        ("side-effect order", &expected_events, &actual_events),
    ] {
        assert_eq!(
            unsafe {
                crate::mainutils::identical::R_compute_identical(
                    source.as_raw(),
                    compiled.as_raw(),
                    0,
                )
            },
            1,
            "{label}: {assignment}"
        );
    }
    assert_eq!(actual_visible, expected_visible, "{assignment}");
    if let Some(original_rhs) = original_rhs {
        assert_eq!(actual, original_rhs, "return the actual original RHS");
    }
    if collect {
        assert_eq!(callbacks.get(), 1, "detach the pool and collect once");
        assert!(pool.try_vector_elt(1).unwrap().is_nil());
    }
}

#[test]
#[cfg(not(miri))]
fn compiled_subassignment_real_base_returns_rhs_and_preserves_order() {
    let mut session = RSession::new_without_default_packages();
    for (setup_code, assignment) in [
        (
            "x<-c(1L,2L);events<-character();rhsValue<-7L",
            "x[1L]<-rhsValue",
        ),
        (
            "x<-c(1,2);events<-character();rhsValue<-7L",
            "x[1L]<-rhsValue",
        ),
        (
            "x<-c(1,2);events<-character();rhsValue<-NA",
            "x[1L]<-rhsValue",
        ),
        (
            "x<-c(1L,2L);events<-character();rhsValue<-7.5",
            "x[1L]<-rhsValue",
        ),
        (
            "x<-list(1L,2L);events<-character();rhsValue<-structure(list(7L),note='original')",
            "x[1L]<-rhsValue",
        ),
        (
            "x<-matrix(1:4,2L);events<-character();rhsValue<-7L",
            "x[1L,2L]<-rhsValue",
        ),
        (
            "x<-list(1L,2L);events<-character();rhsValue<-7L",
            "x[[1L]]<-rhsValue",
        ),
        (
            "x<-c(1L,2L);events<-character();rhsValue<-7L",
            "x[]<-rhsValue",
        ),
        (
            "x<-list(1L,2L);events<-character();rhsValue<-quote(unbound_data)",
            "x[[1L]]<-rhsValue",
        ),
        (
            "x<-list(1L,2L);events<-character();rhsValue<-quote(a+b)",
            "x[[1L]]<-rhsValue",
        ),
        (
            "x<-c(1L,2L);events<-character();rhs<-function(){events<<-c(events,'rhs');x<<-c(30L,40L);7L};index<-function(){events<<-c(events,'index');1L}",
            "x[index()]<-rhs()",
        ),
    ] {
        compare_assignment(&mut session, setup_code, assignment, false);
    }
}

#[test]
fn owned_compiled_subassignment_callbacks_preserve_rhs_and_index_order() {
    let mut session = RSession::new_for_gc_tests();
    compare_assignment(
        &mut session,
        "x<-c(1L,2L);events<-character();rhs<-function(){events<<-c(events,'rhs');gc();7L};index<-function(){events<<-c(events,'index');gc();1L}",
        "x[index()]<-rhs()",
        true,
    );
}

#[test]
fn owned_compiled_subassignment_custom_setter_keeps_lazy_index_syntax() {
    let mut session = RSession::new_for_gc_tests();
    compare_assignment(
        &mut session,
        "x<-1L;events<-character();rhsValue<-7L;`[<-`<-function(x,i,value){gc();attr(x,'index')<-substitute(i);x}",
        "x[unbound_index]<-rhsValue",
        true,
    );
}

#[test]
fn compiled_subassignment_keeps_bare_dots_on_existing_source_path() {
    let session = RSession::new_for_gc_tests();
    let environment = session.global_env().unwrap().into_owned().unwrap();
    let factory = environment.node_factory().unwrap();
    for syntax in ["x[...]<-7L", "x[[...]]<-7L"] {
        let expression = session
            .owner_token()
            .unwrap()
            .with_arena(|arena| crate::eval::parser::parse(syntax, arena, factory.domain()))
            .unwrap()
            .unwrap();
        assert!(unsafe { compile_expr(expression.as_raw(), environment.as_raw()) }.is_none());
    }
}

#[test]
fn owned_compiled_subassignment_rejects_original_owner_revocation() {
    let mut session = RSession::new_for_gc_tests();
    setup(
        &mut session,
        "x<-1L;rhsValue<-7L;`[<-`<-function(x,i,value){gc();9L}",
    );
    let environment = session.global_env().unwrap().into_owned().unwrap();
    let factory = environment.node_factory().unwrap();
    let expression = session
        .owner_token()
        .unwrap()
        .with_arena(|arena| crate::eval::parser::parse("x[1L]<-rhsValue", arena, factory.domain()))
        .unwrap()
        .unwrap();
    let bytecode =
        unsafe { own_operand(compile_expr(expression.as_raw(), environment.as_raw()).unwrap()) };
    let pool = unsafe { own_operand(super::super::bc_eval::BCODE_CONSTS(bytecode.as_raw())) };
    SexpMut::try_from_checked(pool)
        .unwrap()
        .try_set_vector_elt(0, factory.nil())
        .unwrap();
    drop(expression);
    session.gc();
    let original = environment.runtime_owner.as_ref().unwrap().clone();
    let observed = Rc::new(Cell::new(false));
    let called = observed.clone();
    let callback_owner = original.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !called.replace(true) {
            let pin = callback_owner.pin().unwrap();
            unsafe {
                crate::sexp::instance::revoke_instance_availability(pin.as_ptr());
            }
        }
    }));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        super::super::bc_eval::bcEval(bytecode.as_raw(), environment.as_raw())
    }));
    assert!(observed.get());
    assert!(original.pin().is_err());
    let message = error_message(outcome.expect_err("revoked bytecode cannot publish an RHS"));
    assert_eq!(
        message,
        crate::sexp::object::SexpError::RootUnavailable.to_string()
    );
    let replacement = RSession::new_for_gc_tests();
    assert!(
        replacement
            .owner_token()
            .unwrap()
            .node_factory()
            .wrap(environment.as_raw())
            .is_err()
    );
    drop(session);
}

#[test]
fn owned_compiled_subassignment_preserves_live_callback_panic_identity() {
    let mut session = RSession::new_for_gc_tests();
    setup(
        &mut session,
        "x<-1L;rhsValue<-7L;`[<-`<-function(x,i,value){gc();9L}",
    );
    let environment = session.global_env().unwrap().into_owned().unwrap();
    let factory = environment.node_factory().unwrap();
    let expression = session
        .owner_token()
        .unwrap()
        .with_arena(|arena| crate::eval::parser::parse("x[1L]<-rhsValue", arena, factory.domain()))
        .unwrap()
        .unwrap();
    let bytecode =
        unsafe { own_operand(compile_expr(expression.as_raw(), environment.as_raw()).unwrap()) };
    drop(expression);
    let observed = Rc::new(Cell::new(false));
    let called = observed.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !called.replace(true) {
            std::panic::panic_any(0xA17E_u32);
        }
    }));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        super::super::bc_eval::bcEval(bytecode.as_raw(), environment.as_raw())
    }))
    .expect_err("live callback panic must propagate unchanged");
    assert_eq!(
        *outcome
            .downcast::<u32>()
            .expect("exact original panic type"),
        0xA17E
    );
    assert!(observed.get());
    assert!(environment.runtime_owner.as_ref().unwrap().pin().is_ok());
    let error_reached = Rc::new(Cell::new(false));
    let called = error_reached.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !called.replace(true) {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "original live callback R error".into(),
            });
        }
    }));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        super::super::bc_eval::bcEval(bytecode.as_raw(), environment.as_raw())
    }))
    .expect_err("live R callback error must propagate unchanged");
    // The primitive's normal error attribution converts the bare RError into
    // the canonical R error signal before it reaches the bytecode boundary.
    assert!(matches!(
        *outcome.downcast::<crate::sexp::context::RSignal>().expect("canonical live R error signal"),
        crate::sexp::context::RSignal::Error { message } if message == "original live callback R error"
    ));
    assert!(error_reached.get());
    let (result, _, _) = session.eval_code_with_output_capture("1L");
    assert_eq!(result.unwrap().integer_elt(0), Some(1));
}

#[test]
#[cfg(not(miri))]
fn gnu_subassignment_fixtures_execute_directly_without_source_fallback() {
    let mut session = RSession::new_without_default_packages();
    let cases: &[(&[u8], &str, &str)] = &[
        (
            include_bytes!(
                "../../../../r-embed/tests/fixtures/gnu-bytecode-subassignment/single.rds"
            ),
            "x<-c(1,2);rhs<-7L;i<-1L",
            "identical(x,c(7,2))",
        ),
        (
            include_bytes!(
                "../../../../r-embed/tests/fixtures/gnu-bytecode-subassignment/double.rds"
            ),
            "x<-list(1L,2L);rhs<-quote(unbound_data);i<-1L",
            "identical(x,list(quote(unbound_data),2L))",
        ),
        (
            include_bytes!(
                "../../../../r-embed/tests/fixtures/gnu-bytecode-subassignment/double.rds"
            ),
            "x<-list(1L,2L);rhs<-quote(a+b);i<-1L",
            "identical(x,list(quote(a+b),2L))",
        ),
        (
            include_bytes!(
                "../../../../r-embed/tests/fixtures/gnu-bytecode-subassignment/whole.rds"
            ),
            "x<-c(1L,2L);rhs<-7L",
            "identical(x,c(7L,7L))",
        ),
        (
            include_bytes!(
                "../../../../r-embed/tests/fixtures/gnu-bytecode-subassignment/matrix.rds"
            ),
            "x<-matrix(1:4,2L);rhs<-7L;i<-1L;j<-2L",
            "identical(x,matrix(c(1L,2L,7L,4L),2L))",
        ),
    ];
    for (bytes, initialization, assertion) in cases {
        let raw = bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        setup(
            &mut session,
            &format!("f<-unserialize(as.raw(c({raw})));{initialization}"),
        );
        let environment = session.global_env().unwrap().into_owned().unwrap();
        let function = unsafe {
            own_operand(crate::sexp::envir::R_findVar(
                crate::sexp::symbol::Rf_install(c"f".as_ptr()),
                environment.as_raw(),
            ))
        };
        let body = unsafe { own_operand(crate::sexp::accessors::BODY(function.as_raw())) };
        assert!(unsafe { super::super::bc_eval::BCODE_IS_GNU(body.as_raw()) });
        let rhs = unsafe {
            own_operand(crate::sexp::envir::R_findVar(
                crate::sexp::symbol::Rf_install(c"rhs".as_ptr()),
                environment.as_raw(),
            ))
        };
        // Keep mandatory GNU call syntax constants. The direct VM entry cannot
        // recover via retained source if an instruction is unsupported.
        setup(&mut session, "f<-NULL");
        drop(function);
        session.gc();
        let result = unsafe {
            own_operand(super::super::bc_eval::bcEval(
                body.as_raw(),
                environment.as_raw(),
            ))
        };
        assert_eq!(result, rhs, "GNU must return the original RHS");
        assert_eq!(unsafe { crate::sexp::globals::R_Visible() }, 0);
        let (result, _, _) = session.eval_code_with_output_capture(assertion);
        assert_eq!(result.unwrap().logical_elt(0), Some(1), "{assertion}");
    }
    let raw =
        include_bytes!("../../../../r-embed/tests/fixtures/gnu-bytecode-subassignment/order.rds")
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
    setup(&mut session, &format!("f<-unserialize(as.raw(c({raw})))"));
    let environment = session.global_env().unwrap().into_owned().unwrap();
    let function = unsafe {
        own_operand(crate::sexp::envir::R_findVar(
            crate::sexp::symbol::Rf_install(c"f".as_ptr()),
            environment.as_raw(),
        ))
    };
    let body = unsafe { own_operand(crate::sexp::accessors::BODY(function.as_raw())) };
    assert!(unsafe { super::super::bc_eval::BCODE_IS_GNU(body.as_raw()) });
    setup(&mut session, "f<-NULL");
    drop(function);
    session.gc();
    let result = unsafe {
        own_operand(super::super::bc_eval::bcEval(
            body.as_raw(),
            environment.as_raw(),
        ))
    };
    unsafe {
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::Rf_install(c"actual".as_ptr()),
            result.as_raw(),
            environment.as_raw(),
        );
    }
    let (result, _, _) = session.eval_code_with_output_capture(
        "identical(actual,list(result=list(value=7L,visible=FALSE),x=c(7L,40L),events=c('rhs','index')))"
    );
    assert_eq!(result.unwrap().logical_elt(0), Some(1));
}

fn error_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(error) = payload.downcast_ref::<crate::sexp::context::RError>() {
        return error.message.clone();
    }
    if let Some(crate::sexp::context::RSignal::Error { message }) =
        payload.downcast_ref::<crate::sexp::context::RSignal>()
    {
        return message.clone();
    }
    std::panic::resume_unwind(payload)
}

#[test]
fn owned_compiled_subassignment_index_error_keeps_rhs_effects_and_recovers() {
    let mut session = RSession::new_for_gc_tests();
    let setup_code = "x<-c(1L,2L);events<-character();rhs<-function(){events<<-c(events,'rhs');gc();7L};index<-function(){events<<-c(events,'index');gc();stop('bad-index')}";
    setup(&mut session, setup_code);
    let environment = session.global_env().unwrap().into_owned().unwrap();
    let factory = environment.node_factory().unwrap();
    let expression = session
        .owner_token()
        .unwrap()
        .with_arena(|arena| {
            crate::eval::parser::parse("x[index()]<-rhs()", arena, factory.domain())
        })
        .unwrap()
        .unwrap();
    let source_error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::eval::eval::Rf_eval(expression.as_raw(), environment.as_raw())
    }))
    .expect_err("index callback must signal an R error");
    let source_message = error_message(source_error);
    let events_symbol = unsafe { crate::sexp::symbol::Rf_install(c"events".as_ptr()) };
    let expected_events = unsafe {
        own_operand(crate::sexp::envir::R_findVar(
            events_symbol,
            environment.as_raw(),
        ))
    };
    setup(&mut session, setup_code);
    let bytecode =
        unsafe { own_operand(compile_expr(expression.as_raw(), environment.as_raw()).unwrap()) };
    let pool = unsafe { own_operand(super::super::bc_eval::BCODE_CONSTS(bytecode.as_raw())) };
    SexpMut::try_from_checked(pool)
        .unwrap()
        .try_set_vector_elt(0, factory.nil())
        .unwrap();
    drop(expression);
    session.gc();
    let protections = crate::sexp::protect::R_ProtectCount();
    let compiled_error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        super::super::bc_eval::bcEval(bytecode.as_raw(), environment.as_raw())
    }))
    .expect_err("compiled index callback must signal an R error");
    assert_eq!(error_message(compiled_error), source_message);
    assert_eq!(crate::sexp::protect::R_ProtectCount(), protections);
    let actual_events = unsafe {
        own_operand(crate::sexp::envir::R_findVar(
            events_symbol,
            environment.as_raw(),
        ))
    };
    assert_eq!(
        unsafe {
            crate::mainutils::identical::R_compute_identical(
                expected_events.as_raw(),
                actual_events.as_raw(),
                0,
            )
        },
        1,
        "RHS effects precede the failing index"
    );
    compare_assignment(
        &mut session,
        "x<-c(1L,2L);events<-character();rhsValue<-13L",
        "x[1L]<-rhsValue",
        false,
    );
}
