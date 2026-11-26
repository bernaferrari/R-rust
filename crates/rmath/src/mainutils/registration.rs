#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Native routine registration types.
//!
//! Bundled packages register their Rust-implemented routines through the
//! typed rdynload registry directly; the C-ABI registration exports are
//! gone because no foreign shared object can ever load into the pure-Rust
//! runtime.

pub use crate::mainutils::rdynload::{
    DllInfo, R_CMethodDef, R_CallMethodDef, R_ExternalMethodDef, R_FortranMethodDef,
};

// ---------------------------------------------------------------------------
// R_init_base
// ---------------------------------------------------------------------------

/// Initialize the base package's registered routines.
pub fn R_init_base(_dll: *mut DllInfo) {
    // In the full R implementation, this registers:
    // - callMethods: R_addTaskCallback, R_getTaskCallbackNames, R_removeTaskCallback
    // - fortranMethods: dqrcf, dqrdc2, dqrqty, dqrqy, dqrrsd, dqrxb, dtrco
    // these are stubs since the routines themselves aren't ported.
}
