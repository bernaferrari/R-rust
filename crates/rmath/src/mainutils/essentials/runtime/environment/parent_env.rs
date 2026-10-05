//! Original-domain environment arguments remain owned through admission and writes.
use crate::sexp::{
    object::{Sexp, SexpError, SexpResult},
    owner::{OwnerToken, RuntimeAccess, with_runtime},
    ffi::{SEXP, SEXPTYPE},
};

fn failure(message: impl Into<String>) -> SexpError {
    SexpError::EvaluationFailed { message: message.into() }
}

fn arguments(mut list: Sexp<'static>) -> SexpResult<Vec<Sexp<'static>>> {
    let mut values = Vec::new();
    let mut seen = std::collections::HashSet::new();
    while !list.is_nil() {
        let identity = list.allocation()?.link().ok_or(SexpError::StaleAllocation)?;
        seen.try_reserve(1).map_err(|_| failure("parent environment argument allocation failed"))?;
        if !seen.insert(identity) { return Err(failure("cyclic parent environment argument list")); }
        values.try_reserve(1).map_err(|_| failure("parent environment argument allocation failed"))?;
        values.push(list.try_car()?.into_owned()?);
        list = list.try_cdr()?.into_owned()?;
    }
    Ok(values)
}

fn environment(access: &RuntimeAccess, value: &Sexp<'static>, message: &str) -> SexpResult<Sexp<'static>> {
    if value.typeof_() == SEXPTYPE::ENVSXP { return Ok(value.clone()); }
    if value.typeof_() == SEXPTYPE::OBJSXP {
        let data = access.with_native(|owner| unsafe {
            let data = crate::mainutils::subassign::R_getS4DataSlot(value.as_raw(), SEXPTYPE::ENVSXP.as_c_int());
            owner.sexp(data)?.into_owned()
        })?;
        if data.typeof_() == SEXPTYPE::ENVSXP { return Ok(data); }
    }
    Err(failure(message))
}

fn lookup(access: &RuntimeAccess, env: &Sexp<'static>, name: &std::ffi::CStr) -> SexpResult<Sexp<'static>> {
    access.with_native(|owner| unsafe {
        let symbol = owner.sexp(crate::sexp::symbol::Rf_install(name.as_ptr()))?.into_owned()?;
        owner.sexp(crate::sexp::envir::R_findVarInFrame(env.as_raw(), symbol.as_raw()))?.into_owned()
    })
}

fn set(access: &RuntimeAccess, values: &[Sexp<'static>]) -> SexpResult<Sexp<'static>> {
    if values[0].is_nil() { return Err(failure("use of NULL environment is defunct")); }
    let env = environment(access, &values[0], "argument is not an environment")?;
    let empty = access.with_native(|owner| unsafe { owner.sexp(crate::sexp::globals::R_EmptyEnv())?.into_owned() })?;
    if env.as_raw() == empty.as_raw() { return Err(failure("can not set parent of the empty environment")); }
    let locked = access.with_native(|_| Ok(crate::sexp::envir::environment_is_locked_raw(env.as_raw())))?;
    if locked {
        let base = access.with_native(|owner| unsafe { owner.sexp(crate::sexp::envir::R_BaseNamespace())?.into_owned() })?;
        let info = lookup(access, &env, c".__NAMESPACE__.")?;
        if env.as_raw() == base.as_raw() || info.typeof_() == SEXPTYPE::ENVSXP {
            return Err(failure("can not set the parent environment of a namespace"));
        }
        if env.try_enclos()?.as_raw() == base.as_raw() {
            let name = access.with_native(|owner| unsafe {
                let symbol = owner.sexp(crate::sexp::symbol::Rf_install(c"name".as_ptr()))?.into_owned()?;
                owner.sexp(crate::sexp::attrib_core::getAttrib(env.as_raw(), symbol.as_raw()))?.into_owned()
            })?;
            if name.typeof_() == SEXPTYPE::STRSXP {
                let length = name.len(); access.require_active()?;
                if length == 1 {
                    let character = name.try_string_elt(0)?.into_owned()?; access.require_active()?;
                    if character.try_as_string()?.starts_with("imports:") {
                        return Err(failure("can not set the parent environment of package imports"));
                    }
                }
            }
        }
    }
    if values[1].is_nil() { return Err(failure("use of NULL environment is defunct")); }
    let parent = environment(access, &values[1], "'parent' is not an environment")?;
    let mut cursor = parent.clone();
    let mut seen = std::collections::HashSet::new();
    while cursor.typeof_() == SEXPTYPE::ENVSXP {
        if cursor.as_raw() == env.as_raw() { return Err(failure("cycles in parent chains are not allowed")); }
        if cursor.as_raw() == empty.as_raw() { break; }
        seen.try_reserve(1).map_err(|_| failure("parent chain allocation failed"))?;
        if !seen.insert(cursor.allocation()?.link().ok_or(SexpError::StaleAllocation)?) {
            return Err(failure("cycles in parent chains are not allowed"));
        }
        cursor = cursor.try_enclos()?.into_owned()?;
    }
    access.require_active()?;
    access.with_native(|_| unsafe {
        crate::sexp::accessors::SET_ENCLOS(env.as_raw(), parent.as_raw());
        Ok(())
    })?;
    Ok(values[0].clone())
}

/// The translated caller supplies evaluated arguments. Capture the whole graph
/// before symbol installation, S4 extraction or any provider can collect/reenter.
pub(super) unsafe fn dispatch(raw: SEXP, setter: bool) -> SEXP {
    let result = (|| {
        let owner = unsafe { OwnerToken::current()? };
        let list = owner.sexp(raw)?.into_owned()?;
        let values = arguments(list)?;
        let expected = if setter { 2 } else { 1 };
        if values.len() != expected {
            return Err(failure(format!("{} arguments passed to .Internal({}) which requires {expected}", values.len(), if setter { "parent.env<-" } else { "parent.env" })));
        }
        let runtime = owner.weak_owner().ok_or(SexpError::RootUnavailable)?;
        with_runtime(&runtime, |access| {
            if setter { return set(access, &values); }
            let env = environment(access, &values[0], "argument is not an environment")?;
            let empty = access.with_native(|token| unsafe { token.sexp(crate::sexp::globals::R_EmptyEnv())?.into_owned() })?;
            if env.as_raw() == empty.as_raw() { return Err(failure("the empty environment has no parent")); }
            env.try_enclos()?.into_owned()
        })?
    })();
    result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())).as_raw()
}

#[cfg(test)]
#[path = "parent_env/tests.rs"]
mod tests;
