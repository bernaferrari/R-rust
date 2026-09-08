//! RNG state shared between the R-level stream and the nmath samplers.
//!
//! Ported from standalone/sunif.c (Marsaglia-MultiCarry). The host runtime
//! embeds an [`RngState`] alongside its [`MathState`](crate::state::MathState)
//! so `set_seed`/`unif_rand` and the distribution samplers advance one
//! session-owned stream.
//!
//! A host can also install [`set_unif_hook`] to route `unif_rand` through a
//! richer engine (R's full RNG dispatch with `.Random.seed` round-tripping);
//! the samplers then draw from that stream instead of MultiCarry.

use std::cell::{Cell, RefCell};

/// Marsaglia-MultiCarry generator state: two 32-bit seeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RngState(pub u32, pub u32);

impl Default for RngState {
    fn default() -> Self {
        RngState(1234, 5678)
    }
}

thread_local! {
    static CURRENT_RNG: RefCell<Option<*mut RngState>> = const { RefCell::new(None) };
}

/// Install `rng` as the current thread's RNG state.
///
/// # Safety
///
/// The caller must keep `rng` valid until [`detach_rng`] removes it or
/// another `install_rng` call replaces it.
pub unsafe fn install_rng(rng: *mut RngState) {
    CURRENT_RNG.with(|slot| *slot.borrow_mut() = Some(rng));
}

/// Install `rng` as this thread's current RNG, returning the previous
/// installation (if any) so callers can restore it when their scope ends.
///
/// # Safety
///
/// Same contract as [`install_rng`]; the returned pointer must only be
/// re-installed while still valid.
pub unsafe fn swap_rng(rng: Option<*mut RngState>) -> Option<*mut RngState> {
    CURRENT_RNG.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), rng))
}

/// Remove the current thread's RNG state if it matches `rng`.
pub fn detach_rng(rng: *const RngState) -> bool {
    CURRENT_RNG.with(|slot| {
        let mut cur = slot.borrow_mut();
        match *cur {
            Some(ptr) if std::ptr::eq(ptr, rng as *mut RngState) => {
                *cur = None;
                true
            }
            _ => false,
        }
    })
}

/// Set the RNG seed.
pub fn set_seed(i1: std::os::raw::c_uint, i2: std::os::raw::c_uint) {
    let installed = CURRENT_RNG.with(|slot| *slot.borrow());
    if let Some(ptr) = installed {
        unsafe {
            (*ptr).0 = i1;
            (*ptr).1 = i2;
        }
    } else {
        DEFAULT_RNG.with(|cell| cell.set(RngState(i1, i2)));
    }
}

/// Get the current RNG seed.
pub fn get_seed(i1: *mut std::os::raw::c_uint, i2: *mut std::os::raw::c_uint) {
    if i1.is_null() || i2.is_null() {
        return;
    }
    let installed = CURRENT_RNG.with(|slot| *slot.borrow());
    let seed = if let Some(ptr) = installed {
        unsafe { *ptr }
    } else {
        DEFAULT_RNG.with(|cell| cell.get())
    };
    unsafe {
        *i1 = seed.0;
        *i2 = seed.1;
    }
}

/// Host-supplied uniform generator hook.
///
/// When installed, [`unif_rand`] delegates to it instead of advancing the
/// Marsaglia-MultiCarry state. The R-level runtime installs a bridge to the
/// full RNG dispatch (all RNG kinds, `.Random.seed` round-tripping) so the
/// nmath samplers draw from the same session stream as `runif()`.
pub type UnifRandHook = fn() -> f64;

thread_local! {
    static UNIF_HOOK: Cell<Option<UnifRandHook>> = const { Cell::new(None) };
}

/// Install (or clear, with `None`) this thread's uniform generator hook.
pub fn set_unif_hook(hook: Option<UnifRandHook>) {
    UNIF_HOOK.with(|slot| slot.set(hook));
}

/// Generate a uniform random number in [0, 1).
///
/// Delegates to the installed host hook when present; otherwise advances the
/// built-in Marsaglia-MultiCarry stream.
#[must_use]
pub fn unif_rand() -> f64 {
    if let Some(hook) = UNIF_HOOK.with(Cell::get) {
        return hook();
    }
    multicarry_unif_rand()
}

/// Different kinds of "Bin(n,p)" generators (R_ext/Random.h's `Binomtype`).
///
/// `BTPE` is the corrected algorithm (PR#19049 fixes two signs in the
/// Stirling squeeze terms and sharpens the 1/6 constant); `BUGGY_BTPE`
/// keeps the pre-2026 behavior so old `.Random.seed` streams can be
/// replayed.
#[allow(non_camel_case_types)] // mirror R_ext/Random.h's Binomtype spelling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Binomtype {
    BUGGY_BTPE = 0,
    BTPE = 1,
}

// Standalone default for the binomial generator kind (rbinom.c's
// `ML_Binom_kind`, default `BTPE`).
thread_local! {
    static ML_BINOM_KIND: Cell<Binomtype> = const { Cell::new(Binomtype::BTPE) };
}

/// Host-supplied binomial generator kind hook.
///
/// When installed, [`R_binom_kind`] asks it for the kind instead of the
/// standalone default. The R-level runtime installs a bridge reading the
/// session's `RNGkind(binom.kind=)` setting so `rbinom` follows the
/// session RNG configuration, mirroring the [`UnifRandHook`] bridge.
pub type BinomKindHook = fn() -> Binomtype;

thread_local! {
    static BINOM_KIND_HOOK: Cell<Option<BinomKindHook>> = const { Cell::new(None) };
}

/// Install (or clear, with `None`) this thread's binomial-kind hook.
pub fn set_binom_kind_hook(hook: Option<BinomKindHook>) {
    BINOM_KIND_HOOK.with(|slot| slot.set(hook));
}

/// Set the standalone binomial generator kind (rbinom.c's `ML_Binom_kind`).
pub fn set_binom_kind(kind: Binomtype) {
    ML_BINOM_KIND.with(|slot| slot.set(kind));
}

/// The binomial generator kind `rbinom` should use (rbinom.c's
/// `R_binom_kind()`): the installed host hook's session kind when present,
/// otherwise the standalone default.
#[must_use]
pub fn R_binom_kind() -> Binomtype {
    if let Some(hook) = BINOM_KIND_HOOK.with(Cell::get) {
        return hook();
    }
    ML_BINOM_KIND.with(Cell::get)
}

/// The built-in Marsaglia-MultiCarry stream, ignoring any installed hook.
///
/// Host bridges use this as the fallback when no R instance is active.
#[must_use]
pub fn multicarry_unif_rand() -> f64 {
    let installed = CURRENT_RNG.with(|slot| *slot.borrow());
    let step = |state: &mut RngState| -> f64 {
        let (mut i1, mut i2) = (state.0, state.1);
        i1 = 36969u32.wrapping_mul(i1 & 0xFFFF).wrapping_add(i1 >> 16);
        i2 = 18000u32.wrapping_mul(i2 & 0xFFFF).wrapping_add(i2 >> 16);
        state.0 = i1;
        state.1 = i2;
        ((i1 << 16) ^ (i2 & 0xFFFF)) as f64 * 2.328306437080797e-10
    };
    if let Some(ptr) = installed {
        unsafe { step(&mut *ptr) }
    } else {
        let mut local = DEFAULT_RNG.with(Cell::get);
        let value = step(&mut local);
        DEFAULT_RNG.with(|cell| cell.set(local));
        value
    }
}

pub fn Rf_set_seed(i1: std::os::raw::c_uint, i2: std::os::raw::c_uint) {
    set_seed(i1, i2)
}

#[must_use]
pub fn Rf_unif_rand() -> f64 {
    unif_rand()
}

thread_local! {
    static DEFAULT_RNG: Cell<RngState> = Cell::new(RngState::default());
}
