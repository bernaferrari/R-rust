//! Incremental owning children keep connection reads at the current object.
#![forbid(unsafe_code)]

use crate::sexp::{SEXPTYPE, Sexp, SexpMut, memory::WorkspaceReservation, owner::OwnerToken};

pub(super) struct StreamValues {
    values: Vec<Sexp<'static>>,
    reservation: WorkspaceReservation,
}

impl StreamValues {
    pub(super) fn for_owner(owner: OwnerToken<'_>) -> Result<Self, String> {
        let reservation = owner
            .with_arena(|arena| arena.reserve_workspace(0))
            .map_err(|e| e.to_string())?
            .ok_or_else(memory_error)?;
        Ok(Self::new(reservation))
    }
    pub(super) fn new(reservation: WorkspaceReservation) -> Self {
        Self {
            values: Vec::new(),
            reservation,
        }
    }

    pub(super) fn push(&mut self, value: Sexp<'static>) -> Result<(), String> {
        if self.values.len() == self.values.capacity() {
            let old = self.values.capacity();
            let capacity = old.max(4).checked_mul(2).ok_or_else(memory_error)?;
            let width = std::mem::size_of::<Sexp<'static>>();
            let peak = old
                .checked_add(capacity)
                .and_then(|n| n.checked_mul(width))
                .ok_or_else(memory_error)?;
            self.reservation.try_resize(peak).ok_or_else(memory_error)?;
            let mut replacement = Vec::new();
            replacement
                .try_reserve_exact(capacity)
                .map_err(|_| memory_error())?;
            replacement.append(&mut self.values);
            drop(std::mem::replace(&mut self.values, replacement));
            self.reservation
                .try_resize(capacity * width)
                .expect("shrinking admitted values");
        }
        self.values.push(value);
        Ok(())
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &Sexp<'static>> {
        self.values.iter()
    }

    pub(super) fn finish(
        self,
        owner: OwnerToken<'_>,
        kind: SEXPTYPE,
    ) -> Result<Sexp<'static>, String> {
        owner.require_active().map_err(|e| e.to_string())?;
        let length = self.values.len().try_into().map_err(|_| memory_error())?;
        let vector = owner
            .node_factory()
            .allocate(|arena| Some(arena.alloc_vector(kind, length)))
            .and_then(|value| value.into_owned())
            .map_err(|e| e.to_string())?;
        let mut vector = SexpMut::try_from_checked(vector).map_err(|e| e.to_string())?;
        for (index, value) in self.values.iter().enumerate() {
            vector
                .try_set_vector_elt(index as _, value.clone())
                .map_err(|e| e.to_string())?;
        }
        Ok(vector.freeze())
    }
}

fn memory_error() -> String {
    "unserialize exceeds session memory limit".into()
}
