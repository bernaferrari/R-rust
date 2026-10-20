//! Page descriptor data and replacement peaks belong to the original heap.
use super::*;

fn bare_page() -> usize {
    NODE_BYTES
        + PageMetadata::requested_array_bytes(1).unwrap()
        + std::mem::size_of::<slab_slots::SlotRecord>()
}
fn descriptor_storage(arena: &RArena) -> usize {
    arena.backing.node_pages.borrow().allocation_size()
}
fn non_descriptor_page_storage(arena: &RArena) -> usize {
    arena
        .backing
        .node_pages
        .borrow()
        .iter()
        .map(|p| {
            p.meta.slots() * NODE_BYTES
                + PageMetadata::requested_array_bytes(p.meta.slots()).unwrap()
                + p.payloads.borrow().allocation_size()
        })
        .sum()
}
#[test]
fn physical_page_descriptors_first_node_funds_actual_vector_and_cookie_storage() {
    let mut arena = RArena::new();
    assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
    let descriptors = descriptor_storage(&arena);
    assert!(descriptors > 0);
    let requested = non_descriptor_page_storage(&arena) + descriptors;
    assert!(
        arena.total_bytes_allocated() >= requested,
        "{} charged bytes below {requested} retained bytes including {descriptors} descriptor/cookie bytes",
        arena.total_bytes_allocated()
    );
}
#[test]
fn physical_page_descriptors_original_one_slot_limit_denies_unfunded_descriptors() {
    let mut arena = RArena::with_budget(ArenaBudget::new(bare_page(), 1));
    assert!(
        arena.alloc_node(SEXPTYPE::INTSXP).is_null(),
        "unfunded descriptor storage escaped the original byte limit"
    );
    assert_eq!(arena.node_count(), 0);
    assert_eq!(arena.total_bytes_allocated(), 0);
    arena.set_budget(ArenaBudget::new(RArena::minimum_page_bytes(), 1));
    assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
    assert_eq!(arena.total_bytes_allocated(), RArena::minimum_page_bytes());
}
#[test]
fn physical_page_descriptors_growth_admits_old_and_new_buffer_peak_atomically() {
    let mut arena = RArena::with_budget(ArenaBudget::new(RArena::minimum_page_bytes(), 0));
    let mut originals = Vec::new();
    for i in 0..4 {
        if i != 0 {
            arena.set_budget(ArenaBudget::new(
                arena.total_bytes_allocated() + bare_page(),
                0,
            ));
        }
        let pointer = arena.alloc_node(SEXPTYPE::INTSXP);
        assert!(!pointer.is_null());
        originals.push((pointer, arena.node_token(pointer).unwrap()));
    }
    assert_eq!(arena.backing.node_pages.borrow().len(), 4);
    assert_eq!(arena.backing.node_pages.borrow().capacity(), 4);
    let descriptor = std::mem::size_of::<SlabPage>();
    let before = arena.total_bytes_allocated();
    let new_buffer = 8 * descriptor;
    arena.set_budget(ArenaBudget::new(before + bare_page() + new_buffer - 1, 0));
    assert!(
        arena.alloc_node(SEXPTYPE::INTSXP).is_null(),
        "descriptor replacement escaped its old-plus-new buffer peak"
    );
    assert_eq!(arena.total_bytes_allocated(), before);
    assert_eq!(arena.node_count(), 4);
    assert_eq!(arena.backing.node_pages.borrow().len(), 4);
    assert_eq!(arena.backing.node_pages.borrow().capacity(), 4);
    for (pointer, node) in &originals {
        assert!(node.is_live());
        assert_eq!(arena.node_token(*pointer).unwrap().id(), node.id());
    }
    arena.set_budget(ArenaBudget::new(before + bare_page() + new_buffer, 0));
    assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
    assert_eq!(arena.backing.node_pages.borrow().capacity(), 8);
    assert_eq!(
        arena.total_bytes_allocated(),
        before + bare_page() + new_buffer - 4 * descriptor
    );
}
#[test]
fn physical_page_descriptors_charge_follows_detached_final_backing_owner() {
    let mut arena = RArena::new();
    let pointer = arena.alloc_node(SEXPTYPE::INTSXP);
    let node = arena.node_token(pointer).unwrap();
    let root = node.root_lease().unwrap();
    let counter = arena.allocated_bytes.clone();
    let requested = non_descriptor_page_storage(&arena) + descriptor_storage(&arena);
    let arrays =
        PageMetadata::requested_array_bytes(arena.backing.node_pages.borrow()[0].meta.slots())
            .unwrap();
    drop(arena);
    assert!(node.is_live());
    assert!(
        counter.get() >= requested,
        "detached descriptor buffer lost its original physical-storage charge"
    );
    drop(root);
    assert!(!node.is_live());
    assert_eq!(counter.get(), arrays);
    drop(node);
    assert_eq!(counter.get(), 0);
}
#[test]
fn physical_page_descriptors_cookie_gaps_preserve_original_store_and_generation() {
    let mut owner = crate::sexp::instance::RInstance::new_for_gc_tests();
    owner.arena.set_budget(ArenaBudget::new(
        owner.arena.total_bytes_allocated() + RArena::minimum_page_bytes(),
        0,
    ));
    let mut originals = Vec::new();
    for _ in 0..3 {
        let pointer = owner.arena.alloc_node(SEXPTYPE::INTSXP);
        assert!(!pointer.is_null());
        originals.push((pointer, owner.arena.node_token(pointer).unwrap()));
        owner.arena.set_budget(ArenaBudget::unlimited());
        let permanent = owner
            .persistent_nodes
            .allocate_header(SexprecCore::new(SEXPTYPE::NILSXP))
            .unwrap();
        let token = owner.persistent_nodes.token(permanent).unwrap();
        assert!(
            owner
                .arena
                .backing
                .resolve_link(token.link().unwrap())
                .is_none()
        );
        owner.arena.set_budget(ArenaBudget::new(
            owner.arena.total_bytes_allocated() + bare_page(),
            0,
        ));
    }
    let cookies: Vec<_> = originals
        .iter()
        .map(|(_, node)| node.id().page_cookie())
        .collect();
    assert!(cookies.windows(2).all(|w| w[1] > w[0] + 1));
    for (pointer, node) in &originals {
        assert_eq!(
            owner
                .arena
                .backing
                .resolve_link(node.link().unwrap())
                .unwrap()
                .0,
            *pointer
        );
    }
    let mut foreign = RArena::new();
    let pointer = foreign.alloc_node(SEXPTYPE::INTSXP);
    assert!(
        owner
            .arena
            .backing
            .resolve_link(foreign.node_token(pointer).unwrap().link().unwrap())
            .is_none()
    );
    let (pointer, old) = &originals[0];
    // SAFETY: this fixture owns the exact live header with no root/data loan.
    unsafe {
        owner.arena.free_node(*pointer);
    }
    assert!(
        owner
            .arena
            .backing
            .resolve_link(old.link().unwrap_or_else(|| old.id().link()))
            .is_none()
    );
    let reused = owner.arena.alloc_node(SEXPTYPE::REALSXP);
    assert_eq!(reused, *pointer);
    assert_ne!(owner.arena.node_token(reused).unwrap().id(), old.id());
}
