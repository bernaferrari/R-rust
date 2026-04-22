// Standalone RNG: Marsaglia-MultiCarry
// Ported from standalone/sunif.c

use std::cell::RefCell;

// Keep the seed state thread-local so parallel tests do not perturb one
// another. The previous process-global atomic state made seeded sequences
// depend on unrelated test ordering.
thread_local! {
    static RNG_STATE: RefCell<(u32, u32)> = const { RefCell::new((1234, 5678)) };
}

/// Set the RNG seed.
pub fn set_seed(i1: std::os::raw::c_uint, i2: std::os::raw::c_uint) {
    RNG_STATE.with(|state| {
        *state.borrow_mut() = (i1, i2);
    });
}

/// Get the current RNG seed.
pub fn get_seed(i1: *mut std::os::raw::c_uint, i2: *mut std::os::raw::c_uint) {
    if i1.is_null() || i2.is_null() {
        return;
    }
    RNG_STATE.with(|state| {
        let (seed_i1, seed_i2) = *state.borrow();
        unsafe {
            *i1 = seed_i1;
            *i2 = seed_i2;
        }
    });
}

/// Generate a uniform random number in [0, 1).
#[must_use]
/// This is a faithful port of the Marsaglia-MultiCarry generator.
pub fn unif_rand() -> f64 {
    RNG_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let (i1, i2) = *state;

        // Marsaglia-MultiCarry: uses 16-bit chunks
        // I1 = 36969 * (I1 & 0177777) + (I1 >> 16)
        // Note: 0177777 octal = 0xFFFF = 65535
        let new_i1 = 36969u32.wrapping_mul(i1 & 0xFFFF).wrapping_add(i1 >> 16);
        let new_i2 = 18000u32.wrapping_mul(i2 & 0xFFFF).wrapping_add(i2 >> 16);

        state.0 = new_i1;
        state.1 = new_i2;

        // ((I1 << 16) ^ (I2 & 0177777)) * 2.328306437080797e-10
        ((new_i1 << 16) ^ (new_i2 & 0xFFFF)) as f64 * 2.328306437080797e-10
    })
}

pub fn Rf_set_seed(i1: std::os::raw::c_uint, i2: std::os::raw::c_uint) {
    set_seed(i1, i2)
}

#[must_use]
pub fn Rf_unif_rand() -> f64 {
    unif_rand()
}
