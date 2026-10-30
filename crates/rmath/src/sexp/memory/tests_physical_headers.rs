//! Requested header storage is charged independently of active node counts.
use super::*;

pub(super) fn header_backing_bytes(arena: &RArena) -> usize {
    arena
        .backing
        .node_pages
        .borrow()
        .iter()
        .map(|page| page.meta.slots() * std::mem::size_of::<Cell<SexprecCore>>())
        .sum()
}

pub(super) fn retained_page_bytes(arena: &RArena) -> usize {
    arena.backing.node_pages.borrow().allocation_size()
        + header_backing_bytes(arena)
        + arena
            .backing
            .node_pages
            .borrow()
            .iter()
            .map(|page| {
                PageMetadata::requested_array_bytes(page.meta.slots()).unwrap()
                    + page.payloads.borrow().allocation_size()
            })
            .sum::<usize>()
}

pub(super) fn minimum_page_bytes() -> usize {
    RArena::minimum_page_bytes()
}

#[test]
fn physical_header_prepaid_slots_leave_small_budget_for_later_payloads() {
    let mut arena = RArena::with_budget(ArenaBudget::new(8192, 0));
    assert!(!arena.alloc_charsxp(b"literal").is_null());
    let headers = header_backing_bytes(&arena);
    let before = arena.total_bytes_allocated();
    let first = arena.alloc_vector(SEXPTYPE::STRSXP, 1);
    assert!(!first.is_null());
    assert_eq!(header_backing_bytes(&arena), headers);
    // The first character vector lazily installs its retained empty-string
    // value as well as allocating the vector's own reference payload.
    let blank_pointer = arena.blank_string;
    let blank = arena.node_token(blank_pointer).unwrap();
    let blank_payload = arena.heap_identity().payload_lease(&blank).unwrap();
    assert_eq!(blank_payload.byte_elt(0), Some(0));
    let first_node = arena.node_token(first).unwrap();
    let first_payload = arena.heap_identity().payload_lease(&first_node).unwrap();
    assert_eq!(
        arena.total_bytes_allocated(),
        before + blank_payload.storage_bytes() + first_payload.storage_bytes()
    );
    assert_eq!(
        first_payload.storage_bytes(),
        std::mem::size_of::<NodeLink>()
    );
    let before_second = arena.total_bytes_allocated();
    let second = arena.alloc_vector(SEXPTYPE::STRSXP, 1);
    assert!(!second.is_null());
    assert_eq!(arena.blank_string, blank_pointer);
    assert_eq!(
        arena.total_bytes_allocated(),
        before_second + first_payload.storage_bytes()
    );
    assert!(arena.total_bytes_allocated() <= 8192);
}

#[test]
fn physical_header_first_node_accounts_for_the_complete_retained_buffer() {
    let mut arena = RArena::new();
    let value = arena.alloc_node(SEXPTYPE::INTSXP);
    assert!(!value.is_null());
    let requested = header_backing_bytes(&arena);
    assert!(requested >= std::mem::size_of::<SexprecCore>());
    assert!(
        arena.total_bytes_allocated() >= requested,
        "{} accounted bytes for {requested} retained header bytes",
        arena.total_bytes_allocated()
    );
}

#[test]
fn physical_header_admission_never_exceeds_a_small_byte_budget() {
    let mut arena = RArena::new();
    let limit = 3 * std::mem::size_of::<SexprecCore>();
    arena.set_budget(ArenaBudget::new(limit, 0));
    let value = arena.alloc_node(SEXPTYPE::INTSXP);
    assert!(
        header_backing_bytes(&arena) <= limit,
        "retained header buffer escaped original byte budget"
    );
    if value.is_null() {
        assert_eq!(arena.node_count(), 0);
        assert_eq!(arena.total_bytes_allocated(), 0);
    } else {
        assert!(arena.total_bytes_allocated() >= header_backing_bytes(&arena));
    }
}

#[test]
fn physical_header_charge_follows_detached_backing_until_its_final_owner_drops() {
    let mut arena = RArena::new();
    let value = arena.alloc_node(SEXPTYPE::INTSXP);
    let node = arena.node_token(value).unwrap();
    let root = node.root_lease().unwrap();
    let counter = arena.allocated_bytes.clone();
    let requested = header_backing_bytes(&arena);
    let metadata_requested = arena
        .backing
        .node_pages
        .borrow()
        .iter()
        .map(|page| PageMetadata::requested_array_bytes(page.meta.slots()).unwrap())
        .sum::<usize>();
    drop(arena);
    assert!(node.is_live());
    assert!(
        counter.get() >= requested,
        "detached header backing lost its complete storage charge"
    );
    drop(root);
    assert!(!node.is_live());
    assert_eq!(counter.get(), metadata_requested);
    drop(node);
    assert_eq!(counter.get(), 0);
}
