//! Parallel package - parallel support

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
pub(crate) mod fork;
mod init;
mod ncpus;
mod rngstream;
