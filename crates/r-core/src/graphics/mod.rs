//! The pure-world graphics subsystem: GNU's layout cascade and par()
//! state machine over ValueId, exactly as the oracle fixtures pin it.
//! Drawing primitives build on the region math here — the state machine
//! is the spine every high-level plot command threads through.

pub mod layout;
pub mod state;

pub use state::GraphicsParState;
pub mod pretty;
