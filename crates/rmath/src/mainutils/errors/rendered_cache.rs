#![forbid(unsafe_code)]
//! The rendered-message key owns its exact original-runtime buffer charge.
use crate::sexp::{
    memory::WorkspaceReservation,
    object::{SexpError, SexpResult},
    owner::{WeakOwner, with_runtime},
};

pub(crate) struct RenderedMessage {
    // Destroy the physical buffer before releasing its accounting ticket.
    text: String,
    _admission: WorkspaceReservation,
}

impl RenderedMessage {
    pub(crate) fn copy(owner: &WeakOwner, source: &str) -> SexpResult<Self> {
        let failure = || SexpError::AllocationFailed {
            object: "rendered error message cache",
        };
        with_runtime(owner, |access| {
            let mut admission = access
                .with_arena(|arena| arena.reserve_workspace(source.len()))?
                .ok_or_else(failure)?;
            let mut text = String::new();
            text.try_reserve_exact(source.len())
                .map_err(|_| failure())?;
            admission.try_resize(text.capacity()).ok_or_else(failure)?;
            text.push_str(source);
            access.require_active()?;
            Ok(Self {
                text,
                _admission: admission,
            })
        })?
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.text.capacity()
    }
}
