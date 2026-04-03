// Random number generation
//
// This module contains random number generators for various distributions.

/// Basic uniform random number generator trait
pub trait Rng {
    /// Generate a uniform random number in [0, 1)
    fn unif_rand(&mut self) -> f64;
}

/// Generate standard normal random variable (Box-Muller)
pub fn norm_rand<R: Rng>(rng: &mut R) -> f64 {
    let u1 = rng.unif_rand();
    let u2 = rng.unif_rand();
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

/// Generate exponential random variable
pub fn exp_rand<R: Rng>(rng: &mut R) -> f64 {
    -rng.unif_rand().ln()
}

// Stub implementations for distribution-specific random number generators
pub fn rnorm<R: Rng>(_rng: &mut R, _mu: f64, _sigma: f64) -> f64 {
    0.0 // TODO: Implement
}

pub fn rgamma<R: Rng>(_rng: &mut R, _shape: f64, _scale: f64) -> f64 {
    0.0 // TODO: Implement
}

pub fn rbeta<R: Rng>(_rng: &mut R, _a: f64, _b: f64) -> f64 {
    0.0 // TODO: Implement
}
