//! Lightweight state shim for wasm32 builds.
//!
//! Provides `with_required_current_instance` backed by a thread_local
//! `WasmMathInstance`, so the pure-math dist/rng/appl modules compile
//! without the full sexp/eval/mainutils interpreter tree.

use std::cell::RefCell;
use std::collections::HashMap;

/// Minimal per-thread math state mirroring the fields of `RInstance`
/// that the distribution / RNG / optimisation code accesses.
pub struct WasmMathInstance {
    pub rng_state: (u32, u32),
    pub dist_beta_state: crate::dist::beta::BetaState,
    pub dist_gamma_state: crate::dist::gamma::GammaState,
    pub dist_hyper_state: crate::dist::hypergeometric::RhyperState,
    pub dist_pois_state: crate::dist::poisson::RpoisState,
    pub dist_binom_state: crate::dist::binomial::RbinomState,
    pub signrank_cache: HashMap<i32, Vec<f64>>,
    pub wilcox_cache: HashMap<(i32, i32), Vec<f64>>,
    pub lbfgsb_state: crate::appl::lbfgsb::LbfgsbState,
    // nmath copies use different field names
    pub nmath_beta_state: crate::nmath::dist::beta::BetaState,
    pub nmath_gamma_state: crate::nmath::dist::gamma::GammaState,
    pub nmath_hyper_state: crate::nmath::dist::hypergeometric::RhyperState,
    pub nmath_pois_state: crate::nmath::dist::poisson::RpoisState,
    pub nmath_binom_state: crate::nmath::dist::binomial::RbinomState,
}

impl Default for WasmMathInstance {
    fn default() -> Self {
        Self {
            rng_state: (1234, 5678),
            dist_beta_state: crate::dist::beta::BetaState::default(),
            dist_gamma_state: crate::dist::gamma::GammaState::default(),
            dist_hyper_state: crate::dist::hypergeometric::RhyperState::new(),
            dist_pois_state: crate::dist::poisson::RpoisState::new(),
            dist_binom_state: crate::dist::binomial::RbinomState::new(),
            signrank_cache: HashMap::new(),
            wilcox_cache: HashMap::new(),
            lbfgsb_state: crate::appl::lbfgsb::LbfgsbState::default(),
            nmath_beta_state: crate::nmath::dist::beta::BetaState::default(),
            nmath_gamma_state: crate::nmath::dist::gamma::GammaState::default(),
            nmath_hyper_state: crate::nmath::dist::hypergeometric::RhyperState::new(),
            nmath_pois_state: crate::nmath::dist::poisson::RpoisState::new(),
            nmath_binom_state: crate::nmath::dist::binomial::RbinomState::new(),
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
pub fn with_required_current_instance<F, R>(f: F) -> R
where
    F: FnOnce(&mut WasmMathInstance) -> R,
{
    WASM_INSTANCE.with(|cell| f(&mut cell.borrow_mut()))
}