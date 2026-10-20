//! Physical bookkeeping is funded before allocation, including collection.
use super::*;

fn bookkeeping_storage_bytes(arena: &RArena) -> usize {
    // Payload ownership and reclamation now share the actual admitted records.
    arena
        .backing
        .node_pages
        .borrow()
        .iter()
        .map(|page| page.payloads.borrow().allocation_size())
        .sum()
}
fn other_page_storage_bytes(arena: &RArena) -> usize {
    arena
        .backing
        .node_pages
        .borrow()
        .iter()
        .map(|page| {
            page.meta.slots() * NODE_BYTES
                + PageMetadata::requested_array_bytes(page.meta.slots()).unwrap()
        })
        .sum()
}
#[test]
fn physical_arena_bookkeeping_first_live_node_funds_membership_storage() {
    let mut arena = RArena::new();
    assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
    let requested = other_page_storage_bytes(&arena) + bookkeeping_storage_bytes(&arena);
    assert!(
        arena.total_bytes_allocated() >= requested,
        "{} charged bytes below {requested} retained page and bookkeeping bytes",
        arena.total_bytes_allocated()
    );
}
#[test]
fn physical_arena_bookkeeping_collection_at_limit_never_grows_worklist_storage() {
    let mut arena = RArena::new();
    let pointer = arena.alloc_node(SEXPTYPE::INTSXP);
    let charged = arena.total_bytes_allocated();
    let before = bookkeeping_storage_bytes(&arena);
    arena.set_budget(ArenaBudget::new(charged, 0));
    // SAFETY: this exact original live node is owned by the fixture, without
    // reachable edges, payload loans or root leases; retire it only once.
    unsafe {
        arena.free_node(pointer);
    }
    let after = bookkeeping_storage_bytes(&arena);
    assert!(
        after <= before,
        "reclamation grew native bookkeeping from {before} to {after} bytes at the byte limit"
    );
    assert_eq!(arena.node_count(), 0);
    assert_eq!(arena.free_count(), 1);
    assert_eq!(arena.total_bytes_allocated(), charged);
}
#[test]
fn physical_arena_bookkeeping_reclaimed_nodes_keep_actual_buffers_funded() {
    let mut arena = RArena::new();
    let pointer = arena.alloc_node(SEXPTYPE::INTSXP);
    // SAFETY: the fixture owns this exact live node and no root or payload loan.
    unsafe {
        arena.free_node(pointer);
    }
    let requested = other_page_storage_bytes(&arena) + bookkeeping_storage_bytes(&arena);
    assert!(
        arena.total_bytes_allocated() >= requested,
        "retained reclamation buffers escaped their original ledger"
    );
}
#[test]
fn physical_arena_bookkeeping_membership_authenticates_original_store_and_generation() {
    let mut original = crate::sexp::instance::RInstance::new_for_gc_tests();
    let permanent = original
        .persistent_nodes
        .allocate_chars(b"same heap, other store")
        .unwrap();
    assert!(!original.arena.contains(permanent));
    assert!(original.arena.node_token(permanent).is_none());
    let mut other = RArena::new();
    let foreign = other.alloc_node(SEXPTYPE::INTSXP);
    assert!(!original.arena.contains(foreign));
    assert!(original.arena.node_token(foreign).is_none());
    let pointer = original.arena.alloc_node(SEXPTYPE::INTSXP);
    assert!(!original.arena.contains(permanent));
    assert!(original.arena.node_token(permanent).is_none());
    let old = original.arena.node_token(pointer).unwrap();
    // SAFETY: this fixture owns the exact live node with no root or data loan.
    unsafe {
        original.arena.free_node(pointer);
    }
    assert!(!original.arena.contains(pointer));
    assert!(original.arena.node_token(pointer).is_none());
    let reused = original.arena.alloc_node(SEXPTYPE::REALSXP);
    assert_eq!(reused, pointer);
    assert!(!old.is_live());
    assert!(original.arena.contains(reused));
    assert_ne!(original.arena.node_token(reused).unwrap().id(), old.id());
}
#[test]
fn physical_arena_bookkeeping_normalization_excludes_fresh_unused_slots() {
    let mut arena = RArena::new();
    let pointer = arena.alloc_node(SEXPTYPE::INTSXP);
    let unused = arena.backing.node_pages.borrow()[0]
        .storage
        .raw_slot(1)
        .unwrap();
    assert!(!arena.contains(unused));
    assert!(arena.node_token(unused).is_none());
    arena.normalize_free_list();
    assert_eq!(arena.node_count(), 1);
    assert_eq!(arena.free_count(), 0);
    assert!(arena.contains(pointer));
    assert!(!arena.contains(unused));
}

#[test]
fn physical_arena_bookkeeping_variable_pages_reuse_lifo_and_preserve_generations() {
    let mut arena = RArena::with_budget(ArenaBudget::new(RArena::minimum_page_bytes(), 0));
    let first = arena.alloc_node(SEXPTYPE::INTSXP);
    assert!(!first.is_null());
    assert_eq!(arena.backing.node_pages.borrow()[0].meta.slots(), 1);
    arena.set_budget(ArenaBudget::new(
        arena.total_bytes_allocated() + 4 * RArena::minimum_page_bytes(),
        0,
    ));
    let second = arena.alloc_node(SEXPTYPE::INTSXP);
    let third = arena.alloc_node(SEXPTYPE::INTSXP);
    assert!(!second.is_null());
    assert!(!third.is_null());
    assert_eq!(arena.backing.node_pages.borrow().len(), 2);
    assert_ne!(arena.backing.node_pages.borrow()[1].meta.slots(), 1);
    let old = [first, second, third].map(|p| arena.node_token(p).unwrap());
    // SAFETY: all three exact fixture-owned nodes have no root or data loan.
    unsafe {
        arena.free_node(first);
        arena.free_node(third);
        arena.free_node(second);
    }
    assert_eq!(arena.node_count(), 0);
    assert_eq!(arena.free_count(), 3);
    let before = arena.total_bytes_allocated();
    for expected in [second, third, first] {
        assert_eq!(arena.alloc_node(SEXPTYPE::REALSXP), expected);
    }
    assert_eq!(arena.node_count(), 3);
    assert_eq!(arena.free_count(), 0);
    assert_eq!(arena.total_bytes_allocated(), before);
    for (pointer, previous) in [first, second, third].into_iter().zip(old) {
        assert!(!previous.is_live());
        assert_ne!(arena.node_token(pointer).unwrap().id(), previous.id());
    }
}
