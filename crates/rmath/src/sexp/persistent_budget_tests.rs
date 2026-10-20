use super::*;
use crate::sexp::{instance::RInstance, memory::ArenaBudget};

#[test]
fn permanent_budget_characters_deny_before_native_allocation() {
    let mut instance = RInstance::new_for_gc_tests();
    let before = instance.arena.total_bytes_allocated();
    let nodes = instance.persistent_nodes.len();
    instance.arena.set_budget(ArenaBudget::new(before + 1, 0));
    assert!(
        instance
            .persistent_nodes
            .allocate_chars(b"must obey the original ledger")
            .is_err()
    );
    assert_eq!(instance.persistent_nodes.len(), nodes);
    assert_eq!(instance.arena.total_bytes_allocated(), before);
    assert_eq!(instance.arena.workspace_bytes(), 0);
}

#[test]
fn permanent_budget_headers_share_current_arena_limit() {
    let mut instance = RInstance::new_for_gc_tests();
    let before = instance.arena.total_bytes_allocated();
    let nodes = instance.persistent_nodes.len();
    instance.arena.set_budget(ArenaBudget::new(before + 1, 0));
    assert!(
        instance
            .persistent_nodes
            .allocate_header(SexprecCore::new(SEXPTYPE::LISTSXP))
            .is_err()
    );
    assert_eq!(instance.persistent_nodes.len(), nodes);
    assert_eq!(instance.arena.total_bytes_allocated(), before);
}

#[test]
fn permanent_budget_retained_bytes_are_visible_and_release_after_removal() {
    let mut instance = RInstance::new_for_gc_tests();
    let before = instance.arena.total_bytes_allocated();
    let value = instance
        .persistent_nodes
        .allocate_chars(b"retained bytes")
        .unwrap();
    assert!(instance.arena.total_bytes_allocated() > before);
    assert_eq!(instance.arena.workspace_bytes(), 0);
    assert!(instance.persistent_nodes.remove(value));
    assert_eq!(instance.arena.total_bytes_allocated(), before);
}

#[test]
fn permanent_budget_header_and_metadata_are_admitted_before_publication() {
    let mut instance = RInstance::new_for_gc_tests();
    let before = instance.arena.total_bytes_allocated();
    let width = std::mem::size_of::<SexprecCore>()
        + crate::sexp::heap::PageMetadata::requested_array_bytes(1).unwrap();
    instance
        .arena
        .set_budget(ArenaBudget::new(before + width - 1, 0));
    assert!(
        instance
            .persistent_nodes
            .allocate_header(SexprecCore::new(SEXPTYPE::LISTSXP))
            .is_err()
    );
    instance
        .arena
        .set_budget(ArenaBudget::new(before + width, 0));
    let value = instance
        .persistent_nodes
        .allocate_header(SexprecCore::new(SEXPTYPE::LISTSXP))
        .unwrap();
    assert_eq!(instance.arena.total_bytes_allocated(), before + width);
    assert!(instance.persistent_nodes.remove(value));
    assert_eq!(instance.arena.total_bytes_allocated(), before);
}

#[test]
fn permanent_budget_character_copy_peak_and_live_limit_changes_are_enforced() {
    let mut instance = RInstance::new_for_gc_tests();
    let before = instance.arena.total_bytes_allocated();
    let width = std::mem::size_of::<SexprecCore>();
    let source = vec![b'x'; 2 * width + 31];
    let payload = source.len() + 1;
    instance
        .arena
        .set_budget(ArenaBudget::new(before + width + payload, 0));
    assert!(instance.persistent_nodes.allocate_chars(&source).is_err());
    assert_eq!(instance.arena.total_bytes_allocated(), before);
    instance
        .arena
        .set_budget(ArenaBudget::new(before + 2 * payload, 0));
    let value = instance.persistent_nodes.allocate_chars(&source).unwrap();
    let current = instance.arena.total_bytes_allocated();
    assert_eq!(
        current,
        before
            + width
            + payload
            + crate::sexp::heap::PageMetadata::requested_array_bytes(1).unwrap()
    );
    instance.arena.set_budget(ArenaBudget::new(current + 1, 0));
    assert!(instance.persistent_nodes.allocate_real(1.0).is_err());
    assert_eq!(instance.arena.total_bytes_allocated(), current);
    assert!(instance.persistent_nodes.remove(value));
    // Releasing physical storage admits a small sentinel under the same limit.
    let sentinel = instance
        .persistent_nodes
        .allocate_integer(7, false)
        .unwrap();
    assert!(instance.persistent_nodes.remove(sentinel));
    assert_eq!(instance.arena.total_bytes_allocated(), before);
    assert_eq!(instance.arena.workspace_bytes(), 0);
}

#[test]
fn permanent_budget_charges_follow_retained_storage_after_instance_drop() {
    let mut instance = RInstance::new_for_gc_tests();
    let heap = instance.arena.heap_identity();
    let ledger = heap.byte_ledger();
    let pointer = instance
        .persistent_nodes
        .allocate_chars(b"retained")
        .unwrap();
    let node = instance.persistent_nodes.token(pointer).unwrap();
    let root = node.root_lease().unwrap();
    let payload = heap.payload_lease(&node).unwrap();
    let retained = ledger.allocated_bytes();
    drop(instance);
    assert!(node.is_live());
    assert_eq!(ledger.allocated_bytes(), retained);
    drop(root);
    assert!(!node.is_live());
    drop(node);
    assert_eq!(ledger.allocated_bytes(), b"retained".len() + 1);
    assert_eq!(payload.byte_elt(0), Some(b'r'));
    drop(payload);
    assert_eq!(ledger.allocated_bytes(), 0);
}

#[test]
fn permanent_symbol_preflight_includes_both_original_metadata_pages() {
    let mut instance = RInstance::new_for_gc_tests();
    let before = instance.arena.total_bytes_allocated();
    let nodes = instance.persistent_nodes.len();
    let name = b"metadata transaction";
    let page = std::mem::size_of::<SexprecCore>()
        + crate::sexp::heap::PageMetadata::requested_array_bytes(1).unwrap();
    let peak = 2 * page + name.len() + 1;
    instance
        .arena
        .set_budget(ArenaBudget::new(before + peak - 1, 0));
    assert!(instance.persistent_nodes.allocate_symbol(name).is_err());
    assert_eq!(instance.persistent_nodes.len(), nodes);
    assert_eq!(instance.arena.total_bytes_allocated(), before);
    assert_eq!(instance.arena.workspace_bytes(), 0);
    instance
        .arena
        .set_budget(ArenaBudget::new(before + peak, 0));
    let (pname, symbol) = instance.persistent_nodes.allocate_symbol(name).unwrap();
    assert_eq!(instance.arena.total_bytes_allocated(), before + peak);
    assert!(instance.persistent_nodes.remove(symbol));
    assert!(instance.persistent_nodes.remove(pname));
    assert_eq!(instance.persistent_nodes.len(), nodes);
    assert_eq!(instance.arena.total_bytes_allocated(), before);
}
