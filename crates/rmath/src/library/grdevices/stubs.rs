/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/stubs.c
 *
 *  Stub wrappers and devAskNewPage.
 */

use crate::main::errors::Rf_error_unimplemented;
use crate::sexp::accessors::{CDR, SET_STRING_ELT};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::protect::protect;

use super::colors::mk_char_ptr;

fn unsupported(name: &str) -> ! {
    Rf_error_unimplemented(name);
    unreachable!("Rf_error_unimplemented returned");
}

fn tail(args: SEXP) -> SEXP {
    CDR(args)
}

fn do_contourLines(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsupported("grDevices::contourLines")
}

fn do_getSnapshot(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsupported("grDevices::getSnapshot")
}

fn do_playSnapshot(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsupported("grDevices::playSnapshot")
}

fn do_getGraphicsEvent(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    crate::main::essentials::do_getGraphicsEvent(call, op, args, env)
}

fn do_getGraphicsEventEnv(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsupported("grDevices::getGraphicsEventEnv")
}

fn do_setGraphicsEventEnv(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsupported("grDevices::setGraphicsEventEnv")
}

fn do_bmVersion() -> SEXP {
    {
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, 3);
        let _ans_guard = protect(ans);
        let nms = Rf_allocVector(SEXPTYPE::STRSXP, 3);
        let _nms_guard = protect(nms);

        SET_STRING_ELT(
            nms,
            0 as R_xlen_t,
            crate::sexp::constructors::mk_char_str("libpng"),
        );
        SET_STRING_ELT(
            nms,
            1 as R_xlen_t,
            crate::sexp::constructors::mk_char_str("jpeg"),
        );
        SET_STRING_ELT(
            nms,
            2 as R_xlen_t,
            crate::sexp::constructors::mk_char_str("libtiff"),
        );

        SET_STRING_ELT(
            ans,
            0 as R_xlen_t,
            crate::sexp::constructors::mk_char_str(""),
        );
        SET_STRING_ELT(
            ans,
            1 as R_xlen_t,
            crate::sexp::constructors::mk_char_str(""),
        );
        SET_STRING_ELT(
            ans,
            2 as R_xlen_t,
            crate::sexp::constructors::mk_char_str(""),
        );
        crate::attrib_core::setAttrib(ans, crate::attrib_core::R_NamesSymbol(), nms);

        ans
    }
}

/// contourLines - wrapper for do_contourLines.
pub fn contourLines(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, args, env);
    do_contourLines(call, op, args, env)
}

/// getSnapshot - wrapper for do_getSnapshot.
pub fn getSnapshot(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, args, env);
    do_getSnapshot(call, op, args, env)
}

/// playSnapshot - wrapper for do_playSnapshot.
pub fn playSnapshot(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, args, env);
    do_playSnapshot(call, op, args, env)
}

/// getGraphicsEvent - wrapper for do_getGraphicsEvent.
pub fn getGraphicsEvent(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    do_getGraphicsEvent(call, op, tail(args), env)
}

/// getGraphicsEventEnv - wrapper for do_getGraphicsEventEnv.
pub fn getGraphicsEventEnv(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, args, env);
    do_getGraphicsEventEnv(call, op, args, env)
}

/// setGraphicsEventEnv - wrapper for do_setGraphicsEventEnv.
pub fn setGraphicsEventEnv(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, args, env);
    do_setGraphicsEventEnv(call, op, args, env)
}

/// bmVersion - wrapper for do_bmVersion.
pub(crate) fn bmVersion() -> SEXP {
    do_bmVersion()
}

/// devAskNewPage - get/set the "ask new page" flag for the current device.
pub fn devAskNewPage(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, env);
    {
        crate::sexp::owner::OwnerToken::with_current(|owner| {
            let arguments = owner
                .sexp(args)
                .and_then(crate::sexp::object::Sexp::into_owned)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let authority = crate::sexp::owner::StoredOwner::from_value(&arguments)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let _pin = owner
                .pin()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let value = arguments
                .try_cdr()
                .and_then(|tail| tail.try_car())
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let ask = if value.is_nil() {
                None
            } else {
                let logical = crate::mainutils::coerce::asLogical(value.as_raw());
                authority
                    .require_active()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                if logical == crate::sexp::ffi::NA_LOGICAL {
                    crate::sexp::context::r_error("invalid 'ask' argument");
                }
                Some(logical != 0)
            };
            let old = super::device_registry::page_question(ask);
            authority
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let result = crate::sexp::constructors::Rf_ScalarLogical(old as std::os::raw::c_int);
            crate::sexp::globals::set_R_Visible(ask.is_none() as std::os::raw::c_int);
            result
        })
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
    }
}
