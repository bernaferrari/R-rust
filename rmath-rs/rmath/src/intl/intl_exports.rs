//! Port of intl-exports.c -- Symbol exports for C ABI compatibility.
//!
//! The C version uses Cygwin-specific assembler directives to export
//! `libintl_version` as a data symbol. In our Rust port, we use
//! `#[unsafe(no_mangle)]` to achieve the same effect.

#![allow(non_snake_case)]

/// Version string for libintl. Exported for ABI compatibility with the
/// C library. This is a NUL-terminated byte string.
#[unsafe(no_mangle)]
pub static libintl_version: [u8; 5] = *b"0.21\0";
