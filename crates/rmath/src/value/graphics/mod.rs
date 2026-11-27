//! The pure-world graphics subsystem: GNU's layout cascade and par()
//! state machine over ValueId, exactly as the oracle fixtures pin it.
//! Drawing primitives build on the region math here — the state machine
//! is the spine every high-level plot command threads through.

pub(crate) mod layout;
pub(crate) mod state;

pub(crate) use state::GraphicsParState;
