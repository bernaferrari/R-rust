#![allow(non_snake_case, dead_code)]

//! Owner-bound evaluator runtime access.
//!
//! The evaluator still mirrors GNU R's C structure in many places, but mutable
//! session state lives on `RInstance`. This module is the narrow bridge for
//! translated evaluator code that needs the active session's environments or
//! visibility flag without reaching through ambient global wrappers directly.

use std::os::raw::c_int;

use crate::sexp::context::{R_GlobalContext_in, RCNTXT};
use crate::sexp::ffi::SEXP;
use crate::sexp::globals::{
    R_BaseEnv_in, R_GlobalEnv_in, R_Visible_in, set_R_GlobalEnv_in, set_R_Visible_in,
};
use crate::sexp::instance::{RInstance, current_instance_ptr};

#[inline]
fn active_instance_ptr() -> *mut RInstance {
    current_instance_ptr().expect("evaluator state requires an active RInstance")
}

#[inline]
pub(crate) fn global_env() -> SEXP {
    unsafe { R_GlobalEnv_in(&mut *active_instance_ptr()) }
}

#[inline]
pub(crate) fn base_env() -> SEXP {
    unsafe { R_BaseEnv_in(&mut *active_instance_ptr()) }
}

#[inline]
pub(crate) fn global_context() -> *mut RCNTXT {
    unsafe { R_GlobalContext_in(&mut *active_instance_ptr()) }
}

#[inline]
pub(crate) fn set_global_env(env: SEXP) {
    unsafe { set_R_GlobalEnv_in(&mut *active_instance_ptr(), env) }
}

#[inline]
pub(crate) fn visible() -> c_int {
    unsafe { R_Visible_in(&mut *active_instance_ptr()) }
}

#[inline]
pub(crate) fn set_visible(value: c_int) {
    unsafe { set_R_Visible_in(&mut *active_instance_ptr(), value) }
}

#[inline]
pub(crate) fn set_visible_for_print_flag(flag: c_int) {
    set_visible(if flag != 1 {
        crate::sexp::ffi::TRUE
    } else {
        crate::sexp::ffi::FALSE
    });
}

#[must_use]
pub(crate) struct VisibilityGuard {
    instance: *mut RInstance,
    saved: c_int,
}

impl VisibilityGuard {
    #[inline]
    pub(crate) fn new() -> Self {
        let instance = active_instance_ptr();
        let saved = unsafe { R_Visible_in(&mut *instance) };
        VisibilityGuard { instance, saved }
    }
}

impl Drop for VisibilityGuard {
    fn drop(&mut self) {
        unsafe {
            set_R_Visible_in(&mut *self.instance, self.saved);
        }
    }
}
