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
use crate::sexp::instance::with_required_current_instance;

#[inline]
pub(crate) fn global_env() -> SEXP {
    with_required_current_instance(|instance| unsafe { R_GlobalEnv_in(instance) })
}

#[inline]
pub(crate) fn base_env() -> SEXP {
    with_required_current_instance(|instance| unsafe { R_BaseEnv_in(instance) })
}

#[inline]
pub(crate) fn global_context() -> *mut RCNTXT {
    with_required_current_instance(|instance| unsafe { R_GlobalContext_in(instance) })
}

#[inline]
pub(crate) fn set_global_env(env: SEXP) {
    with_required_current_instance(|instance| unsafe { set_R_GlobalEnv_in(instance, env) });
}

#[inline]
pub(crate) fn visible() -> c_int {
    with_required_current_instance(|instance| unsafe { R_Visible_in(instance) })
}

#[inline]
pub(crate) fn set_visible(value: c_int) {
    with_required_current_instance(|instance| unsafe { set_R_Visible_in(instance, value) });
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
    saved: c_int,
}

impl VisibilityGuard {
    #[inline]
    pub(crate) fn new() -> Self {
        VisibilityGuard { saved: visible() }
    }
}

impl Drop for VisibilityGuard {
    fn drop(&mut self) {
        set_visible(self.saved);
    }
}
