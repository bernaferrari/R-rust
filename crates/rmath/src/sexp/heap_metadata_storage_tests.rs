use super::*;

fn retained_array_bytes(metadata: &PageMetadata) -> usize {
    (metadata.live.len() + metadata.old.len() + metadata.rooted.len())
        * std::mem::size_of::<Cell<u64>>()
        + metadata.roots.len() * std::mem::size_of::<Cell<usize>>()
        + metadata.epochs.len() * std::mem::size_of::<Cell<u32>>()
        + metadata.generations.len() * std::mem::size_of::<Cell<u64>>()
}

#[test]
fn page_metadata_accounts_for_every_retained_collector_array() {
    let heap = HeapIdentity::new();
    let ledger = heap.byte_ledger();
    let metadata = PageMetadata::try_new(heap, 0, 130).unwrap();
    let requested = retained_array_bytes(&metadata);
    assert_eq!(PageMetadata::requested_array_bytes(130).unwrap(), requested);
    assert!(requested > 0);
    assert!(
        ledger.allocated_bytes() >= requested,
        "{} accounted bytes for {requested} retained collector-array bytes",
        ledger.allocated_bytes()
    );
    drop(metadata);
    assert_eq!(ledger.allocated_bytes(), 0);
}

#[test]
fn page_metadata_rejects_its_arrays_under_the_original_small_heap_limit() {
    let arena =
        super::super::memory::RArena::with_budget(super::super::memory::ArenaBudget::new(1, 0));
    let heap = arena.heap_identity();
    let ledger = heap.byte_ledger();
    assert!(
        PageMetadata::try_new(heap, 0, 130).is_err(),
        "collector arrays escaped the original byte budget"
    );
    assert_eq!(ledger.allocated_bytes(), 0);
}

#[test]
fn page_metadata_charge_survives_retirement_until_its_final_reader_drops() {
    let heap = HeapIdentity::new();
    let ledger = heap.byte_ledger();
    let page = NodePage::try_new(heap, 0, 3, || 0u8).unwrap();
    page.metadata.activate(0, false).unwrap();
    let node = page.token(0).unwrap();
    let reader = page.projection();
    let requested = retained_array_bytes(&page.metadata);
    drop(page);
    assert!(!node.is_live());
    assert!(ledger.allocated_bytes() >= requested);
    drop(node);
    assert!(ledger.allocated_bytes() >= requested);
    drop(reader);
    assert_eq!(ledger.allocated_bytes(), 0);
}
