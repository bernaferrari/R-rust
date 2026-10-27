use super::*;
use crate::sexp::{owner::StoredOwner, session::RSession};

#[test]
fn transient_scope_preserves_live_outer_buffers_through_nested_success_and_unwind() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let before = owner.with_arena(|arena| arena.workspace_bytes()).unwrap();
        let outer = TransientScope::enter(StoredOwner::from_token(owner)).unwrap();
        let data = R_alloc(1, 16).cast::<u8>();
        *data = 73;
        {
            let _inner = TransientScope::enter(StoredOwner::from_token(owner)).unwrap();
            R_alloc(1, 32);
            assert_eq!(vmax_len_in(owner.as_ptr()), 2);
        }
        assert_eq!(vmax_len_in(owner.as_ptr()), 1);
        assert_eq!(*data, 73);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            before + 16
        );
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _inner = TransientScope::enter(StoredOwner::from_token(owner)).unwrap();
            R_alloc(1, 48);
            std::panic::panic_any(713_u32);
        }));
        assert_eq!(failure.unwrap_err().downcast_ref::<u32>(), Some(&713));
        assert_eq!(vmax_len_in(owner.as_ptr()), 1);
        assert_eq!(*data, 73);
        drop(outer);
        assert_eq!(vmax_len_in(owner.as_ptr()), 0);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            before
        );
    });
}

#[test]
fn transient_scope_cleans_original_revoked_owner_without_touching_replacement() {
    let original = RSession::new_for_gc_tests();
    let weak = original.owner_token().unwrap().weak_owner().unwrap();
    let physical = weak.pin().unwrap();
    let guard = original.with_active(|| unsafe {
        let owner = original.owner_token().unwrap();
        R_alloc(1, 16);
        let guard = TransientScope::enter(StoredOwner::Managed(weak.clone())).unwrap();
        R_alloc(1, 32);
        assert_eq!(vmax_len_in(owner.as_ptr()), 2);
        guard
    });
    drop(original);
    assert!(!weak.is_live());
    let replacement = RSession::new_for_gc_tests();
    replacement.with_active(|| unsafe {
        let replacement_owner = replacement.owner_token().unwrap();
        let live = R_alloc(1, 24).cast::<u8>();
        *live = 29;
        let replacement_bytes = replacement_owner
            .with_arena(|arena| arena.workspace_bytes())
            .unwrap();
        drop(guard);
        // The independent physical pin permits observing cleanup of the
        // revoked original. No liveness capability is recreated from its address.
        assert_eq!(vmax_len_in(physical.as_ptr()), 1);
        assert_eq!((*physical.as_ptr()).arena.workspace_bytes(), 16);
        assert_eq!(vmax_len_in(replacement_owner.as_ptr()), 1);
        assert_eq!(*live, 29);
        assert_eq!(
            replacement_owner
                .with_arena(|arena| arena.workspace_bytes())
                .unwrap(),
            replacement_bytes
        );
        vmaxset_in(physical.as_ptr(), ptr::null_mut());
        vmaxset_in(replacement_owner.as_ptr(), ptr::null_mut());
    });
    drop(physical);
    assert!(weak.pin().is_err());
    assert_eq!(weak.allocation_strong_count(), 0);
}
