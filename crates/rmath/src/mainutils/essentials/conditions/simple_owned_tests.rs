use super::*;
use crate::sexp::{
    encoding::{self, TextEncoding},
    object::SexpMut,
    session::RSession,
};
use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

unsafe fn construct(kind: u8, args: SEXP) -> SEXP {
    unsafe {
        match kind {
            0 => do_simpleError(R_NilValue(), R_NilValue(), args, R_NilValue()),
            1 => do_simpleWarning(R_NilValue(), R_NilValue(), args, R_NilValue()),
            _ => do_simpleCondition(R_NilValue(), R_NilValue(), args, R_NilValue()),
        }
    }
}

fn native_constructor(kind: u8, action: u8) {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let weak = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let pin = weak.pin().unwrap();
    let instance = pin.as_ptr();
    let factory = weak.node_factory().unwrap();
    let foreign = Rc::new(RefCell::new(None::<RSession>));
    let foreign_bytes = Rc::new(Cell::new(0));
    let mut input = SexpMut::try_from_checked(
        factory
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 2)))
            .unwrap(),
    )
    .unwrap();
    let first = factory
        .allocate(|a| encoding::allocate_character(a, b"a\xff", TextEncoding::Utf8))
        .unwrap();
    let second = factory
        .allocate(|a| encoding::allocate_character(a, b"b\x80", TextEncoding::Latin1))
        .unwrap();
    input.try_set_string_elt(0, first).unwrap();
    input.try_set_string_elt(1, second).unwrap();
    let input = input.freeze();
    let source = input.allocation().unwrap().clone();
    let function = factory
        .wrap(unsafe { Rf_install(c"condition_target".as_ptr()) })
        .unwrap();
    let argument = factory.wrap(unsafe { Rf_install(c"x".as_ptr()) }).unwrap();
    let arguments = factory
        .pairlist_cell(&argument, &factory.nil(), &factory.nil())
        .unwrap();
    let call = factory.call(&function, &arguments).unwrap();
    let call_source = call.allocation().unwrap().clone();
    let tail = factory
        .pairlist_cell(&call, &factory.nil(), &factory.nil())
        .unwrap();
    let args = factory
        .pairlist_cell(&input, &tail, &factory.nil())
        .unwrap()
        .into_owned()
        .unwrap();
    drop((input, call, tail));
    let seen = Rc::new(Cell::new(0));
    unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            if action != 0 {
                let heap = source.heap_identity();
                let observed = source.clone();
                let observed_call = call_source.clone();
                let saved = args.clone();
                let called = seen.clone();
                let revoker = Rc::downgrade(&facade);
                let other = foreign.clone();
                let other_bytes = foreign_bytes.clone();
                crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                    if called.get() != 0 {
                        return;
                    }
                    let names = crate::sexp::memory::fresh_allocation_roots_for_marking(&heap)
                        .into_iter()
                        .any(|(_, node)| {
                            let header = heap.node_snapshot(&node).unwrap();
                            header.sxpinfo.type_of() == SEXPTYPE::STRSXP
                                && heap
                                    .payload_lease(&node)
                                    .is_some_and(|p| p.capacity() == if kind == 2 { 2 } else { 3 })
                        });
                    if !names {
                        return;
                    }
                    called.set(1);
                    (*instance).memory_state.gc_force_gap = 0;
                    SETCAR(saved.as_raw(), R_NilValue());
                    SETCDR(saved.as_raw(), R_NilValue());
                    crate::sexp::gengc::full_gc();
                    assert!(
                        observed.is_live(),
                        "original selected message survives detached arguments and actual constructor GC"
                    );
                    assert!(
                        observed_call.is_live(),
                        "original call survives detached arguments and actual constructor GC"
                    );
                    match action {
                        2 => std::panic::panic_any(991u32),
                        3 => drop(revoker.upgrade().unwrap().borrow_mut().take()),
                        4 => {
                            let session = RSession::new_for_gc_tests();
                            other_bytes.set(session.with_active_in(|other_instance| {
                                (*other_instance).error_state.in_error = 73;
                                (*other_instance).error_state.expressions = 812;
                                session
                                    .owner_token()
                                    .unwrap()
                                    .with_arena(|arena| {
                                        arena.total_bytes_allocated() + arena.workspace_bytes()
                                    })
                                    .unwrap()
                            }));
                            *other.borrow_mut() = Some(session);
                            drop(revoker.upgrade().unwrap().borrow_mut().take());
                        }
                        _ => {}
                    }
                }));
                (*instance).memory_state.gc_force_gap = 1;
                (*instance).memory_state.gc_force_wait = 1;
            }
            let result = catch_unwind(AssertUnwindSafe(|| construct(kind, args.as_raw())));
            if action != 0 {
                assert_eq!(seen.get(), 1, "actual attribute vector constructor reached");
            }
            assert_eq!((*instance).memory_state.in_gc, 0);
            assert_eq!(
                source
                    .heap_identity()
                    .byte_ledger()
                    .workspace_bytes_for_test(),
                0
            );
            if action >= 2 {
                let error = result.unwrap_err();
                if action == 2 {
                    assert_eq!(*error.downcast::<u32>().unwrap(), 991);
                } else {
                    assert!(!weak.is_live());
                }
                if let Some(other) = foreign.borrow().as_ref() {
                    other.with_active_in(|other_instance| {
                        assert_eq!((*other_instance).error_state.in_error, 73);
                        assert_eq!((*other_instance).error_state.expressions, 812);
                        assert_eq!(
                            other
                                .owner_token()
                                .unwrap()
                                .with_arena(
                                    |arena| arena.total_bytes_allocated() + arena.workspace_bytes()
                                )
                                .unwrap(),
                            foreign_bytes.get()
                        );
                    });
                }
                return;
            }
            let value = factory.wrap(result.unwrap()).unwrap();
            assert_eq!(value.len(), 2, "message and call fields");
            let message = value.try_vector_elt(0).unwrap();
            assert_eq!(message.len(), 2, "entire as.character message vector");
            for (index, bytes, tag) in [
                (0, &b"a\xff"[..], TextEncoding::Utf8),
                (1, &b"b\x80"[..], TextEncoding::Latin1),
            ] {
                let ch = message.try_string_elt(index).unwrap();
                let node = ch.allocation().unwrap();
                let heap = node.heap_identity();
                let header = heap.node_snapshot(node).unwrap();
                assert_eq!(TextEncoding::from_gp(header.sxpinfo.gp()), tag);
                let payload = heap.payload_lease(node).unwrap();
                assert_eq!(
                    (0..bytes.len())
                        .map(|i| payload.byte_elt(i).unwrap())
                        .collect::<Vec<_>>(),
                    bytes
                );
            }
            assert_eq!(
                value.try_vector_elt(1).unwrap().allocation().unwrap(),
                &call_source
            );
        });
    }
}

#[test]
fn owned_simple_constructor_error_preserves_vector_bytes_tags_and_call() {
    native_constructor(0, 0);
}
#[test]
fn owned_simple_constructor_warning_preserves_vector_bytes_tags_and_call() {
    native_constructor(1, 0);
}
#[test]
fn owned_simple_constructor_condition_preserves_vector_bytes_tags_and_call() {
    native_constructor(2, 0);
}
#[test]
fn owned_simple_constructor_error_roots_selection_before_actual_attribute_gc() {
    native_constructor(0, 1);
}
#[test]
fn owned_simple_constructor_warning_roots_selection_before_actual_attribute_gc() {
    native_constructor(1, 1);
}
#[test]
fn owned_simple_constructor_condition_roots_selection_before_actual_attribute_gc() {
    native_constructor(2, 1);
}

#[test]
fn owned_simple_constructor_error_panic_releases_original_roots() {
    native_constructor(0, 2);
}

#[test]
fn owned_simple_constructor_error_revocation_rejects_publication() {
    native_constructor(0, 3);
}

#[test]
fn owned_simple_constructor_error_revocation_cannot_allocate_in_foreign_runtime() {
    native_constructor(0, 4);
}

#[test]
fn owned_simple_constructor_warning_panic_releases_original_roots() {
    native_constructor(1, 2);
}

#[test]
fn owned_simple_constructor_warning_revocation_rejects_publication() {
    native_constructor(1, 3);
}

#[test]
fn owned_simple_constructor_warning_revocation_cannot_allocate_in_foreign_runtime() {
    native_constructor(1, 4);
}

#[test]
fn owned_simple_constructor_condition_panic_releases_original_roots() {
    native_constructor(2, 2);
}

#[test]
fn owned_simple_constructor_condition_revocation_rejects_publication() {
    native_constructor(2, 3);
}

#[test]
fn owned_simple_constructor_condition_revocation_cannot_allocate_in_foreign_runtime() {
    native_constructor(2, 4);
}

fn original_cap(kind: u8) {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let message = factory.strings(&["original budget"]).unwrap();
    let args = factory
        .pairlist_cell(&message, &factory.nil(), &factory.nil())
        .unwrap();
    let before = owner
        .with_arena(|arena| arena.total_bytes_allocated())
        .unwrap();
    owner
        .with_arena(|arena| arena.set_budget(crate::sexp::memory::ArenaBudget::new(1, 0)))
        .unwrap();
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        construct(kind, args.as_raw())
    }));
    assert!(
        result.is_err(),
        "the unchanged original cap denies the first constructor allocation"
    );
    owner
        .with_arena(|arena| {
            assert_eq!(arena.budget().max_bytes, 1);
            assert_eq!(arena.total_bytes_allocated(), before);
            assert_eq!(arena.workspace_bytes(), 0);
        })
        .unwrap();
    assert!(
        message
            .try_string_elt(0)
            .unwrap()
            .try_char_eq(b"original budget")
            .unwrap()
    );
}

#[test]
fn owned_simple_constructor_error_obeys_lowered_original_cap() {
    original_cap(0);
}
#[test]
fn owned_simple_constructor_warning_obeys_lowered_original_cap() {
    original_cap(1);
}
#[test]
fn owned_simple_constructor_condition_obeys_lowered_original_cap() {
    original_cap(2);
}

#[test]
fn owned_simple_constructor_foreign_arguments_reject_before_current_allocation() {
    let first = RSession::new_for_gc_tests();
    let factory = first.owner_token().unwrap().node_factory();
    let message = factory.strings(&["foreign message"]).unwrap();
    let args = factory
        .pairlist_cell(&message, &factory.nil(), &factory.nil())
        .unwrap();
    let second = RSession::new_for_gc_tests();
    second.with_active(|| {
        let owner = second.owner_token().unwrap();
        let before = owner
            .with_arena(|arena| arena.total_bytes_allocated() + arena.workspace_bytes())
            .unwrap();
        for kind in 0..3 {
            assert!(
                catch_unwind(AssertUnwindSafe(|| unsafe {
                    construct(kind, args.as_raw())
                }))
                .is_err()
            );
            assert_eq!(
                owner
                    .with_arena(|arena| arena.total_bytes_allocated() + arena.workspace_bytes())
                    .unwrap(),
                before
            );
        }
    });
}

#[test]
fn native_simple_constructor_generic_dispatch_keeps_exact_method_result_and_call() {
    let mut session = RSession::new_without_default_packages();
    let (result, _, _) = session.eval_code_with_output_capture(
        "as.character.native_condition_probe<-function(x,...) {gc();structure(c(11L,17L),names=c('a','b'))}; x<-structure(1L,class='native_condition_probe')",
    );
    result.unwrap();
    let factory = session.owner_token().unwrap().node_factory();
    let input = unsafe {
        factory
            .wrap(crate::sexp::envir::R_findVarInFrame(
                crate::sexp::globals::R_GlobalEnv(),
                Rf_install(c"x".as_ptr()),
            ))
            .unwrap()
    };
    let call = factory
        .wrap(unsafe { Rf_install(c"original_call".as_ptr()) })
        .unwrap();
    let tail = factory
        .pairlist_cell(&call, &factory.nil(), &factory.nil())
        .unwrap();
    let args = factory
        .pairlist_cell(&input, &tail, &factory.nil())
        .unwrap();
    for kind in 0..3 {
        let condition = factory
            .wrap(unsafe { construct(kind, args.as_raw()) })
            .unwrap();
        let message = condition.try_vector_elt(0).unwrap();
        assert_eq!(message.typeof_(), SEXPTYPE::INTSXP);
        assert_eq!(message.integer_elt(0), Some(11));
        assert_eq!(message.integer_elt(1), Some(17));
        assert!(
            !message.try_attrib().unwrap().is_nil(),
            "GNU preserves a custom generic's returned attributes"
        );
        assert_eq!(condition.try_vector_elt(1).unwrap().as_raw(), call.as_raw());
    }
}
