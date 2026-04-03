#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Cursor management for GraphApp.

use std::ptr;

use super::types::*;

pub static mut ArrowCursor: cursor = ptr::null_mut();
pub static mut BlankCursor: cursor = ptr::null_mut();
pub static mut WatchCursor: cursor = ptr::null_mut();
pub static mut CaretCursor: cursor = ptr::null_mut();
pub static mut TextCursor: cursor = ptr::null_mut();
pub static mut HandCursor: cursor = ptr::null_mut();
pub static mut CrossCursor: cursor = ptr::null_mut();

pub unsafe fn init_cursors() { /* TODO */
}
