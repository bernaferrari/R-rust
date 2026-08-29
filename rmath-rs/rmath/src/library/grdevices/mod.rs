//! grDevices package - graphics devices

pub(crate) mod axis_scales;
mod chull;
mod clippath;
pub(crate) mod colors;
#[cfg(not(target_os = "android"))]
mod devcairo;
pub(crate) mod device_registry;
mod devices;
#[cfg(not(target_os = "android"))]
mod devpictex;
pub(crate) mod devps;
#[cfg(not(target_os = "android"))]
mod devquartz;
#[cfg(not(target_os = "android"))]
pub(crate) mod devwindows;
mod group;
mod init;
mod mask;
mod patterns;
#[cfg(not(target_os = "android"))]
mod qdbitmap;
#[cfg(not(target_os = "android"))]
mod qdpdf;
mod stubs;
#[cfg(not(target_os = "android"))]
mod winbitmap;
