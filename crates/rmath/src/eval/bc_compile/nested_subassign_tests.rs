//! Pinned GNU source and cmpfun agree on these bounded replacement paths.
use super::*;
use crate::sexp::session::RSession;

fn setup(session: &mut RSession, source: &str) {
    let (value, _, _) = session.eval_code_with_output_capture(source);
    drop(value.unwrap().into_owned().unwrap());
}

#[cfg(not(miri))]
fn run_assignment(session: &mut RSession, source: &str, assignment: &str, superassign: bool) {
    fn value(environment: &Sexp<'static>, name: &std::ffi::CStr) -> Sexp<'static> {
        unsafe {
            own_operand(crate::sexp::envir::R_findVar(
                crate::sexp::symbol::Rf_install(name.as_ptr()),
                environment.as_raw(),
            ))
        }
    }
    setup(session, source);
    let global = session.global_env().unwrap().into_owned().unwrap();
    let environment = if superassign {
        value(&global, c"frame")
    } else {
        global.clone()
    };
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
    assert_eq!(
        expected,
        value(&global, c"rhsValue"),
        "source returns original RHS"
    );
    assert_eq!(unsafe { crate::sexp::globals::R_Visible() }, 0);
    let expected_object = value(&global, c"x");
    let expected_events = value(&global, c"events");
    let expected_local = superassign.then(|| value(&environment, c"x"));
    drop(expression);
    drop(environment);
    setup(session, source);
    let environment = if superassign {
        value(&global, c"frame")
    } else {
        global.clone()
    };
    let expression = session
        .owner_token()
        .unwrap()
        .with_arena(|arena| crate::eval::parser::parse(assignment, arena, factory.domain()))
        .unwrap()
        .unwrap();
    let body = unsafe {
        own_operand(
            compile_expr(expression.as_raw(), environment.as_raw())
                .expect("bounded replacement must compile"),
        )
    };
    let pool = unsafe { own_operand(super::super::bc_eval::BCODE_CONSTS(body.as_raw())) };
    SexpMut::try_from_checked(pool)
        .unwrap()
        .try_set_vector_elt(0, factory.nil())
        .unwrap();
    drop(expression);
    session.gc();
    let rhs = value(&global, c"rhsValue");
    let actual = execute(&body, &environment);
    assert_eq!(actual, rhs, "assignment returns the exact original RHS");
    assert_eq!(unsafe { crate::sexp::globals::R_Visible() }, 0);
    for (expected, actual) in [
        (expected_object, value(&global, c"x")),
        (expected_events, value(&global, c"events")),
    ] {
        assert_eq!(
            unsafe {
                crate::mainutils::identical::R_compute_identical(
                    expected.as_raw(),
                    actual.as_raw(),
                    0,
                )
            },
            1,
            "source/compiled agreement"
        );
    }
    if let Some(expected) = expected_local {
        assert_eq!(
            unsafe {
                crate::mainutils::identical::R_compute_identical(
                    expected.as_raw(),
                    value(&environment, c"x").as_raw(),
                    0,
                )
            },
            1,
            "local shadow unchanged"
        );
    }
}

#[test]
#[cfg(not(miri))]
fn compiled_nested_subassignment_selects_object_after_rhs() {
    let mut session = RSession::new_without_default_packages();
    run_assignment(
        &mut session,
        "x<-list(a=c(1L,2L),b=99L);events<-character();rhsValue<-7L;rhs<-function(){events<<-c(events,'rhs');x<<-list(a=c(30L,40L),b=88L);rhsValue};index<-function(){events<<-c(events,'index');1L}",
        "x$a[index()]<-rhs()",
        false,
    );
    setup(
        &mut session,
        "stopifnot(identical(x,list(a=c(7L,40L),b=88L)),identical(events,c('rhs','index')), !exists('.Compiler.sub',inherits=FALSE))",
    );
}

#[test]
#[cfg(not(miri))]
fn compiled_super_subassignment_skips_local_shadow() {
    let mut session = RSession::new_without_default_packages();
    run_assignment(
        &mut session,
        "x<-c(1L,2L);frame<-new.env();frame$x<-c(50L,60L);events<-character();rhsValue<-7L;rhs<-function(){events<<-c(events,'rhs');rhsValue};index<-function(){events<<-c(events,'index');1L}",
        "x[index()]<<-rhs()",
        true,
    );
    setup(
        &mut session,
        "stopifnot(identical(x,c(7L,2L)),identical(frame$x,c(50L,60L)),identical(events,c('rhs','index')))",
    );
}

#[test]
#[cfg(not(miri))]
fn compiled_nested_subassignment_preserves_lazy_calls_and_order() {
    let mut session = RSession::new_without_default_packages();
    let source = "x<-list(a=c(1L,2L));events<-character();metadata<-list();rhsValue<-7L;`$`<-function(x,name){events<<-c(events,'get');metadata[[1L]]<<-list(substitute(x),substitute(name));.subset2(x,as.character(substitute(name)))};`[<-`<-function(x,i,value){events<<-c(events,'inner');metadata[[2L]]<<-list(substitute(x),substitute(i),substitute(value));.Primitive('[<-')(x,i,value=value)};`$<-`<-function(x,name,value){events<<-c(events,'outer');metadata[[3L]]<<-list(substitute(x),substitute(name),substitute(value));x[[as.character(substitute(name))]]<-value;x};rhs<-function(){events<<-c(events,'rhs');rhsValue};index<-function(){events<<-c(events,'index');1L}";
    run_assignment(&mut session, source, "x$a[index()]<-rhs()", false);
    setup(
        &mut session,
        "stopifnot(identical(x,list(a=c(7L,2L))),identical(events,c('rhs','get','inner','index','outer')),identical(metadata,list(list(as.name('*tmp*'),quote(a)),list(as.name('*tmp*'),quote(index()),quote(rhs())),list(as.name('*tmp*'),quote(a),as.name('*vtmp*')))))",
    );
}

fn compiled_body(session: &RSession, syntax: &str) -> (Sexp<'static>, Sexp<'static>) {
    let environment = session.global_env().unwrap().into_owned().unwrap();
    let factory = environment.node_factory().unwrap();
    let expression = session
        .owner_token()
        .unwrap()
        .with_arena(|arena| crate::eval::parser::parse(syntax, arena, factory.domain()))
        .unwrap()
        .unwrap();
    let body =
        unsafe { own_operand(compile_expr(expression.as_raw(), environment.as_raw()).unwrap()) };
    let pool = unsafe { own_operand(super::super::bc_eval::BCODE_CONSTS(body.as_raw())) };
    SexpMut::try_from_checked(pool)
        .unwrap()
        .try_set_vector_elt(0, factory.nil())
        .unwrap();
    drop(expression);
    (body, environment)
}

fn execute(body: &Sexp<'static>, environment: &Sexp<'static>) -> Sexp<'static> {
    unsafe {
        own_operand(super::super::bc_eval::bcEval(
            body.as_raw(),
            environment.as_raw(),
        ))
    }
}

fn error_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(error) = payload.downcast_ref::<crate::sexp::context::RError>() {
        error.message.clone()
    } else if let Some(crate::sexp::context::RSignal::Error { message }) =
        payload.downcast_ref::<crate::sexp::context::RSignal>()
    {
        message.clone()
    } else {
        panic!("expected typed R error");
    }
}

#[test]
fn owned_nested_subassignment_detached_pool_survives_getter_collection() {
    use std::{cell::Cell, rc::Rc};
    let mut session = RSession::new_for_gc_tests();
    setup(
        &mut session,
        "x<-list(a=1L,b=2L);rhsValue<-7L;`$`<-function(x,name){gc();.subset2(x,as.character(substitute(name)))};`[<-`<-function(x,i,value){attr(x,'index')<-substitute(i);x}",
    );
    let (body, environment) = compiled_body(&session, "x$a[unbound_index]<-rhsValue");
    session.gc();
    let pool = unsafe { own_operand(super::super::bc_eval::BCODE_CONSTS(body.as_raw())) };
    let called = Rc::new(Cell::new(false));
    let observed = called.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !observed.replace(true) {
            let nil = pool.node_factory().unwrap().nil();
            let mut mutation = SexpMut::try_from_checked(pool.clone()).unwrap();
            for index in 0..pool.len() {
                mutation.try_set_vector_elt(index, nil.clone()).unwrap();
            }
            drop(mutation);
            crate::sexp::gengc::full_gc();
        }
    }));
    let result = execute(&body, &environment);
    assert_eq!(result.integer_elt(0), Some(7));
    assert_eq!(unsafe { crate::sexp::globals::R_Visible() }, 0);
    assert!(called.get());
    let (actual, _, _) = session.eval_code_with_output_capture(
        "identical(x,list(a=structure(1L,index=quote(unbound_index)),b=2L))",
    );
    assert_eq!(actual.unwrap().logical_elt(0), Some(1));
}

#[test]
fn owned_nested_subassignment_rejects_malformed_path_before_callbacks() {
    let mut session = RSession::new_for_gc_tests();
    setup(
        &mut session,
        "x<-list(a=1L);rhsValue<-7L;events<-0L;`$`<-function(x,name){events<<-1L;x}",
    );
    let (body, environment) = compiled_body(&session, "x$a[1L]<-rhsValue");
    let words = unsafe { own_operand(crate::sexp::accessors::VECTOR_ELT(body.as_raw(), 0)) };
    let length = words.len() as usize;
    // The terminal path instruction has six operands followed by RETURN.
    let opcode = length - 8;
    assert_eq!(
        words.integer_elt(opcode as i64),
        Some(opcodes::OP_REPLACEMENT_PATH)
    );
    let pool = unsafe { own_operand(super::super::bc_eval::BCODE_CONSTS(body.as_raw())) };
    let scalar = (0..pool.len())
        .find(|&index| pool.try_vector_elt(index).unwrap().typeof_() == SEXPTYPE::INTSXP)
        .unwrap();
    for (offset, bad, message) in [
        (4, 2, "invalid REPLACEMENT_PATH"),
        (5, -1, "negative replacement path argument count"),
        (5, 99, "REPLACEMENT_PATH argument stack underflow"),
        (3, scalar as i32, "invalid REPLACEMENT_PATH"),
        (2, scalar as i32, "invalid REPLACEMENT_PATH"),
        (1, scalar as i32, "invalid REPLACEMENT_PATH"),
    ] {
        let index = (opcode + offset) as i64;
        let original = words.integer_elt(index).unwrap();
        SexpMut::try_from_checked(words.clone())
            .unwrap()
            .try_set_integer_elt(index, bad)
            .unwrap();
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            execute(&body, &environment)
        }))
        .unwrap_err();
        assert!(error_message(error).contains(message));
        let events = unsafe {
            own_operand(crate::sexp::envir::R_findVar(
                crate::sexp::symbol::Rf_install(c"events".as_ptr()),
                environment.as_raw(),
            ))
        };
        assert_eq!(events.integer_elt(0), Some(0));
        SexpMut::try_from_checked(words.clone())
            .unwrap()
            .try_set_integer_elt(index, original)
            .unwrap();
    }
    assert_eq!(execute(&body, &environment).integer_elt(0), Some(7));
}

#[test]
fn owned_nested_subassignment_preserves_error_and_live_panic_recovery() {
    use std::{cell::Cell, rc::Rc};
    let mut session = RSession::new_for_gc_tests();
    setup(
        &mut session,
        "x<-list(a=1L);rhsValue<-7L;`$`<-function(x,name){gc();.subset2(x,as.character(substitute(name)))};`[<-`<-function(x,i,value){stop('inner setter failed')}",
    );
    let (body, environment) = compiled_body(&session, "x$a[1L]<-rhsValue");
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        execute(&body, &environment)
    }))
    .unwrap_err();
    assert!(error_message(error).contains("inner setter failed"));
    let called = Rc::new(Cell::new(false));
    let observed = called.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !observed.replace(true) {
            std::panic::panic_any(0xC164_u32);
        }
    }));
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        execute(&body, &environment)
    }))
    .unwrap_err();
    assert_eq!(*payload.downcast::<u32>().unwrap(), 0xC164);
    assert!(called.get());
    setup(&mut session, "stopifnot(identical(x,list(a=1L)));1L");
}

#[test]
fn owned_nested_subassignment_refuses_original_runtime_revocation() {
    use std::{cell::Cell, rc::Rc};
    let mut session = RSession::new_for_gc_tests();
    setup(
        &mut session,
        "x<-list(a=1L);rhsValue<-7L;`$`<-function(x,name){gc();x}",
    );
    let (body, environment) = compiled_body(&session, "x$a[1L]<-rhsValue");
    let original = environment.runtime_owner.as_ref().unwrap().clone();
    let owner = original.clone();
    let called = Rc::new(Cell::new(false));
    let observed = called.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !observed.replace(true) {
            let pin = owner.pin().unwrap();
            unsafe {
                crate::sexp::instance::revoke_instance_availability(pin.as_ptr());
            }
        }
    }));
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        execute(&body, &environment)
    }))
    .unwrap_err();
    assert_eq!(
        error_message(error),
        crate::sexp::object::SexpError::RootUnavailable.to_string()
    );
    assert!(called.get());
    assert!(original.pin().is_err());
    let replacement = RSession::new_for_gc_tests();
    assert!(
        replacement
            .owner_token()
            .unwrap()
            .node_factory()
            .wrap(environment.as_raw())
            .is_err()
    );
}

#[test]
#[cfg(not(miri))]
fn gnu_nested_subassignment_fixtures_execute_without_source_fallback() {
    let mut session = RSession::new_without_default_packages();
    let cases: &[(&[u8], &str, bool, &str)] = &[
        (
            include_bytes!(
                "../../../../r-embed/tests/fixtures/gnu-bytecode-nested-subassignment/nested.rds"
            ),
            "x<-list(a=c(1L,2L),b=99L);events<-character();rhsValue<-7L;rhs<-function(){events<<-c(events,'rhs');x<<-list(a=c(30L,40L),b=88L);rhsValue};index<-function(){events<<-c(events,'index');1L}",
            false,
            "stopifnot(identical(x,list(a=c(7L,40L),b=88L)),identical(events,c('rhs','index')))",
        ),
        (
            include_bytes!(
                "../../../../r-embed/tests/fixtures/gnu-bytecode-nested-subassignment/super.rds"
            ),
            "x<-c(1L,2L);frame<-new.env();frame$x<-c(50L,60L);events<-character();rhsValue<-7L;rhs<-function(){events<<-c(events,'rhs');rhsValue};index<-function(){events<<-c(events,'index');1L}",
            true,
            "stopifnot(identical(x,c(7L,2L)),identical(frame$x,c(50L,60L)),identical(events,c('rhs','index')))",
        ),
    ];
    for (bytes, source, superassign, assertion) in cases {
        let raw = bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        setup(
            &mut session,
            &format!("f<-unserialize(as.raw(c({raw})));{source}"),
        );
        let global = session.global_env().unwrap().into_owned().unwrap();
        let function = unsafe {
            own_operand(crate::sexp::envir::R_findVar(
                crate::sexp::symbol::Rf_install(c"f".as_ptr()),
                global.as_raw(),
            ))
        };
        let body = unsafe { own_operand(crate::sexp::accessors::BODY(function.as_raw())) };
        assert!(unsafe { super::super::bc_eval::BCODE_IS_GNU(body.as_raw()) });
        let environment = if *superassign {
            unsafe {
                own_operand(crate::sexp::envir::R_findVar(
                    crate::sexp::symbol::Rf_install(c"frame".as_ptr()),
                    global.as_raw(),
                ))
            }
        } else {
            global.clone()
        };
        setup(&mut session, "f<-NULL");
        drop(function);
        session.gc();
        let rhs = unsafe {
            own_operand(crate::sexp::envir::R_findVar(
                crate::sexp::symbol::Rf_install(c"rhsValue".as_ptr()),
                global.as_raw(),
            ))
        };
        assert_eq!(execute(&body, &environment), rhs);
        assert_eq!(unsafe { crate::sexp::globals::R_Visible() }, 0);
        setup(&mut session, assertion);
    }
}
