//! Lazy-load databases, RDS, and package source loading.

use super::*;

/// GNU `makeNamespace` + `namespaceExport`: `.__NAMESPACE__.` holds
/// `exports` so `.isExported` / `.minimalName` / `show()` work.
pub(crate) unsafe fn ensure_namespace_info(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
    directives: Option<&NamespaceDirectives>,
) {
    unsafe {
        // GNU namespaceExport binds exports in the exports environment
        // only. An inherited re-export such as stats4::show (via
        // importFrom(methods, show)) must NOT gain a namespace-frame
        // binding, or `asNamespace(pkg)[["show"]]` stops being NULL and
        // `pkg:::name` semantics diverge from GNU.
        let factory = crate::sexp::object::SessionNodeFactory::new(
            crate::sexp::owner::OwnerToken::current()
                .unwrap_or_else(|e| package_error(e.to_string())),
        );
        let namespace = factory
            .wrap(package_env)
            .unwrap_or_else(|e| package_error(e.to_string()));
        // Export presence includes inherited re-exports (imports env), so
        // the exports environment still records the self-symbol alias that
        // `::` resolves by evaluating the symbol in the namespace. Only the
        // one-level imports environment is consulted: a full inherited
        // lookup would force unrelated lazy promises mid-load.
        let import_parent = crate::sexp::accessors::ENCLOS(package_env);
        let export_present = |symbol: SEXP| {
            crate::sexp::envir::R_findVarInFrame(namespace.as_raw(), symbol)
                != crate::sexp::globals::R_UnboundValue()
                || (!import_parent.is_null()
                    && import_parent != R_NilValue()
                    && crate::sexp::envir::R_findVarInFrame(import_parent, symbol)
                        != crate::sexp::globals::R_UnboundValue())
                // GNU namespaceExport resolves inherited re-exports through
                // the namespace's full search path, which ends at base
                // (e.g. graphics::plot is the base generic).
                || crate::sexp::envir::R_findVarInFrame(
                    crate::sexp::envir::R_BaseNamespace(),
                    symbol,
                ) != crate::sexp::globals::R_UnboundValue()
        };
        let info_sym = Rf_install(c".__NAMESPACE__.".as_ptr());
        let existing = crate::sexp::envir::R_findVarInFrame(package_env, info_sym);
        if !existing.is_null()
            && existing != crate::sexp::globals::R_UnboundValue()
            && TYPEOF(existing) == SEXPTYPE::ENVSXP
        {
            let exports =
                crate::sexp::envir::R_findVarInFrame(existing, Rf_install(c"exports".as_ptr()));
            if TYPEOF(exports) == SEXPTYPE::ENVSXP {
                let exports = factory
                    .wrap(exports)
                    .unwrap_or_else(|e| package_error(e.to_string()));
                for name in namespace_exports(directives, namespace.as_raw()) {
                    let Ok(name) = CString::new(name) else {
                        continue;
                    };
                    let symbol = Rf_install(name.as_ptr());
                    if export_present(symbol) {
                        crate::sexp::envir::defineVar(symbol, symbol, exports.as_raw());
                    }
                }
            }
            ensure_s3methods_slot(existing);
            ensure_imports_slot(existing, directives);
            return;
        }

        let info = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_BaseEnv(),
            R_NilValue(),
        );
        let _info = protect(info);
        let exports = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_BaseEnv(),
            R_NilValue(),
        );
        let _exports = protect(exports);

        for name in namespace_exports(directives, package_env) {
            let Ok(cname) = CString::new(name.as_str()) else {
                continue;
            };
            let symbol = Rf_install(cname.as_ptr());
            let value = crate::sexp::envir::R_findVarInFrame(package_env, symbol);
            if value.is_null() || value == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            crate::sexp::envir::defineVar(symbol, symbol, exports);
        }

        crate::sexp::envir::defineVar(Rf_install(c"exports".as_ptr()), exports, info);

        let spec_names = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _spec_names = protect(spec_names);
        crate::sexp::accessors::SET_STRING_ELT(
            spec_names,
            0,
            crate::sexp::constructors::Rf_mkChar(c"name".as_ptr()),
        );
        crate::sexp::accessors::SET_STRING_ELT(
            spec_names,
            1,
            crate::sexp::constructors::Rf_mkChar(c"version".as_ptr()),
        );
        let spec = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _spec = protect(spec);
        crate::sexp::accessors::SET_STRING_ELT(
            spec,
            0,
            crate::sexp::constructors::Rf_mkChar(
                CString::new(package).unwrap_or_default().as_ptr(),
            ),
        );
        crate::sexp::accessors::SET_STRING_ELT(
            spec,
            1,
            crate::sexp::constructors::Rf_mkChar(c"".as_ptr()),
        );
        crate::sexp::attrib_core::setAttrib(
            spec,
            crate::sexp::attrib_core::R_NamesSymbol(),
            spec_names,
        );
        crate::sexp::envir::defineVar(Rf_install(c"spec".as_ptr()), spec, info);
        ensure_s3methods_slot(info);

        let path = Rf_mkString(
            CString::new(package_dir.to_string_lossy().as_ref())
                .unwrap_or_default()
                .as_ptr(),
        );
        crate::sexp::envir::defineVar(Rf_install(c"path".as_ptr()), path, info);
        crate::sexp::envir::defineVar(info_sym, info, package_env);
        ensure_imports_slot(info, directives);
    }
}

pub(super) unsafe fn ensure_imports_slot(info: SEXP, directives: Option<&NamespaceDirectives>) {
    unsafe {
        let symbol = Rf_install(c"imports".as_ptr());
        if crate::sexp::envir::R_findVarInFrame(info, symbol)
            != crate::sexp::globals::R_UnboundValue()
        {
            return;
        }
        let result = (|| {
            use crate::sexp::object::{SessionNodeFactory, SexpMut};
            let factory = SessionNodeFactory::new(crate::sexp::owner::OwnerToken::current()?);
            let info = factory.wrap(info)?;
            let mut entries: Vec<(String, Option<Vec<String>>)> = vec![("base".into(), None)];
            for import in directives.into_iter().flat_map(|d| &d.imports) {
                let (package, names) = match import {
                    NamespaceImport::All { package } => (
                        package,
                        Some(
                            cached_namespace_by_name(package)
                                .map(|ns| namespace_info_export_names(ns))
                                .unwrap_or_default(),
                        ),
                    ),
                    NamespaceImport::From { package, names } => (package, Some(names.clone())),
                };
                if let Some((_, previous)) = entries.iter_mut().find(|(p, _)| p == package) {
                    match (previous.as_mut(), names) {
                        (Some(previous), Some(names)) => {
                            for name in &names {
                                push_unique(previous, name.clone());
                            }
                        }
                        (_, None) => *previous = None,
                        (None, Some(_)) => {}
                    }
                } else {
                    entries.push((package.clone(), names));
                }
            }
            let mut values = Vec::new();
            for (_, names) in &entries {
                values.push(if let Some(names) = names {
                    let names: Vec<_> = names.iter().map(String::as_str).collect();
                    let value = factory.strings(&names)?;
                    let labels = factory.strings(&names)?;
                    crate::sexp::attrib_core::setAttrib(
                        value.as_raw(),
                        crate::sexp::attrib_core::R_NamesSymbol(),
                        labels.as_raw(),
                    );
                    value
                } else {
                    factory.domain().logical(true)
                });
            }
            let imports = factory.allocate(|arena| {
                Some(arena.alloc_vector(SEXPTYPE::VECSXP, values.len() as R_xlen_t))
            })?;
            let mut imports = SexpMut::try_from_checked(imports)?;
            for (index, value) in values.into_iter().enumerate() {
                imports.try_set_vector_elt(index as R_xlen_t, value)?;
            }
            let imports = imports.freeze();
            let names: Vec<_> = entries.iter().map(|(name, _)| name.as_str()).collect();
            let names = factory.strings(&names)?;
            crate::sexp::attrib_core::setAttrib(
                imports.as_raw(),
                crate::sexp::attrib_core::R_NamesSymbol(),
                names.as_raw(),
            );
            crate::sexp::envir::defineVar(symbol, imports.as_raw(), info.as_raw());
            Ok::<_, crate::sexp::SexpError>(())
        })();
        result.unwrap_or_else(|error| package_error(error.to_string()));
    }
}

pub(crate) fn reject_unsupported_internal_data(
    package: &str,
    package_dir: &Path,
) -> Result<(), String> {
    for path in [
        package_dir.join("R").join("sysdata.rda"),
        package_dir.join("R").join("sysdata.RData"),
        package_dir.join("R").join("sysdata.rdb"),
        package_dir.join("R").join("sysdata.rdx"),
    ] {
        if path.is_file() {
            return Err(format!(
                "package '{}' uses unsupported internal serialized data {}; this pure-R Android runtime supports R source files and data/*.R only",
                package,
                path.display()
            ));
        }
    }
    Ok(())
}

pub(crate) fn package_lazy_db_base(package_dir: &Path, package: &str) -> PathBuf {
    package_dir.join("R").join(package)
}

pub(crate) fn package_has_lazy_db(package_dir: &Path, package: &str) -> bool {
    let base = package_lazy_db_base(package_dir, package);
    base.with_extension("rdx").is_file() && base.with_extension("rdb").is_file()
}

pub(crate) fn reject_unsupported_lazyload_code(
    package: &str,
    package_dir: &Path,
) -> Result<(), String> {
    let r_dir = package_dir.join("R");
    if !r_dir.is_dir() {
        return Ok(());
    }

    let mut has_rdb = false;
    let mut has_rdx = false;
    let entries = std::fs::read_dir(&r_dir)
        .map_err(|err| format!("could not read R directory for package '{package}': {err}"))?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if ext.eq_ignore_ascii_case("rdb") {
            has_rdb = true;
        } else if ext.eq_ignore_ascii_case("rdx") {
            has_rdx = true;
        }
    }

    if has_rdb ^ has_rdx {
        let orphan = if has_rdb { "rdb" } else { "rdx" };
        return Err(format!(
            "package '{}' uses unsupported byte-compiled/lazyload R code {}; incomplete lazy-load database (missing .{orphan})",
            package,
            r_dir.display()
        ));
    }

    Ok(())
}

pub(crate) unsafe fn load_rds_file(path: &Path) -> Result<SEXP, String> {
    unsafe {
        let bytes = std::fs::read(path)
            .map_err(|err| format!("cannot read RDS file '{}': {err}", path.display()))?;
        let raw_vec = Rf_allocVector3(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t);
        if raw_vec.is_null() {
            return Err(format!(
                "allocation failed while reading '{}'",
                path.display()
            ));
        }
        let _raw_guard = protect(raw_vec);
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), RAW(raw_vec), bytes.len());
        }
        Ok(crate::mainutils::serialize::R_unserialize(
            raw_vec,
            R_NilValue(),
        ))
    }
}

pub(crate) unsafe fn lazy_load_db_fetch_value(
    key: SEXP,
    datafile: SEXP,
    compressed: SEXP,
) -> Result<SEXP, String> {
    unsafe {
        let args = Rf_cons(
            key,
            Rf_cons(
                datafile,
                Rf_cons(compressed, Rf_cons(R_NilValue(), R_NilValue())),
            ),
        );
        let value = crate::mainutils::serialize::do_lazyLoadDBfetch(
            R_NilValue(),
            R_NilValue(),
            args,
            R_NilValue(),
        );
        if value.is_null() {
            Err("lazy-load database fetch returned NULL".to_string())
        } else {
            Ok(value)
        }
    }
}

pub(crate) unsafe fn eager_lazy_load_package_db(
    filebase: &Path,
    envir: SEXP,
    skip: &[&str],
) -> Result<(), String> {
    unsafe { lazy_lazy_load_package_db(filebase, envir, skip) }
}

pub(crate) unsafe fn lazy_lazy_load_package_db(
    filebase: &Path,
    envir: SEXP,
    skip: &[&str],
) -> Result<(), String> {
    unsafe {
        if envir.is_null() || TYPEOF(envir) != SEXPTYPE::ENVSXP {
            return Err("lazy-load requires a package environment".to_string());
        }

        let rdx_path = filebase.with_extension("rdx");
        let rdb_path = filebase.with_extension("rdb");
        let map = load_rds_file(&rdx_path)?;
        let _map_guard = protect(map);

        let variables = list_element_by_name(map, "variables").ok_or_else(|| {
            format!(
                "lazy-load map '{}' is missing a variables table",
                rdx_path.display()
            )
        })?;
        if TYPEOF(variables) != SEXPTYPE::VECSXP {
            return Err(format!(
                "lazy-load map '{}' has an invalid variables table",
                rdx_path.display()
            ));
        }

        let compressed = list_element_by_name(map, "compressed").unwrap_or(Rf_ScalarInteger(0));
        let datafile = Rf_mkString(
            CString::new(rdb_path.to_string_lossy().into_owned())
                .unwrap_or_default()
                .as_ptr(),
        );
        if datafile.is_null() {
            return Err(format!(
                "could not create lazy-load data path for '{}'",
                rdb_path.display()
            ));
        }
        let _datafile_guard = protect(datafile);

        let hook = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_EmptyEnv(),
            R_NilValue(),
        );
        let _hook_guard = protect(hook);
        let cache = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_EmptyEnv(),
            R_NilValue(),
        );
        let _cache_guard = protect(cache);
        crate::sexp::envir::defineVar(Rf_install(c"cache".as_ptr()), cache, hook);
        crate::sexp::envir::defineVar(Rf_install(c"datafile".as_ptr()), datafile, hook);
        crate::sexp::envir::defineVar(Rf_install(c"compressed".as_ptr()), compressed, hook);
        if let Some(refs) = list_element_by_name(map, "references") {
            let refs_env = crate::sexp::memory_ext::NewEnvironment(
                R_NilValue(),
                crate::sexp::globals::R_EmptyEnv(),
                R_NilValue(),
            );
            let _refs_guard = protect(refs_env);
            if TYPEOF(refs) == SEXPTYPE::VECSXP {
                let _ = crate::mainutils::essentials::do_list2env(
                    R_NilValue(),
                    R_NilValue(),
                    Rf_cons(refs, Rf_cons(refs_env, R_NilValue())),
                    R_NilValue(),
                );
            }
            crate::sexp::envir::defineVar(Rf_install(c"refs".as_ptr()), refs_env, hook);
        }

        let names = crate::sexp::attrib_core::getAttrib(
            variables,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );

        let mut lazy_names = Vec::new();
        let mut lazy_keys = Vec::new();
        for index in 0..XLENGTH(variables) {
            let name =
                if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
                    string_at_or_empty(names, index)
                } else {
                    String::new()
                };
            if name.is_empty() || skip.iter().any(|candidate| *candidate == name) {
                continue;
            }
            lazy_names.push(name);
            lazy_keys.push(VECTOR_ELT(variables, index));
        }

        if lazy_names.is_empty() {
            return Ok(());
        }

        let fetch_sym = Rf_install(c"lazyLoadDBfetch".as_ptr());
        let template = Rf_cons(
            fetch_sym,
            Rf_cons(
                R_NilValue(),
                Rf_cons(datafile, Rf_cons(compressed, Rf_cons(hook, R_NilValue()))),
            ),
        );
        if !template.is_null() {
            crate::sexp::accessors::SET_TYPEOF(template, SEXPTYPE::LANGSXP.as_c_int());
        }
        let _template_guard = protect(template);

        for (index, name) in lazy_names.iter().enumerate() {
            let key = lazy_keys[index];
            let expr0 = crate::mainutils::duplicate::duplicate(template);
            let _expr0_guard = protect(expr0);
            SETCAR(CDR(expr0), key);
            let symbol = Rf_install(CString::new(name.clone()).unwrap_or_default().as_ptr());
            crate::sexp::envir::defineVar(
                symbol,
                crate::sexp::memory_ext::mkPROMISE(expr0, envir),
                envir,
            );
        }

        Ok(())
    }
}

pub(crate) unsafe fn source_package_r_files(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
) -> Result<(), String> {
    unsafe {
        let r_dir = package_dir.join("R");
        if !r_dir.is_dir() {
            return Ok(());
        }

        let description = std::fs::read_to_string(package_dir.join("DESCRIPTION"))
            .map(|content| description_fields(&content))
            .unwrap_or_default();
        let collate_files = description
            .get("Collate")
            .map(|value| description_file_list(value))
            .unwrap_or_default();

        let mut files = std::fs::read_dir(&r_dir)
            .map_err(|err| format!("could not read R directory for package '{package}': {err}"))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("r"))
            })
            .collect::<Vec<_>>();
        files.sort();

        if !collate_files.is_empty() {
            let mut ordered = Vec::with_capacity(files.len());
            let mut collated_names = BTreeSet::<String>::new();
            for file_name in collate_files {
                if file_name.contains('/') || file_name.contains('\\') {
                    return Err(format!(
                        "package '{}' has unsupported Collate entry '{}'",
                        package, file_name
                    ));
                }
                let file = r_dir.join(&file_name);
                if !file.is_file() {
                    return Err(format!(
                        "package '{}' Collate entry '{}' does not exist",
                        package, file_name
                    ));
                }
                collated_names.insert(file_name);
                ordered.push(file);
            }
            for file in files {
                let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if !collated_names.contains(name) {
                    ordered.push(file);
                }
            }
            files = ordered;
        }

        // Source with the package directory installed as the resolution
        // root for relative file paths: package code evaluated at install
        // time upstream (crayon's ansi-palette.R reads
        // "tools/ansi-palettes.txt") sees the package root, not the host
        // process CWD.
        let saved_package_dir = crate::sexp::instance::with_required_current_instance(|inst| {
            (*inst)
                .loading_package_dir
                .replace(normalized_package_dir(package_dir))
        });
        let sourced: Result<(), String> =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for file in files {
                    source_r_file_into_env(&file, package_env)?;
                }
                Ok(())
            }))
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
        crate::sexp::instance::with_required_current_instance(|inst| {
            (*inst).loading_package_dir = saved_package_dir;
        });
        sourced?;

        if package_has_lazy_db(package_dir, package) {
            let filebase = package_lazy_db_base(package_dir, package);
            eager_lazy_load_package_db(&filebase, package_env, &[".__NAMESPACE__."])?;
        }

        Ok(())
    }
}

pub(crate) unsafe fn source_package_lazy_data(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
) -> Result<Vec<String>, String> {
    unsafe {
        let data_dir = package_dir.join("data");
        if !data_dir.is_dir() {
            return Ok(Vec::new());
        }

        let rdata_base = data_dir.join("Rdata");
        if rdata_base.with_extension("rdx").is_file() {
            eager_lazy_load_package_db(&rdata_base, package_env, &[])?;
        } else if !package_declares_lazy_data(package_dir)? {
            // Ordinary source datasets belong to explicit data(..., envir=).
            // Attaching them here leaks those values into the search path.
            return Ok(Vec::new());
        }

        let before = frame_binding_names(package_env, true)
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut files = std::fs::read_dir(&data_dir)
            .map_err(|err| format!("could not read data directory for package '{package}': {err}"))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("r"))
            })
            .collect::<Vec<_>>();
        files.sort();

        for file in files {
            source_r_file_into_env(&file, package_env)?;
        }

        let mut names = frame_binding_names(package_env, true);
        if !rdata_base.with_extension("rdx").is_file() {
            names.retain(|name| !before.contains(name.as_str()));
        }
        names.retain(|name| !name.starts_with('.'));
        names.sort();
        names.dedup();
        Ok(names)
    }
}

pub(crate) unsafe fn define_lazy_data_names(package_env: SEXP, names: &[String]) {
    unsafe {
        let values = string_vector(names);
        if !values.is_null() {
            crate::sexp::envir::defineVar(lazy_data_names_symbol(), values, package_env);
        }
    }
}

pub(crate) unsafe fn lazy_data_names_binding(package_env: SEXP) -> Vec<String> {
    use crate::sexp::{
        object::{Sexp, SexpError, SexpResult},
        owner::{OwnerToken, RuntimeAccess, StoredOwner, with_runtime},
    };

    fn fail(error: SexpError) -> ! {
        std::panic::panic_any(RError {
            message: error.to_string(),
        })
    }
    fn lookup(
        access: &RuntimeAccess,
        environment: &Sexp<'static>,
        name: &std::ffi::CStr,
    ) -> SexpResult<Sexp<'static>> {
        access.domain().link(environment)?;
        let symbol = access
            .with_native(|owner| unsafe { owner.sexp(Rf_install(name.as_ptr()))?.into_owned() })?;
        let value = access.with_native(|owner| unsafe {
            owner
                .sexp(crate::sexp::envir::R_findVarInFrame(
                    environment.as_raw(),
                    symbol.as_raw(),
                ))?
                .into_owned()
        })?;
        if value.typeof_() == SEXPTYPE::PROMSXP {
            access.with_native(|owner| unsafe {
                owner
                    .sexp(crate::sexp::envir::forcePromise(value.as_raw()))?
                    .into_owned()
            })
        } else {
            Ok(value)
        }
    }
    fn binding_names(
        access: &RuntimeAccess,
        environment: &Sexp<'static>,
    ) -> SexpResult<Vec<String>> {
        let unbound = access.domain().unbound();
        let mut chains = Vec::new();
        chains
            .try_reserve(1)
            .map_err(|_| SexpError::AllocationFailed {
                object: "namespace lazy data names",
            })?;
        chains.push(environment.try_frame()?);
        let table = environment.try_hashtab()?;
        if !table.is_nil() {
            if table.typeof_() != SEXPTYPE::VECSXP {
                return Err(SexpError::TypeMismatch {
                    expected: "environment hash table",
                    actual: table.typeof_(),
                });
            }
            for index in 0..table.len() {
                chains
                    .try_reserve(1)
                    .map_err(|_| SexpError::AllocationFailed {
                        object: "namespace lazy data names",
                    })?;
                chains.push(table.try_vector_elt(index)?);
            }
        }
        let mut names = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for mut cell in chains {
            while !cell.is_nil() {
                seen.try_reserve(1)
                    .map_err(|_| SexpError::AllocationFailed {
                        object: "namespace lazy data names",
                    })?;
                let identity = cell
                    .allocation()?
                    .link()
                    .ok_or(SexpError::StaleAllocation)?;
                if !seen.insert(identity) {
                    return Err(SexpError::EvaluationFailed {
                        message: "cyclic namespace binding chain".into(),
                    });
                }
                if cell.try_car()? != unbound {
                    let tag = cell.try_tag()?;
                    if tag.typeof_() == SEXPTYPE::SYMSXP {
                        let name = tag.try_printname()?.try_as_string()?;
                        if name != ".packageName" && name != ".namespaceEnv" {
                            names
                                .try_reserve(1)
                                .map_err(|_| SexpError::AllocationFailed {
                                    object: "namespace lazy data names",
                                })?;
                            names.push(name);
                        }
                    }
                }
                cell = cell.try_cdr()?;
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    // Retain the original namespace and physical runtime through metadata
    // lookup/force callbacks; no replacement ambient runtime may be adopted.
    let namespace = unsafe { OwnerToken::current() }
        .and_then(|owner| owner.sexp(package_env))
        .and_then(Sexp::into_owned)
        .unwrap_or_else(|error| fail(error));
    let authority = StoredOwner::from_value(&namespace).unwrap_or_else(|error| fail(error));
    let owner = authority
        .managed()
        .unwrap_or_else(|| fail(SexpError::RootUnavailable));
    let _pin = owner.pin().unwrap_or_else(|error| fail(error));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_runtime(&owner, |access| -> SexpResult<Vec<String>> {
            let info = lookup(access, &namespace, c".__NAMESPACE__.")?;
            if info.typeof_() == SEXPTYPE::ENVSXP {
                let lazy = lookup(access, &info, c"lazydata")?;
                if lazy.typeof_() == SEXPTYPE::ENVSXP {
                    return binding_names(access, &lazy);
                }
            }
            // Preserve installed packages that still expose the legacy index.
            let value = lookup(access, &namespace, c".lazyDataNames")?;
            if value.typeof_() != SEXPTYPE::STRSXP {
                return Ok(Vec::new());
            }
            let mut names = Vec::new();
            for index in 0..value.len() {
                if let Some(name) = value.try_string_value_elt(index)?
                    && !name.is_empty()
                    && name != "NA"
                {
                    names
                        .try_reserve(1)
                        .map_err(|_| SexpError::AllocationFailed {
                            object: "namespace lazy data names",
                        })?;
                    names.push(name);
                }
            }
            Ok(names)
        })
    }));
    authority
        .require_active()
        .unwrap_or_else(|error| fail(error));
    match result {
        Ok(result) => result
            .and_then(|result| result)
            .unwrap_or_else(|error| fail(error)),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
