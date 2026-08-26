//! Lightweight state shim for wasm32 builds.
//!
//! Provides `with_required_current_instance` backed by a thread_local
//! holding the `appl` continuation state (L-BFGS-B caches), so the pure
//! math `appl` modules compile on wasm32 without the full sexp/eval/
//! mainutils interpreter tree.
//!
//! The dist/rng samplers live in the `rmath-nmath` crate and manage their
//! own per-thread state through `rmath_nmath::state` on every target, so
//! this shim only carries what `crate::appl` reads.

use std::cell::RefCell;

/// Minimal per-thread math state for the wasm32 math-only build.
pub(crate) struct WasmMathInstance {
    pub(crate) lbfgsb_state: crate::appl::lbfgsb::LbfgsbState,
}

impl Default for WasmMathInstance {
    fn default() -> Self {
        Self {
            lbfgsb_state: crate::appl::lbfgsb::LbfgsbState::default(),
        }
    }
}

thread_local! {
    static WASM_INSTANCE: RefCell<WasmMathInstance> =
        RefCell::new(WasmMathInstance::default());
}

/// Access the per-thread math instance (wasm version).
///
/// This is API-compatible with
/// `sexp::instance::with_required_current_instance` — callers use the
/// same field names, so no code changes are needed in the appl modules.
pub(crate) fn with_required_current_instance<F, R>(f: F) -> R
where
    F: FnOnce(&mut WasmMathInstance) -> R,
{
    WASM_INSTANCE.with(|cell| f(&mut cell.borrow_mut()))
}
