//! Real text-connection construction followed by allocation/publication hooks.
use super::*;
use crate::sexp::{RSession, object::PairlistBuilder, output::OutputCaptureGuard};
use std::{cell::Cell, rc::Rc};
fn text_publication_callback(action: u8) {
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
        let mut capture = OutputCaptureGuard::start();
        capture.set_connection(index);
        let close_args = factory
            .pairlist_cell(&connection, &factory.nil(), &factory.nil())
            .unwrap()
            .into_owned()
            .unwrap();
        let called = Rc::new(Cell::new(0));
        let count = called.clone();
        let instance = owner.as_ptr();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if count.replace(1) != 0 {
                return;
            }
            (*instance).memory_state.gc_force_gap = 0;
            if action == 2 {
                let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    do_close(
                        ptr::null_mut(),
                        ptr::null_mut(),
                        close_args.as_raw(),
                        ptr::null_mut(),
                    )
                }));
                let error = failed.expect_err("an active capture destination must reject close");
                assert_eq!(
                    error.downcast_ref::<RError>().unwrap().message,
                    "cannot close an active capture connection"
                );
            }
            if action == 3 {
                assert!(crate::sexp::output::stop_capture().stdout.is_empty());
                crate::sexp::output::start_capture();
                crate::mainutils::printutils::Rprintf(c"replacement".as_ptr(), ptr::null_mut());
            }
            if action == 1 {
                let nested = OutputCaptureGuard::start();
                crate::mainutils::printutils::Rprintf(c"nested".as_ptr(), ptr::null_mut());
                assert_eq!(nested.finish().stdout, "nested");
            }
        }));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::mainutils::printutils::Rprintf(c"outer\n".as_ptr(), ptr::null_mut());
        }));
        (*instance).memory_state.gc_force_gap = 0;
        if action == 3 {
            let error = result.expect_err("the callback removed the original frame");
            assert_eq!(
                error.downcast_ref::<RError>().unwrap().message,
                "original capture frame is no longer available"
            );
            let failed =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| capture.finish()));
            assert!(
                failed.is_err(),
                "the stale guard must not consume its replacement"
            );
            assert_eq!(crate::sexp::output::stop_capture().stdout, "replacement");
        } else {
            assert!(
                result.is_ok(),
                "text publication must permit balanced nested output"
            );
            assert!(
                capture.finish().stdout.is_empty(),
                "the native destination consumes the outer text"
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
        assert_eq!(value.len(), 1);
        assert!(
            value
                .try_string_elt(0)
                .unwrap()
                .try_char_eq(b"outer")
                .unwrap()
        );
        assert!(connection_table()[index as usize].as_ref().unwrap().isopen);
    });
}
#[test]
fn text_publication_callback_balanced_capture_can_reenter_during_real_allocation() {
    text_publication_callback(1);
}
#[test]
fn text_publication_callback_allocation_control_keeps_original_connection() {
    text_publication_callback(0);
}

#[test]
fn text_publication_callback_cannot_close_or_retire_the_active_capture_destination() {
    text_publication_callback(2);
}

#[test]
fn text_publication_callback_replacement_cannot_receive_or_finish_the_original_write() {
    text_publication_callback(3);
}
