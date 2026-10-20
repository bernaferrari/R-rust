//! Shared admission for all physical storage in one heap identity.
#![forbid(unsafe_code)]

use super::{TransientReservation, WorkspaceReservation};
use crate::sexp::{heap::HeapIdentity, payload::PayloadLease};
use std::{cell::Cell, rc::Rc};

#[derive(Clone)]
pub(crate) struct MemoryLedger {
    allocated: Rc<Cell<usize>>,
    transient: Rc<Cell<usize>>,
    limit: Rc<Cell<usize>>,
}

pub(super) struct ArenaCounters {
    pub(super) allocated: Rc<Cell<usize>>,
    pub(super) transient: Rc<Cell<usize>>,
    pub(super) limit: Rc<Cell<usize>>,
}

impl MemoryLedger {
    pub(crate) fn new() -> Self {
        Self {
            allocated: Rc::new(Cell::new(0)),
            transient: Rc::new(Cell::new(0)),
            limit: Rc::new(Cell::new(0)),
        }
    }
    pub(super) fn counters(&self) -> ArenaCounters {
        ArenaCounters {
            allocated: self.allocated.clone(),
            transient: self.transient.clone(),
            limit: self.limit.clone(),
        }
    }
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.allocated.get()
    }
    pub(crate) fn reserve(&self, bytes: usize) -> Option<WorkspaceReservation> {
        WorkspaceReservation::from_counters(
            self.allocated.clone(),
            self.transient.clone(),
            self.limit.clone(),
            bytes,
        )
    }
    pub(crate) fn reserve_payload(
        &self,
        heap: &HeapIdentity,
        bytes: usize,
    ) -> Option<TransientReservation> {
        Some(
            TransientReservation::new(
                self.limit.get(),
                self.allocated.get(),
                0,
                &self.transient,
                bytes,
            )?
            .for_payload(heap.clone(), self.allocated.clone()),
        )
    }
    pub(crate) fn admit_payload(&self, heap: &HeapIdentity, lease: &PayloadLease) -> Option<()> {
        if lease.belongs_to(heap) {
            return lease.bind_to_heap(heap, &self.allocated);
        }
        let mut reservation = self.reserve(lease.logical_bytes())?;
        reservation.bind_payload(heap, lease)
    }
}

/// Header charge follows its actual owning page, including retained backing.
pub(crate) struct AllocationCharge {
    pub(super) allocated: Rc<Cell<usize>>,
    pub(super) bytes: usize,
}
impl Drop for AllocationCharge {
    fn drop(&mut self) {
        self.allocated.set(
            self.allocated
                .get()
                .checked_sub(self.bytes)
                .expect("allocation byte ledger balance"),
        );
    }
}
