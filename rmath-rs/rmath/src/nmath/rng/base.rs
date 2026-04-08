// Standalone RNG: Marsaglia-MultiCarry
// Ported from standalone/sunif.c
//
// Uses thread-local state to ensure thread safety without serialization.
// Each thread gets its own independent RNG sequence.

use std::cell::Cell;

thread_local! {
    static RNG_I1: Cell<u32> = Cell::new(1234);
    static RNG_I2: Cell<u32> = Cell::new(5678);
}

/// Set the RNG seed.
pub extern "C" fn set_seed(i1: std::os::raw::c_uint, i2: std::os::raw::c_uint) {
    RNG_I1.with(|c| c.set(i1));
    RNG_I2.with(|c| c.set(i2));
}

/// Get the current RNG seed.
pub extern "C" fn get_seed(i1: *mut std::os::raw::c_uint, i2: *mut std::os::raw::c_uint) {
    if i1.is_null() || i2.is_null() {
        return;
    }
    unsafe {
        *i1 = RNG_I1.with(|c| c.get());
        *i2 = RNG_I2.with(|c| c.get());
    }
}

/// Generate a uniform random number in [0, 1).
/// This is a faithful port of the Marsaglia-MultiCarry generator.
///
/// Thread safety: uses thread-local state, so concurrent calls from different
/// threads produce independent sequences without serialization.
pub extern "C" fn unif_rand() -> f64 {
    RNG_I1.with(|i1_cell| {
        RNG_I2.with(|i2_cell| {
            let i1 = i1_cell.get();
            let i2 = i2_cell.get();

            // Marsaglia-MultiCarry: uses 16-bit chunks
            // I1 = 36969 * (I1 & 0177777) + (I1 >> 16)
            // Note: 0177777 octal = 0xFFFF = 65535
            let new_i1 = 36969u32.wrapping_mul(i1 & 0xFFFF).wrapping_add(i1 >> 16);
            let new_i2 = 18000u32.wrapping_mul(i2 & 0xFFFF).wrapping_add(i2 >> 16);

            i1_cell.set(new_i1);
            i2_cell.set(new_i2);

            // ((I1 << 16) ^ (I2 & 0177777)) * 2.328306437080797e-10
            ((new_i1 << 16) ^ (new_i2 & 0xFFFF)) as f64 * 2.328306437080797e-10
        })
    })
}

pub extern "C" fn Rf_set_seed(i1: std::os::raw::c_uint, i2: std::os::raw::c_uint) {
    set_seed(i1, i2)
}

pub extern "C" fn Rf_unif_rand() -> f64 {
    unif_rand()
}
