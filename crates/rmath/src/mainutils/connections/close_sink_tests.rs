//! Preserve GNU active-sink close behavior while cleanup is isolated.
use super::*;
use crate::sexp::{
    RSession, Sexp,
    object::{PairlistBuilder, SessionNodeFactory},
};

unsafe fn text_connection<'s>(f: &SessionNodeFactory<'s>, name: &str) -> Sexp<'s> {
    let mut args = PairlistBuilder::from_factory(f.clone());
    for value in [
        f.strings(&[name]).unwrap(),
        f.strings(&[name]).unwrap(),
        f.strings(&["w"]).unwrap(),
        f.domain().logical(false),
    ] {
        args.push(value, None).unwrap();
    }
    let args = args.finish().unwrap();
    unsafe {
        f.wrap(do_textConnection(
            ptr::null_mut(),
            ptr::null_mut(),
            args.as_raw(),
            crate::sexp::globals::R_GlobalEnv(),
        ))
        .unwrap()
    }
}
unsafe fn set_sink(f: &SessionNodeFactory<'_>, connection: &Sexp<'_>, message: bool) {
    let mut args = PairlistBuilder::from_factory(f.clone());
    for value in [
        connection.clone(),
        f.domain().logical(false),
        f.domain().logical(message),
        f.domain().logical(false),
    ] {
        args.push(value, None).unwrap();
    }
    let args = args.finish().unwrap();
    unsafe {
        do_sink(
            ptr::null_mut(),
            ptr::null_mut(),
            args.as_raw(),
            ptr::null_mut(),
        );
    }
}
fn sink_close_case(nested: bool, lower: bool, popped: bool, message: bool) {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let f = session.owner_token().unwrap().node_factory();
        let first = text_connection(&f, "sink_first");
        set_sink(&f, &first, message);
        let second = nested.then(|| text_connection(&f, "sink_second"));
        if let Some(second) = &second {
            set_sink(&f, second, false);
        }
        if popped {
            let minus_one = f.wrap(Rf_ScalarInteger(-1)).unwrap();
            set_sink(&f, &minus_one, false);
        }
        let target = match &second {
            Some(second) if !lower => second,
            _ => &first,
        };
        let index = target.try_integer_elt(0).unwrap();
        let original = connection_token(index).unwrap();
        let args = f.pairlist_cell(target, &f.nil(), &f.nil()).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            do_close(
                ptr::null_mut(),
                ptr::null_mut(),
                args.as_raw(),
                ptr::null_mut(),
            );
        }));
        if popped || (!message && !lower) {
            assert!(
                result.is_ok(),
                "GNU permits the top sink and a popped destination to close"
            );
            assert!(connection_table()[index as usize].is_none());
        } else {
            let error = result.expect_err("every active sink destination must reject close");
            assert_eq!(
                error.downcast_ref::<RError>().unwrap().message,
                if message {
                    "cannot close 'message' sink connection"
                } else {
                    "cannot close 'output' sink connection"
                }
            );
            assert_eq!(original.require_current().unwrap(), index);
            assert!(connection_table()[index as usize].as_ref().unwrap().isopen);
        }
    });
}
#[test]
fn sink_close_single_top_destination_preserves_gnu_quirk() {
    sink_close_case(false, false, false, false);
}
#[test]
fn sink_close_nested_top_destination_preserves_gnu_quirk() {
    sink_close_case(true, false, false, false);
}
#[test]
fn sink_close_lower_active_destination_control() {
    sink_close_case(true, true, false, false);
}
#[test]
fn sink_close_popped_destination_control_ignores_stale_entries() {
    sink_close_case(true, false, true, false);
}
#[test]
fn sink_close_message_destination_control() {
    sink_close_case(false, false, false, true);
}
