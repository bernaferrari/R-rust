//! Session-owned math state for the distribution samplers.
//!
//! R's nmath routines carry continuation caches (beta/gamma sampler memo
//! tables, the Wilcoxon/signrank rank tables). In this port that state is
//! session-owned: the host runtime (e.g. `rmath::sexp`) embeds a
//! [`MathState`] in its per-session instance and installs it for the
//! current thread, so concurrent sessions never share sampler caches.
//!
//! Standalone consumers (a pure `libRmath.a`-style build) install their own
//! state with [`install_state`], or fall back to a per-thread default.

use std::cell::RefCell;
use std::collections::HashMap;

/// Per-session continuation state used by the nmath distribution samplers.
#[derive(Default)]
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

thread_local! {
    static CURRENT_STATE: RefCell<Option<*mut MathState>> = const { RefCell::new(None) };
    static DEFAULT_STATE: RefCell<MathState> = RefCell::new(MathState::default());
}

/// Install `state` as the current thread's math state.
///
/// # Safety
///
/// The caller must keep `state` valid (and not mutably aliased elsewhere)
/// until [`detach_state`] removes it, or until another `install_state` call
/// replaces it.
///
/// Replace the current math state, returning the previously installed one.
pub fn replace_state(new: *mut MathState) -> Option<*mut MathState> {
    CURRENT_STATE.with(|slot| slot.borrow_mut().replace(new))
}

/// Replace the current RNG state, returning the previously installed one.
pub fn replace_rng(new: *mut crate::rng::RngState) -> Option<*mut crate::rng::RngState> {
    CURRENT_RNG.with(|slot| slot.borrow_mut().replace(new))
}

thread_local! {
    static CURRENT_RNG: RefCell<Option<*mut crate::rng::RngState>> =
        const { RefCell::new(None) };
}

/// Restore a previously installed math state (or none) returned by
/// [`replace_state`].
pub fn restore_state(previous: Option<*mut MathState>) {
    CURRENT_STATE.with(|slot| *slot.borrow_mut() = previous);
}

/// Restore a previously installed RNG state (or none) returned by
/// [`replace_rng`].
pub fn restore_rng(previous: Option<*mut crate::rng::RngState>) {
    CURRENT_RNG.with(|slot| *slot.borrow_mut() = previous);
}

pub unsafe fn install_state(state: *mut MathState) {
    CURRENT_STATE.with(|slot| *slot.borrow_mut() = Some(state));
}

/// Remove the current thread's math state if it matches `state`.
///
/// Returns `true` when the pointer matched and was cleared.
pub fn detach_state(state: *const MathState) -> bool {
    CURRENT_STATE.with(|slot| {
        let mut cur = slot.borrow_mut();
        match *cur {
            Some(ptr) if std::ptr::eq(ptr, state as *mut MathState) => {
                *cur = None;
                true
            }
            _ => false,
        }
    })
}

/// Run `f` with the installed math state, installing `fallback` first if no
/// state is present. The fallback borrow lasts only for this call.
pub fn with_state_or<F, R>(fallback: &mut MathState, f: F) -> R
where
    F: FnOnce(&mut MathState) -> R,
{
    let installed = CURRENT_STATE.with(|slot| *slot.borrow());
    match installed {
        Some(ptr) => unsafe { f(&mut *ptr) },
        None => f(fallback),
    }
}

/// Execute a closure with the current math state.
///
/// A missing installed state indicates an unscoped standalone entrypoint;
/// use [`with_state_or`] from hosts that do not always install state.
pub fn with_required_current_instance<F, R>(f: F) -> R
where
    F: FnOnce(&mut MathState) -> R,
{
    match CURRENT_STATE.with(|slot| *slot.borrow()) {
        Some(ptr) => unsafe { f(&mut *ptr) },
        None => DEFAULT_STATE.with(|state| f(&mut state.borrow_mut())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_state_persists_between_calls() {
        with_required_current_instance(|state| {
            state.signrank_cache.insert(3, vec![1.5]);
        });

        let cached_value = with_required_current_instance(|state| state.signrank_cache[&3][0]);

        assert_eq!(cached_value, 1.5);
    }
}
