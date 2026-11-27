//! Session-owned math state for the distribution samplers.
//!
//! R's nmath routines carry continuation caches (beta/gamma sampler memo
//! tables, the Wilcoxon/signrank rank tables). In this port that state is
//! explicit: the host runtime owns a [`MathState`] per session and passes it
//! through the sampler call chain, so concurrent sessions never share
//! sampler caches.

use std::collections::HashMap;

/// Per-session continuation state used by the nmath distribution samplers.
#[derive(Debug, Default)]
pub struct MathState {
    /// Signed-rank distribution memo table.
    pub signrank_cache: HashMap<i32, Vec<f64>>,
    /// Wilcoxon rank-sum distribution memo table.
    pub wilcox_cache: HashMap<(i32, i32), Vec<f64>>,
    /// Binomial sampler cache.
    pub binom_state: crate::dist::binomial::RbinomState,
    /// Poisson sampler cache.
    pub pois_state: crate::dist::poisson::RpoisState,
    /// Hypergeometric sampler cache.
    pub hyper_state: crate::dist::hypergeometric::RhyperState,
    /// Gamma sampler cache.
    pub gamma_state: crate::dist::gamma::GammaState,
    /// Beta sampler cache.
    pub beta_state: crate::dist::beta::BetaState,
}
