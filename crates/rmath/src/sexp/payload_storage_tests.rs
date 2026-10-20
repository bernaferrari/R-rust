use super::*;
use crate::sexp::memory::{ArenaBudget, RArena};

fn requested_chunk_bytes(payload: &OwnedPayload) -> usize {
    match &payload.storage {
        Storage::Bytes(values) => values.capacity() * std::mem::size_of::<ByteChunk>(),
        Storage::Integers(values) => values.capacity() * std::mem::size_of::<IntegerChunk>(),
        _ => panic!("fixture requires the actual byte or integer chunks"),
    }
}

fn assert_original_ledger_covers_chunks(payload: OwnedPayload) {
    let requested = requested_chunk_bytes(&payload);
    let logical = payload.layout().size();
    let heap = HeapIdentity::new();
    let ledger = heap.byte_ledger();
    let lease = PayloadLease::from_owned(payload).unwrap();
    ledger.admit_payload(&heap, &lease).unwrap();
    assert_eq!(lease.logical_bytes(), logical);
    assert!(
        ledger.allocated_bytes() >= requested,
        "{} accounted bytes for {requested} retained chunk bytes ({logical} logical bytes)",
        ledger.allocated_bytes()
    );
    let retained = lease.clone();
    drop(lease);
    assert!(ledger.allocated_bytes() >= requested);
    drop(retained);
    assert_eq!(ledger.allocated_bytes(), 0);
}

#[test]
fn raw_payload_tail_storage_is_charged_through_its_final_lease() {
    for length in 1..=17 {
        assert_original_ledger_covers_chunks(
            OwnedPayload::zeroed_vector(SEXPTYPE::RAWSXP, length).unwrap(),
        );
    }
}

#[test]
fn integer_payload_tail_storage_is_charged_through_its_final_lease() {
    for length in 1..=17 {
        assert_original_ledger_covers_chunks(
            OwnedPayload::zeroed_vector(SEXPTYPE::INTSXP, length).unwrap(),
        );
    }
}

#[test]
fn character_payload_tail_storage_includes_the_terminator_chunk() {
    for length in 0..=17 {
        assert_original_ledger_covers_chunks(
            OwnedPayload::characters(&vec![b'a'; length]).unwrap(),
        );
    }
}

#[test]
fn scratch_payload_tail_storage_is_charged_through_its_final_lease() {
    for length in 1..=17 {
        assert_original_ledger_covers_chunks(OwnedPayload::zeroed_bytes(length).unwrap());
    }
}

#[test]
fn checked_raw_payload_denies_chunk_tail_before_native_allocation() {
    let page = RArena::minimum_page_bytes();
    let chunk = std::mem::size_of::<ByteChunk>();
    let mut arena = RArena::with_budget(ArenaBudget::new(page + chunk - 1, 0));
    let attempts = crate::sexp::memory::buffer_allocation_attempts();
    assert!(arena.alloc_vector_checked(SEXPTYPE::RAWSXP, 1).is_err());
    assert_eq!(crate::sexp::memory::buffer_allocation_attempts(), attempts);
    assert_eq!(arena.node_count(), 0);
    assert_eq!(arena.total_bytes_allocated(), 0);
}
