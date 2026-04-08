#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Font management for GraphApp.
//!
//! Ported from fonts.c - font creation and measurement.

use std::cell::Cell;
use std::os::raw::c_int;
use std::ptr;

use super::types::*;

pub thread_local! { static FixedFont: Cell<font> = Cell::new(ptr::null_mut()); }
pub thread_local! { static SystemFont: Cell<font> = Cell::new(ptr::null_mut()); }
pub thread_local! { static Times: Cell<font> = Cell::new(ptr::null_mut()); }
pub thread_local! { static Helvetica: Cell<font> = Cell::new(ptr::null_mut()); }
pub thread_local! { static Courier: Cell<font> = Cell::new(ptr::null_mut()); }

pub fn init_fonts() { /* TODO */
}

pub unsafe fn getSysFontSize() -> c_int {
    10
}
