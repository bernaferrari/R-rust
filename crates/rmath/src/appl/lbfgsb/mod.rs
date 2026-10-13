#![allow(
    clippy::self_assignment,
    clippy::never_loop,
    clippy::redundant_closure_call,
    clippy::needless_late_init
)]
#![allow(clippy::comparison_chain)]
#![allow(clippy::manual_memcpy, clippy::comparison_to_empty)]
// Port of R's src/appl/lbfgsb.c to Rust
//
// L-BFGS-B (version 2.3) - A limited memory algorithm for bound constrained optimization
//
// Original Fortran by Ciyou Zhu, in collaboration with R.H. Byrd, P. Lu-Chen and J. Nocedal.
// f2c translation, then hand-edited.
//
// Byrd, R. H., Lu, P., Nocedal, J. and Zhu, C. (1995) A limited
// memory algorithm for bound constrained optimization.
// SIAM J. Scientific Computing, 16, 1190--1208.

use libm::*;
use std::os::raw::{c_char, c_int};

use crate::sexp::instance::with_required_current_instance;

// =====================================================================
// Inline BLAS replacements
// =====================================================================

mod mainlb;
mod subsolvers;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

pub(crate) use mainlb::*;
pub(crate) use subsolvers::*;
