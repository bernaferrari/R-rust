use super::{catch_eval_result, print_panic_r_error};
use crate::sexp::{
    context::{RError, RSignal, handle_closure_signal, handle_loop_signal},
    object::Sexp,
};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn message() -> String {
    let mut value = String::with_capacity(128);
    value.push_str("error with UTF-8: á中😀");
    value
}

#[test]
fn session_error_handoff_preserves_the_original_string_allocation() {
    let message = message();
    let pointer = message.as_ptr();
    let capacity = message.capacity();
    let result = catch_eval_result(|| -> Result<Sexp<'static>, String> {
        std::panic::panic_any(RError { message });
    });
    let error = result.unwrap_err();
    assert_eq!(error.message, "error with UTF-8: á中😀");
    assert_eq!(error.message.as_ptr(), pointer);
    assert_eq!(error.message.capacity(), capacity);
}

#[test]
fn loop_and_closure_error_handoffs_preserve_the_original_string_allocation() {
    for closure in [false, true] {
        let message = message();
        let pointer = message.as_ptr();
        let capacity = message.capacity();
        let payload: Box<dyn std::any::Any + Send> = Box::new(RError { message });
        let panic = catch_unwind(AssertUnwindSafe(|| {
            if closure {
                handle_closure_signal(payload);
            } else {
                handle_loop_signal(payload);
            }
        }))
        .unwrap_err();
        let signal = panic.downcast::<RSignal>().unwrap();
        let RSignal::Error { message } = *signal else {
            panic!("expected an R error signal");
        };
        assert_eq!(message, "error with UTF-8: á中😀");
        assert_eq!(message.as_ptr(), pointer);
        assert_eq!(message.capacity(), capacity);
    }
}

#[test]
fn guard_handoff_preserves_error_storage_and_unrecognized_panics() {
    for signal in [false, true] {
        let message = message();
        let pointer = message.as_ptr();
        let capacity = message.capacity();
        let payload: Box<dyn std::any::Any + Send> = if signal {
            Box::new(RSignal::Error { message })
        } else {
            Box::new(RError { message })
        };
        let error = print_panic_r_error(payload).unwrap();
        assert_eq!(error.message, "error with UTF-8: á中😀");
        assert_eq!(error.message.as_ptr(), pointer);
        assert_eq!(error.message.capacity(), capacity);
    }
    let payload: Box<dyn std::any::Any + Send> = Box::new(73_u64);
    let pointer = payload.as_ref() as *const _ as *const ();
    let payload = print_panic_r_error(payload).unwrap_err();
    assert_eq!(payload.as_ref() as *const _ as *const (), pointer);
    assert_eq!(*payload.downcast::<u64>().unwrap(), 73);
    let payload: Box<dyn std::any::Any + Send> = Box::new(RSignal::Break);
    let pointer = payload.as_ref() as *const _ as *const ();
    let payload = print_panic_r_error(payload).unwrap_err();
    assert_eq!(payload.as_ref() as *const _ as *const (), pointer);
    assert!(matches!(
        *payload.downcast::<RSignal>().unwrap(),
        RSignal::Break
    ));
}
