//! LAPACK module - linear algebra FFI wrappers
//!
//! Contains:
//! - `lapack`: LAPACK/BLAS Fortran FFI declarations and helper functions
//! - `lapack_impl`: Exported stubs for LAPACK wrapper functions (deferred to Phase 3)
//! - `accelerate`: macOS Accelerate framework integration (stub)
//! - `init_win`: Windows-only stdio mode initialization
//! - `veclib_g95c`: macOS vecLib CBLAS compatibility wrappers (Fortran-callable)

#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
mod accelerate;
#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
mod init_win;
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
mod lapack_impl;
#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
mod veclib_g95c;
