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
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
pub(crate) mod x11;
