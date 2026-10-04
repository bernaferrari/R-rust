//! Public source regressions; expected binding behavior was checked in GNU r90451.
use crate::sexp::{object::Sexp, session::RSession};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn evaluate(session: &RSession, source: &str) -> Sexp<'static> {
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let expression = owner
        .with_arena(|arena| crate::eval::parser::parse(source, arena, factory.domain()))
        .unwrap()
        .unwrap();
    unsafe {
        factory.wrap(crate::eval::eval::Rf_eval(
            expression.as_raw(),
            session.global_env().unwrap().as_raw(),
        ))
    }
    .unwrap()
    .into_owned()
    .unwrap()
}

#[test]
fn owned_restore_tmp_success_preserves_bound_null_and_unrelated_bindings() {
    let session = RSession::new_for_gc_tests();
    assert_eq!(
        evaluate(&session, "{`*tmp*`<-NULL;x<-list(1L);x[[1L]][1L]<-7L;exists('*tmp*',inherits=FALSE)&&is.null(`*tmp*`)&&identical(x,list(7L))}")
            .logical_elt(0),
        Some(1)
    );
}

#[test]
fn owned_restore_tmp_error_removes_prior_binding_like_gnu() {
    let session = RSession::new_for_gc_tests();
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        evaluate(
            &session,
            "{`*tmp*`<-41L;x<-list(1L);x[[1L]][stop('temporary setter failed')]<-7L}",
        )
    }))
    .unwrap_err();
    assert!(
        payload.is::<crate::sexp::context::RError>()
            || payload.is::<crate::sexp::context::RSignal>()
    );
    assert_eq!(
        evaluate(
            &session,
            "!exists('*tmp*',inherits=FALSE)&&identical(x,list(1L))"
        )
        .logical_elt(0),
        Some(1)
    );
}

fn message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(error) = payload.downcast_ref::<crate::sexp::context::RError>() {
        &error.message
    } else if let Some(crate::sexp::context::RSignal::Error { message }) =
        payload.downcast_ref::<crate::sexp::context::RSignal>()
    {
        message
    } else {
        panic!("expected an R evaluation error")
    }
}

fn has_tmp(environment: &Sexp<'static>) -> bool {
    let mut cell = environment.try_frame().unwrap();
    while !cell.is_nil() {
        if cell.try_tag_name_eq(b"*tmp*").unwrap() {
            return true;
        }
        cell = cell.try_cdr().unwrap();
    }
    false
}

#[test]
fn owned_restore_tmp_rejects_preexisting_active_and_locked_bindings() {
    for (setup, expected, check) in [
        (
            "makeActiveBinding('*tmp*',function(value){n<<-n+1L;41L},globalenv())",
            "active binding",
            "bindingIsActive('*tmp*',globalenv())&&identical(n,1L)",
        ),
        (
            "`*tmp*`<-41L;lockBinding('*tmp*',globalenv())",
            "is locked",
            "bindingIsLocked('*tmp*',globalenv())&&identical(`*tmp*`,41L)",
        ),
    ] {
        let session = RSession::new_for_gc_tests();
        drop(evaluate(
            &session,
            &format!("{{x<-list(1L);n<-0L;{setup}}}"),
        ));
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            evaluate(&session, "x[[1L]][1L]<-7L")
        }))
        .unwrap_err();
        assert!(message(payload.as_ref()).contains(expected));
        assert_eq!(evaluate(&session, check).logical_elt(0), Some(1));
    }
}

#[test]
fn owned_restore_tmp_admission_getter_error_keeps_previous_binding() {
    let session = RSession::new_for_gc_tests();
    drop(evaluate(
        &session,
        "{x<-list(1L);n<-0L;makeActiveBinding('*tmp*',function(value){n<<-n+1L;gc();stop('admission getter failed')},globalenv())}",
    ));
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        evaluate(&session, "x[[1L]][1L]<-7L")
    }))
    .unwrap_err();
    assert!(message(payload.as_ref()).contains("admission getter failed"));
    assert_eq!(
        evaluate(
            &session,
            "bindingIsActive('*tmp*',globalenv())&&identical(n,1L)&&identical(x,list(1L))"
        )
        .logical_elt(0),
        Some(1)
    );
}

#[test]
fn owned_restore_tmp_admission_getter_revocation_keeps_previous_binding() {
    let session = RSession::new_for_gc_tests();
    drop(evaluate(
        &session,
        "{x<-list(1L);makeActiveBinding('*tmp*',function(value){gc();41L},globalenv())}",
    ));
    let environment = session.global_env().unwrap().into_owned().unwrap();
    let original = environment.runtime_owner.as_ref().unwrap().clone();
    let captured = original.clone();
    let seen = Rc::new(Cell::new(false));
    let observed = seen.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !observed.replace(true) {
            let pin = captured.pin().unwrap();
            unsafe {
                crate::sexp::instance::revoke_instance_availability(pin.as_ptr());
            }
        }
    }));
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        evaluate(&session, "x[[1L]][1L]<-7L")
    }))
    .unwrap_err();
    assert_eq!(
        message(payload.as_ref()),
        crate::sexp::object::SexpError::RootUnavailable.to_string()
    );
    assert!(seen.get());
    assert!(original.pin().is_err());
    assert!(
        has_tmp(&environment),
        "admission failure must not arm cleanup"
    );
}

#[test]
fn owned_restore_tmp_saved_value_survives_detachment_gc_and_unrelated_insertion() {
    let session = RSession::new_for_gc_tests();
    drop(evaluate(
        &session,
        "{`*tmp*`<-c(41L,42L);x<-list(1L);`[<-`<-function(x,i,value){saved<-x;gc();.Primitive('[<-')(saved,i,value=value)}}",
    ));
    let environment = session.global_env().unwrap().into_owned().unwrap();
    let old = unsafe {
        crate::sexp::envir::R_findVarInFrame(
            environment.as_raw(),
            crate::sexp::symbol::Rf_install(c"*tmp*".as_ptr()),
        )
    };
    let old_node = crate::sexp::memory::checked_projection(old).unwrap().1;
    let seen = Rc::new(Cell::new(false));
    let observed = seen.clone();
    let captured = environment.clone();
    let old_identity = old_node.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !observed.replace(true) {
            // The cell remains GNU's recorded location, but neither its CAR
            // nor any other source root retains the saved original vector.
            drop(evaluate_current(&captured, "`*tmp*`<-NULL;unrelated<-99L"));
            crate::sexp::gengc::full_gc();
            assert!(
                old_identity.is_live(),
                "saved value has no incidental owning lease"
            );
        }
    }));
    assert_eq!(
        evaluate(&session, "x[[1L]][1L]<-7L").integer_elt(0),
        Some(7)
    );
    assert!(seen.get());
    assert_eq!(
        evaluate(
            &session,
            "identical(`*tmp*`,c(41L,42L))&&identical(unrelated,99L)&&identical(x,list(7L))"
        )
        .logical_elt(0),
        Some(1)
    );
    assert!(old_node.is_live());
}

fn evaluate_current(environment: &Sexp<'static>, source: &str) -> Sexp<'static> {
    let owner = unsafe { crate::sexp::owner::OwnerToken::current() }.unwrap();
    let factory = owner.node_factory();
    let expression = owner
        .with_arena(|arena| crate::eval::parser::parse(source, arena, factory.domain()))
        .unwrap()
        .unwrap();
    unsafe {
        factory.wrap(crate::eval::eval::Rf_eval(
            expression.as_raw(),
            environment.as_raw(),
        ))
    }
    .unwrap()
    .into_owned()
    .unwrap()
}

#[test]
fn owned_restore_tmp_live_panic_keeps_exact_payload_and_cleans_locked_temporary() {
    let session = RSession::new_for_gc_tests();
    drop(evaluate(
        &session,
        "{`*tmp*`<-41L;x<-list(1L);`[<-`<-function(x,i,value){saved<-x;gc();.Primitive('[<-')(saved,i,value=value)}}",
    ));
    let environment = session.global_env().unwrap().into_owned().unwrap();
    let seen = Rc::new(Cell::new(false));
    let observed = seen.clone();
    let captured = environment.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !observed.replace(true) {
            drop(evaluate_current(
                &captured,
                "lockBinding('*tmp*',globalenv())",
            ));
            std::panic::panic_any(0x3275_u32);
        }
    }));
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        evaluate(&session, "x[[1L]][1L]<-7L")
    }))
    .unwrap_err();
    assert_eq!(*payload.downcast::<u32>().unwrap(), 0x3275);
    assert!(seen.get());
    assert!(!has_tmp(&environment));
    assert_eq!(evaluate(&session, "1L").integer_elt(0), Some(1));
}

#[test]
fn owned_restore_tmp_foreign_activation_cannot_redirect_cleanup() {
    let session = RSession::new_for_gc_tests();
    drop(evaluate(
        &session,
        "{`*tmp*`<-41L;x<-list(1L);`[<-`<-function(x,i,value){saved<-x;gc();.Primitive('[<-')(saved,i,value=value)}}",
    ));
    let environment = session.global_env().unwrap().into_owned().unwrap();
    let replacement = Rc::new(RefCell::new(None));
    let captured = replacement.clone();
    let seen = Rc::new(Cell::new(false));
    let observed = seen.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !observed.replace(true) {
            let foreign = RSession::new_for_gc_tests();
            drop(evaluate(&foreign, "`*tmp*`<-88L"));
            *captured.borrow_mut() = Some(foreign);
            std::panic::panic_any(0xFA3275_u32);
        }
    }));
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        evaluate(&session, "x[[1L]][1L]<-7L")
    }))
    .unwrap_err();
    assert_eq!(*payload.downcast::<u32>().unwrap(), 0xFA3275);
    assert!(seen.get());
    assert!(!has_tmp(&environment));
    let foreign = replacement.borrow();
    foreign.as_ref().unwrap().with_active(|| {
        assert_eq!(
            evaluate(foreign.as_ref().unwrap(), "`*tmp*`").integer_elt(0),
            Some(88)
        );
    });
    session.with_active(|| assert_eq!(evaluate(&session, "1L").integer_elt(0), Some(1)));
}

#[test]
fn owned_restore_tmp_revoked_original_cleans_without_second_panic() {
    let session = RSession::new_for_gc_tests();
    drop(evaluate(
        &session,
        "{`*tmp*`<-41L;x<-list(1L);`[<-`<-function(x,i,value){saved<-x;gc();.Primitive('[<-')(saved,i,value=value)}}",
    ));
    let environment = session.global_env().unwrap().into_owned().unwrap();
    let original = environment.runtime_owner.as_ref().unwrap().clone();
    let captured = original.clone();
    let seen = Rc::new(Cell::new(false));
    let observed = seen.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !observed.replace(true) {
            let pin = captured.pin().unwrap();
            unsafe {
                crate::sexp::instance::revoke_instance_availability(pin.as_ptr());
            }
            std::panic::panic_any(0xDE3275_u32);
        }
    }));
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        evaluate(&session, "x[[1L]][1L]<-7L")
    }))
    .unwrap_err();
    assert!(seen.get());
    assert_eq!(
        message(payload.as_ref()),
        crate::sexp::object::SexpError::RootUnavailable.to_string(),
    );
    assert!(original.pin().is_err());
    assert!(!has_tmp(&environment));
}

#[test]
fn owned_restore_tmp_normal_restore_lock_error_keeps_current_binding_like_gnu() {
    for prior in [false, true] {
        let session = RSession::new_for_gc_tests();
        let initial = if prior { "`*tmp*`<-41L;" } else { "" };
        drop(evaluate(
            &session,
            &format!(
                "{{{initial}x<-list(1L);`[[<-`<-function(x,i,value){{saved<-x;lockBinding('*tmp*',globalenv());.Primitive('[[<-')(saved,i,value=value)}}}}"
            ),
        ));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            evaluate(&session, "x[[1L]][1L]<-7L")
        }));
        if prior {
            assert!(message(result.unwrap_err().as_ref()).contains("locked binding"));
            assert_eq!(evaluate(&session, "exists('*tmp*',inherits=FALSE)&&bindingIsLocked('*tmp*',globalenv())&&identical(x,list(7L))").logical_elt(0), Some(1));
        } else {
            assert_eq!(result.unwrap().integer_elt(0), Some(7));
            assert_eq!(
                evaluate(
                    &session,
                    "!exists('*tmp*',inherits=FALSE)&&identical(x,list(7L))"
                )
                .logical_elt(0),
                Some(1)
            );
        }
    }
}

#[test]
fn owned_restore_tmp_normal_restore_rejects_detached_original_cell_like_gnu() {
    let session = RSession::new_for_gc_tests();
    drop(evaluate(
        &session,
        "{`*tmp*`<-41L;x<-list(1L);n<-0L;`[[<-`<-function(x,i,value){saved<-x;rm(list='*tmp*',envir=globalenv());makeActiveBinding('*tmp*',function(value){n<<-n+1L;41L},globalenv());.Primitive('[[<-')(saved,i,value=value)}}",
    ));
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        evaluate(&session, "x[[1L]][1L]<-7L")
    }))
    .unwrap_err();
    assert!(message(payload.as_ref()).contains("locked binding"));
    assert_eq!(
        evaluate(
            &session,
            "bindingIsActive('*tmp*',globalenv())&&identical(n,0L)&&identical(x,list(7L))"
        )
        .logical_elt(0),
        Some(1)
    );
}
