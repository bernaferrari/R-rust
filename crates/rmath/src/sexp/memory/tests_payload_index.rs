//! The payload ownership index is physical storage, even for reclaimed slots.
use super::*;

fn page_storage_without_index(arena: &RArena) -> usize {
    arena
        .backing
        .node_pages
        .borrow()
        .iter()
        .map(|p| {
            p.meta.slots() * NODE_BYTES
                + PageMetadata::requested_array_bytes(p.meta.slots()).unwrap()
        })
        .sum()
}
fn index_storage(arena: &RArena) -> usize {
    arena
        .backing
        .node_pages
        .borrow()
        .iter()
        .map(|p| p.payloads.borrow().allocation_size())
        .sum()
}
#[test]
fn physical_payload_index_first_payload_accounts_for_actual_index_buffer() {
    let mut arena = RArena::new();
    let pointer = arena.alloc_vector(SEXPTYPE::RAWSXP, 1);
    let node = arena.node_token(pointer).unwrap();
    let payload = arena.heap_identity().payload_lease(&node).unwrap();
    let index = index_storage(&arena);
    assert!(index > 0);
    let requested = page_storage_without_index(&arena) + payload.storage_bytes() + index;
    assert!(
        arena.total_bytes_allocated() >= requested,
        "{} accounted bytes for {requested} retained bytes including {index} index bytes",
        arena.total_bytes_allocated()
    );
}
#[test]
fn physical_payload_index_original_one_slot_budget_denies_unfunded_index() {
    let bare = NODE_BYTES + PageMetadata::requested_array_bytes(1).unwrap() + 8;
    let mut arena = RArena::with_budget(ArenaBudget::new(bare, 1));
    assert!(
        arena.alloc_vector(SEXPTYPE::RAWSXP, 1).is_null(),
        "payload index escaped the original one-slot budget"
    );
    assert_eq!(arena.node_count(), 0);
    assert_eq!(arena.total_bytes_allocated(), 0);
    arena.set_budget(ArenaBudget::new(
        bare + std::mem::size_of::<super::slab_slots::SlotRecord>(),
        1,
    ));
    assert!(!arena.alloc_vector(SEXPTYPE::RAWSXP, 1).is_null());
    assert_eq!(
        arena.total_bytes_allocated(),
        bare + std::mem::size_of::<super::slab_slots::SlotRecord>()
    );
}
#[test]
fn physical_payload_index_reclaimed_slots_keep_index_storage_charged() {
    let mut arena = RArena::new();
    let pointer = arena.alloc_vector(SEXPTYPE::RAWSXP, 1);
    let node = arena.node_token(pointer).unwrap();
    let before = arena.total_bytes_allocated();
    let page_storage = page_storage_without_index(&arena);
    let index = index_storage(&arena);
    // SAFETY: this fixture owns the exact live arena node, has no payload
    // loan or root, and retires it before inspecting the retained index.
    unsafe {
        arena.free_node(pointer);
    }
    assert!(!node.is_live());
    assert_eq!(arena.node_count(), 0);
    assert!(
        arena.total_bytes_allocated() >= page_storage + index,
        "empty payload index lost its retained-buffer charge"
    );
    let reused = arena.alloc_vector(SEXPTYPE::RAWSXP, 1);
    assert_eq!(reused, pointer);
    assert_ne!(arena.node_token(reused).unwrap().id(), node.id());
    assert_eq!(arena.total_bytes_allocated(), before);
}
#[test]
fn physical_payload_index_charge_follows_detached_page_until_final_owner_drops() {
    let mut arena = RArena::new();
    let pointer = arena.alloc_vector(SEXPTYPE::RAWSXP, 1);
    let node = arena.node_token(pointer).unwrap();
    let root = node.root_lease().unwrap();
    let counter = arena.allocated_bytes.clone();
    let metadata =
        PageMetadata::requested_array_bytes(arena.backing.node_pages.borrow()[0].meta.slots())
            .unwrap();
    let requested = page_storage_without_index(&arena) + index_storage(&arena) + 8;
    drop(arena);
    assert!(node.is_live());
    assert!(
        counter.get() >= requested,
        "detached page lost its actual payload-index charge"
    );
    drop(root);
    assert!(!node.is_live());
    assert_eq!(counter.get(), metadata);
    drop(node);
    assert_eq!(counter.get(), 0);
}
