// Standalone RNG facade for nmath.
// Keep nmath's RNG entrypoints on the same session-owned state as crate::rng.

/// Set the RNG seed.
pub fn set_seed(i1: std::os::raw::c_uint, i2: std::os::raw::c_uint) {
    crate::rng::set_seed(i1, i2)
}

/// Get the current RNG seed.
pub fn get_seed(i1: *mut std::os::raw::c_uint, i2: *mut std::os::raw::c_uint) {
    crate::rng::get_seed(i1, i2)
}

/// Generate a uniform random number in [0, 1).
pub fn unif_rand() -> f64 {
    crate::rng::unif_rand()
}

pub fn Rf_set_seed(i1: std::os::raw::c_uint, i2: std::os::raw::c_uint) {
    set_seed(i1, i2)
}

pub fn Rf_unif_rand() -> f64 {
    unif_rand()
}
