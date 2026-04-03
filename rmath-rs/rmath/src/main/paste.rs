#![allow(
    unsafe_op_in_unsafe_fn,
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]

//! Port of R's src/main/paste.c
//!
//! The original C implementation provides:
//!   - do_paste()      -- .Internal(paste(...)) and .Internal(paste0(...))
//!   - do_filepath()   -- .Internal(filepath(...))
//!   - do_format()     -- .Internal(format(...))
//!   - do_formatinfo() -- .Internal(format.info(...))
//!
//! The full implementation lives in paste_impl.rs.  This module re-exports
//! the encoding constants and all the do_* functions for backward
//! compatibility and FFI linkage.
//!
//! Ported from r-source/src/main/paste.c

// Re-export encoding constants and standalone utilities from paste_impl.
pub use super::paste_impl::{
    CE_BYTES, CE_LATIN1, CE_NATIVE, CE_UTF8, MAXELTSIZE, R_stpcpy, do_filepath, do_format,
    do_formatinfo, do_paste,
};
