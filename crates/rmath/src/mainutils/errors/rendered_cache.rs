#![forbid(unsafe_code)]
//! Match the error buffer to its bare host message.

use std::ops::Range;

use crate::sexp::{
    memory::WorkspaceReservation,
    object::{SexpError, SexpResult},
    owner::{WeakOwner, with_runtime},
};

pub(crate) enum RenderedMessage {
    Owned {
        // Destroy the physical buffer before releasing its accounting ticket.
        text: String,
        _admission: WorkspaceReservation,
    },
    InBuffer(Range<usize>),
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
            Ok(Self::Owned {
                text,
                _admission: admission,
            })
        })?
    }

    pub(super) fn matches(&self, buffer: &[u8], message: &str) -> bool {
        match self {
            Self::Owned { text, .. } => text == message,
            Self::InBuffer(range) => {
                let Some(bytes) = buffer.get(range.clone()) else {
                    return false;
                };
                let mut remaining = Some(message);
                crate::sexp::output::visit_capture_text(bytes, |part| {
                    remaining = remaining.and_then(|text| text.strip_prefix(part));
                    remaining.is_some()
                });
                remaining == Some("")
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        match self {
            Self::Owned { text, .. } => text.capacity(),
            Self::InBuffer(_) => 0,
        }
    }
}
