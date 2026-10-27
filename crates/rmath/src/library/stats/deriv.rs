//! GNU `stats/src/deriv.c`: symbolic `D()` and `deriv()`.

use crate::sexp::accessors::{INTEGER, LOGICAL, REAL, STRING_ELT};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
#[cfg(test)]
use crate::sexp::globals::R_NilValue;
use crate::sexp::symbol::Rf_install;

use std::ffi::{CStr, CString};

// Raw native boundaries retain the original runtime across every callback and
// unwind. The expression kernel itself owns checked children and result nodes.
#[path = "deriv/owned.rs"]
mod owned;

unsafe fn run_owned_derivative(
    first: SEXP,
    second: Option<SEXP>,
    operation: impl FnOnce(
        &crate::sexp::owner::RuntimeAccess,
        &crate::sexp::object::Sexp<'static>,
        Option<&crate::sexp::object::Sexp<'static>>,
    ) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>>,
) -> SEXP {
    use crate::sexp::{
        object::Sexp,
        owner::{OwnerToken, StoredOwner, with_runtime},
    };
    let fail = |message: String| -> ! {
        std::panic::panic_any(crate::sexp::context::RError { message });
    };
    let token = unsafe { OwnerToken::current() }.unwrap_or_else(|error| fail(error.to_string()));
    let authority = StoredOwner::from_token(token);
    let owner = authority
        .managed()
        .unwrap_or_else(|| fail("runtime owner unavailable".into()));
    let _pin = owner.pin().unwrap_or_else(|error| fail(error.to_string()));
    let first = token
        .sexp(first)
        .and_then(Sexp::into_owned)
        .unwrap_or_else(|error| fail(error.to_string()));
    let second = second
        .map(|value| token.sexp(value).and_then(Sexp::into_owned))
        .transpose()
        .unwrap_or_else(|error| fail(error.to_string()));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_runtime(&owner, |access| operation(access, &first, second.as_ref()))
    }));
    authority
        .require_active()
        .unwrap_or_else(|error| fail(error.to_string()));
    match result {
        Ok(result) => result
            .and_then(|result| result)
            .unwrap_or_else(|error| {
                // GNU deriv.c raises these with error(), which verror()
                // attributes to getCurrentCall(): the enclosing closure
                // (D/deriv), never the .External builtin call.
                unsafe {
                    crate::mainutils::errors::errorcall_str(
                        crate::mainutils::errors::condition_caller_call(),
                        &error.to_string(),
                    );
                }
            })
            .as_raw(),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn derivative_symbol(
    access: &crate::sexp::owner::RuntimeAccess,
    name: &str,
) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>> {
    let name =
        CString::new(name).map_err(|_| crate::sexp::object::SexpError::EvaluationFailed {
            message: "invalid variable name".into(),
        })?;
    access.with_native(|owner| unsafe { owner.sexp(Rf_install(name.as_ptr()))?.into_owned() })
}

/// `.External(C_doD, expr, name)`.
pub unsafe fn do_d(args: SEXP) -> SEXP {
    unsafe {
        run_owned_derivative(args, None, |access, arguments, _| {
            owned::direct(access, arguments, derivative_symbol, |access| {
                access.with_native(|_| {
                    crate::mainutils::errors::Rf_warning(
                        c"only the first element is used as variable name".as_ptr(),
                    );
                    Ok(())
                })
            })
        })
    }
}

// Native auxiliary builders retain actual inputs and every intermediate in
// the original runtime. The remaining common-subexpression traversal below
// is migrated independently from these bounded construction helpers.
#[path = "deriv/builders.rs"]
mod builders;

fn duplicate_builder_value(
    access: &crate::sexp::owner::RuntimeAccess,
    value: &crate::sexp::object::Sexp<'static>,
) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>> {
    access.domain().link(value)?;
    access.with_native(|owner| unsafe {
        owner
            .sexp(crate::mainutils::duplicate::Rf_duplicate(value.as_raw()))?
            .into_owned()
    })
}

#[cfg(test)]
unsafe fn create_grad(names: SEXP) -> SEXP {
    unsafe {
        run_owned_derivative(names, None, |access, names, _| {
            builders::Builder::new(access).initialize(names, false)
        })
    }
}
#[cfg(test)]
unsafe fn create_hess(names: SEXP) -> SEXP {
    unsafe {
        run_owned_derivative(names, None, |access, names, _| {
            builders::Builder::new(access).initialize(names, true)
        })
    }
}
#[cfg(test)]
unsafe fn deriv_assign(name: SEXP, expr: SEXP) -> SEXP {
    unsafe {
        run_owned_derivative(name, Some(expr), |access, name, expression| {
            builders::Builder::new(access).assign_gradient(
                name,
                expression.ok_or(crate::sexp::object::SexpError::RootUnavailable)?,
            )
        })
    }
}
#[cfg(test)]
unsafe fn hess_assign1(name: SEXP, expr: SEXP) -> SEXP {
    unsafe {
        run_owned_derivative(name, Some(expr), |access, name, expression| {
            builders::Builder::new(access).assign_hessian(
                name,
                None,
                expression.ok_or(crate::sexp::object::SexpError::RootUnavailable)?,
            )
        })
    }
}
#[cfg(test)]
unsafe fn hess_assign2(name1: SEXP, name2: SEXP, expr: SEXP) -> SEXP {
    unsafe {
        // Retain all three operands before any helper allocation. The tuple
        // is built with owning values inside the original-runtime boundary.
        run_owned_derivative(name1, Some(name2), |access, first, second| {
            let expression = access.with_native(|owner| owner.sexp(expr)?.into_owned())?;
            builders::Builder::new(access).assign_hessian(
                first,
                Some(second.ok_or(crate::sexp::object::SexpError::RootUnavailable)?),
                &expression,
            )
        })
    }
}
#[cfg(test)]
unsafe fn add_grad() -> SEXP {
    unsafe {
        run_owned_derivative(R_NilValue(), None, |access, _, _| {
            builders::Builder::new(access).attach(false)
        })
    }
}
#[cfg(test)]
unsafe fn add_hess() -> SEXP {
    unsafe {
        run_owned_derivative(R_NilValue(), None, |access, _, _| {
            builders::Builder::new(access).attach(true)
        })
    }
}

fn install_builder_formal(
    access: &crate::sexp::owner::RuntimeAccess,
    name: &[u8],
) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>> {
    let name = CStr::from_bytes_with_nul(name).map_err(|_| {
        crate::sexp::object::SexpError::EvaluationFailed {
            message: "invalid variable name".into(),
        }
    })?;
    // The caller retains admitted immutable native bytes. No R payload loan
    // or interpreter field borrow spans this original-owner native boundary.
    access.with_native(|owner| unsafe { owner.sexp(Rf_install(name.as_ptr()))?.into_owned() })
}

fn builder_environment(
    access: &crate::sexp::owner::RuntimeAccess,
) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>> {
    access.with_native(|owner| unsafe { owner.sexp((*owner.as_ptr()).global_env)?.into_owned() })
}
#[cfg(test)]
unsafe fn closure_with_formals(names: SEXP, body: SEXP) -> SEXP {
    unsafe {
        run_owned_derivative(names, Some(body), |access, names, body| {
            builders::Builder::new(access).closure(
                names,
                body.ok_or(crate::sexp::object::SexpError::RootUnavailable)?,
            )
        })
    }
}
/// `.External(C_deriv, expr, namevec, function.arg, tag, hessian)`.
pub unsafe fn do_deriv(args: SEXP) -> SEXP {
    unsafe {
        run_owned_derivative(args, None, |access, arguments, _| {
            outer::build(access, arguments)
        })
    }
}

#[path = "deriv/outer.rs"]
mod outer;

fn derivative_string_element(
    access: &crate::sexp::owner::RuntimeAccess,
    value: &crate::sexp::object::Sexp<'static>,
    index: R_xlen_t,
) -> crate::sexp::object::SexpResult<crate::sexp::object::Sexp<'static>> {
    access.domain().link(value)?;
    access.with_native(|owner| unsafe {
        let value = STRING_ELT(value.as_raw(), index);
        owner.require_active()?;
        owner.sexp(value)?.into_owned()
    })
}
fn derivative_string_prefix(
    access: &crate::sexp::owner::RuntimeAccess,
    value: &crate::sexp::object::Sexp<'static>,
) -> crate::sexp::object::SexpResult<usize> {
    access.domain().link(value)?;
    access.with_native(|owner| unsafe {
        let pointer = crate::sexp::accessors::CHAR(value.as_raw());
        owner.require_active()?;
        // Only pure byte validation runs while this immutable R payload is
        // borrowed. No R callback or allocation spans the returned length.
        Ok(CStr::from_ptr(pointer)
            .to_str()
            .map_or(0, |text| text.len()))
    })
}
fn derivative_hessian_flag(
    access: &crate::sexp::owner::RuntimeAccess,
    value: &crate::sexp::object::Sexp<'static>,
) -> crate::sexp::object::SexpResult<bool> {
    access.domain().link(value)?;
    access.with_native(|owner| unsafe {
        if value.is_empty()
            && matches!(
                value.typeof_(),
                SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP | SEXPTYPE::REALSXP
            )
        {
            return Err(crate::sexp::object::SexpError::EvaluationFailed {
                message: "invalid hessian argument".into(),
            });
        }
        let result = match value.typeof_() {
            SEXPTYPE::LGLSXP => {
                let data = LOGICAL(value.as_raw());
                owner.require_active()?;
                data.read() != 0
            }
            SEXPTYPE::INTSXP => {
                let data = INTEGER(value.as_raw());
                owner.require_active()?;
                data.read() != 0
            }
            SEXPTYPE::REALSXP => {
                let data = REAL(value.as_raw());
                owner.require_active()?;
                data.read() != 0.0
            }
            _ => false,
        };
        Ok(result)
    })
}
fn equal_derivative_scalars(
    access: &crate::sexp::owner::RuntimeAccess,
    first: &crate::sexp::object::Sexp<'static>,
    second: &crate::sexp::object::Sexp<'static>,
) -> crate::sexp::object::SexpResult<bool> {
    access.domain().link(first)?;
    access.domain().link(second)?;
    access.with_native(|owner| unsafe {
        if first.is_empty() || second.is_empty() {
            return Err(crate::sexp::object::SexpError::EvaluationFailed {
                message: "invalid expression in 'equal'".into(),
            });
        }
        if first.typeof_() == SEXPTYPE::REALSXP {
            let data = REAL(first.as_raw());
            owner.require_active()?;
            let first = data.read();
            let data = REAL(second.as_raw());
            owner.require_active()?;
            Ok(first == data.read())
        } else {
            let data = INTEGER(first.as_raw());
            owner.require_active()?;
            let first = data.read();
            let data = INTEGER(second.as_raw());
            owner.require_active()?;
            Ok(first == data.read())
        }
    })
}
fn derivative_is_zero(
    access: &crate::sexp::owner::RuntimeAccess,
    value: &crate::sexp::object::Sexp<'static>,
) -> crate::sexp::object::SexpResult<bool> {
    access.domain().link(value)?;
    access.with_native(|owner| unsafe {
        if value.is_empty() {
            return Ok(false);
        }
        match value.typeof_() {
            SEXPTYPE::REALSXP => {
                let data = REAL(value.as_raw());
                owner.require_active()?;
                Ok(data.read() == 0.0)
            }
            SEXPTYPE::INTSXP => {
                let data = INTEGER(value.as_raw());
                owner.require_active()?;
                Ok(data.read() == 0)
            }
            _ => Ok(false),
        }
    })
}

#[cfg(test)]
#[path = "deriv/owned_tests.rs"]
mod owned_tests;

#[cfg(test)]
#[path = "deriv/builder_tests.rs"]
mod builder_tests;

#[cfg(test)]
#[path = "deriv/outer_tests.rs"]
mod outer_tests;
