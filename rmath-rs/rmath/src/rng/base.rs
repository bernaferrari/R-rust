// Standalone RNG: Marsaglia-MultiCarry
// Ported from standalone/sunif.c, with state owned by the active RInstance.

/// Set the RNG seed.
pub fn set_seed(i1: std::os::raw::c_uint, i2: std::os::raw::c_uint) {
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.rng_state = (i1, i2);
    });
}

/// Get the current RNG seed.
pub fn get_seed(i1: *mut std::os::raw::c_uint, i2: *mut std::os::raw::c_uint) {
    if i1.is_null() || i2.is_null() {
        return;
    }
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        *i1 = inst.rng_state.0;
        *i2 = inst.rng_state.1;
    });
}

/// Generate a uniform random number in [0, 1).
#[must_use]
/// This is a faithful port of the Marsaglia-MultiCarry generator.
pub fn unif_rand() -> f64 {
    crate::sexp::instance::with_required_current_instance(|inst| {
        let (i1, i2) = inst.rng_state;
        let new_i1 = 36969u32.wrapping_mul(i1 & 0xFFFF).wrapping_add(i1 >> 16);
        let new_i2 = 18000u32.wrapping_mul(i2 & 0xFFFF).wrapping_add(i2 >> 16);
        inst.rng_state = (new_i1, new_i2);
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
