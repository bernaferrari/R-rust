use crate::sexp::{
    heap::{HeapIdentity, NodeLink},
    payload::{PayloadError, PayloadLease},
};
use std::{cell::Cell, fmt::Debug, rc::Rc};

type CellAdopter<T> = fn(Rc<[Cell<T>]>) -> Result<PayloadLease, PayloadError>;

fn preserve_original_projection<T: Copy + PartialEq + Debug>(
    initial: T,
    replacement: T,
    adopt: CellAdopter<T>,
    write: fn(&PayloadLease, usize, T) -> Option<()>,
) {
    let cells: Rc<[Cell<T>]> = Rc::from([Cell::new(initial), Cell::new(initial)]);
    let original = Rc::as_ptr(&cells).cast::<Cell<T>>();
    let lease = adopt(cells).unwrap();
    assert_eq!(lease.native_projection(), original.cast_mut().cast::<u8>());
    let heap = HeapIdentity::new();
    let ledger = heap.byte_ledger();
    ledger.admit_payload(&heap, &lease).unwrap();
    assert_eq!(write(&lease, 1, replacement), Some(()));
    // SAFETY: the projection belongs to these same original two initialized
    // cells, now owned by the live lease. No payload loan overlaps this read.
    unsafe {
        assert_eq!((*original).get(), initial);
        assert_eq!((*original.add(1)).get(), replacement);
    }
    let retained = lease.clone();
    drop(lease);
    retained.make_immutable();
    assert_eq!(write(&retained, 1, initial), None);
    // SAFETY: the retained lease still owns the same sealed allocation;
    // this compatibility projection performs only a bounded read.
    unsafe {
        assert_eq!((*original.add(1)).get(), replacement);
    }
    drop(retained);
    assert_eq!(ledger.allocated_bytes(), 0);
}

#[test]
fn original_unique_cell_projection_remains_valid_through_adoption_and_seal() {
    preserve_original_projection(
        7u8,
        29,
        PayloadLease::from_byte_cells,
        PayloadLease::set_byte_elt,
    );
    preserve_original_projection(
        7,
        29,
        PayloadLease::from_integer_cells,
        PayloadLease::set_integer_elt,
    );
    preserve_original_projection(
        7.,
        29.,
        PayloadLease::from_real_cells,
        PayloadLease::set_real_elt,
    );
    preserve_original_projection(
        NodeLink::NULL,
        NodeLink::NULL,
        PayloadLease::from_reference_cells,
        PayloadLease::set_reference_elt,
    );
}
