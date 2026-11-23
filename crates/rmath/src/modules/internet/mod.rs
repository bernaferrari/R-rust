//! Internet module for sockets and downloads.

#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod internet;
#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod rsock;
#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
#[cfg(not(target_arch = "wasm32"))]
mod sock;
