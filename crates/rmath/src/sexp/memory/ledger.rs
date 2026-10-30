//! Shared admission for all physical storage in one heap identity.
#![forbid(unsafe_code)]

use super::{TransientReservation, WorkspaceReservation};
use crate::sexp::{heap::HeapIdentity, payload::PayloadLease};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// Shared only for independently droppable transient admissions. Arena state
/// and its allocated-byte ledger remain confined to the interpreter thread.
#[derive(Clone, Default)]
pub(super) struct TransientCounter(Arc<AtomicUsize>);

impl TransientCounter {
    pub(super) fn get(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }

    pub(super) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub(super) fn compare_exchange(&self, old: usize, new: usize) -> Result<usize, usize> {
        self.0
            .compare_exchange_weak(old, new, Ordering::Relaxed, Ordering::Relaxed)
    }

    pub(super) fn release(&self, bytes: usize) {
        loop {
            let current = self.get();
            let remaining = current
                .checked_sub(bytes)
                .expect("reservation byte ledger balance");
            if self.compare_exchange(current, remaining).is_ok() {
                return;
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct MemoryLedger {
    allocated: Rc<Cell<usize>>,
    transient: TransientCounter,
    limit: Rc<Cell<usize>>,
}

pub(super) struct ArenaCounters {
    pub(super) allocated: Rc<Cell<usize>>,
    pub(super) transient: TransientCounter,
    pub(super) limit: Rc<Cell<usize>>,
}

impl MemoryLedger {
    pub(crate) fn new() -> Self {
        Self {
            allocated: Rc::new(Cell::new(0)),
            transient: TransientCounter::default(),
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
    #[cfg(test)]
    pub(crate) fn workspace_bytes_for_test(&self) -> usize {
        self.transient.get()
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
        let mut reservation = self.reserve(lease.storage_bytes())?;
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::MemoryLedger;

    #[test]
    fn send_workspace_stays_charged_until_dropped_on_another_thread() {
        let ledger = MemoryLedger::new();
        let counters = ledger.counters();
        counters.limit.set(100);
        counters.allocated.set(20);
        let ticket = ledger.reserve(70).unwrap().into_send();
        assert_eq!(ledger.workspace_bytes_for_test(), 70);
        assert!(ledger.reserve(11).is_none());
        let observer = counters.transient.clone();
        drop(ledger);
        std::thread::spawn(move || drop(ticket)).join().unwrap();
        assert_eq!(observer.get(), 0);
    }

    #[test]
    fn concurrent_send_ticket_release_preserves_other_reservations() {
        let ledger = MemoryLedger::new();
        let tickets: Vec<_> = (0..256)
            .map(|_| ledger.reserve(1).unwrap().into_send())
            .collect();
        let worker = std::thread::spawn(move || drop(tickets));
        for _ in 0..1000 {
            let mut ticket = ledger.reserve(2).unwrap();
            ticket.try_resize(3).unwrap();
            ticket.try_resize(1).unwrap();
        }
        worker.join().unwrap();
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
    }

    #[test]
    fn send_workspace_retains_original_charge_after_session_close() {
        let mut session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let (ticket, counter) = owner
            .with_arena(|arena| {
                (
                    arena.reserve_workspace(64).unwrap().into_send(),
                    arena.transient_bytes.clone(),
                )
            })
            .unwrap();
        assert_eq!(counter.get(), 64);
        session.close();
        assert_eq!(counter.get(), 64);
        std::thread::spawn(move || drop(ticket)).join().unwrap();
        assert_eq!(counter.get(), 0);
    }

    #[test]
    fn send_workspace_never_charges_another_session() {
        let first = crate::sexp::session::RSession::new_for_gc_tests();
        let second = crate::sexp::session::RSession::new_for_gc_tests();
        let (ticket, first_counter) = first.with_active_in(|_| {
            first
                .owner_token()
                .unwrap()
                .with_arena(|arena| {
                    (
                        arena.reserve_workspace(64).unwrap().into_send(),
                        arena.transient_bytes.clone(),
                    )
                })
                .unwrap()
        });
        let second_counter = second.with_active_in(|_| {
            second
                .owner_token()
                .unwrap()
                .with_arena(|arena| arena.transient_bytes.clone())
                .unwrap()
        });
        assert_eq!(first_counter.get(), 64);
        assert_eq!(second_counter.get(), 0);
        std::thread::spawn(move || drop(ticket)).join().unwrap();
        assert_eq!(first_counter.get(), 0);
        assert_eq!(second_counter.get(), 0);
    }
}
