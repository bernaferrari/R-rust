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

#[test]
fn physical_header_prepaid_slots_leave_small_budget_for_later_payloads() {
    let mut arena = RArena::with_budget(ArenaBudget::new(8192, 0));
    assert!(!arena.alloc_charsxp(b"literal").is_null());
    let headers = header_backing_bytes(&arena);
    let before = arena.total_bytes_allocated();
    assert!(!arena.alloc_vector(SEXPTYPE::STRSXP, 1).is_null());
    assert_eq!(header_backing_bytes(&arena), headers);
    assert_eq!(
        arena.total_bytes_allocated(),
        before + std::mem::size_of::<NodeLink>()
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
    drop(arena);
    assert!(node.is_live());
    assert!(
        counter.get() >= requested,
        "detached header backing lost its complete storage charge"
    );
    drop(root);
    assert!(!node.is_live());
    assert_eq!(counter.get(), 0);
}
