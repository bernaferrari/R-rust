//! Real text-connection construction followed by allocation/publication hooks.
use super::*;
use crate::sexp::{RSession, object::PairlistBuilder};
use std::{cell::Cell, rc::Rc};
fn text_writer_callback(action: u8) {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let env = factory
            .wrap(crate::sexp::globals::R_GlobalEnv())
            .unwrap()
            .into_owned()
            .unwrap();
        let mut builder = PairlistBuilder::from_factory(factory.clone());
        for value in [
            factory.strings(&["publication-writer"]).unwrap(),
            factory.strings(&["publication_lines"]).unwrap(),
            factory.strings(&["w"]).unwrap(),
            factory.domain().logical(false),
        ] {
            builder.push(value, None).unwrap();
        }
        let arguments = builder.finish().unwrap();
        let connection = factory
            .wrap(do_textConnection(
                factory.nil().as_raw(),
                factory.nil().as_raw(),
                arguments.as_raw(),
                env.as_raw(),
            ))
            .unwrap()
            .into_owned()
            .unwrap();
        let index = connection.try_integer_elt(0).unwrap();
        let close_args = factory
            .pairlist_cell(&connection, &factory.nil(), &factory.nil())
            .unwrap()
            .into_owned()
            .unwrap();
        let value_args = close_args.clone();
        let mut line_builder = PairlistBuilder::from_factory(factory.clone());
        for value in [
            factory.strings(&["outer"]).unwrap(),
            connection.clone(),
            factory.strings(&["\n"]).unwrap(),
            factory.domain().logical(false),
        ] {
            line_builder.push(value, None).unwrap();
        }
        let line_args = line_builder.finish().unwrap().into_owned().unwrap();
        if action == 4 {
            connection_write_bytes(index, b"outer");
        }
        if action == 5 {
            connection_write_bytes(index, b"visible\nouter");
        }
        let mut replacement_builder = PairlistBuilder::from_factory(factory.clone());
        for value in [
            factory.strings(&["replacement-writer"]).unwrap(),
            factory.strings(&["replacement_lines"]).unwrap(),
            factory.strings(&["w"]).unwrap(),
            factory.domain().logical(false),
        ] {
            replacement_builder.push(value, None).unwrap();
        }
        let replacement_args = replacement_builder.finish().unwrap().into_owned().unwrap();
        let callback_close_args = close_args.clone();
        let called = Rc::new(Cell::new(0));
        let count = called.clone();
        let instance = owner.as_ptr();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if count.replace(1) != 0 {
                return;
            }
            (*instance).memory_state.gc_force_gap = 0;
            if action == 7 {
                connection_table()[index as usize] = None;
                let fresh = do_textConnection(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    replacement_args.as_raw(),
                    crate::sexp::globals::R_GlobalEnv(),
                );
                let replacement = crate::sexp::accessors::INTEGER_ELT(fresh, 0);
                assert_eq!(
                    replacement, index,
                    "the real constructor must reuse the original slot"
                );
                connection_write_bytes(replacement, b"replacement\n");
            }
            if action == 6 {
                r_error("writer callback failure");
            }
            if action == 2 || action == 4 {
                let nested = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    do_close(
                        ptr::null_mut(),
                        ptr::null_mut(),
                        callback_close_args.as_raw(),
                        ptr::null_mut(),
                    );
                }));
                let failure = nested.expect_err("close must reject an in-flight text publication");
                assert_eq!(
                    failure.downcast_ref::<RError>().unwrap().message,
                    "text connection is busy publishing output"
                );
            }
            if action == 1 || action == 3 || action == 5 {
                let nested = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    connection_write_bytes(index, b"nested\n");
                }));
                let failure =
                    nested.expect_err("reentrant write must fail before mutating original lines");
                assert_eq!(
                    failure.downcast_ref::<RError>().unwrap().message,
                    "text connection is busy publishing output"
                );
            }
        }));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if action == 3 {
                do_writeLines(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    line_args.as_raw(),
                    env.as_raw(),
                );
            } else if action == 4 {
                do_close(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    close_args.as_raw(),
                    env.as_raw(),
                );
            } else if action == 5 {
                let value = factory
                    .wrap(do_textConnectionValue(
                        ptr::null_mut(),
                        ptr::null_mut(),
                        value_args.as_raw(),
                        env.as_raw(),
                    ))
                    .unwrap()
                    .into_owned()
                    .unwrap();
                assert_eq!(value.len(), 1);
                assert!(
                    value
                        .try_string_elt(0)
                        .unwrap()
                        .try_char_eq(b"visible")
                        .unwrap()
                );
                do_close(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    close_args.as_raw(),
                    env.as_raw(),
                );
            } else {
                connection_write_bytes(index, b"outer\n");
            }
        }));
        (*instance).memory_state.gc_force_gap = 0;
        if action == 6 || action == 7 {
            let error = result.expect_err("the callback invalidated this operation");
            assert_eq!(
                error.downcast_ref::<RError>().unwrap().message,
                if action == 6 {
                    "writer callback failure"
                } else {
                    "original connection is no longer available"
                }
            );
            if action == 6 {
                connection_write_bytes(index, b"recovered\n");
            }
        } else {
            assert!(
                result.is_ok(),
                "direct writer must survive a checked reentrant rejection"
            );
        }
        assert_eq!(
            called.get(),
            1,
            "the actual writer allocation must run the hook"
        );
        assert_eq!((*instance).memory_state.in_gc, 0);
        let value = factory
            .wrap(crate::sexp::envir::R_findVarInFrame(
                env.as_raw(),
                crate::sexp::symbol::Rf_install(c"publication_lines".as_ptr()),
            ))
            .unwrap();
        assert_eq!(
            value.len(),
            if action == 6 || action == 5 {
                2
            } else if action == 7 {
                0
            } else {
                1
            }
        );
        if action != 7 {
            assert!(
                value
                    .try_string_elt(0)
                    .unwrap()
                    .try_char_eq(if action == 5 { b"visible" } else { b"outer" })
                    .unwrap()
            );
        }
        if action == 4 || action == 5 {
            assert!(connection_table()[index as usize].is_none());
        } else {
            let table = connection_table();
            let conn = table[index as usize].as_ref().unwrap();
            assert!(conn.isopen && !conn.text_busy);
            if action == 7 {
                let state = conn.text_output.as_ref().unwrap();
                assert_eq!(state.lines, ["replacement"]);
                assert_eq!(state.name.as_deref(), Some("replacement_lines"));
            }
        }
        if action == 5 {
            assert!(
                value
                    .try_string_elt(1)
                    .unwrap()
                    .try_char_eq(b"outer")
                    .unwrap()
            );
        }
        if action == 6 {
            assert!(
                value
                    .try_string_elt(1)
                    .unwrap()
                    .try_char_eq(b"recovered")
                    .unwrap()
            );
        }
    });
}
#[test]
fn text_writer_callback_allocation_control_keeps_original_connection() {
    text_writer_callback(0);
}
#[test]
fn text_writer_callback_reentrant_write_is_checked_without_a_live_table_loan() {
    text_writer_callback(1);
}

#[test]
fn text_writer_callback_close_is_checked_before_retirement() {
    text_writer_callback(2);
}
#[test]
fn text_writer_callback_write_lines_ends_the_table_loan_before_publication() {
    text_writer_callback(3);
}
#[test]
fn text_writer_callback_close_final_fragment_owns_publication_state() {
    text_writer_callback(4);
}
#[test]
fn text_writer_callback_value_owns_lines_through_allocation() {
    text_writer_callback(5);
}
#[test]
fn text_writer_callback_unwind_restores_original_lines_and_writer_availability() {
    text_writer_callback(6);
}
#[test]
fn text_writer_callback_same_slot_replacement_never_receives_original_buffers() {
    text_writer_callback(7);
}

fn text_writer_owner_exit(action: u8) {
    use std::cell::RefCell;
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let original = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let factory = original.node_factory().unwrap();
    let mut args = PairlistBuilder::from_factory(factory.clone());
    for value in [
        factory.strings(&["owner-writer"]).unwrap(),
        factory.strings(&["owner_lines"]).unwrap(),
        factory.strings(&["w"]).unwrap(),
        factory.domain().logical(false),
    ] {
        args.push(value, None).unwrap();
    }
    let args = args.finish().unwrap().into_owned().unwrap();
    let connection = facade.borrow().as_ref().unwrap().with_active(|| unsafe {
        factory
            .wrap(do_textConnection(
                ptr::null_mut(),
                ptr::null_mut(),
                args.as_raw(),
                crate::sexp::globals::R_GlobalEnv(),
            ))
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let index = connection.try_integer_elt(0).unwrap();
    let closed = Rc::downgrade(&facade);
    let replacement = Rc::new(RefCell::new(None));
    let replaced = replacement.clone();
    let calls = Rc::new(Cell::new(0));
    let called = calls.clone();
    let pointer = original.identity_ptr();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| unsafe {
        if called.replace(1) != 0 {
            return;
        }
        (*pointer).memory_state.gc_force_gap = 0;
        if action == 0 {
            closed
                .upgrade()
                .unwrap()
                .borrow_mut()
                .as_mut()
                .unwrap()
                .close();
        } else {
            drop(closed.upgrade().unwrap().borrow_mut().take());
            *replaced.borrow_mut() = Some(RSession::new_for_gc_tests());
        }
    }));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(pointer, || {
            (*pointer).memory_state.gc_force_gap = 1;
            (*pointer).memory_state.gc_force_wait = 1;
            connection_write_bytes(index, b"original\n");
        });
    }));
    assert_eq!(calls.get(), 1);
    let error = outcome.expect_err("revoked runtime must reject publication");
    assert_eq!(
        error.downcast_ref::<RError>().unwrap().message,
        crate::sexp::object::SexpError::RootUnavailable.to_string()
    );
    assert!(original.pin().is_err());
    drop(facade.borrow_mut().take());
    assert_eq!(
        original.allocation_strong_count(),
        0,
        "values and text storage must not retain the runtime"
    );
    if let Some(replacement) = replacement.borrow().as_ref() {
        replacement.with_active(|| {
            let table = connection_table();
            assert_eq!(table.iter().filter(|slot| slot.is_some()).count(), 3);
            assert!(!crate::sexp::output::is_capturing());
        });
    }
}
#[test]
fn text_writer_owner_close_revokes_publication_without_retaining_the_runtime() {
    text_writer_owner_exit(0);
}
#[test]
fn text_writer_owner_drop_and_replacement_never_receive_original_publication() {
    text_writer_owner_exit(1);
}
