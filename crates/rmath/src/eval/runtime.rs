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
    instance: *mut crate::sexp::instance::RInstance,
    pin: Option<crate::sexp::owner::OwnerPin>,
    availability: crate::sexp::instance::InstanceLiveness,
}

impl VisibilityGuard {
    #[inline]
    pub(crate) fn new() -> Self {
        with_required_current_instance(|instance| unsafe {
            let pin = (*instance)
                .runtime_owner
                .as_ref()
                .map(|owner| owner.pin().expect("live visibility owner"));
            let availability = crate::sexp::instance::instance_liveness(instance);
            VisibilityGuard {
                saved: R_Visible_in(instance),
                instance,
                pin,
                availability,
            }
        })
    }
}

impl Drop for VisibilityGuard {
    fn drop(&mut self) {
        let instance = match &self.pin {
            Some(pin) => pin.as_ptr(),
            None if self.availability.is_live() => self.instance,
            None => return,
        };
        // Cleanup belongs to the original pinned owner even after revocation,
        // and must not dispatch through a newer or absent ambient runtime.
        unsafe {
            set_R_Visible_in(instance, self.saved);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn owned_visibility_guard_restores_original_closed_owner_during_unwind() {
        let mut session = RSession::new_for_gc_tests();
        let guard = VisibilityGuard::new();
        let original = guard.instance;
        let inspection_pin = unsafe { (*original).runtime_owner.clone() }.unwrap().pin().unwrap();
        let saved = guard.saved;
        set_visible(1 - saved);
        session.close();
        drop(session);
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _guard = guard;
            panic!("visibility unwind probe");
        }));
        assert!(result.is_err());
        assert_eq!(unsafe { R_Visible_in(inspection_pin.as_ptr()) }, saved);
        assert!(!crate::sexp::instance::has_current_instance());
    }
}
