//! The pinned GNU oracle intentionally repeats an outer index in its setter.
use super::*;
use crate::sexp::session::RSession;

const NESTED: &str = "x<-list(c(1L,2L));events<-character();rhsValue<-7L;i<-function(){events<<-c(events,'i');1L};j<-function(){events<<-c(events,'j');2L};rhs<-function(){events<<-c(events,'rhs');rhsValue}";
const ATTRIBUTE: &str = "x<-list(c(1L,2L));events<-character();rhsValue<-quote(unbound_data);i<-function(){events<<-c(events,'i');1L};name<-function(){events<<-c(events,'name');'note'};rhs<-function(){events<<-c(events,'rhs');rhsValue}";
const NAMES: &str = "x<-list(c(1L,2L));events<-character();rhsValue<-c('left','right');i<-function(){events<<-c(events,'i');1L};rhs<-function(){events<<-c(events,'rhs');rhsValue}";

const SYMBOL_HEADS: &str = "x<-list(c(1L,2L));events<-character();metadata<-list();rhsValue<-7L;i<-function(){events<<-c(events,'i');1L};j<-function(){events<<-c(events,'j');2L};rhs<-function(){events<<-c(events,'rhs');rhsValue};leaf<-function(x,slot){events<<-c(events,'leafGet');metadata<<-.Primitive('[[<-')(metadata,1L,value=list(substitute(x),substitute(slot),.subset2(as.list(sys.call()),1L)));.subset2(x,slot)};`leaf<-`<-function(x,slot,value){events<<-c(events,'leafSet');metadata<<-.Primitive('[[<-')(metadata,3L,value=list(substitute(x),substitute(slot),substitute(value),.subset2(as.list(sys.call()),1L)));.Primitive('[[<-')(x,slot,value=value)};`tip<-`<-function(x,index,value){events<<-c(events,'tipSet');metadata<<-.Primitive('[[<-')(metadata,2L,value=list(substitute(x),substitute(index),substitute(value),.subset2(as.list(sys.call()),1L)));.Primitive('[<-')(x,index,value=value)}";
const SYMBOL_ASSERTION: &str = "stopifnot(identical(x,list(c(1L,7L))),identical(events,c('rhs','leafGet','i','tipSet','j','leafSet','i')),identical(metadata,list(list(as.name('*tmp*'),quote(i()),as.name('leaf')),list(as.name('*tmp*'),quote(j()),quote(rhs()),as.name('tip<-')),list(as.name('*tmp*'),quote(i()),as.name('*vtmp*'),as.name('leaf<-')))))";

fn setup(session: &mut RSession, source: &str) {
    let (value, captured, _) = session.eval_code_with_output_capture(source);
    drop(
        value
            .unwrap_or_else(|error| panic!("{error:?}\n{captured:?}"))
            .into_owned()
            .unwrap(),
    );
}
fn binding(environment: &Sexp<'static>, name: &std::ffi::CStr) -> Sexp<'static> {
    unsafe {
        own_operand(crate::sexp::envir::R_findVar(
            crate::sexp::symbol::Rf_install(name.as_ptr()),
            environment.as_raw(),
        ))
    }
}
fn parse(session: &RSession, syntax: &str, environment: &Sexp<'static>) -> Sexp<'static> {
    session
        .owner_token()
        .unwrap()
        .with_arena(|arena| {
            crate::eval::parser::parse(syntax, arena, environment.node_factory().unwrap().domain())
        })
        .unwrap()
        .unwrap()
}
#[cfg(not(miri))]
fn compare(session: &mut RSession, source: &str, syntax: &str, assertion: &str) {
    setup(session, source);
    let environment = session.global_env().unwrap().into_owned().unwrap();
    let expression = parse(session, syntax, &environment);
    let expected = unsafe {
        own_operand(crate::eval::eval::Rf_eval(
            expression.as_raw(),
            environment.as_raw(),
        ))
    };
    assert_eq!(expected, binding(&environment, c"rhsValue"));
    assert_eq!(unsafe { crate::sexp::globals::R_Visible() }, 0);
    setup(session, assertion);
    let expected_object = binding(&environment, c"x");
    let expected_events = binding(&environment, c"events");
    drop(expression);
    setup(session, source);
    let expression = parse(session, syntax, &environment);
    let body = unsafe {
        own_operand(
            compile_expr(expression.as_raw(), environment.as_raw())
                .expect("general replacement must compile"),
        )
    };
    let pool = unsafe { own_operand(super::super::bc_eval::BCODE_CONSTS(body.as_raw())) };
    SexpMut::try_from_checked(pool)
        .unwrap()
        .try_set_vector_elt(0, environment.node_factory().unwrap().nil())
        .unwrap();
    drop(expression);
    session.gc();
    let rhs = binding(&environment, c"rhsValue");
    let actual = unsafe {
        own_operand(super::super::bc_eval::bcEval(
            body.as_raw(),
            environment.as_raw(),
        ))
    };
    assert_eq!(actual, rhs, "return original RHS identity");
    assert_eq!(unsafe { crate::sexp::globals::R_Visible() }, 0);
    for (expected, actual) in [
        (expected_object, binding(&environment, c"x")),
        (expected_events, binding(&environment, c"events")),
    ] {
        assert_eq!(
            unsafe {
                crate::mainutils::identical::R_compute_identical(
                    expected.as_raw(),
                    actual.as_raw(),
                    0,
                )
            },
            1
        );
    }
    setup(session, assertion);
}
#[test]
#[cfg(not(miri))]
fn compiled_general_nested_subscripts_repeat_outer_index_like_gnu() {
    compare(
        &mut RSession::new_without_default_packages(),
        NESTED,
        "x[[i()]][j()]<-rhs()",
        "stopifnot(identical(x,list(c(1L,7L))),identical(events,c('rhs','i','j','i')))",
    );
}
#[test]
#[cfg(not(miri))]
fn compiled_general_attribute_replacement_keeps_symbol_data_and_order() {
    compare(
        &mut RSession::new_without_default_packages(),
        ATTRIBUTE,
        "attr(x[[i()]],name())<-rhs()",
        "stopifnot(identical(x,list(structure(c(1L,2L),note=quote(unbound_data)))),identical(events,c('rhs','i','name','i')))",
    );
}
#[test]
#[cfg(not(miri))]
fn compiled_general_names_replacement_preserves_rhs_and_order() {
    compare(
        &mut RSession::new_without_default_packages(),
        NAMES,
        "names(x[[i()]])<-rhs()",
        "stopifnot(identical(x,list(c(left=1L,right=2L))),identical(events,c('rhs','i','i')))",
    );
}

fn compile(session: &RSession, syntax: &str) -> (Sexp<'static>, Sexp<'static>) {
    let environment = session.global_env().unwrap().into_owned().unwrap();
    let expression = parse(session, syntax, &environment);
    let body =
        unsafe { own_operand(compile_expr(expression.as_raw(), environment.as_raw()).unwrap()) };
    let pool = unsafe { own_operand(super::super::bc_eval::BCODE_CONSTS(body.as_raw())) };
    SexpMut::try_from_checked(pool)
        .unwrap()
        .try_set_vector_elt(0, environment.node_factory().unwrap().nil())
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
fn owned_general_replacement_sole_targets_and_rhs_survive_detached_pool_gc() {
    use std::{cell::Cell, rc::Rc};
    let mut session = RSession::new_for_gc_tests();
    setup(
        &mut session,
        "x<-list(c(1L,2L));rhs<-function(){c('left','right')};`[[`<-function(x,i){child<-.subset2(x,1L);gc();child};`[[<-`<-function(x,i,value){gc();list(value)}",
    );
    let (body, environment) = compile(&session, "names(x[[unbound_index]])<-rhs()");
    session.gc();
    let pool = unsafe { own_operand(super::super::bc_eval::BCODE_CONSTS(body.as_raw())) };
    let captured = environment.clone();
    let count = Rc::new(Cell::new(0));
    let observed = count.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        let previous = observed.replace(observed.get() + 1);
        if previous == 0 {
            let nil = pool.node_factory().unwrap().nil();
            let mut mutable = SexpMut::try_from_checked(pool.clone()).unwrap();
            for index in 0..pool.len() {
                mutable.try_set_vector_elt(index, nil.clone()).unwrap();
            }
            drop(mutable);
            unsafe {
                crate::sexp::envir::defineVar(
                    crate::sexp::symbol::Rf_install(c"x".as_ptr()),
                    nil.as_raw(),
                    captured.as_raw(),
                );
            }
            crate::sexp::gengc::full_gc();
        }
    }));
    let value = execute(&body, &environment);
    assert_eq!(unsafe { crate::sexp::globals::R_Visible() }, 0);
    assert_eq!(value.typeof_(), SEXPTYPE::STRSXP);
    assert_eq!(value.len(), 2);
    assert!(count.get() >= 2, "getter and outer setter both collect");
    let (ok, _, _) =
        session.eval_code_with_output_capture("identical(x,list(c(left=1L,right=2L)))");
    assert_eq!(ok.unwrap().logical_elt(0), Some(1));
}

#[test]
fn owned_general_replacement_rejects_bad_levels_and_cyclic_syntax() {
    let mut session = RSession::new_for_gc_tests();
    setup(
        &mut session,
        "x<-list(1L);rhsValue<-7L;events<-0L;`[[`<-function(x,i){events<<-1L;.subset2(x,1L)}",
    );
    let (body, environment) = compile(&session, "attr(x[[1L]],'note')<-rhsValue");
    let words = unsafe { own_operand(crate::sexp::accessors::VECTOR_ELT(body.as_raw(), 0)) };
    let opcode = (0..words.len())
        .find(|&index| words.integer_elt(index) == Some(opcodes::OP_REPLACEMENT_CHAIN))
        .unwrap();
    let pool = unsafe { own_operand(super::super::bc_eval::BCODE_CONSTS(body.as_raw())) };
    let scalar = (0..pool.len())
        .find(|&index| pool.try_vector_elt(index).unwrap().typeof_() == SEXPTYPE::INTSXP)
        .unwrap();
    for (offset, bad) in [
        (2, 2),
        (4, -1),
        (4, 0),
        (4, 1000),
        (5, scalar as i32),
        (6, scalar as i32),
        (7, -1),
        (7, 1000),
        (9, scalar as i32),
    ] {
        let index = opcode + offset;
        let original = words.integer_elt(index).unwrap();
        SexpMut::try_from_checked(words.clone())
            .unwrap()
            .try_set_integer_elt(index, bad)
            .unwrap();
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            execute(&body, &environment)
        }))
        .unwrap_err();
        assert!(!error_message(error).is_empty());
        assert_eq!(binding(&environment, c"events").integer_elt(0), Some(0));
        SexpMut::try_from_checked(words.clone())
            .unwrap()
            .try_set_integer_elt(index, original)
            .unwrap();
    }
    assert_eq!(execute(&body, &environment).integer_elt(0), Some(7));
    let expression = parse(&session, "names(x[[1L]])<-rhsValue", &environment);
    let lhs = unsafe { own_operand(CAR(CDR(expression.as_raw()))) };
    // Deliberately make an invalid native syntax graph; checked path traversal
    // must reject the actual CAR cycle before invoking Rust recursion.
    unsafe {
        crate::sexp::accessors::SETCAR(CDR(lhs.as_raw()), lhs.as_raw());
    }
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        compile_expr(expression.as_raw(), environment.as_raw())
    }))
    .unwrap_err();
    assert!(error_message(error).contains("cyclic replacement syntax spine"));
}

#[test]
fn owned_general_replacement_keeps_live_errors_and_panic_recovery() {
    use std::{cell::Cell, rc::Rc};
    let mut session = RSession::new_for_gc_tests();
    setup(
        &mut session,
        "x<-list(1L);rhsValue<-7L;`[[`<-function(x,i){gc();.subset2(x,1L)};`attr<-`<-function(x,which,value){stop('general setter failed')}",
    );
    let (body, environment) = compile(&session, "attr(x[[1L]],'note')<-rhsValue");
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        execute(&body, &environment)
    }))
    .unwrap_err();
    assert!(error_message(error).contains("general setter failed"));
    let observed = Rc::new(Cell::new(false));
    let count = observed.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !count.replace(true) {
            std::panic::panic_any(0xC165_u32);
        }
    }));
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        execute(&body, &environment)
    }))
    .unwrap_err();
    assert_eq!(*payload.downcast::<u32>().unwrap(), 0xC165);
    assert!(observed.get());
    setup(&mut session, "stopifnot(identical(x,list(1L)));1L");
}

#[test]
fn owned_general_replacement_refuses_revoked_original_runtime() {
    use std::{cell::Cell, rc::Rc};
    let mut session = RSession::new_for_gc_tests();
    setup(
        &mut session,
        "x<-list(1L);rhsValue<-7L;`[[`<-function(x,i){gc();x}",
    );
    let (body, environment) = compile(&session, "attr(x[[1L]],'note')<-rhsValue");
    let original = environment.runtime_owner.as_ref().unwrap().clone();
    let owner = original.clone();
    let observed = Rc::new(Cell::new(false));
    let count = observed.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !count.replace(true) {
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
    assert!(observed.get());
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
fn gnu_general_replacement_fixtures_execute_direct_vm_after_closure_release() {
    let mut session = RSession::new_without_default_packages();
    let cases: &[(&[u8], &str, &str)] = &[
        (
            include_bytes!(
                "../../../../r-embed/tests/fixtures/gnu-bytecode-general-replacement/nested.rds"
            ),
            NESTED,
            "stopifnot(identical(x,list(c(1L,7L))),identical(events,c('rhs','i','j','i')))",
        ),
        (
            include_bytes!(
                "../../../../r-embed/tests/fixtures/gnu-bytecode-general-replacement/attribute.rds"
            ),
            ATTRIBUTE,
            "stopifnot(identical(x,list(structure(c(1L,2L),note=quote(unbound_data)))),identical(events,c('rhs','i','name','i')))",
        ),
        (
            include_bytes!(
                "../../../../r-embed/tests/fixtures/gnu-bytecode-general-replacement/names.rds"
            ),
            NAMES,
            "stopifnot(identical(x,list(c(left=1L,right=2L))),identical(events,c('rhs','i','i')))",
        ),
        (
            include_bytes!(
                "../../../../r-embed/tests/fixtures/gnu-bytecode-general-replacement/symbol_heads.rds"
            ),
            SYMBOL_HEADS,
            SYMBOL_ASSERTION,
        ),
    ];
    for (bytes, source, assertion) in cases {
        let raw = bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        setup(
            &mut session,
            &format!("f<-unserialize(as.raw(c({raw})));{source}"),
        );
        let environment = session.global_env().unwrap().into_owned().unwrap();
        let function = binding(&environment, c"f");
        let body = unsafe { own_operand(crate::sexp::accessors::BODY(function.as_raw())) };
        assert!(unsafe { super::super::bc_eval::BCODE_IS_GNU(body.as_raw()) });
        setup(&mut session, "f<-NULL");
        drop(function);
        session.gc();
        assert_eq!(
            execute(&body, &environment),
            binding(&environment, c"rhsValue")
        );
        assert_eq!(unsafe { crate::sexp::globals::R_Visible() }, 0);
        setup(&mut session, assertion);
    }
}

#[test]
#[cfg(not(miri))]
fn compiled_general_replacement_keeps_tagged_syntax_and_sharing() {
    let source = "x<-list(c(1L,2L));events<-character();metadata<-list();rhsValue<-7L;i<-function(){events<<-c(events,'i');1L};j<-function(){events<<-c(events,'j');2L};rhs<-function(){events<<-c(events,'rhs');rhsValue};`[[`<-function(x,which){events<<-c(events,'get');metadata<<-.Primitive('[[<-')(metadata,1L,value=list(substitute(x),substitute(which),.subset2(as.list(sys.call()),1L)));.subset2(x,which)};`[<-`<-function(x,where,value){events<<-c(events,'inner');metadata<<-.Primitive('[[<-')(metadata,2L,value=list(substitute(x),substitute(where),substitute(value),.subset2(as.list(sys.call()),1L)));attr(value,'changed')<-TRUE;.Primitive('[<-')(x,where,value=value)};`[[<-`<-function(x,which,value){events<<-c(events,'outer');metadata<<-.Primitive('[[<-')(metadata,3L,value=list(substitute(x),substitute(which),substitute(value),.subset2(as.list(sys.call()),1L)));.Primitive('[[<-')(x,which,value=value)}";
    compare(
        &mut RSession::new_without_default_packages(),
        source,
        "x[[which=i()]][where=j()]<-rhs()",
        "stopifnot(identical(rhsValue,7L),identical(x,list(c(1L,7L))),identical(events,c('rhs','get','i','inner','j','outer','i')),identical(metadata,list(list(as.name('*tmp*'),quote(i()),as.name('[[')),list(as.name('*tmp*'),quote(j()),quote(rhs()),as.name('[<-')),list(as.name('*tmp*'),quote(i()),as.name('*vtmp*'),as.name('[[<-')))))",
    );
}

#[test]
#[cfg(not(miri))]
fn compiled_general_replacement_deeper_and_enclosing_roots() {
    let mut session = RSession::new_without_default_packages();
    compare(
        &mut session,
        "x<-list(list(c(1L,2L)));events<-character();rhsValue<-7L;i<-function(){events<<-c(events,'i');1L};j<-function(){events<<-c(events,'j');1L};k<-function(){events<<-c(events,'k');2L};rhs<-function(){events<<-c(events,'rhs');rhsValue}",
        "x[[i()]][[j()]][k()]<-rhs()",
        "stopifnot(identical(x,list(list(c(1L,7L)))),identical(events,c('rhs','i','j','k','j','i')))",
    );
    setup(
        &mut session,
        "x<-list(c(1L,2L));frame<-new.env();frame$x<-list(c(50L,60L));events<-character();rhsValue<-c('left','right');i<-function(){events<<-c(events,'i');1L};rhs<-function(){events<<-c(events,'rhs');x<<-list(c(30L,40L));rhsValue}",
    );
    let global = session.global_env().unwrap().into_owned().unwrap();
    let environment = binding(&global, c"frame");
    let expression = parse(&session, "names(x[[i()]])<<-rhs()", &environment);
    let body =
        unsafe { own_operand(compile_expr(expression.as_raw(), environment.as_raw()).unwrap()) };
    let pool = unsafe { own_operand(super::super::bc_eval::BCODE_CONSTS(body.as_raw())) };
    SexpMut::try_from_checked(pool)
        .unwrap()
        .try_set_vector_elt(0, environment.node_factory().unwrap().nil())
        .unwrap();
    drop(expression);
    session.gc();
    assert_eq!(execute(&body, &environment), binding(&global, c"rhsValue"));
    assert_eq!(unsafe { crate::sexp::globals::R_Visible() }, 0);
    setup(
        &mut session,
        "stopifnot(identical(x,list(c(left=30L,right=40L))),identical(frame$x,list(c(50L,60L))),identical(events,c('rhs','i','i')))",
    );
}

#[test]
#[cfg(not(miri))]
fn compiled_general_symbol_heads_preserve_resolution_and_named_syntax() {
    compare(
        &mut RSession::new_without_default_packages(),
        SYMBOL_HEADS,
        "tip(leaf(x,slot=i()),index=j())<-rhs()",
        SYMBOL_ASSERTION,
    );
}

#[test]
#[cfg(not(miri))]
fn source_general_symbol_heads_keep_original_rhs_and_outward_code() {
    let mut session = RSession::new_without_default_packages();
    setup(&mut session, SYMBOL_HEADS);
    setup(
        &mut session,
        "r<-withVisible(tip(leaf(x,slot=i()),index=j())<-rhs());stopifnot(identical(r,list(value=7L,visible=FALSE)))",
    );
    setup(&mut session, SYMBOL_ASSERTION);
}

#[test]
fn owned_source_general_replacement_cached_codes_survive_collection() {
    use std::{cell::Cell, rc::Rc};
    let mut session = RSession::new_for_gc_tests();
    setup(
        &mut session,
        "x<-list(1L);innerCode<-NULL;outerCode<-NULL;rhs<-function(){c(7L,8L)};leaf<-function(x,index){gc();.subset2(x,1L)};`leaf<-`<-function(x,index,value){outerCode<<-substitute(value);gc();list(value)};`tip<-`<-function(x,value){innerCode<<-substitute(value);gc();value}",
    );
    let count = Rc::new(Cell::new(0));
    let observed = count.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        observed.set(observed.get() + 1);
        crate::sexp::gengc::full_gc();
    }));
    let (result, _, visible) =
        session.eval_code_with_output_capture("tip(leaf(x,ignored_unbound_index))<-rhs()");
    let result = result.unwrap().into_owned().unwrap();
    assert_eq!(result.integer_elt(0), Some(7));
    assert_eq!(result.integer_elt(1), Some(8));
    assert!(!visible);
    assert!(count.get() >= 3, "getter and both setters collect");
    setup(
        &mut session,
        "stopifnot(identical(x,list(c(7L,8L))),identical(innerCode,quote(rhs())),identical(outerCode,as.name('*vtmp*')))",
    );
}
