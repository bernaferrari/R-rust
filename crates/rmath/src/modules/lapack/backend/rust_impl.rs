/*
 * Pure Rust LAPACK backend using faer 0.24.
 *
 * Provides the same function signatures as the Fortran FFI in lapack.rs,
 * dispatching to faer-rs for linear algebra computations.
 * Enabled via the `rust-backend` feature flag.
 */

use crate::modules::lapack::lapack::Rcomplex;
use crate::sexp::instance::with_current_instance;
use crate::sexp::memory::{TransientReservation, with_arena_in};
use faer::linalg::solvers::{DenseSolveCore, Solve};
use faer::{Mat, MatRef, Side, c64};
// ============================================================
// Helper functions
// ============================================================

mod chol;
mod cond;
/// Reserve the temporary buffers used by a numerical kernel when an R
/// session is active. Standalone low-level LAPACK tests intentionally run
/// without an ambient session and therefore retain the historical unlimited
mod helpers;
mod norm;
mod qr;
mod solve_lu;
mod svd_eigen;
pub use chol::*;
pub use cond::*;
pub use helpers::*;
pub use norm::*;
pub use qr::*;
pub use solve_lu::*;
pub use svd_eigen::*;
