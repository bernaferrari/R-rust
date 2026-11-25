//! LAPACK-shaped pure-Rust linear algebra
//!
//! Contains:
//! - `lapack`: shared LAPACK validation and conversion helpers
//! - `backend`: pure-Rust faer implementations behind LAPACK-shaped operations
//! - `lapack_impl`: exported stubs for LAPACK wrapper functions

#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
pub(crate) mod backend;
#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
mod lapack;
#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
pub(crate) mod lapack_impl;
mod workspace_size;

#[cfg(test)]
mod backend_tests;

#[cfg(test)]
mod complex_qr_tests;

#[cfg(test)]
mod real_qr_tests;

#[cfg(test)]
mod real_chol_tests;

#[cfg(test)]
mod real_chol2inv_tests;

#[cfg(test)]
mod real_solve_tests;

#[cfg(test)]
mod complex_solve_tests;

#[cfg(test)]
mod real_svd_tests;

#[cfg(test)]
mod complex_svd_tests;

#[cfg(test)]
mod real_dlange_tests;

#[cfg(test)]
mod real_dgecon_tests;

#[cfg(test)]
mod real_dtrcon_tests;

#[cfg(test)]
mod complex_dlange_tests;

#[cfg(test)]
mod complex_dgecon_tests;

#[cfg(test)]
mod complex_ztrcon_tests;

#[cfg(test)]
mod complex_rs_tests;

#[cfg(test)]
mod complex_rg_tests;

#[cfg(test)]
mod real_qr_coef_tests;

#[cfg(test)]
mod real_qr_qy_tests;

#[cfg(test)]
mod complex_qr_coef_tests;

#[cfg(test)]
mod complex_qr_qy_tests;

#[cfg(test)]
mod real_det_ge_tests;
