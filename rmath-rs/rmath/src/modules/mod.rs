//! R modules (internet, lapack, X11)

pub(crate) mod internet;
#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
pub mod lapack;
#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
#[cfg(not(target_os = "android"))]
pub(crate) mod x11;
