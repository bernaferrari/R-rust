#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Drawing context management for GraphApp.
//!
//! Ported from context.c - internal functions for manipulating device contexts.

use std::os::raw::c_void;
use std::ptr;

use super::types::*;

pub unsafe fn init_contexts() { /* TODO */
}
pub unsafe fn finish_contexts() { /* TODO */
}
pub unsafe fn add_context(obj: object, dc: *mut c_void, old: *mut c_void) { /* TODO */
}
pub unsafe fn get_context(obj: object) -> *mut c_void {
    ptr::null_mut()
}
pub unsafe fn remove_context(obj: object) { /* TODO */
}
pub unsafe fn del_context(obj: object) { /* TODO */
}
pub unsafe fn del_all_contexts() { /* TODO */
}
pub unsafe fn fix_brush(dc: *mut c_void, obj: drawing, brush: *mut c_void) { /* TODO */
}
