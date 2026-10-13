//! Symbol/variable lookup, promise forcing, and dispatch helpers — extracted verbatim from eval/eval.rs

use super::*;

// Lookup/evaluation may reenter. Its locals end before applying the resolved
// function; the caller retains the original call, arguments and environment.
#[inline(never)]
pub(crate) unsafe fn resolve_call_function<'a>(
    fun: Sexp<'a>,
    e: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    Ok({
        if fun.clone().typeof_() == SEXPTYPE::SYMSXP {
            match (unsafe {
                /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */
                find_fun_result(fun.clone(), rho.clone())
            })? {
                Some(value) => value,
                None => match primitive_for_symbol(fun.clone()) {
                    Some(primitive) => primitive,
                    None => unsafe {
                        crate::mainutils::errors::R_FunctionNotFoundError(fun.as_raw(), e.as_raw())
                    },
                },
            }
        } else {
            (unsafe {
                /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */
                eval_safe(fun.clone(), rho.clone())
            })?
        }
    })
}

pub(crate) fn primitive_for_symbol<'a>(symbol: Sexp<'a>) -> Option<Sexp<'a>> {
    let name = unsafe { get_symbol_name(symbol.as_raw()) };
    if crate::eval::builtin::is_hidden_builtin_name(&name) {
        return None;
    }
    if crate::eval::builtin::evaluated_builtin_handler(&name).is_some() {
        let primitive =
            unsafe { crate::eval::primitive::make_primitive_binding(&name, SEXPTYPE::BUILTINSXP) };
        if !primitive.is_null() && primitive != unsafe { R_NilValue() } {
            return Some(unsafe { Sexp::from_raw_unchecked(primitive) });
        }
    }
    if let Some(primitive) = CString::new(name.as_str())
        .ok()
        .map(|n| unsafe { crate::mainutils::names::R_Primitive(n.as_ptr()) })
        .filter(|primitive| !primitive.is_null() && *primitive != unsafe { R_NilValue() })
        .map(|primitive| unsafe { Sexp::from_raw_unchecked(primitive) })
    {
        return Some(primitive);
    }
    if let Some(kind) = crate::eval::builtin::builtin_primitive_kind(&name) {
        let primitive = unsafe { crate::eval::primitive::make_primitive_binding(&name, kind) };
        if !primitive.is_null() && primitive != unsafe { R_NilValue() } {
            return Some(unsafe { Sexp::from_raw_unchecked(primitive) });
        }
    }
    None
}

/// Safe variable lookup using Sexp types.
///
/// Walks the environment chain looking for a symbol binding.
/// # Safety
/// Activate the live owner of all inputs and retain their reachable graphs
/// through allocation and R reentry. No Rust payload loan may cross execution.
pub unsafe fn find_var_safe<'a>(symbol: Sexp<'a>, rho: Sexp<'a>) -> Option<Sexp<'a>> {
    (unsafe { /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */ find_var_result(symbol, rho) }).ok().flatten()
}

/// Raise R's missing-argument error, attributed like upstream.
///
/// Upstream `Rf_eval`'s SYMSXP case raises `R_MissingArgError(e,
/// getLexicalCall(rho))` — the call of the enclosing closure context — so the
/// top-level render shows `Error in f() : argument "x" is missing, with no
/// default`. `R_getCurrentCall()` returns that innermost context call here.
pub(crate) fn missing_arg_error(name: &str) -> ! {
    unsafe {
        let c_name = std::ffi::CString::new(name).unwrap_or_default();
        crate::mainutils::errors::R_MissingArgError_c(
            c_name.as_ptr(),
            crate::mainutils::errors::R_getCurrentCall(),
            c"evalError".as_ptr(),
        );
    }
    unreachable!("R_MissingArgError_c signals");
}

/// Checked variable lookup using typed SEXP field access.
///
/// `Ok(None)` means the binding was not found. `Err` means the environment
/// chain or binding cells were structurally invalid for the operation.
/// # Safety
/// Activate the live owner of all inputs and retain their reachable graphs
/// through allocation and R reentry. No Rust payload loan may cross execution.
pub(crate) unsafe fn find_var_result<'a>(
    symbol: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Option<Sexp<'a>>, String> {
    if symbol == unsafe { Sexp::from_raw_unchecked(R_DotsSymbol()) } {
        return Ok(None);
    }
    let missing_symbol = unsafe {
        let raw = symbol.clone().as_raw();
        let pname = crate::sexp::accessors::PRINTNAME(raw);
        pname.is_null()
            || crate::sexp::accessors::CHAR(pname).is_null()
            || std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(pname))
                .to_bytes()
                .is_empty()
    };
    if missing_symbol {
        missing_arg_error("");
    }

    // GNU Rf_eval SYMSXP: DDVAL names (`..1`, `..2`, ...) go through
    // ddfindVar, not ordinary findVar.
    if crate::sexp::envir::dd_val(symbol.clone()).is_some() {
        let Some(value) = (unsafe {
            /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */
            crate::sexp::envir::dd_find_var_safe(symbol.clone(), rho)
        }) else {
            return Ok(None);
        };
        if value.clone().as_raw() == unsafe { R_MissingArg() } {
            let name = unsafe { get_symbol_name(symbol.as_raw()) };
            missing_arg_error(&name);
        }
        return Ok(Some(value));
    }

    // GNU Rf_eval SYMSXP: findVar is unforced. A MissingArg *binding* is
    // a missing formal. A promise whose forced value is the empty symbol
    // (lapply/vapply over formals) is a real value.
    let binding = (unsafe {
        /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */
        crate::sexp::envir::find_var_binding_result(symbol.clone(), rho.clone())
    })?;
    let Some(binding) = binding else {
        return Ok(None);
    };
    if binding.clone().as_raw() == unsafe { R_MissingArg() } {
        let name = unsafe { get_symbol_name(symbol.as_raw()) };
        missing_arg_error(&name);
    }
    if binding.clone().typeof_() == SEXPTYPE::PROMSXP {
        return (unsafe { /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */ eval_promise_safe(binding, rho) }).map(|v| { mark_named_on_read(v.clone().as_raw()); Some(v) });
    }
    mark_named_on_read(binding.clone().as_raw());
    Ok(Some(binding))
}
pub(crate) fn mark_named_on_read(x: SEXP) {
    unsafe {
        if !x.is_null() && crate::sexp::accessors::NAMED(x) == 0 {
            crate::sexp::accessors::SET_NAMED(x, 1);
        }
    }
}

/// Safe promise evaluation.
pub(crate) unsafe fn eval_promise_safe<'a>(
    prom: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    // If already evaluated, return the value
    let val = prom
        .clone()
        .try_prvalue()
        .clone()
        .map_err(|err| sexp_err("promise value lookup", err))?;
    if val.clone().as_raw() != unsafe { R_UnboundValue() } {
        return Ok(val);
    }

    // Force the promise
    let factory = rho
        .node_factory()
        .map_err(|err| sexp_err("promise owner", err))?;
    let raw_result = unsafe { forcePromise(prom.as_raw()) };
    factory
        .wrap(raw_result)
        .map_err(|err| sexp_err("forced promise result", err))
}

/// Safe dots evaluation.
pub(crate) fn eval_dots_safe<'a>(_dots: Sexp<'a>, _rho: Sexp<'a>) -> Result<Sexp<'a>, String> {
    Err(EvalError::IncorrectDotsContext.to_string())
}
