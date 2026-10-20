use super::*;

type CellAdopter<T> = fn(Rc<[Cell<T>]>) -> Result<PayloadLease, PayloadError>;

fn reject_strong_alias<T: Copy>(value: T, adopt: CellAdopter<T>) {
    let cells: Rc<[Cell<T>]> = Rc::from([Cell::new(value)]);
    assert!(matches!(
        adopt(cells.clone()),
        Err(PayloadError::AliasedStorage)
    ));
    assert_eq!(Rc::strong_count(&cells), 1);
}

fn reject_weak_alias<T: Copy>(value: T, adopt: CellAdopter<T>) {
    let cells: Rc<[Cell<T>]> = Rc::from([Cell::new(value)]);
    let weak = Rc::downgrade(&cells);
    assert!(matches!(adopt(cells), Err(PayloadError::AliasedStorage)));
    assert!(weak.upgrade().is_none());
}

#[test]
fn integer_cell_adoption_rejects_external_strong_alias() {
    reject_strong_alias(7, PayloadLease::from_integer_cells);
}

#[test]
fn real_cell_adoption_rejects_external_strong_alias() {
    reject_strong_alias(7., PayloadLease::from_real_cells);
}

#[test]
fn reference_cell_adoption_rejects_external_strong_alias() {
    reject_strong_alias(NodeLink::NULL, PayloadLease::from_reference_cells);
}

#[test]
fn integer_cell_adoption_rejects_external_weak_alias() {
    reject_weak_alias(7, PayloadLease::from_integer_cells);
}

#[test]
fn real_cell_adoption_rejects_external_weak_alias() {
    reject_weak_alias(7., PayloadLease::from_real_cells);
}

#[test]
fn reference_cell_adoption_rejects_external_weak_alias() {
    reject_weak_alias(NodeLink::NULL, PayloadLease::from_reference_cells);
}

#[test]
fn external_integer_cells_cannot_outlive_their_heap_storage_charge() {
    let cells: Rc<[Cell<i32>]> = Rc::from([Cell::new(7)]);
    let heap = HeapIdentity::new();
    let ledger = heap.byte_ledger();
    let result = PayloadLease::from_integer_cells(cells.clone());
    if matches!(result, Err(PayloadError::AliasedStorage)) {
        assert_eq!(ledger.allocated_bytes(), 0);
        assert_eq!(cells[0].get(), 7);
        return;
    }
    let lease = result.unwrap();
    ledger.admit_payload(&heap, &lease).unwrap();
    drop(lease);
    assert!(
        ledger.allocated_bytes() >= std::mem::size_of_val(cells.as_ref()),
        "external typed cells retained storage after its heap charge was released"
    );
}

#[test]
fn external_real_cell_writer_cannot_bypass_a_heap_payload_seal() {
    let cells: Rc<[Cell<f64>]> = Rc::from([Cell::new(7.)]);
    let result = PayloadLease::from_real_cells(cells.clone());
    if matches!(result, Err(PayloadError::AliasedStorage)) {
        assert_eq!(cells[0].get(), 7.);
        return;
    }
    let lease = result.unwrap();
    let heap = HeapIdentity::new();
    heap.byte_ledger().admit_payload(&heap, &lease).unwrap();
    lease.make_immutable();
    cells[0].set(29.);
    assert!(lease.is_immutable());
    assert_eq!(lease.real_elt(0), Some(7.));
}
