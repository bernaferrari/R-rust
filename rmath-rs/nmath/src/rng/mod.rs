//! RNG state shared between the R-level stream and the nmath samplers.
//!
//! Ported from standalone/sunif.c (Marsaglia-MultiCarry). The host runtime
//! embeds an [`RngState`] alongside its [`MathState`](crate::state::MathState)
//! so `set_seed`/`unif_rand` and the distribution samplers advance one
//! session-owned stream.

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

/// Generate a uniform random number in [0, 1).
///
/// Faithful port of the Marsaglia-MultiCarry generator.
#[must_use]
pub fn unif_rand() -> f64 {
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
