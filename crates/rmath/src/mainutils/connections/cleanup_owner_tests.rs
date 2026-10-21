use super::cleanup::ConnectionCleanup;
use super::connection_table;
use crate::sexp::object::SexpError;
use crate::sexp::{RSession, object::PairlistBuilder};
fn owner_exit(close: bool) {
    let mut original = Some(RSession::new_for_gc_tests());
    original.as_mut().unwrap().enable_browser_files();
    original
        .as_ref()
        .unwrap()
        .with_active_in(|instance| unsafe {
            (*instance).browser_files.set_test_clock(0.0);
        });
    let weak = original
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let mut guard = original.as_ref().unwrap().with_active(|| unsafe {
        let owner = crate::sexp::owner::OwnerToken::current().unwrap();
        let factory = owner.node_factory();
        let mut arguments = PairlistBuilder::from_factory(factory.clone());
        for value in [
            factory.strings(&["cleanup-owner.txt"]).unwrap(),
            factory.strings(&["w"]).unwrap(),
        ] {
            arguments.push(value, None).unwrap();
        }
        let arguments = arguments.finish().unwrap();
        let connection = factory
            .wrap(crate::mainutils::connections::do_file(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                arguments.as_raw(),
                std::ptr::null_mut(),
            ))
            .unwrap();
        let mut guard = ConnectionCleanup::new(owner, true);
        guard.attach(connection.try_integer_elt(0).unwrap());
        guard
    });
    assert_eq!(weak.allocation_strong_count(), 2);
    if close {
        original.as_mut().unwrap().close();
    } else {
        drop(original.take());
    }
    let replacement = RSession::new_for_gc_tests();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| guard.close()));
    let error = result.expect_err("revoked original cleanup must reject work");
    assert_eq!(
        error
            .downcast_ref::<crate::sexp::context::RError>()
            .unwrap()
            .message,
        SexpError::RootUnavailable.to_string()
    );
    drop(original.take());
    assert_eq!(
        weak.allocation_strong_count(),
        1,
        "cleanup must retain physical storage until its own drop"
    );
    drop(guard);
    assert_eq!(
        weak.allocation_strong_count(),
        0,
        "cleanup must not retain the original runtime after drop"
    );
    replacement.with_active(|| {
        assert_eq!(
            connection_table()
                .iter()
                .filter(|entry| entry.is_some())
                .count(),
            3
        )
    });
}
#[test]
fn cleanup_owner_close_never_redirects_to_replacement_runtime() {
    owner_exit(true);
}
#[test]
fn cleanup_owner_drop_retains_physical_storage_only_until_guard_drop() {
    owner_exit(false);
}
