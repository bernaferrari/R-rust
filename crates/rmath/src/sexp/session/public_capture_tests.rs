use super::RSession;
use crate::sexp::output::{self, OutputCaptureGuard};
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};

#[derive(Debug, PartialEq)]
struct CapturePanic(u64);

#[test]
fn public_capture_unwind_restores_the_parent_frame_and_panic_payload() {
    let session = RSession::new_for_gc_tests();
    let (_, outer) = session.with_output_capture(|| {
        output::capture_stdout("before  ");
        let panic = catch_unwind(AssertUnwindSafe(|| {
            session.with_output_capture(|| {
                output::capture_stdout("discarded inner output");
                output::capture_stderr("discarded inner error");
                panic_any(CapturePanic(73));
            });
        }))
        .expect_err("the user's panic must propagate");
        assert_eq!(*panic.downcast::<CapturePanic>().unwrap(), CapturePanic(73));
        output::capture_stderr("middle\n");
        output::capture_stdout("after\n\n");
    });
    assert_eq!(outer.stdout, "before  after\n\n");
    assert_eq!(outer.stderr, "middle\n");
    assert_eq!(outer.interleaved, "before  middle\nafter\n\n");
    assert!(!session.with_active(output::is_capturing));
    let (value, next) = session.with_output_capture(|| {
        output::capture_stdout("fresh");
        41
    });
    assert_eq!(value, 41);
    assert_eq!(next.stdout, "fresh");
    assert!(!session.with_active(output::is_capturing));
}

#[test]
fn public_capture_unwind_restores_an_idle_bank_and_detached_activation() {
    let ambient = RSession::new_for_gc_tests();
    let detached = RSession::construct_detached(RSession::new_for_gc_tests);
    let ambient_pointer = ambient.instance_ptr();
    let panic = catch_unwind(AssertUnwindSafe(|| {
        detached.with_output_capture(|| {
            output::capture_stdout("discarded");
            panic_any(CapturePanic(97));
        });
    }))
    .expect_err("the user's panic must propagate");
    assert_eq!(*panic.downcast::<CapturePanic>().unwrap(), CapturePanic(97));
    assert_eq!(
        crate::sexp::instance::current_instance_ptr(),
        Some(ambient_pointer)
    );
    assert!(!detached.with_active(output::is_capturing));
    assert!(!ambient.with_active(output::is_capturing));
    assert_eq!(
        crate::sexp::instance::current_instance_ptr(),
        Some(ambient_pointer)
    );
}

#[test]
fn public_capture_finishes_its_original_bank_after_revocation_and_reentry() {
    for unwind in [false, true] {
        let original = RSession::new_for_gc_tests();
        let original_pointer = original.instance_ptr();
        let mut replacement = None;
        let mut replacement_capture = None;
        let result = catch_unwind(AssertUnwindSafe(|| {
            original.with_output_capture(|| {
                output::capture_stdout("original  ");
                output::capture_stderr("original error\n");
                replacement = Some(RSession::new_for_gc_tests());
                replacement_capture = Some(OutputCaptureGuard::start());
                output::capture_stdout("replacement\n\n");
                // The facade still retains physical storage. Revocation is the
                // callback boundary being tested, not a stale-pointer access.
                unsafe { crate::sexp::instance::revoke_instance_availability(original_pointer) };
                if unwind {
                    panic_any(CapturePanic(101));
                }
                53
            })
        }));
        if unwind {
            let panic = result.expect_err("the callback panic must propagate");
            assert_eq!(
                *panic.downcast::<CapturePanic>().unwrap(),
                CapturePanic(101)
            );
        } else {
            let (value, captured) = result.unwrap();
            assert_eq!(value, 53);
            assert_eq!(captured.stdout, "original  ");
            assert_eq!(captured.stderr, "original error\n");
            assert_eq!(captured.interleaved, "original  original error\n");
        }
        assert!(!original.is_active());
        assert!(!original.inst().output_capture.borrow().is_capturing());
        let replacement = replacement.unwrap();
        assert!(replacement.with_active(output::is_capturing));
        let captured = replacement_capture.take().unwrap().finish();
        assert_eq!(captured.stdout, "replacement\n\n");
        assert!(!replacement.with_active(output::is_capturing));
    }
}
