#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Font management for GraphApp.
//!
//! Ported from fonts.c - font creation and measurement.

use std::os::raw::c_int;
use std::ptr;

use super::types::*;

pub static mut FixedFont: font = ptr::null_mut();
pub static mut SystemFont: font = ptr::null_mut();
pub static mut Times: font = ptr::null_mut();
pub static mut Helvetica: font = ptr::null_mut();
pub static mut Courier: font = ptr::null_mut();

pub unsafe fn init_fonts() { /* TODO */
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getSysFontSize() -> c_int {
    10
}
