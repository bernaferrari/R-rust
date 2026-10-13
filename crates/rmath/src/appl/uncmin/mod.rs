#![allow(clippy::self_assignment, clippy::assigning_clones)]
#![allow(clippy::comparison_chain)]
#![allow(unused_variables, clippy::manual_memcpy, clippy::comparison_to_empty)]
#![allow(unused_assignments)]
// Ported from R's appl/uncmin.c
//
// Dennis-Schnabel minimizer, used by R's nlm().
//
// ../appl/uncmin.f -- translated by f2c, hand edited by Saikat DebRoy.
// Contains: fdhess, optif9, and all supporting routines.
use crate::utils::{fmax2, fmin2};

// =====================================================================
// Math helpers: GNU uncmin.c calls the system libm (pow via Rexp10, hypot,
// log10). Rust std routes f64::powf / f64::hypot / f64::log10 to the same
// system libm, whereas the pure-Rust `libm` crate need not be bit-identical,
// and nlm() results are sensitive to the last ulp of these.
// =====================================================================

mod driver;
mod support;
#[cfg(test)]
mod tests;

pub use driver::*;
pub use support::*;
