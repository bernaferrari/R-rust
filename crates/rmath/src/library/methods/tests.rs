/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/tests.c
 *
 *  methods package test utilities.
 */

use std::os::raw::c_void;

use crate::mainutils::memory_main::{R_ExternalPtrAddr, R_MakeExternalPtr};
use crate::sexp::context::RError;
use crate::sexp::ffi::*;
use crate::sexp::globals::R_NilValue;

/// R_dummy_extern_place - placeholder for external pointer initializers.
pub unsafe fn R_dummy_extern_place() -> SEXP {
    std::panic::panic_any(RError {
        message: "calling the C routine used as an initializer for 'externalptr' objects"
            .to_string(),
    });
}

/// R_externalptr_prototype_object - create the prototype for externalptr objects.
pub unsafe fn R_externalptr_prototype_object() -> SEXP {
    unsafe {
        R_MakeExternalPtr(
            R_dummy_extern_place as *mut c_void,
            R_NilValue(),
            R_NilValue(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::accessors::TYPEOF;

    fn assert_r_error(action: impl FnOnce()) -> String {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action))
            .expect_err("expected RError panic");
        payload
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("expected RError payload")
            .message
            .clone()
    }

    #[test]
    fn externalptr_prototype_contains_dummy_initializer() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let ptr = R_externalptr_prototype_object();

            assert_eq!(TYPEOF(ptr), SEXPTYPE::EXTPTRSXP);
            assert_eq!(R_ExternalPtrAddr(ptr), R_dummy_extern_place as *mut c_void);
        }
    }

    #[test]
    fn dummy_external_initializer_errors_like_r() {
        let _session = crate::sexp::session::RSession::new();
        let message = assert_r_error(|| unsafe {
            R_dummy_extern_place();
        });
        assert!(message.contains("initializer for 'externalptr'"));
    }
}
