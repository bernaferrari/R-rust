//! Namespace cache, package data sets, metadata definitions.

use super::*;

pub(crate) unsafe fn load_package_namespace(
    package: &str,
    package_dir: &Path,
    loading: &mut Vec<String>,
) -> Result<(SEXP, Option<NamespaceDirectives>), String> {
    unsafe {
        if package == "compiler" {
            let env = crate::eval::compiler::namespace();
            return Ok((env, None));
        }
        // The namespace is registered before its R code runs, but a cache hit
        // also reinstalls native helpers. Those helpers evaluate package code
        // (`RweaveLatexRuncode <- ...` in utils) which can lazy-load a
        // namespace reference and call back into this function. Returning the
        // in-flight environment here matches GNU: the registry entry is the
        // namespace under construction, and `.onLoad` does not run again.
        let load_guard = NamespaceLoadGuard::enter(package);
        if load_guard.reentered {
            if let Some(env) = cached_package_namespace(package, package_dir) {
                let directives = read_namespace_directives(package_dir).ok().flatten();
                return Ok((env, directives));
            }
            return Err(format!("cyclic namespace load while loading '{package}'"));
        }
        if let Some(env) = cached_package_namespace(package, package_dir) {
            // A completed namespace already has its native descriptors and
            // initialization hooks. Reinstalling them on a lookup mutates the
            // namespace and can rerun package code after it has been sealed.
            if crate::sexp::envir::environment_is_locked_raw(env) {
                return Ok((env, read_namespace_directives(package_dir)?));
            }
            if package == "methods" {
                crate::library::methods::native_calls::install_methods_call_symbols(env);
                retarget_methods_generics(env);
            }

            if package == "tools" {
                crate::library::tools::native_calls::install_tools_call_symbols(env);
                crate::library::tools::native_calls::install_tools_assert_closures(env);
            }

            if package == "stats" {
                crate::library::stats::random::install_stats_call_symbols(env);
            }
            if package == "splines" {
                crate::library::splines::splines::install_splines_call_symbols(env);
            }
            if package == "utils" {
                crate::library::utils::install_utils_call_symbols(env);
            }
            if package == "grDevices" {
                crate::library::grdevices::install_call_symbols(env);
                crate::library::grdevices::colors::initPalette();
                #[cfg(feature = "renderplot-device")]
                crate::mainutils::graphics_recording::install_namespace_replay(env);
            }
            if package == "grid" {
                crate::library::grid::install_call_symbols(env);
            }
            if package == "graphics" {
                crate::library::graphics::install_call_symbols(env);
            }

            let directives = read_namespace_directives(package_dir)?;
            ensure_namespace_info(package, package_dir, env, directives.as_ref());
            return Ok((env, directives));
        }

        // GNU `makeNamespace`: the namespace's enclosure chain ends at
        // `.BaseNamespaceEnv` (parent `.GlobalEnv`), not `baseenv()`
        // (parent `emptyenv()`). `getFunction` stops at `isBaseNamespace`;
        // a chain that never reaches that environment loops on
        // `parent.env(emptyenv())`.
        let package_env = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::envir::R_BaseNamespace(),
            R_NilValue(),
        );
        if package_env.is_null() {
            return Err(format!(
                "could not create namespace for package '{}'",
                package
            ));
        }
        let _package_env_guard = crate::sexp::protect::protect(package_env);

        define_package_name(package, package_env);
        if !is_builtin_package_dependency(package) {
            reject_unsupported_internal_data(package, package_dir)?;
            reject_unsupported_lazyload_code(package, package_dir)?;
        }

        // GNU makeNamespace assigns `ns$.__S3MethodsTable__.` before
        // `.Internal(registerNamespace)` and well before sealNamespace locks
        // the namespace; nothing may add bindings once it is locked below, so
        // every loaded namespace must carry its table from construction on.
        ensure_s3_methods_table(package_env)?;

        // Register the in-flight namespace before sourcing its R files:
        // upstream loadNamespace records the namespace first, so package
        // code that calls `asNamespace(pkg)` mid-load (crayon's
        // `assign(style, make_style(style), envir = asNamespace("crayon"))`)
        // receives the environment under construction instead of triggering
        // a nested load of the same package.
        cache_package_namespace(package, package_dir, package_env);
        if package == "methods" {
            crate::library::methods::native_calls::install_methods_call_symbols(package_env);
        }
        if package == "grDevices" {
            // Its sourced initialization calls color wrappers before the
            // post-load setup; their typed native descriptors must exist now.
            crate::library::grdevices::install_call_symbols(package_env);
        }

        let populated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            populate_package_namespace(package, package_dir, package_env, loading)
        }));
        let namespace = match populated {
            Ok(result) => match result {
                Ok(namespace) => namespace,
                Err(message) => {
                    uncache_package_namespace(package);
                    return Err(message);
                }
            },
            Err(payload) => {
                // Sourced R code errors unwind as RSignal panics; drop the
                // half-built namespace so a later attempt reloads cleanly.
                uncache_package_namespace(package);
                std::panic::resume_unwind(payload);
            }
        };
        if package == "methods" {
            finalize_methods_namespace(package_env);
        }

        if package == "stats" {
            crate::library::stats::random::install_stats_call_symbols(package_env);
        }
        if package == "splines" {
            crate::library::splines::splines::install_splines_call_symbols(package_env);
        }

        if package == "tools" {
            crate::library::tools::native_calls::install_tools_call_symbols(package_env);
            crate::library::tools::native_calls::install_tools_assert_closures(package_env);
        }
        if package == "utils" {
            crate::library::utils::install_utils_call_symbols(package_env);
            ensure_utils_os_version(package_env);
        }
        if package == "grDevices" {
            crate::library::grdevices::install_call_symbols(package_env);
            crate::library::grdevices::colors::initPalette();
            #[cfg(feature = "renderplot-device")]
            crate::mainutils::graphics_recording::install_namespace_replay(package_env);
        }
        if package == "grid" {
            crate::library::grid::install_call_symbols(package_env);
        }
        if package == "graphics" {
            crate::library::graphics::install_call_symbols(package_env);
        }
        if package != "methods" {
            cache_loaded_namespace_metadata(package_env);
        }
        ensure_namespace_info(package, package_dir, package_env, namespace.as_ref());
        crate::sexp::envir::lock_environment_raw(package_env);

        Ok((package_env, namespace))
    }
}

pub(crate) fn normalized_package_dir(package_dir: &Path) -> PathBuf {
    std::fs::canonicalize(package_dir).unwrap_or_else(|_| package_dir.to_path_buf())
}

pub(crate) fn cached_package_namespace(package: &str, package_dir: &Path) -> Option<SEXP> {
    let package_dir = normalized_package_dir(package_dir);
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst)
            .package_namespace_cache
            .get(package)
            .and_then(|(cached_dir, env)| (*cached_dir == package_dir).then_some(*env))
    })
}

pub(crate) fn cache_package_namespace(package: &str, package_dir: &Path, package_env: SEXP) {
    let package_dir = normalized_package_dir(package_dir);
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        if let Some((cached_dir, _)) = (*inst).package_namespace_cache.get(package) {
            if cached_dir.to_string_lossy().starts_with("<builtin:") {
                return;
            }
        }
        (*inst)
            .package_namespace_cache
            .insert(package.to_string(), (package_dir, package_env));
    });
}

pub(crate) fn uncache_package_namespace(package: &str) {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).package_namespace_cache.remove(package);
    });
}

pub(crate) fn cached_namespace_by_name(package: &str) -> Option<SEXP> {
    if package == "base" {
        return Some(unsafe { crate::sexp::envir::R_BaseNamespace() });
    }
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst)
            .package_namespace_cache
            .get(package)
            .map(|(_, env)| *env)
    })
}

pub(crate) fn package_arg_values(package_arg: SEXP) -> Vec<String> {
    unsafe {
        if package_arg.is_null() || package_arg == R_NilValue() || XLENGTH(package_arg) == 0 {
            return Vec::new();
        }
        (0..XLENGTH(package_arg))
            .map(|i| elt_to_string(package_arg, i))
            .filter(|package| !package.is_empty() && package != "NA")
            .collect()
    }
}

pub(crate) fn list_package_data_sets(packages: &[String]) -> Vec<String> {
    let mut names = Vec::<String>::new();
    if find_package_path("datasets").is_empty()
        && (packages.is_empty() || packages.iter().any(|package| package == "datasets"))
    {
        names.extend(crate::library::datasets::topics());
    }
    for package_dir in data_package_dirs(packages) {
        let data_dir = package_dir.join("data");
        let Ok(entries) = std::fs::read_dir(data_dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    ["r", "rda", "rdata"]
                        .iter()
                        .any(|candidate| ext.eq_ignore_ascii_case(candidate))
                })
                && let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
            {
                push_unique(&mut names, name.to_string());
            }
        }
    }
    names.sort();
    names
}

pub(crate) fn data_package_dirs(packages: &[String]) -> Vec<PathBuf> {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        if packages.is_empty() {
            return (*inst)
                .path_policy
                .library_paths()
                .iter()
                .filter_map(|library| std::fs::read_dir(library).ok())
                .flat_map(|entries| entries.filter_map(Result::ok).map(|entry| entry.path()))
                .filter(|path| path.join("DESCRIPTION").is_file())
                .collect();
        }

        packages
            .iter()
            .filter_map(|package| (*inst).path_policy.find_package_path(package))
            .collect()
    })
}

pub(crate) unsafe fn load_package_data_set(
    topic: &str,
    packages: &[String],
    target_env: SEXP,
) -> Result<bool, String> {
    unsafe {
        for package_dir in data_package_dirs(packages) {
            let data_dir = package_dir.join("data");
            let source_file = data_dir.join(format!("{topic}.R"));
            if source_file.is_file() {
                source_r_file_into_env(&source_file, target_env)?;
                return Ok(true);
            }
            let serialized_allowed = crate::sexp::instance::with_current_instance(|inst| unsafe {
                (*inst).path_policy.allows_serialized_data()
            })
            .unwrap_or(true);
            let reject_serialized = |file: &std::path::Path| -> Result<(), String> {
                if serialized_allowed {
                    return Ok(());
                }
                Err(format!(
                    "unsupported serialized/lazy data {}; this pure-R Android runtime supports R source files and data/*.R only",
                    file.display()
                ))
            };
            let rdb = data_dir.join("Rdata.rdb");
            if rdb.is_file() {
                reject_serialized(&rdb)?;
                let scratch = crate::sexp::memory_ext::NewEnvironment(
                    R_NilValue(),
                    crate::sexp::globals::R_EmptyEnv(),
                    R_NilValue(),
                );
                let _scratch = protect(scratch);
                let base = data_dir.join("Rdata");
                eager_lazy_load_package_db(&base, scratch, &[])?;
                let sym = crate::sexp::symbol::Rf_install(
                    std::ffi::CString::new(topic).unwrap_or_default().as_ptr(),
                );
                let value = crate::sexp::envir::R_findVarInFrame(scratch, sym);
                if !value.is_null() && value != crate::sexp::globals::R_UnboundValue() {
                    crate::sexp::envir::defineVar(sym, value, target_env);
                    return Ok(true);
                }
            }

            let per_topic = [
                data_dir.join(format!("{topic}.rda")),
                data_dir.join(format!("{topic}.RData")),
            ];
            if let Some(path) = per_topic.iter().find(|path| path.is_file()) {
                reject_serialized(path)?;
                super::package_data::load_file(path, target_env)?;
                return Ok(true);
            }
        }
        if find_package_path("datasets").is_empty()
            && (packages.is_empty() || packages.iter().any(|package| package == "datasets"))
        {
            let owner = crate::sexp::owner::OwnerToken::current().map_err(|e| e.to_string())?;
            let target = owner
                .sexp(target_env)
                .and_then(crate::sexp::object::Sexp::into_owned)
                .map_err(|e| e.to_string())?;
            return crate::library::datasets::load_topic(topic, target);
        }
        Ok(false)
    }
}

pub(crate) unsafe fn source_r_file_into_env(file: &Path, env: SEXP) -> Result<(), String> {
    unsafe {
        let code = std::fs::read_to_string(file)
            .map_err(|err| format!("could not read {}: {err}", file.display()))?;
        let parser_factory = crate::eval::parser::active_factory();
        let expr = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse(&code, arena, parser_factory.clone())
                .map_err(|err| err.to_string())
        })?;
        let _ = crate::eval::eval::Rf_eval(expr.clone().as_raw(), env);
        Ok(())
    }
}

pub(super) unsafe fn define_package_name(package: &str, package_env: SEXP) {
    unsafe {
        let package_string = Rf_mkString(CString::new(package).unwrap_or_default().as_ptr());
        if !package_string.is_null() {
            crate::sexp::envir::defineVar(package_name_symbol(), package_string, package_env);
        }
    }
}

pub(crate) unsafe fn define_package_metadata(package: &str, package_env: SEXP) {
    unsafe {
        define_package_name(package, package_env);
        let search_name = format!("package:{package}");
        let search_string = Rf_mkString(CString::new(search_name).unwrap_or_default().as_ptr());
        if !search_string.is_null() {
            crate::sexp::attrib_core::setAttrib(package_env, name_symbol(), search_string);
        }
        let package_path = find_package_path(package);
        if !package_path.is_empty() {
            let path_string = Rf_mkString(CString::new(package_path).unwrap_or_default().as_ptr());
            if !path_string.is_null() {
                crate::sexp::attrib_core::setAttrib(
                    package_env,
                    Rf_install(c"path".as_ptr()),
                    path_string,
                );
            }
        }
    }
}

/// GNU utils' `.osVersion()`: a pretty OS description. Assigned into the
/// utils namespace when its `.onLoad` could not (GNU's zzz.R).
pub(super) unsafe fn ensure_utils_os_version(package_env: SEXP) {
    unsafe {
        let sym = Rf_install(c"osVersion".as_ptr());
        let existing = crate::sexp::envir::R_findVarInFrame(package_env, sym);
        if !existing.is_null() && existing != crate::sexp::globals::R_UnboundValue() {
            return;
        }
        let Some(text) = gnu_os_version_string() else {
            return;
        };
        let Ok(cstring) = std::ffi::CString::new(text) else {
            return;
        };
        let value = crate::sexp::constructors::Rf_mkString(cstring.as_ptr());
        let _v = protect(value);
        crate::sexp::envir::defineVar(sym, value, package_env);
    }
}

pub(super) fn gnu_os_version_string() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let plist = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
    let mut version: Option<String> = None;
    for (index, line) in plist.lines().enumerate() {
        if line.contains("ProductUserVisibleVersion") {
            if let Some(next) = plist.lines().nth(index + 1) {
                version = next
                    .split("<string>")
                    .nth(1)
                    .and_then(|rest| rest.split("</string>").next())
                    .map(|ver| ver.to_string());
            }
            break;
        }
    }
    let version = version?;
    let first = version.split('.').next().unwrap_or("");
    let codename = match first {
        "11" => Some("Big Sur"),
        "12" => Some("Monterey"),
        "13" => Some("Ventura"),
        "14" => Some("Sonoma"),
        "15" => Some("Sequoia"),
        "26" => Some("Tahoe"),
        "27" => Some("Golden Gate"),
        _ => None,
    };
    Some(match codename {
        Some(name) => format!("macOS {name} {version}"),
        None => format!("macOS {version}"),
    })
}

pub(super) unsafe fn ensure_s3methods_slot(info: SEXP) {
    unsafe {
        let sym = Rf_install(c"S3methods".as_ptr());
        let existing = crate::sexp::envir::R_findVarInFrame(info, sym);
        if !existing.is_null() && existing != crate::sexp::globals::R_UnboundValue() {
            return;
        }
        let methods = Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(dim) = 0;
        *INTEGER(dim).add(1) = 4;
        crate::sexp::attrib_core::setAttrib(methods, crate::sexp::attrib_core::R_DimSymbol(), dim);
        crate::sexp::envir::defineVar(sym, methods, info);
    }
}

pub(super) unsafe fn record_namespace_s3_metadata(
    package_env: SEXP,
    directives: &NamespaceDirectives,
) {
    unsafe {
        let info_sym = Rf_install(c".__NAMESPACE__.".as_ptr());
        let info = crate::sexp::envir::R_findVarInFrame(package_env, info_sym);
        if info.is_null()
            || info == crate::sexp::globals::R_UnboundValue()
            || TYPEOF(info) != SEXPTYPE::ENVSXP
        {
            return;
        }
        let _info = protect(info);
        let sym = Rf_install(c"S3methods".as_ptr());
        let rows: Vec<_> = directives
            .s3_methods
            .iter()
            .filter(|method| !method.generic.rsplit("::").next().unwrap_or("").is_empty())
            .collect();
        let n = i32::try_from(rows.len())
            .unwrap_or_else(|_| package_error("too many S3 method declarations".to_owned()));
        let methods = Rf_allocVector3(SEXPTYPE::STRSXP, i64::from(n) * 4);
        let _methods = protect(methods);
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        let _dim = protect(dim);
        *INTEGER(dim) = n;
        *INTEGER(dim).add(1) = 4;
        crate::sexp::attrib_core::setAttrib(methods, crate::sexp::attrib_core::R_DimSymbol(), dim);
        // GNU metadata describes all declarations, including unavailable
        // platform-specific methods. Binding registration skips those later.
        for (row, method) in rows.iter().enumerate() {
            let (package, generic) = match method.generic.rsplit_once("::") {
                Some((package, generic)) => (Some(package), generic),
                None => (None, method.generic.as_str()),
            };
            let name = method
                .method
                .clone()
                .unwrap_or_else(|| format!("{generic}.{}", method.class));
            for (col, text) in [
                Some(generic),
                Some(method.class.as_str()),
                Some(name.as_str()),
                package,
            ]
            .into_iter()
            .enumerate()
            {
                let value = match text {
                    Some(text) => Rf_mkChar(CString::new(text).unwrap_or_default().as_ptr()),
                    None => crate::sexp::globals::R_NaString(),
                };
                SET_STRING_ELT(methods, row as i64 + col as i64 * i64::from(n), value);
            }
        }
        crate::sexp::envir::defineVar(sym, methods, info);
    }
}
