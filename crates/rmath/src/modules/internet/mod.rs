//! Internet module - socket, HTTP, libcurl support

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
pub(crate) mod libcurl;
#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
#[cfg(not(target_arch = "wasm32"))]
mod libcurl_wrap;
#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod rhttpd;
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
#[allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
#[cfg(not(target_arch = "wasm32"))]
mod sockconn;
