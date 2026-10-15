#![forbid(unsafe_code)]
//! Host-owned buffers share the original session's arena admission ledger.

use crate::sexp::{memory::WorkspaceReservation, owner::WeakOwner};

/// Accounted runtime bytes, excluding allocator metadata, shared process
/// caches and host allocations which have not requested a reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RMemoryUsage {
    pub arena_bytes: u64,
    pub workspace_bytes: u64,
    pub total_bytes: u64,
}

/// Thread-confined admission for a host buffer. Reserve before allocation;
/// resize before growth and retain this guard for the buffer's entire lifetime.
/// Dropping it releases only its original session's charge, even after close.
pub struct RWorkspaceReservation {
    reservation: WorkspaceReservation,
    owner: WeakOwner,
}

impl RWorkspaceReservation {
    pub(crate) fn new(reservation: WorkspaceReservation, owner: WeakOwner) -> Self {
        Self { reservation, owner }
    }

    pub fn bytes(&self) -> usize {
        self.reservation.bytes()
    }

    /// Failed admission leaves the previous charge unchanged.
    pub fn try_resize(&mut self, bytes: usize) -> Result<(), String> {
        if !self.owner.is_live() {
            return Err("workspace owner is closed".into());
        }
        self.reservation
            .try_resize(bytes)
            .ok_or_else(|| "session memory budget exceeded by native workspace".into())
    }
}
