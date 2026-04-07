// Standalone RNG: Marsaglia-MultiCarry
// Ported from standalone/sunif.c

use std::sync::atomic::{AtomicU32, Ordering};

// RNG state: use atomic for thread safety (matching C's static globals)
static I1: AtomicU32 = AtomicU32::new(1234);
static I2: AtomicU32 = AtomicU32::new(5678);

/// Set the RNG seed.
#[unsafe(no_mangle)]
pub extern "C" fn set_seed(i1: std::os::raw::c_uint, i2: std::os::raw::c_uint) {
    I1.store(i1, Ordering::SeqCst);
    I2.store(i2, Ordering::SeqCst);
}

/// Get the current RNG seed.
#[unsafe(no_mangle)]
pub extern "C" fn get_seed(i1: *mut std::os::raw::c_uint, i2: *mut std::os::raw::c_uint) {
    if i1.is_null() || i2.is_null() {
        return;
    }
    unsafe {
        *i1 = I1.load(Ordering::SeqCst);
        *i2 = I2.load(Ordering::SeqCst);
    }
}

/// Generate a uniform random number in [0, 1).
#[must_use]
/// This is a faithful port of the Marsaglia-MultiCarry generator.
#[unsafe(no_mangle)]
pub extern "C" fn unif_rand() -> f64 {
    let i1 = I1.load(Ordering::SeqCst);
    let i2 = I2.load(Ordering::SeqCst);

    // Marsaglia-MultiCarry: uses 16-bit chunks
    // I1 = 36969 * (I1 & 0177777) + (I1 >> 16)
    // Note: 0177777 octal = 0xFFFF = 65535
    let new_i1 = 36969u32.wrapping_mul(i1 & 0xFFFF).wrapping_add(i1 >> 16);
    let new_i2 = 18000u32.wrapping_mul(i2 & 0xFFFF).wrapping_add(i2 >> 16);

    I1.store(new_i1, Ordering::SeqCst);
    I2.store(new_i2, Ordering::SeqCst);

    // ((I1 << 16) ^ (I2 & 0177777)) * 2.328306437080797e-10
    let result = ((new_i1 << 16) ^ (new_i2 & 0xFFFF)) as f64 * 2.328306437080797e-10;
    result
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_set_seed(i1: std::os::raw::c_uint, i2: std::os::raw::c_uint) {
    set_seed(i1, i2)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn Rf_unif_rand() -> f64 {
    unif_rand()
}
