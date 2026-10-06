use super::PackageImage;
use crate::sexp::{
    object::{Sexp, SexpResult},
    owner::{OwnerPin, OwnerToken, RuntimeAccess},
};

pub(super) fn base() -> SexpResult<Sexp<'static>> {
    // SAFETY: invoked only by the translated package/session entry while active.
    unsafe {
        OwnerToken::current()?
            .sexp(crate::sexp::globals::R_BaseEnv())?
            .into_owned()
    }
}

pub(super) fn base_namespace(access: &RuntimeAccess) -> SexpResult<Sexp<'static>> {
    access.with_native(|owner| unsafe {
        owner
            .sexp(crate::sexp::envir::R_BaseNamespace())?
            .into_owned()
    })
}

pub(super) fn symbol(access: &RuntimeAccess, name: &str) -> SexpResult<Sexp<'static>> {
    let name =
        std::ffi::CString::new(name).map_err(|_| crate::sexp::SexpError::EvaluationFailed {
            message: "portable package symbol contains NUL".into(),
        })?;
    access.with_native(|owner| unsafe {
        owner
            .sexp(crate::sexp::symbol::Rf_install(name.as_ptr()))?
            .into_owned()
    })
}

pub(super) fn bind(
    access: &RuntimeAccess,
    environment: &Sexp<'static>,
    name: &str,
    value: &Sexp<'static>,
) -> SexpResult<()> {
    access.domain().link(environment)?;
    access.domain().link(value)?;
    let symbol = symbol(access, name)?;
    access.with_native(|_| unsafe {
        crate::sexp::envir::defineVar(symbol.as_raw(), value.as_raw(), environment.as_raw());
        Ok(())
    })
}

pub(super) fn lookup(
    access: &RuntimeAccess,
    environment: &Sexp<'static>,
    name: &str,
) -> SexpResult<Sexp<'static>> {
    access.domain().link(environment)?;
    let symbol = symbol(access, name)?;
    access.with_native(|owner| unsafe {
        owner
            .sexp(crate::sexp::envir::R_findVarInFrame(
                environment.as_raw(),
                symbol.as_raw(),
            ))?
            .into_owned()
    })
}

pub(super) fn set_attribute(
    access: &RuntimeAccess,
    value: &Sexp<'static>,
    name: &str,
    attribute: &Sexp<'static>,
) -> SexpResult<()> {
    access.domain().link(value)?;
    access.domain().link(attribute)?;
    let name = symbol(access, name)?;
    access.with_native(|_| unsafe {
        crate::sexp::attrib_core::setAttrib(value.as_raw(), name.as_raw(), attribute.as_raw());
        Ok(())
    })
}

pub(super) fn cached(
    access: &RuntimeAccess,
    image: &PackageImage,
) -> SexpResult<Option<Sexp<'static>>> {
    access.with_native(|owner| {
        crate::mainutils::essentials::cached_namespace_by_name(image.name)
            .map(|raw| owner.sexp(raw)?.into_owned())
            .transpose()
    })
}

pub(super) fn decode(
    access: &RuntimeAccess,
    raw: &Sexp<'static>,
    hook: &Sexp<'static>,
) -> SexpResult<Sexp<'static>> {
    access.domain().link(raw)?;
    access.domain().link(hook)?;
    // SAFETY: both graphs own their exact leases throughout decoder callbacks.
    access.with_native(|owner| unsafe {
        owner
            .sexp(crate::mainutils::serialize::R_unserialize(
                raw.as_raw(),
                hook.as_raw(),
            ))?
            .into_owned()
    })
}

pub(super) fn force(access: &RuntimeAccess, value: &Sexp<'static>) -> SexpResult<Sexp<'static>> {
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

pub(super) fn restore_methods_metadata(
    access: &RuntimeAccess,
    value: &Sexp<'static>,
) -> SexpResult<()> {
    access.domain().link(value)?;
    let Some(namespace) = cached(access, super::image("methods").expect("registered image"))?
    else {
        return Ok(());
    };
    let name_symbol = symbol(access, "className")?;
    access.with_native(|owner| unsafe {
        let name = owner
            .sexp(crate::sexp::attrib_core::getAttrib(
                value.as_raw(),
                name_symbol.as_raw(),
            ))?
            .into_owned()?;
        if name.typeof_() == crate::sexp::SEXPTYPE::STRSXP
            && name.len() == 1
            && name.try_string_elt(0)?.try_char_eq(b"envRefClass")?
        {
            crate::mainutils::essentials::retarget_envref_definition_parent(
                value.as_raw(),
                namespace.as_raw(),
            );
        }
        Ok(())
    })
}

pub(super) fn evaluate(
    access: &RuntimeAccess,
    code: &str,
    environment: &Sexp<'static>,
) -> Result<Sexp<'static>, String> {
    access
        .domain()
        .link(environment)
        .map_err(|e| e.to_string())?;
    let domain = access.domain();
    let expressions = access
        .with_arena(|arena| crate::eval::parser::parse_expressions(code, arena, domain.clone()))
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let mut result = domain.nil();
    for expression in expressions {
        result = access
            .with_native(|owner| unsafe {
                owner
                    .sexp(crate::eval::eval::Rf_eval(
                        expression.as_raw(),
                        environment.as_raw(),
                    ))?
                    .into_owned()
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(result)
}

pub(super) fn lock(access: &RuntimeAccess, environment: &Sexp<'static>) -> SexpResult<()> {
    access.domain().link(environment)?;
    access.with_native(|_| unsafe {
        crate::sexp::envir::lock_environment_raw(environment.as_raw());
        Ok(())
    })
}

pub(super) fn install_native(
    access: &RuntimeAccess,
    image: &PackageImage,
    namespace: &Sexp<'static>,
) -> SexpResult<()> {
    access.domain().link(namespace)?;
    access.with_native(|_| unsafe {
        match image.name {
            "methods" => crate::library::methods::native_calls::install_methods_call_symbols(
                namespace.as_raw(),
            ),
            "utils" => crate::library::utils::install_utils_call_symbols(namespace.as_raw()),
            "grDevices" => {
                crate::library::grdevices::install_call_symbols(namespace.as_raw());
                crate::library::grdevices::colors::initPalette();
            }
            "tools" => {
                crate::library::tools::native_calls::install_tools_call_symbols(namespace.as_raw());
                crate::library::tools::native_calls::install_tools_assert_closures(
                    namespace.as_raw(),
                );
            }
            _ => unreachable!("closed portable package registry"),
        }
        Ok(())
    })
}

pub(super) fn finalize(
    access: &RuntimeAccess,
    image: &PackageImage,
    namespace: &Sexp<'static>,
) -> SexpResult<()> {
    access.domain().link(namespace)?;
    if image.name == "methods" {
        return access.with_native(|_| unsafe {
            crate::mainutils::essentials::finalize_methods_namespace(namespace.as_raw());
            Ok(())
        });
    }
    let info = lookup(access, namespace, ".__NAMESPACE__.")?;
    let metadata = lookup(access, &info, "S3methods")?;
    let directives =
        crate::mainutils::essentials::parse_namespace_directives(image.namespace_source);
    access.with_native(|_| unsafe {
        crate::mainutils::essentials::register_namespace_s3_methods(
            image.name,
            namespace.as_raw(),
            &directives,
        )
        .map_err(|message| crate::sexp::SexpError::EvaluationFailed { message })
    })?;
    // Registration updates dispatch tables, while the original image already
    // carries the complete metadata. Do not append those captured rows twice.
    bind(access, &info, "S3methods", &metadata)?;
    let source = if image.name == "utils" {
        include_str!("utils_onload.R").to_owned()
    } else {
        format!(".onLoad('{}', '{}')", image.directory, image.name)
    };
    evaluate(access, &source, namespace)
        .map_err(|message| crate::sexp::SexpError::EvaluationFailed { message })?;
    #[cfg(feature = "renderplot-device")]
    if image.name == "grDevices" {
        access.with_native(|_| unsafe {
            crate::mainutils::graphics_recording::install_namespace_replay(namespace.as_raw());
            Ok(())
        })?;
    }
    Ok(())
}

pub(super) fn attach(access: &RuntimeAccess, environment: &Sexp<'static>) -> SexpResult<()> {
    access.domain().link(environment)?;
    access.with_native(|_| unsafe {
        crate::mainutils::essentials::attach_package_env(environment.as_raw());
        Ok(())
    })
}

/// An operation-local original runtime pin protects rollback even after revocation.
/// No provider or returned value retains this guard.
pub(super) struct Publication {
    pin: OwnerPin,
    namespace: Sexp<'static>,
    committed: bool,
    package: &'static str,
}

impl Publication {
    pub(super) fn begin(
        access: &RuntimeAccess,
        image: &'static PackageImage,
        namespace: &Sexp<'static>,
    ) -> SexpResult<Self> {
        access.domain().link(namespace)?;
        let pin = access
            .with_native(|owner| owner.pin())?
            .ok_or(crate::sexp::SexpError::RootUnavailable)?;
        access.with_native(|owner| unsafe {
            let cache = &mut (*owner.as_ptr()).package_namespace_cache;
            cache
                .try_reserve(1)
                .map_err(|_| crate::sexp::SexpError::AllocationFailed {
                    object: "portable package namespace cache",
                })?;
            cache.insert(
                image.name.into(),
                (
                    std::path::PathBuf::from(image.directory),
                    namespace.as_raw(),
                ),
            );
            Ok(())
        })?;
        Ok(Self {
            pin,
            namespace: namespace.clone(),
            committed: false,
            package: image.name,
        })
    }

    pub(super) fn commit(mut self, access: &RuntimeAccess) -> SexpResult<()> {
        access.require_active()?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for Publication {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        // SAFETY: the sole original Rc remains physically pinned through cleanup.
        // No loan spans a callback. Remove only our still-owned exact identity,
        // never a namespace installed by a callback after this build began.
        unsafe {
            let cache = &mut (*self.pin.as_ptr()).package_namespace_cache;
            if cache
                .get(self.package)
                .is_some_and(|(_, value)| *value == self.namespace.as_raw())
            {
                cache.remove(self.package);
            }
        }
    }
}
