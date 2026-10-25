//! Native adapter for the checked UTF-8 string-vector kernels.
use super::vector_text_owned::{self, Operation};
use crate::sexp::{
    ffi::{SEXP, SEXPTYPE},
    object::Sexp,
    owner::{OwnerToken, with_runtime},
};
pub(super) unsafe fn evaluate(args: SEXP, rho: SEXP, operation: Operation) -> SEXP {
    fn fail(e: crate::sexp::object::SexpError) -> ! {
        crate::sexp::context::r_error(e.to_string())
    }
    let owner = unsafe { OwnerToken::current() }.unwrap_or_else(|e| fail(e));
    let _pin = owner.pin().unwrap_or_else(|e| fail(e));
    let own = |raw| {
        owner
            .sexp(raw)
            .and_then(Sexp::into_owned)
            .unwrap_or_else(|e| fail(e))
    };
    let args = own(args);
    let environment = own(rho);
    let na = own(unsafe { crate::sexp::globals::R_NaString() });
    let weak = owner.weak_owner().unwrap_or_else(|| {
        crate::sexp::context::r_error("string operation requires a managed runtime")
    });
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_runtime(&weak, |access| {
            vector_text_owned::evaluate(access, &args, &na, operation, |access, value, kind| {
                access.with_native(|original| {
                    let raw = if kind == SEXPTYPE::STRSXP {
                        let factory = original.node_factory();
                        let arguments =
                            factory.pairlist_cell(value, &factory.nil(), &factory.nil())?;
                        unsafe {
                            crate::mainutils::essentials_basic::do_as_character(
                                factory.nil().as_raw(),
                                factory.nil().as_raw(),
                                arguments.as_raw(),
                                environment.as_raw(),
                            )
                        }
                    } else {
                        unsafe {
                            crate::main::coerce::coerceVector(value.as_raw(), kind.as_c_int())
                        }
                    };
                    original.sexp(raw)?.into_owned()
                })
            })
        })
    }));
    owner.require_active().unwrap_or_else(|e| fail(e));
    match outcome {
        Ok(Ok(Ok(v))) => v.as_raw(),
        Ok(Ok(Err(e))) | Ok(Err(e)) => fail(e),
        Err(p) => std::panic::resume_unwind(p),
    }
}
