//! grDevices package - graphics devices

pub(crate) mod axis_scales;
mod chull;
mod clippath;
pub(crate) mod colors;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod devcairo;
pub(crate) mod device_registry;
mod devices;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod devpictex;
pub(crate) mod devps;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod devquartz;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
pub(crate) mod devwindows;
mod group;
mod init;
mod mask;
mod patterns;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod qdbitmap;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod qdpdf;
mod stubs;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod winbitmap;
