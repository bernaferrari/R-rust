//! Pinned GNU datasets, with the original lazy database and promise syntax.
use crate::sexp::{
    object::{Sexp, SexpResult},
    owner::{OwnerToken, RuntimeAccess},
};

mod inventory;
mod owned;

pub(crate) const DATABASE: &str = "<builtin:datasets>/data/Rdata.rdb";
const DIRECTORY: &str = "<builtin:datasets>";

pub(crate) fn namespace() -> Result<Sexp<'static>, String> {
    owned::namespace(current_base()?)
}

pub(crate) fn attach() -> Result<(), String> {
    owned::attach(current_base()?)
}

pub(crate) fn value(name: &str) -> Result<Option<Sexp<'static>>, String> {
    owned::value(current_base()?, name)
}

pub(crate) fn topics() -> Vec<String> {
    inventory::TOPICS
        .iter()
        .map(|(topic, _)| (*topic).into())
        .collect()
}

pub(crate) fn index() -> Result<Sexp<'static>, String> {
    owned::index(current_base()?)
}

pub(crate) fn load_topic(topic: &str, target: Sexp<'static>) -> Result<bool, String> {
    owned::load_topic(topic, target)
}

pub(crate) fn read_database(
    file: Sexp<'static>,
    key: Sexp<'static>,
) -> Result<Option<Sexp<'static>>, String> {
    owned::read_database(file, key)
}

/// Ordinary files are inspected without invoking an ALTREP/provider accessor.
pub(crate) fn is_database(file: &Sexp<'_>) -> SexpResult<bool> {
    if file.typeof_() != crate::sexp::SEXPTYPE::STRSXP
        || file.len() != 1
        || file.header().sxpinfo.alt()
    {
        return Ok(false);
    }
    file.try_string_elt(0)?.try_char_eq(DATABASE.as_bytes())
}

pub(crate) fn fetch(
    key: Sexp<'static>,
    file: Sexp<'static>,
    compressed: Sexp<'static>,
    hook: Sexp<'static>,
) -> Result<Sexp<'static>, String> {
    owned::fetch(key, file, compressed, hook)
}

fn current_base() -> Result<Sexp<'static>, String> {
    // SAFETY: this translated package entry is called under its original active runtime.
    unsafe {
        OwnerToken::current()
            .and_then(|owner| owner.sexp(crate::sexp::globals::R_BaseEnv()))
            .and_then(Sexp::into_owned)
            .map_err(|e| e.to_string())
    }
}

fn base_namespace(access: &RuntimeAccess) -> SexpResult<Sexp<'static>> {
    access.with_native(|owner| unsafe {
        owner
            .sexp(crate::sexp::envir::R_BaseNamespace())?
            .into_owned()
    })
}

fn lock(access: &RuntimeAccess, environment: &Sexp<'static>) -> SexpResult<()> {
    access.domain().link(environment)?;
    access.with_native(|_| {
        crate::sexp::envir::lock_environment_raw(environment.as_raw());
        Ok(())
    })
}

fn symbol(access: &RuntimeAccess, name: &str) -> SexpResult<Sexp<'static>> {
    let name = std::ffi::CString::new(name).expect("literal datasets symbol contains no NUL");
    access.with_native(|owner| unsafe {
        owner
            .sexp(crate::sexp::symbol::Rf_install(name.as_ptr()))?
            .into_owned()
    })
}

fn bind(
    access: &RuntimeAccess,
    environment: &Sexp<'static>,
    name: &str,
    value: &Sexp<'static>,
) -> SexpResult<()> {
    let domain = access.domain();
    domain.link(environment)?;
    if environment.typeof_() != crate::sexp::SEXPTYPE::ENVSXP {
        return Err(crate::sexp::SexpError::TypeMismatch {
            expected: "environment",
            actual: environment.typeof_(),
        });
    }
    domain.link(value)?;
    let name = symbol(access, name)?;
    access.with_native(|_| unsafe {
        crate::sexp::envir::defineVar(name.as_raw(), value.as_raw(), environment.as_raw());
        Ok(())
    })
}

fn lookup(
    access: &RuntimeAccess,
    environment: &Sexp<'static>,
    name: &str,
) -> SexpResult<Sexp<'static>> {
    access.domain().link(environment)?;
    if environment.typeof_() != crate::sexp::SEXPTYPE::ENVSXP {
        return Err(crate::sexp::SexpError::TypeMismatch {
            expected: "environment",
            actual: environment.typeof_(),
        });
    }
    let name = symbol(access, name)?;
    access.with_native(|owner| unsafe {
        owner
            .sexp(crate::sexp::envir::R_findVarInFrame(
                environment.as_raw(),
                name.as_raw(),
            ))?
            .into_owned()
    })
}

fn cached(access: &RuntimeAccess) -> SexpResult<Option<Sexp<'static>>> {
    access.with_native(|owner| {
        crate::mainutils::essentials::cached_namespace_by_name("datasets")
            .map(|raw| owner.sexp(raw)?.into_owned())
            .transpose()
    })
}

fn publish_namespace(access: &RuntimeAccess, namespace: &Sexp<'static>) -> SexpResult<()> {
    access.domain().link(namespace)?;
    access.with_native(|owner| {
        // SAFETY: callback-free publication into the original runtime's existing
        // traced namespace cache. The owning namespace remains live throughout;
        // literal virtual paths need no filesystem canonicalization.
        unsafe {
            let cache = &mut (*owner.as_ptr()).package_namespace_cache;
            cache
                .try_reserve(1)
                .map_err(|_| crate::sexp::SexpError::AllocationFailed {
                    object: "datasets namespace cache",
                })?;
            cache.insert(
                "datasets".into(),
                (std::path::PathBuf::from(DIRECTORY), namespace.as_raw()),
            );
        }
        Ok(())
    })
}

fn attach_environment(access: &RuntimeAccess, environment: &Sexp<'static>) -> SexpResult<()> {
    access.domain().link(environment)?;
    access.with_native(|_| unsafe {
        crate::sexp::envir::lock_environment_raw(environment.as_raw());
        crate::mainutils::essentials::attach_package_env(environment.as_raw());
        Ok(())
    })
}

fn set_attribute(
    access: &RuntimeAccess,
    value: &Sexp<'static>,
    name: &str,
    attribute: &Sexp<'static>,
) -> SexpResult<()> {
    let domain = access.domain();
    domain.link(value)?;
    domain.link(attribute)?;
    let name = symbol(access, name)?;
    access.with_native(|_| unsafe {
        crate::sexp::attrib_core::setAttrib(value.as_raw(), name.as_raw(), attribute.as_raw());
        Ok(())
    })
}

fn force(access: &RuntimeAccess, value: &Sexp<'static>) -> SexpResult<Sexp<'static>> {
    access.domain().link(value)?;
    access.with_native(|owner| unsafe {
        let result = if value.typeof_() == crate::sexp::SEXPTYPE::PROMSXP {
            crate::sexp::envir::forcePromise(value.as_raw())
        } else {
            value.as_raw()
        };
        owner.sexp(result)?.into_owned()
    })
}

fn compression(access: &RuntimeAccess, value: &Sexp<'static>) -> SexpResult<i32> {
    access.domain().link(value)?;
    access.with_native(|_| unsafe { Ok(crate::mainutils::coerce::asInteger(value.as_raw())) })
}

fn decode(
    access: &RuntimeAccess,
    raw: &Sexp<'static>,
    hook: &Sexp<'static>,
) -> SexpResult<Sexp<'static>> {
    let domain = access.domain();
    domain.link(raw)?;
    domain.link(hook)?;
    // The input and hook retain exact independent leases through reader callbacks.
    access.with_native(|owner| unsafe {
        owner
            .sexp(crate::mainutils::serialize::R_unserialize(
                raw.as_raw(),
                hook.as_raw(),
            ))?
            .into_owned()
    })
}

#[cfg(test)]
mod tests;
