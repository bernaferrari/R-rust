//! Namespace directives types and populate/attach environments.

use super::*;

#[derive(Clone, Debug, Default)]
pub(crate) struct NamespaceDirectives {
    pub(crate) exports: Vec<String>,
    pub(crate) export_patterns: Vec<String>,
    pub(crate) imports: Vec<NamespaceImport>,
    pub(crate) s3_methods: Vec<S3MethodDirective>,
    pub(crate) native_libraries: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum NamespaceImport {
    All { package: String },
    From { package: String, names: Vec<String> },
}

#[derive(Clone, Debug)]
pub(crate) struct S3MethodDirective {
    pub(super) generic: String,
    pub(super) class: String,
    pub(super) method: Option<String>,
}

pub(crate) unsafe fn populate_package_namespace(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
    loading: &mut Vec<String>,
) -> Result<Option<NamespaceDirectives>, String> {
    unsafe {
        let namespace = read_namespace_directives(package_dir)?;
        apply_description_depends(package, package_dir, package_env, loading)?;
        if let Some(directives) = namespace.as_ref() {
            if !is_builtin_package_dependency(package) {
                reject_native_namespace_directives(package, directives)?;
            }
            apply_namespace_imports(package, package_env, directives, loading)?;
        }

        source_package_r_files(package, package_dir, package_env)?;
        let lazy_data_names = source_package_lazy_data(package, package_dir, package_env)?;
        define_lazy_data_names(package_env, &lazy_data_names);
        if let Some(directives) = namespace.as_ref() {
            // The namespace metadata must exist before registration records
            // its declarations; the final refresh also publishes later exports.
            ensure_namespace_info(package, package_dir, package_env, Some(directives));
            register_namespace_s3_methods(
                package,
                package_env,
                directives,
                S3MethodMetadata::Record,
            )?;
        }
        Ok(namespace)
    }
}

pub(crate) unsafe fn apply_description_depends(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
    loading: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        for dependency in package_depends_names(package_dir)? {
            if is_builtin_package_dependency(&dependency) {
                continue;
            }
            if loading.iter().any(|entry| entry == &dependency) {
                return Err(format!(
                    "package '{}' has cyclic Depends dependency involving '{}'",
                    package, dependency
                ));
            }

            let dependency_dir = find_package_path(&dependency);
            if dependency_dir.is_empty() {
                return Err(format!(
                    "package '{}' depends on missing package '{}'",
                    package, dependency
                ));
            }

            loading.push(dependency.clone());
            let import = NamespaceImport::All {
                package: dependency.clone(),
            };
            let result = import_namespace_bindings(
                package,
                package_env,
                Path::new(&dependency_dir),
                &import,
                loading,
            );
            loading.pop();
            result?;
        }
        Ok(())
    }
}

/// GNU `importFrom(methods, show)` copies the live generic. Builtin
/// packages are not loaded from a library tree, but their namespaces
/// are already in the session cache after `require(methods)`.
pub(super) unsafe fn bind_cached_builtin_imports(
    package_env: SEXP,
    import: &NamespaceImport,
) -> Result<(), String> {
    unsafe {
        let import_package = match import {
            NamespaceImport::All { package } | NamespaceImport::From { package, .. } => {
                package.as_str()
            }
        };
        let Some(import_env) = cached_namespace_by_name(import_package) else {
            return Ok(());
        };
        let imported_names = match import {
            NamespaceImport::All { .. } => {
                let names = namespace_info_export_names(import_env);
                if names.is_empty() {
                    namespace_exports(None, import_env)
                } else {
                    names
                }
            }
            NamespaceImport::From { names, .. } => names.clone(),
        };
        for name in imported_names {
            let Ok(symbol_name) = CString::new(name.as_str()) else {
                continue;
            };
            let symbol = Rf_install(symbol_name.as_ptr());
            let value = crate::sexp::envir::R_findVarInFrame(import_env, symbol);
            if value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_UnboundValue()
            {
                continue;
            }
            crate::sexp::envir::defineVar(symbol, value, namespace_imports_env(package_env));
        }
        Ok(())
    }
}

pub(crate) unsafe fn apply_namespace_imports(
    package: &str,
    package_env: SEXP,
    directives: &NamespaceDirectives,
    loading: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        for import in &directives.imports {
            let import_package = match import {
                NamespaceImport::All { package } | NamespaceImport::From { package, .. } => {
                    package.as_str()
                }
            };

            // Base-distribution packages have no extra library tree, but
            // `importFrom(methods, show)` still has to copy the live
            // generic from the cached namespace. Skip only when that
            // namespace is not in the session yet.
            if is_builtin_package_dependency(import_package) {
                bind_cached_builtin_imports(package_env, import)?;
                continue;
            }

            if loading.iter().any(|entry| entry == import_package) {
                return Err(format!(
                    "package '{}' has cyclic namespace import involving '{}'",
                    package, import_package
                ));
            }
            let import_dir = find_package_path(import_package);
            if import_dir.is_empty() {
                return Err(format!(
                    "package '{}' imports missing package '{}'",
                    package, import_package
                ));
            }

            loading.push(import_package.to_string());
            let result = import_namespace_bindings(
                package,
                package_env,
                Path::new(&import_dir),
                import,
                loading,
            );
            loading.pop();
            result?;
        }
        Ok(())
    }
}

pub(crate) unsafe fn import_namespace_bindings(
    package: &str,
    package_env: SEXP,
    import_dir: &Path,
    import: &NamespaceImport,
    loading: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        let import_package = match import {
            NamespaceImport::All { package } | NamespaceImport::From { package, .. } => {
                package.as_str()
            }
        };

        let (import_env, namespace) = load_package_namespace(import_package, import_dir, loading)?;
        let _import_guard = crate::sexp::protect::protect(import_env);

        let imported_names = match import {
            NamespaceImport::All { .. } => namespace_exports(namespace.as_ref(), import_env),
            NamespaceImport::From { names, .. } => names.clone(),
        };

        let mut missing = Vec::new();
        for name in imported_names {
            let Ok(symbol_name) = CString::new(name.as_str()) else {
                missing.push(name);
                continue;
            };
            let symbol = Rf_install(symbol_name.as_ptr());
            let value = crate::sexp::envir::R_findVarInFrame(import_env, symbol);
            if value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_UnboundValue()
            {
                missing.push(name);
            } else {
                crate::sexp::envir::defineVar(symbol, value, namespace_imports_env(package_env));
            }
        }

        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "package '{}' imports undefined objects from '{}': {}",
                package,
                import_package,
                missing.join(", ")
            ))
        }
    }
}

pub(super) unsafe fn namespace_imports_env(package_env: SEXP) -> SEXP {
    unsafe {
        let parent = crate::sexp::accessors::ENCLOS(package_env);
        let marker = Rf_install(c".__imports__.".as_ptr());
        if !parent.is_null()
            && parent != R_NilValue()
            && TYPEOF(parent) == SEXPTYPE::ENVSXP
            && crate::sexp::envir::R_findVarInFrame(parent, marker)
                != crate::sexp::globals::R_UnboundValue()
        {
            return parent;
        }
        let imports = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            if parent.is_null() {
                crate::sexp::envir::R_BaseNamespace()
            } else {
                parent
            },
            R_NilValue(),
        );
        crate::sexp::envir::defineVar(marker, Rf_ScalarLogical(1), imports);
        crate::sexp::accessors::SET_ENCLOS(package_env, imports);
        imports
    }
}

pub(crate) unsafe fn make_package_attach_env(
    package: &str,
    namespace: Option<&NamespaceDirectives>,
    package_env: SEXP,
) -> Result<SEXP, String> {
    unsafe { make_package_attach_env_inner(package, namespace, package_env, false) }
}

pub(crate) unsafe fn make_package_attach_env_lenient(
    package: &str,
    namespace: Option<&NamespaceDirectives>,
    package_env: SEXP,
) -> Result<SEXP, String> {
    unsafe { make_package_attach_env_inner(package, namespace, package_env, true) }
}

pub(super) unsafe fn make_package_attach_env_inner(
    package: &str,
    namespace: Option<&NamespaceDirectives>,
    package_env: SEXP,
    skip_missing: bool,
) -> Result<SEXP, String> {
    unsafe {
        let Some(directives) = namespace else {
            return Ok(package_env);
        };

        let attach_env = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_BaseEnv(),
            R_NilValue(),
        );
        if attach_env.is_null() {
            return Err(format!(
                "could not create attach environment for package '{}'",
                package
            ));
        }
        let _attach_guard = crate::sexp::protect::protect(attach_env);

        define_package_metadata(package, attach_env);
        crate::sexp::envir::defineVar(namespace_env_symbol(), package_env, attach_env);
        let s3_table = crate::sexp::envir::R_findVarInFrame(package_env, s3_methods_table_symbol());
        if !s3_table.is_null()
            && s3_table != crate::sexp::globals::R_UnboundValue()
            && TYPEOF(s3_table) == SEXPTYPE::ENVSXP
        {
            crate::sexp::envir::defineVar(s3_methods_table_symbol(), s3_table, attach_env);
        }

        let mut exports = namespace_exports(Some(directives), package_env);
        for name in lazy_data_names_binding(package_env) {
            push_unique(&mut exports, name);
        }
        for name in namespace_info_export_names(package_env) {
            push_unique(&mut exports, name);
        }
        // GNU exportMethods also exports the per-generic method tables
        // (.__T__coef:stats4, .__M__...) onto the attached package env.
        // exportClasses copies .__C__* class metadata so cacheMetaData
        // and getClass(where="package:pkg") see the definitions.
        for name in frame_binding_names(package_env, true) {
            if name.starts_with(".__T__")
                || name.starts_with(".__M__")
                || name.starts_with(".__C__")
            {
                push_unique(&mut exports, name);
            }
        }

        // Crayon-style dynamic exports: top-level package code may
        // `assign(name, value, envir = asNamespace("pkg"))` AFTER the
        // files are sourced (its `sapply(names(builtin_styles), ...)`
        // block). Those bindings live in the namespace env but were never
        // declared as lexical assignments, so a strict frame check misses
        // them. Fall back to the full namespace-env lookup before
        // declaring an export missing.
        let mut missing = Vec::new();
        for export in exports {
            let Ok(symbol_name) = CString::new(export.as_str()) else {
                if !skip_missing {
                    missing.push(export);
                }
                continue;
            };
            let symbol = Rf_install(symbol_name.as_ptr());
            let mut value = crate::sexp::envir::R_findVarInFrame(package_env, symbol);
            if value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_UnboundValue()
            {
                value = crate::sexp::envir::R_findVar(symbol, package_env);
            }
            if package == "graphics"
                && export == "plot"
                && value == crate::sexp::globals::R_UnboundValue()
                && let Some(primitive) = crate::eval::eval::primitive_for_symbol(
                    crate::sexp::object::Sexp::from_raw_unchecked(symbol),
                )
            {
                value = primitive.as_raw();
            }
            if value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_UnboundValue()
            {
                if !skip_missing {
                    missing.push(export);
                }
                continue;
            }
            crate::sexp::envir::defineVar(symbol, value, attach_env);
        }

        if package == "methods" {
            bind_methods_base_primitives(package_env);
            purge_missing_arg_placeholders(package_env);
            retarget_methods_generics(package_env);
        }
        if missing.is_empty() {
            Ok(attach_env)
        } else {
            Err(format!(
                "package '{}' has undefined exports: {}",
                package,
                missing.join(", ")
            ))
        }
    }
}
