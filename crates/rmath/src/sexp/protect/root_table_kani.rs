use super::root_storage::{RootStorage, StorageError};
use super::*;
#[kani::proof]
fn root_table_exhaustion_is_atomic() {
    let mut table = RootStorage::default();
    table.set_next_generation_for_test(u64::MAX);
    assert_eq!(
        table.try_claim(1u8, false),
        Err(StorageError::GenerationExhausted)
    );
    assert_eq!(table.checkpoint(), u64::MAX);
    assert_eq!(table.len(), 0);
}
#[kani::proof]
#[kani::unwind(4)]
fn root_table_ops_preserve_invariant() {
    let mut table = RootStorage::default();
    let old = table.try_claim(1u8, false).unwrap();
    table.release(old);
    let live = table.try_claim(2u8, false).unwrap();
    assert_eq!(old.index, live.index);
    assert_ne!(old.generation, live.generation);
    assert!(!table.release(old));
    assert_eq!(table.get(live), Some(&2));
}
#[kani::proof]
#[kani::unwind(4)]
fn root_table_restore_keeps_managed() {
    let mut table = RootStorage::default();
    let checkpoint = table.checkpoint();
    let raw = table.try_claim(1u8, false).unwrap();
    let managed = table.try_claim(2u8, true).unwrap();
    table.restore(checkpoint);
    assert_eq!(table.get(raw), None);
    assert_eq!(table.get(managed), Some(&2));
}
#[kani::proof]
#[kani::unwind(4)]
fn root_table_legacy_stack_is_disjoint() {
    let mut table = RootStorage::default();
    let mut legacy = Vec::new();
    let _ = table.try_claim(1u8, false).unwrap();
    let before = (table.len(), table.checkpoint());
    legacy.push(2u8);
    legacy.pop();
    assert_eq!((table.len(), table.checkpoint()), before);
}
