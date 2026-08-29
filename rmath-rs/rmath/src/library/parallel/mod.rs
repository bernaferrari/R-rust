//! Parallel package - parallel support

#[cfg(not(target_os = "android"))]
pub(crate) mod fork;
mod init;
mod ncpus;
mod rngstream;
