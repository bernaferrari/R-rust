use super::*;
use crate::sexp::{context, session::RSession};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn owned_render_cache_actual_default_error_retains_original_capacity_charge_until_clear() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    R_SetShowErrorMessages(false);
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        render::verrorcall_dflt(globals::R_NilValue(), c"a\xffb".as_ptr(), ptr::null_mut());
    }));
    let error = result.unwrap_err().downcast::<context::RError>().unwrap();
    assert_eq!(error.message, "a\\xffb");
    assert!(state::error_was_last_rendered(&error.message));
    let capacity =
        state::with_error_state(|s| s.last_rendered_message.as_ref().unwrap().capacity());
    let workspace = owner.with_arena(|a| a.workspace_bytes()).unwrap();
    assert_eq!(
        workspace, capacity,
        "the exact retained String capacity requires original workspace admission"
    );
    state::clear_last_rendered_message();
    assert!(!state::error_was_last_rendered(&error.message));
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
}

fn render_literal(message: &CStr) -> context::RError {
    *catch_unwind(AssertUnwindSafe(|| unsafe {
        render::verrorcall_dflt(globals::R_NilValue(), message.as_ptr(), ptr::null_mut());
    }))
    .unwrap_err()
    .downcast::<context::RError>()
    .unwrap()
}

#[test]
fn owned_render_cache_replacement_releases_previous_charge_and_compares_exactly() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    R_SetShowErrorMessages(false);
    let first = render_literal(c"first\xffmessage");
    let second = render_literal(c"second longer message");
    assert!(!state::error_was_last_rendered(&first.message));
    assert!(state::error_was_last_rendered(&second.message));
    assert!(state::try_last_rendered_message(&first.message).is_none());
    assert_eq!(
        state::try_last_rendered_message(&second.message).unwrap(),
        "Error: second longer message\n"
    );
    let capacity =
        state::with_error_state(|s| s.last_rendered_message.as_ref().unwrap().capacity());
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), capacity);
    state::clear_last_rendered_message();
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
}

#[test]
fn owned_render_cache_denied_replacement_preserves_previous_buffer_charge_and_flags() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    R_SetShowErrorMessages(false);
    let first = render_literal(c"first message");
    let old_buffer = state::error_buffer_snapshot();
    let capacity =
        state::with_error_state(|s| s.last_rendered_message.as_ref().unwrap().capacity());
    let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    let expressions = R_Expressions();
    owner
        .with_arena(|a| a.set_budget(crate::sexp::memory::ArenaBudget::new(1, 0)))
        .unwrap();
    let error = render_literal(c"a replacement requiring a new admitted buffer");
    assert!(error.message.contains("rendered error message cache"));
    assert_eq!(R_GetInError(), 0);
    assert_eq!(R_Expressions(), expressions);
    assert_eq!(
        state::error_buffer_snapshot().as_bytes(),
        old_buffer.as_bytes()
    );
    assert!(state::error_was_last_rendered(&first.message));
    owner
        .with_arena(|a| {
            assert_eq!(a.budget().max_bytes, 1);
            assert_eq!(a.total_bytes_allocated(), before);
            assert_eq!(a.workspace_bytes(), capacity);
        })
        .unwrap();
    state::clear_last_rendered_message();
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
}

#[test]
fn owned_render_cache_copy_keeps_independent_text_and_releases_on_panic() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut source = String::from("source é \\xff");
        let text = RenderedMessage::copy(&weak, &source).unwrap();
        source.clear();
        assert_eq!(text.as_str(), "source é \\xff");
        assert_eq!(
            owner.with_arena(|a| a.workspace_bytes()).unwrap(),
            text.capacity()
        );
        std::panic::panic_any(991u32);
    }));
    assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 991);
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
}

#[test]
fn owned_render_cache_copy_obeys_lowered_original_cap_before_allocation() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    owner
        .with_arena(|a| a.set_budget(crate::sexp::memory::ArenaBudget::new(1, 0)))
        .unwrap();
    assert!(RenderedMessage::copy(&weak, "denied copy").is_err());
    owner
        .with_arena(|a| {
            assert_eq!(a.budget().max_bytes, 1);
            assert_eq!(a.total_bytes_allocated(), before);
            assert_eq!(a.workspace_bytes(), 0);
        })
        .unwrap();
}

#[test]
fn owned_render_cache_copy_rejects_inactive_owner_before_foreign_allocation() {
    let first = RSession::new_for_gc_tests();
    let original = first.owner_token().unwrap().weak_owner().unwrap();
    let second = RSession::new_for_gc_tests();
    second.with_active(|| {
        let owner = second.owner_token().unwrap();
        let before = owner
            .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes())
            .unwrap();
        assert!(RenderedMessage::copy(&original, "foreign copy").is_err());
        assert_eq!(
            owner
                .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes())
                .unwrap(),
            before
        );
    });
}

#[test]
fn owned_render_cache_physical_copy_outlives_revoked_owner_and_releases_original_ledger() {
    let first = RSession::new_for_gc_tests();
    let owner = first.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    let value = owner.node_factory().strings(&["ledger witness"]).unwrap();
    let ledger = value.allocation().unwrap().heap_identity().byte_ledger();
    let copy = RenderedMessage::copy(&weak, "retained copy").unwrap();
    let capacity = copy.capacity();
    drop(value);
    drop(first);
    assert!(!weak.is_live());
    assert_eq!(ledger.workspace_bytes_for_test(), capacity);
    let second = RSession::new_for_gc_tests();
    second.with_active(|| {
        let owner = second.owner_token().unwrap();
        let before = owner
            .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes())
            .unwrap();
        assert_eq!(copy.as_str(), "retained copy");
        assert!(RenderedMessage::copy(&weak, "revoked copy").is_err());
        drop(copy);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(
            owner
                .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes())
                .unwrap(),
            before
        );
    });
}
