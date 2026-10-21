use super::*;

const ENTRY_BYTES: usize = std::mem::size_of::<(SEXP, CheckedNode)>();

fn rooted_arena<T>(action: impl FnOnce(&mut RArena, &HeapIdentity) -> T) -> T {
    let mut arena = RArena::new();
    let identity = arena.heap_identity();
    let pointer = arena.alloc_node(SEXPTYPE::LISTSXP);
    let token = arena.node_token(pointer).unwrap();
    let _root = token.root_lease().unwrap();
    action(&mut arena, &identity)
}

#[test]
fn automatic_root_snapshot_retains_its_original_storage_admission() {
    rooted_arena(|_, identity| {
        let ledger = identity.byte_ledger();
        let before = ledger.workspace_bytes_for_test();
        let roots = automatic_roots(identity);
        assert_eq!(roots.len(), 1);
        assert!(ledger.workspace_bytes_for_test() >= before + ENTRY_BYTES);
        drop(roots);
        assert_eq!(ledger.workspace_bytes_for_test(), before);
    });
}

#[test]
fn automatic_root_snapshot_denies_unfunded_native_storage() {
    rooted_arena(|arena, identity| {
        let before = arena.total_bytes_allocated();
        arena.set_budget(ArenaBudget::new(before, 0));
        let denied =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| automatic_roots(identity)));
        assert!(
            denied
                .expect_err("automatic root snapshot exceeded original headroom")
                .downcast_ref::<crate::sexp::context::RError>()
                .is_some()
        );
        assert_eq!(arena.workspace_bytes(), 0);
        assert_eq!(arena.budget().max_bytes, before);
    });
}

#[test]
fn automatic_root_iterator_keeps_its_ticket_after_exhaustion_and_heap_teardown() {
    let (mut roots, ledger) = rooted_arena(|_, identity| {
        (
            automatic_roots(identity).into_iter(),
            identity.byte_ledger(),
        )
    });
    let other = crate::sexp::RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap();
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        assert!(ledger.workspace_bytes_for_test() >= ENTRY_BYTES);
        assert!(roots.next().is_some());
        assert!(roots.next().is_none());
        assert!(ledger.workspace_bytes_for_test() >= ENTRY_BYTES);
        drop(roots);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
    });
}

#[test]
fn fresh_root_snapshot_retains_admission_inside_a_registered_lend() {
    let session = crate::sexp::RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        owner
            .with_arena(|arena| {
                let identity = arena.heap_identity();
                arena.alloc_node(SEXPTYPE::LISTSXP);
                let before = arena.workspace_bytes();
                let roots = fresh_allocation_roots(&identity);
                assert_eq!(roots.len(), 1);
                assert!(arena.workspace_bytes() >= before + ENTRY_BYTES);
                drop(roots);
                assert_eq!(arena.workspace_bytes(), before);
            })
            .unwrap();
    });
}

#[test]
fn fresh_root_snapshot_denies_then_recovers_at_the_same_current_lend_limit() {
    let session = crate::sexp::RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        owner
            .with_arena(|arena| {
                let identity = arena.heap_identity();
                arena.alloc_node(SEXPTYPE::LISTSXP);
                let stale = arena.alloc_node(SEXPTYPE::LISTSXP);
                let limit = arena.total_bytes_allocated() + ENTRY_BYTES;
                arena.set_budget(ArenaBudget::new(limit, 0));
                let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    fresh_allocation_roots(&identity)
                }));
                assert!(
                    denied
                        .expect_err(
                            "fresh root snapshot exceeded the current registered lend limit"
                        )
                        .downcast_ref::<crate::sexp::context::RError>()
                        .is_some()
                );
                assert_eq!(arena.workspace_bytes(), 0);
                unsafe { arena.free_node(stale) };
                let small = fresh_allocation_roots(&identity);
                assert_eq!(small.len(), 1);
                assert!(arena.workspace_bytes() >= ENTRY_BYTES);
                drop(small);
                assert_eq!(arena.workspace_bytes(), 0);
                assert_eq!(arena.budget().max_bytes, limit);
            })
            .unwrap();
    });
}

#[test]
fn empty_automatic_root_snapshot_needs_no_headroom_control() {
    let mut arena = RArena::new();
    arena.alloc_node(SEXPTYPE::LISTSXP);
    let identity = arena.heap_identity();
    let resident = arena.total_bytes_allocated();
    assert!(resident > 1);
    for limit in [resident, 1] {
        arena.set_budget(ArenaBudget::new(limit, 0));
        assert!(automatic_roots(&identity).is_empty());
        assert_eq!(arena.workspace_bytes(), 0);
        assert_eq!(arena.budget().max_bytes, limit);
    }
}

#[test]
fn automatic_marking_stream_walks_root_bitmap_words_without_buffers_or_directory_loans() {
    let mut arena = RArena::new();
    let identity = arena.heap_identity();
    let mut leases = Vec::new();
    let mut expected = Vec::new();
    for index in 0..=128 {
        let pointer = arena.alloc_node(SEXPTYPE::LISTSXP);
        if [1, 63, 64, 66, 127, 128].contains(&index) {
            let token = arena.node_token(pointer).unwrap();
            leases.push(token.root_lease().unwrap());
            expected.push((pointer, token.id().clone()));
        }
    }
    let before = arena.total_bytes_allocated();
    arena.set_budget(ArenaBudget::new(before, 0));
    let mut roots = automatic_roots_for_marking(&identity);
    for (expected_pointer, expected_id) in expected {
        let (pointer, token) = roots.next().unwrap();
        assert_eq!(pointer, expected_pointer);
        assert_eq!(token.id(), &expected_id);
        super::slab::SLAB_META.with(|directory| {
            let directory = directory.borrow_mut();
            assert!(
                !directory.is_empty(),
                "the iterator must end its directory loan before yielding"
            );
        });
        assert_eq!(arena.workspace_bytes(), 0);
        assert_eq!(arena.total_bytes_allocated(), before);
    }
    assert!(roots.next().is_none());
    drop(leases);
}

#[test]
fn automatic_and_fresh_roots_survive_actual_collection_at_one_byte_then_release() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let session = crate::sexp::RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let automatic = owner.node_factory().strings(&["automatic root"]).unwrap();
        let automatic_token = automatic.allocation().unwrap().clone();
        let garbage = owner
            .with_arena(|arena| {
                let node = arena.alloc_node(SEXPTYPE::LISTSXP);
                arena.node_token(node).unwrap()
            })
            .unwrap();
        unsafe {
            (*owner.as_ptr()).memory_state.gc_force_gap = 1;
            (*owner.as_ptr()).memory_state.gc_force_wait = 1;
        }
        let notifications = Arc::new(AtomicUsize::new(0));
        let calls = notifications.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                crate::sexp::gengc::full_gc();
            }
        }));
        let fresh = owner
            .with_arena(|arena| {
                let nodes = [
                    arena.alloc_node(SEXPTYPE::LISTSXP),
                    arena.alloc_node(SEXPTYPE::LISTSXP),
                    arena.alloc_node(SEXPTYPE::LISTSXP),
                    arena.alloc_node(SEXPTYPE::LISTSXP),
                ];
                let token = arena.node_token(nodes[0]).unwrap();
                arena.set_budget(ArenaBudget::new(1, 0));
                token
            })
            .unwrap();
        assert!(notifications.load(Ordering::SeqCst) > 0);
        assert!(
            !garbage.is_live(),
            "actual deferred collection must reclaim the unrooted neighbor"
        );
        assert!(
            fresh.is_live(),
            "fresh result must survive lend cleanup collection"
        );
        assert!(automatic_token.is_live());
        crate::sexp::gengc::full_gc();
        assert!(!fresh.is_live(), "fresh scope must end after lend cleanup");
        assert!(automatic_token.is_live());
        drop(automatic);
        crate::sexp::gengc::full_gc();
        assert!(!automatic_token.is_live());
        owner
            .with_arena(|arena| {
                assert_eq!(arena.budget().max_bytes, 1);
                assert_eq!(arena.workspace_bytes(), 0);
            })
            .unwrap();
    });
}
