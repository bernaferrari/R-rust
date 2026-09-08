//! Port of osdep.c -- OS-dependent initialization.
//!
//! The C version conditionally includes platform-specific files:
//! - On Cygwin: includes intl-exports.c
//! - On OS/2: includes os2compat.c
//! - Otherwise: a typedef int dummy to avoid compiler warnings.
//!
//! In our Rust port, this is a no-op module. Platform-specific
//! exports are handled in intl_exports.rs.

#![allow(non_snake_case)]
