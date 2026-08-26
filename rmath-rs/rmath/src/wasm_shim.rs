//! Lightweight state shim for wasm32 builds.
//!
//! Provides `with_required_current_instance` backed by a thread_local
//! `WasmMathInstance`, so the pure-math dist/rng/appl modules compile
//! without the full sexp/eval/mainutils interpreter tree.

use std::cell::RefCell;
use std::collections::HashMap;

/// Minimal per-thread math state mirroring the fields of `RInstance`
/// that the distribution / RNG / optimisation code accesses.
#[allow(dead_code)]
pub(crate) struct WasmMathInstance {
    pub(crate) rng_state: rmath_nmath::RngState,
    pub(crate) dist_beta_state: crate::dist::beta::BetaState,
    pub(crate) dist_gamma_state: crate::dist::gamma::GammaState,
    pub(crate) dist_hyper_state: crate::dist::hypergeometric::RhyperState,
    pub(crate) dist_pois_state: crate::dist::poisson::RpoisState,
    pub(crate) dist_binom_state: crate::dist::binomial::RbinomState,
    pub(crate) signrank_cache: HashMap<i32, Vec<f64>>,
    pub(crate) wilcox_cache: HashMap<(i32, i32), Vec<f64>>,
    pub(crate) lbfgsb_state: crate::appl::lbfgsb::LbfgsbState,
}

impl Default for WasmMathInstance {
    fn default() -> Self {
        Self {
            rng_state: rmath_nmath::RngState::default(),
            dist_beta_state: crate::dist::beta::BetaState::default(),
            dist_gamma_state: crate::dist::gamma::GammaState::default(),
            dist_hyper_state: crate::dist::hypergeometric::RhyperState::new(),
            dist_pois_state: crate::dist::poisson::RpoisState::new(),
            dist_binom_state: crate::dist::binomial::RbinomState::new(),
            signrank_cache: HashMap::new(),
            wilcox_cache: HashMap::new(),
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
/// This is API-compatible with `sexp::instance::with_required_current_instance`
/// — callers use the same field names, so no code changes are needed in
/// dist/rng/appl modules.
pub(crate) fn with_required_current_instance<F, R>(f: F) -> R
where
    F: FnOnce(&mut WasmMathInstance) -> R,
{
    WASM_INSTANCE.with(|cell| f(&mut cell.borrow_mut()))
}
