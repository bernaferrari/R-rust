//! Incremental owning children keep connection reads at the current object.
#![forbid(unsafe_code)]

use crate::sexp::{Sexp, memory::WorkspaceReservation};

pub(super) struct StreamValues {
    values: Vec<Sexp<'static>>,
    reservation: WorkspaceReservation,
}

impl StreamValues {
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
}

fn memory_error() -> String {
    "unserialize exceeds session memory limit".into()
}
