//! Exercise genuine coercion attribution, roots, nesting and original owners.
use super::{CoercionWarning, WARN_IMAG, WARN_NA};
use crate::sexp::{
    RSession,
    ffi::SEXP,
    instance::RuntimeValue,
    object::Sexp,
    protect::{R_ProtectCount, protect_raw_pointer, unprotect_count},
};

fn coercion_scope(call: SEXP) -> crate::mainutils::errors::WarningCallGuard {
    crate::mainutils::errors::warning_call_guard(call)
}

fn call(session: &mut RSession, name: &str) -> Sexp<'static> {
    session
        .eval_code_with_output_capture(&format!("quote({name}())"))
        .0
        .unwrap()
        .into_owned()
        .unwrap()
}

fn last_warning_call(session: &RSession) -> Sexp<'static> {
    session.with_active_in(|instance| unsafe {
        let state = &(*instance).error_state;
        state
            .warnings
            .owned()
            .unwrap()
            .try_vector_elt(i64::from(state.collect_warnings - 1))
            .unwrap()
            .into_owned()
            .unwrap()
    })
}

#[test]
fn coercion_warning_scope_preserves_existing_legacy_root_stack() {
    let mut session = RSession::new_for_gc_tests();
    let attributed = call(&mut session, "attributed");
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let sentinel = owner.node_factory().character("legacy sentinel").unwrap();
        let before = R_ProtectCount();
        protect_raw_pointer(sentinel.as_raw());
        let scope = coercion_scope(attributed.as_raw());
        assert_eq!(R_ProtectCount(), before + 1);
        drop(scope);
        assert_eq!(
            R_ProtectCount(),
            before + 1,
            "coercion cleanup cannot release another legacy root"
        );
        owner.full_gc().unwrap();
        assert!(sentinel.try_char_eq(b"legacy sentinel").unwrap());
        unprotect_count(1);
    });
}

#[test]
fn coercion_warning_scope_restores_nested_attribution_across_full_gc() {
    let mut session = RSession::new_for_gc_tests();
    let outer = call(&mut session, "outer");
    let inner = call(&mut session, "inner");
    session.with_active(|| {
        let outer_scope = coercion_scope(outer.as_raw());
        let inner_scope = coercion_scope(inner.as_raw());
        unsafe { CoercionWarning(WARN_NA) };
        assert_eq!(last_warning_call(&session).as_raw(), inner.as_raw());
        drop(inner_scope);
        session.owner_token().unwrap().full_gc().unwrap();
        unsafe { CoercionWarning(WARN_IMAG) };
        assert_eq!(
            last_warning_call(&session).as_raw(),
            outer.as_raw(),
            "ending an inner scope must restore its original outer call"
        );
        drop(outer_scope);
    });
}

#[test]
fn coercion_warning_scope_keeps_same_thread_sessions_independent() {
    let mut left = RSession::new_for_gc_tests();
    let left_call = call(&mut left, "left_call");
    let mut right = RSession::new_for_gc_tests();
    let right_call = call(&mut right, "right_call");
    left.with_active(|| {
        let left_scope = coercion_scope(left_call.as_raw());
        right.with_active(|| {
            let right_scope = coercion_scope(right_call.as_raw());
            unsafe { CoercionWarning(WARN_NA) };
            assert_eq!(last_warning_call(&right).as_raw(), right_call.as_raw());
            drop(right_scope);
        });
        unsafe { CoercionWarning(WARN_IMAG) };
        assert_eq!(last_warning_call(&left).as_raw(), left_call.as_raw());
        drop(left_scope);
    });
}

#[test]
fn coercion_warning_scope_unwind_restores_original_revoked_owner_only() {
    let mut original = RSession::new_for_gc_tests();
    let previous = call(&mut original, "previous");
    let attributed = call(&mut original, "attributed");
    let owner = original.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    let pin = weak.pin().unwrap();
    unsafe {
        (*pin.as_ptr()).error_state.warning_call = RuntimeValue::from_owned(previous.clone());
    }
    let scope = original.with_active(|| coercion_scope(attributed.as_raw()));
    original.close();
    drop(original);
    let other = RSession::new_for_gc_tests();
    other.with_active(|| unsafe {
        let other_owner = other.owner_token().unwrap();
        let sentinel = other_owner.node_factory().character("other root").unwrap();
        let before = R_ProtectCount();
        protect_raw_pointer(sentinel.as_raw());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _scope = scope;
            panic!("unwind coercion attribution");
        }));
        assert!(result.is_err());
        assert_eq!(
            R_ProtectCount(),
            before + 1,
            "other session's root survives"
        );
        assert!(crate::mainutils::errors::warning_call_override().is_null());
        let restored = (*pin.as_ptr()).error_state.warning_call.owned().unwrap();
        assert_eq!(restored.as_raw(), previous.as_raw());
        other_owner.full_gc().unwrap();
        assert!(sentinel.try_char_eq(b"other root").unwrap());
        unprotect_count(1);
    });
    drop(pin);
    assert_eq!(weak.allocation_strong_count(), 0);
}
