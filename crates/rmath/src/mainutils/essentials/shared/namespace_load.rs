//! Namespace loading: symbols, pure-R packages, methods wiring.

use super::*;

pub(crate) unsafe fn package_name_symbol() -> SEXP {
    unsafe { Rf_install(c".packageName".as_ptr()) }
}

pub(crate) unsafe fn name_symbol() -> SEXP {
    unsafe { Rf_install(c"name".as_ptr()) }
}

pub(crate) unsafe fn namespace_env_symbol() -> SEXP {
    unsafe { Rf_install(c".namespaceEnv".as_ptr()) }
}

pub(crate) unsafe fn lazy_data_names_symbol() -> SEXP {
    unsafe { Rf_install(c".lazyDataNames".as_ptr()) }
}

pub(crate) unsafe fn package_name_binding(env: SEXP) -> Option<String> {
    unsafe {
        let value = crate::sexp::envir::R_findVarInFrame(env, package_name_symbol());
        if value.is_null()
            || value == R_NilValue()
            || value == crate::sexp::globals::R_UnboundValue()
            || TYPEOF(value) != SEXPTYPE::STRSXP
            || XLENGTH(value) < 1
        {
            return None;
        }
        Some(elt_to_string(value, 0))
    }
}

pub(crate) unsafe fn package_attached(package: &str) -> bool {
    unsafe {
        let mut env = crate::sexp::accessors::ENCLOS(crate::sexp::globals::R_GlobalEnv());
        let base = crate::sexp::globals::R_BaseEnv();
        while !env.is_null() && env != base {
            if package_name_binding(env).as_deref() == Some(package) {
                return true;
            }
            env = crate::sexp::accessors::ENCLOS(env);
        }
        false
    }
}

pub(super) unsafe fn attached_package_env(package: &str) -> Option<SEXP> {
    unsafe {
        let mut env = crate::sexp::accessors::ENCLOS(crate::sexp::globals::R_GlobalEnv());
        let base = crate::sexp::globals::R_BaseEnv();
        while !env.is_null() && env != base {
            if package_name_binding(env).as_deref() == Some(package) {
                return Some(env);
            }
            env = crate::sexp::accessors::ENCLOS(env);
        }
        None
    }
}

pub(crate) unsafe fn load_pure_r_package(package: &str, package_dir: &Path) -> Result<(), String> {
    let mut loading = Vec::<String>::new();
    unsafe { load_pure_r_package_recursive(package, package_dir, &mut loading) }
}

pub(crate) unsafe fn load_pure_r_package_recursive(
    package: &str,
    package_dir: &Path,
    loading_packages: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        let description = package_dir.join("DESCRIPTION");
        if !description.is_file() {
            return Err(format!("package '{}' has no DESCRIPTION", package));
        }
        if loading_packages.iter().any(|entry| entry == package) {
            return Err(format!(
                "cyclic package dependency while loading '{}': {} -> {}",
                package,
                loading_packages.join(" -> "),
                package
            ));
        }
        if package_needs_compilation(&description)? && !is_builtin_package_dependency(package) {
            return Err(format!(
                "package '{}' declares NeedsCompilation: yes; this pure-R Android runtime does not load compiled package code",
                package
            ));
        }

        loading_packages.push(package.to_string());
        let result = (|| {
            load_package_dependencies(package, package_dir, loading_packages)?;

            let mut loading = vec![package.to_string()];
            let (package_env, namespace) =
                load_package_namespace(package, package_dir, &mut loading)?;
            let _package_env_guard = crate::sexp::protect::protect(package_env);
            let attach_env = if package == "utils" {
                make_package_attach_env_lenient(package, namespace.as_ref(), package_env)?
            } else {
                make_package_attach_env(package, namespace.as_ref(), package_env)?
            };
            if package == "methods" {
                bind_methods_base_primitives(package_env);
                purge_missing_arg_placeholders(package_env);
                retarget_methods_generics(package_env);
            }
            attach_package_env(attach_env);
            if package == "utils" {
                install_utils_str_option(package_env);
            }
            if package == "methods" {
                run_methods_onload_cache_metadata(package_env);
                export_s4_metadata_to_package_env(package_env, attach_env);
                retarget_envref_object_parent(package_env);
            }

            Ok(())
        })();
        loading_packages.pop();
        result
    }
}

pub(crate) unsafe fn load_package_dependencies(
    package: &str,
    package_dir: &Path,
    loading_packages: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        for dependency in package_depends_names(package_dir)? {
            if is_builtin_package_dependency(&dependency) || package_attached(&dependency) {
                continue;
            }
            let dependency_path = find_package_path(&dependency);
            if dependency_path.is_empty() {
                return Err(format!(
                    "package '{}' depends on missing package '{}'",
                    package, dependency
                ));
            }
            load_pure_r_package_recursive(
                &dependency,
                Path::new(&dependency_path),
                loading_packages,
            )?;
        }
        Ok(())
    }
}

pub(crate) fn package_needs_compilation(description: &Path) -> Result<bool, String> {
    let content = std::fs::read_to_string(description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content)
        .get("NeedsCompilation")
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true")
        }))
}

pub(crate) fn package_declares_lazy_data(package_dir: &Path) -> Result<bool, String> {
    let description = package_dir.join("DESCRIPTION");
    let content = std::fs::read_to_string(&description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content)
        .get("LazyData")
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true")
        }))
}

pub(crate) fn description_fields(description: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::<String, String>::new();
    let mut current_key: Option<String> = None;

    for line in description.lines() {
        if line.trim().is_empty() {
            break;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(key) = current_key.as_ref()
                && let Some(value) = fields.get_mut(key)
            {
                if !value.is_empty() {
                    value.push('\n');
                }
                value.push_str(line.trim());
            }
            continue;
        }

        let Some((field, value)) = line.split_once(':') else {
            current_key = None;
            continue;
        };
        let field = field.trim();
        if field.is_empty() || field.chars().any(char::is_whitespace) {
            current_key = None;
            continue;
        }
        let field = field.to_string();
        fields.insert(field.clone(), value.trim().to_string());
        current_key = Some(field);
    }

    fields
}

pub(crate) fn description_file_list(value: &str) -> Vec<String> {
    let mut files = Vec::new();
    let mut chars = value.char_indices().peekable();

    while let Some((_, ch)) = chars.peek().copied() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        let mut file = String::new();
        if ch == '\'' || ch == '"' {
            let quote = ch;
            chars.next();
            let mut escaped = false;
            for (_, next) in chars.by_ref() {
                if escaped {
                    file.push(next);
                    escaped = false;
                } else if next == '\\' {
                    escaped = true;
                } else if next == quote {
                    break;
                } else {
                    file.push(next);
                }
            }
        } else {
            while let Some((_, next)) = chars.peek().copied() {
                if next.is_whitespace() {
                    break;
                }
                file.push(next);
                chars.next();
            }
        }

        if !file.trim().is_empty() {
            push_unique(&mut files, file.trim().to_string());
        }
    }

    files
}

pub(crate) fn description_package_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .filter_map(|entry| {
            let name = entry
                .split_once('(')
                .map(|(name, _)| name)
                .unwrap_or(entry)
                .trim();
            (!name.is_empty()).then(|| name.to_string())
        })
        .fold(Vec::<String>::new(), |mut names, name| {
            push_unique(&mut names, name);
            names
        })
}

pub(crate) fn package_depends_names(package_dir: &Path) -> Result<Vec<String>, String> {
    let description = package_dir.join("DESCRIPTION");
    let content = std::fs::read_to_string(&description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content)
        .get("Depends")
        .map(|value| description_package_list(value))
        .unwrap_or_default())
}

pub(crate) fn is_builtin_package_dependency(package: &str) -> bool {
    matches!(
        package,
        "R" | "base"
            | "compiler"
            | "datasets"
            | "grDevices"
            | "graphics"
            | "grid"
            | "methods"
            | "parallel"
            | "splines"
            | "stats"
            | "stats4"
            | "tcltk"
            | "tools"
            | "utils"
    )
}

pub(crate) unsafe fn bind_methods_base_primitives(ns: SEXP) {
    unsafe {
        for name in [
            "standardGeneric",
            "ngettext",
            "gettext",
            "bindtextdomain",
            "dgettext",
            "asS4",
            ".asS4",
            "parent.env",
            "parent.env<-",
            "list2env",
            "as.environment",
        ] {
            let symbol = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            let value = crate::sexp::envir::R_findVarInFrame(ns, symbol);
            let kind = TYPEOF(value);
            if kind == SEXPTYPE::CLOSXP
                || kind == SEXPTYPE::BUILTINSXP
                || kind == SEXPTYPE::SPECIALSXP
            {
                continue;
            }
            let prim = crate::eval::primitive::make_primitive_binding(name, SEXPTYPE::BUILTINSXP);
            if !prim.is_null() && prim != R_NilValue() {
                crate::sexp::envir::defineVar(symbol, prim, ns);
            }
        }
    }
}

/// Finish the same original namespace lifecycle for installed and portable images.
/// No runtime/field loan remains live while a step can evaluate R code.
pub(crate) unsafe fn finalize_methods_namespace(namespace: SEXP) {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let namespace = owner
            .sexp(namespace)
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let authority = crate::sexp::owner::StoredOwner::from_value(&namespace)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let _pin = owner
            .pin()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        for step in [
            bind_methods_base_primitives as unsafe fn(SEXP),
            purge_missing_arg_placeholders,
            retarget_methods_generics,
            run_methods_onload_cache_metadata,
            retarget_envref_object_parent,
        ] {
            authority
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            step(namespace.as_raw());
            authority
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
    }
}

/// Restore the session-specific methods dispatch state after loading its image.
/// The installed image contains its class and implicit-generic tables already;
/// GNU's installed `.onLoad` initializes dispatch without recaching that namespace.
pub(crate) unsafe fn run_methods_onload_cache_metadata(where_env: SEXP) {
    unsafe {
        if where_env.is_null() || where_env == R_NilValue() {
            return;
        }
        let Some(ns) = cached_namespace_by_name("methods") else {
            return;
        };
        eval_methods_ns_fun(ns, c".initImplicitGenerics", where_env, None);
        let captured_image = crate::sexp::instance::with_required_current_instance(|instance| {
            (*instance)
                .package_namespace_cache
                .get("methods")
                .is_some_and(|(path, namespace)| {
                    *namespace == ns && path == Path::new("<builtin:methods>")
                })
        });
        // The original installed image includes .implicitTable and its method
        // environments after registration. Re-registering replays loading work
        // that GNU's installed .onLoad does not perform.
        if !captured_image {
            register_implicit_generics_table(ns);
        }
        // GNU zzz.R: initMethodDispatch(where); .Call(C_R_set_method_dispatch, TRUE).
        // The installed image already ran onLoad, so the session must still
        // install the standardGeneric pointer or .isMethodsDispatchOn() stays FALSE.
        // Set table_dispatch_on first: R_initMethodDispatch reads that bit to
        // choose R_dispatchGeneric / R_quick_dispatch (GNU post-onload state).
        let on = Rf_ScalarLogical(TRUE);
        let _on = protect(on);
        crate::library::methods::methods_list_dispatch::R_set_method_dispatch(on);
        crate::library::methods::methods_list_dispatch::R_initMethodDispatch(ns);

        if let Some(attach_env) = attached_package_env("methods") {
            export_s4_metadata_to_package_env(ns, attach_env);
        }
        {
            let primitive = crate::sexp::envir::R_findVarInFrame(
                crate::sexp::globals::R_BaseEnv(),
                Rf_install(c"$".as_ptr()),
            );
            // Admit primitive dispatch for both installed and captured namespaces;
            // restore the original
            // generic when the first S4 lookup resets the method table.
            crate::mainutils::objects::do_set_prim_method(
                primitive,
                c"reset".as_ptr(),
                R_NilValue(),
                R_NilValue(),
            );
        }
        if !captured_image {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::mainutils::gram_main::R_ParseEvalString(
                    c"cd <- tryCatch(getClassDef(\"envRefClass\"), error=function(e) NULL)
if (!is.null(cd) && is.environment(cd@refMethods)) {
  op <- cd@refMethods$.objectParent
  ns <- asNamespace(\"methods\")
  if (is.environment(op) && !identical(op, ns)) parent.env(op) <- ns
}

NULL"
                        .as_ptr(),
                    ns,
                )
            }));
        }
    }
}

/// Materialize the original methods generic only when S4 primitive dispatch
/// first requests it. Ordinary console and plotting work need no S4 table.
pub(crate) unsafe fn ensure_methods_primitive_generic(op: SEXP) {
    unsafe {
        if op.is_null() || crate::mainutils::objects::R_primitive_generic(op) != R_NilValue() {
            return;
        }
        let Some(ns) = cached_namespace_by_name("methods") else {
            return;
        };
        let name = std::ffi::CStr::from_ptr(crate::mainutils::relop::PRIMNAME(op));
        register_original_primitive_generic(ns, name);
    }
}

pub(crate) unsafe fn ensure_methods_primitive_generic_for_object(
    op: SEXP,
    object: SEXP,
) -> bool {
    unsafe {
        if op.is_null() {
            return false;
        }
        if crate::mainutils::objects::R_primitive_generic(op) != R_NilValue() {
            return true;
        }
        let Some(ns) = cached_namespace_by_name("methods") else {
            return false;
        };
        let name = std::ffi::CStr::from_ptr(crate::mainutils::relop::PRIMNAME(op));

        // Primitive generics such as `names` and `length` include an `ANY`
        // method in the pinned `.BasicFunsList`. Materializing that generic
        // for every S4 value can retain a large dispatch environment even
        // when this owner's method table has no class-specific entry. Use the
        // current owner generic table as evidence first. Unknown or
        // multi-argument signatures take the full path to preserve dispatch.
        let table = crate::sexp::envir::R_findVarInFrame(
            ns,
            Rf_install(c".BasicFunsList".as_ptr()),
        );
        if table.is_null() || table == R_NilValue() || TYPEOF(table) != SEXPTYPE::VECSXP {
            return true;
        }
        let names = crate::sexp::attrib_core::getAttrib(
            table,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return true;
        }
        let mut generic_def = R_NilValue();
        for index in 0..crate::sexp::accessors::XLENGTH(table) {
            let generic_name = crate::sexp::accessors::STRING_ELT(names, index);
            if generic_name.is_null() {
                continue;
            }
            let generic_name = crate::sexp::accessors::CHAR(generic_name);
            if generic_name.is_null()
                || std::ffi::CStr::from_ptr(generic_name).to_bytes() != name.to_bytes()
            {
                continue;
            }
            generic_def = crate::sexp::accessors::VECTOR_ELT(table, index);
            break;
        }
        if generic_def.is_null() || generic_def == R_NilValue() {
            return true;
        }
        if TYPEOF(generic_def) != SEXPTYPE::CLOSXP {
            return true;
        }
        let generic_env = crate::sexp::accessors::CLOENV(generic_def);
        if generic_env.is_null()
            || generic_env == R_NilValue()
            || TYPEOF(generic_env) != SEXPTYPE::ENVSXP
        {
            return true;
        }
        let sig_length = crate::sexp::envir::R_findVarInFrame(
            generic_env,
            Rf_install(c".SigLength".as_ptr()),
        );
        if sig_length.is_null()
            || sig_length == R_NilValue()
            || TYPEOF(sig_length) != SEXPTYPE::INTSXP
            || crate::sexp::accessors::XLENGTH(sig_length) == 0
            || *crate::sexp::accessors::INTEGER(sig_length) != 1
        {
            return true;
        }
        let methods_table = crate::sexp::envir::R_findVarInFrame(
            generic_env,
            Rf_install(c".AllMTable".as_ptr()),
        );
        if methods_table.is_null()
            || methods_table == R_NilValue()
            || TYPEOF(methods_table) != SEXPTYPE::ENVSXP
        {
            return true;
        }
        let classes = crate::mainutils::objects::R_data_class2(object);
        if classes.is_null() || classes == R_NilValue() || TYPEOF(classes) != SEXPTYPE::STRSXP {
            return true;
        }
        let _classes_guard = protect(classes);
        let mut has_specific_method = false;
        for index in 0..crate::sexp::accessors::XLENGTH(classes) {
            let class = crate::sexp::accessors::STRING_ELT(classes, index);
            if class.is_null() {
                continue;
            }
            let class_name = crate::sexp::accessors::CHAR(class);
            if class_name.is_null() {
                continue;
            }
            let class_symbol = crate::sexp::symbol::Rf_install(class_name);
            let method = crate::sexp::envir::R_findVarInFrame(methods_table, class_symbol);
            if !method.is_null()
                && method != crate::sexp::globals::R_UnboundValue()
                && method != R_NilValue()
            {
                let is_any = std::ffi::CStr::from_ptr(class_name).to_bytes() == b"ANY";
                // The unmodified `.BasicFunsList` uses the primitive itself
                // as the ANY fallback. It does not need S4 method dispatch;
                // an explicit user ANY method is a closure and must dispatch.
                if is_any
                    && matches!(
                        TYPEOF(method),
                        kind if kind == SEXPTYPE::BUILTINSXP.as_c_int()
                            || kind == SEXPTYPE::SPECIALSXP.as_c_int()
                    )
                {
                    continue;
                }
                has_specific_method = true;
                break;
            }
        }
        if !has_specific_method {
            return false;
        }
        register_original_primitive_generic(ns, name);
        crate::mainutils::objects::R_primitive_generic(op) != R_NilValue()
    }
}

/// Materialize a primitive generic from the pinned methods namespace's
/// `.BasicFunsList` without recursively calling getGeneric during dispatch.
pub(super) unsafe fn register_original_primitive_generic(
    ns: SEXP,
    primitive_name: &std::ffi::CStr,
) {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| package_error(error.to_string()));
        let namespace = owner
            .sexp(ns)
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|error| package_error(error.to_string()));
        let authority = crate::sexp::owner::StoredOwner::from_value(&namespace)
            .unwrap_or_else(|error| package_error(error.to_string()));
        let _pin = owner
            .pin()
            .unwrap_or_else(|error| package_error(error.to_string()));
        let table =
            crate::sexp::envir::R_findVarInFrame(ns, Rf_install(c".BasicFunsList".as_ptr()));
        let table = owner
            .sexp(table)
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|error| package_error(error.to_string()));
        let table_raw = if TYPEOF(table.as_raw()) == SEXPTYPE::PROMSXP {
            crate::sexp::envir::forcePromise(table.as_raw())
        } else {
            table.as_raw()
        };
        authority
            .require_active()
            .unwrap_or_else(|error| package_error(error.to_string()));
        let table = owner
            .sexp(table_raw)
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|error| package_error(error.to_string()));
        let names = owner
            .sexp(crate::sexp::attrib_core::getAttrib(
                table.as_raw(),
                crate::sexp::attrib_core::R_NamesSymbol(),
            ))
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|error| package_error(error.to_string()));
        for index in 0..table.len() {
            if names
                .try_string_elt(index)
                .and_then(|name| name.try_char_eq(primitive_name.to_bytes()))
                .unwrap_or_else(|error| package_error(error.to_string()))
            {
                let generic = table
                    .try_vector_elt(index)
                    .unwrap_or_else(|error| package_error(error.to_string()));
                let primitive = crate::sexp::envir::R_findVarInFrame(
                    crate::sexp::globals::R_BaseEnv(),
                    Rf_install(primitive_name.as_ptr()),
                );
                crate::mainutils::objects::do_set_prim_method(
                    primitive,
                    c"reset".as_ptr(),
                    generic.as_raw(),
                    R_NilValue(),
                );
                return;
            }
        }
        package_error(format!(
            "methods namespace has no primitive generic {}",
            primitive_name.to_string_lossy()
        ));
    }
}

/// GNU `library()`/`attach()`: `methods:::cacheMetaData(env, TRUE)`.
/// GNU bytecode for cacheMetaData GETFUN/CALL-loops on a package attach
/// env; run the stored source for that one call and restore BODY after.
pub(crate) unsafe fn cache_attached_package_metadata(attach_env: SEXP) {
    unsafe {
        if attach_env.is_null() {
            return;
        }
        let Some(methods_ns) = cached_namespace_by_name("methods") else {
            return;
        };
        let mut fun =
            crate::sexp::envir::R_findVarInFrame(methods_ns, Rf_install(c"cacheMetaData".as_ptr()));
        if fun.is_null() || fun == crate::sexp::globals::R_UnboundValue() {
            return;
        }
        if TYPEOF(fun) == SEXPTYPE::PROMSXP {
            fun = crate::sexp::envir::forcePromise(fun);
        }
        if TYPEOF(fun) != SEXPTYPE::CLOSXP {
            return;
        }
        let _source_body = methods_metadata_source_body(fun);
        let attach = Rf_ScalarLogical(TRUE);
        let _attach = protect(attach);
        let call = crate::sexp::constructors::Rf_lang3(fun, attach_env, attach);
        let _call = protect(call);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(call, crate::sexp::globals::R_GlobalEnv())
        }));
    }
}

/// Register the installed implicit prototypes without importing their packages.
/// GNU methods keeps the stats-labeled toeplitz prototype in this table even
/// while the actual stats namespace is unloaded. Its closure is supplied later
/// by the ordinary stats namespace loader, retaining that namespace's identity.
pub(super) unsafe fn register_implicit_generics_table(ns: SEXP) {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| package_error(error.to_string()));
        let runtime = owner
            .weak_owner()
            .unwrap_or_else(|| package_error("implicit generics require a managed runtime"));
        let namespace = owner
            .sexp(ns)
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|error| package_error(error.to_string()));
        crate::sexp::owner::with_runtime(&runtime, |access| {
            let domain = access.domain();
            let parsed = access.with_arena(|arena| {
                crate::eval::parser::parse_expressions(
                    "registerImplicitGenerics(where = asNamespace(\"methods\"))",
                    arena,
                    domain,
                )
            })?;
            access.with_native(|_| {
                crate::eval::parser::flush_literal_warnings();
                Ok(())
            })?;
            let Ok(expressions) = parsed else {
                return Ok(());
            };
            // Keep the original namespace and actual parsed expression owners
            // through every allocating/collecting callback. Original cleanup
            // remains valid if callbacks revoke the session.
            let evaluation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for expression in &expressions {
                    access.with_native(|token| {
                        let result =
                            crate::eval::eval::Rf_eval(expression.as_raw(), namespace.as_raw());
                        token.sexp(result)?.into_owned()?;
                        Ok(())
                    })?;
                }
                Ok::<(), crate::sexp::object::SexpError>(())
            }));
            access.require_active()?;
            if let Ok(result) = evaluation {
                result?;
            }
            Ok::<(), crate::sexp::object::SexpError>(())
        })
        .and_then(|result| result)
        .unwrap_or_else(|error| package_error(error.to_string()));
    }
}

/// Retain the exact compiled body while evaluating its stored R expression.
/// Only cacheMetaData uses this fallback. Its invocation must also suspend
/// JIT compilation: applyClosure checks JIT before reading BODY and otherwise
/// recompiles this temporary source immediately. Arguments and environments
/// remain unchanged; the owner's prior JIT setting is restored after the call.
pub(super) struct MethodsMetadataSourceBody<'s> {
    function: crate::sexp::object::Sexp<'s>,
    original: crate::sexp::object::Sexp<'s>,
    allocation: crate::sexp::heap::CheckedNode,
    owner: crate::sexp::owner::OwnerToken<'s>,
    availability: crate::sexp::instance::InstanceLiveness,
    jit_enabled: i32,
}

impl Drop for MethodsMetadataSourceBody<'_> {
    fn drop(&mut self) {
        let original = self
            .original
            .allocation()
            .expect("compiled body has an allocation");
        if self.availability.is_live() {
            // Both nodes remain owned; no storage loan or callback crosses the
            // barrier. Teardown still restores the retained canonical body.
            unsafe {
                if !crate::sexp::gengc::write_barrier_in(
                    self.owner.as_ptr(),
                    self.function.as_raw(),
                    self.original.as_raw(),
                ) {
                    std::alloc::handle_alloc_error(std::alloc::Layout::new::<SEXP>());
                }
            }
        }
        self.allocation
            .heap_identity()
            .set_edge(
                &self.allocation,
                crate::sexp::ffi::EdgeField::ClosureBody,
                crate::sexp::heap::ReferenceChild::Node(original),
            )
            .expect("owned methods closure retains its original body");
        if self.availability.is_live()
            && crate::eval::jit::get_R_jit_enabled_in(self.owner.as_ptr()) == 0
        {
            // Preserve an explicit callback change to a different JIT level.
            crate::eval::jit::set_R_jit_enabled_in(self.owner.as_ptr(), self.jit_enabled);
        }
    }
}

pub(super) unsafe fn methods_metadata_source_body<'s>(
    fun: SEXP,
) -> Option<MethodsMetadataSourceBody<'s>> {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current().ok()?;
        let factory = crate::sexp::object::SessionNodeFactory::new(owner);
        let function = factory.wrap(fun).ok()?;
        let original = function.try_body().ok()?;
        if !original.is_bytecode() {
            return None;
        }
        let source = factory
            .wrap(crate::eval::bc_eval::BCODE_EXPR(original.as_raw()))
            .ok()?;
        if source.typeof_() != SEXPTYPE::LANGSXP {
            return None;
        }
        let allocation = function.allocation().ok()?.clone();
        let source_allocation = source.allocation().ok()?;
        let guard = MethodsMetadataSourceBody {
            function,
            original,
            allocation,
            owner,
            availability: crate::sexp::instance::instance_liveness(owner.as_ptr()),
            jit_enabled: crate::eval::jit::get_R_jit_enabled_in(owner.as_ptr()),
        };
        if !crate::sexp::gengc::write_barrier_in(
            owner.as_ptr(),
            guard.function.as_raw(),
            source.as_raw(),
        ) {
            std::alloc::handle_alloc_error(std::alloc::Layout::new::<SEXP>());
        }
        guard.allocation.heap_identity().set_edge(
            &guard.allocation,
            crate::sexp::ffi::EdgeField::ClosureBody,
            crate::sexp::heap::ReferenceChild::Node(source_allocation),
        )?;
        crate::eval::jit::set_R_jit_enabled_in(owner.as_ptr(), 0);
        Some(guard)
    }
}

pub(super) unsafe fn eval_methods_ns_fun(
    ns: SEXP,
    name: &std::ffi::CStr,
    where_env: SEXP,
    attach: Option<SEXP>,
) {
    unsafe {
        let mut fun = crate::sexp::envir::R_findVarInFrame(ns, Rf_install(name.as_ptr()));
        if fun.is_null() || fun == crate::sexp::globals::R_UnboundValue() {
            return;
        }
        if TYPEOF(fun) == SEXPTYPE::PROMSXP {
            fun = crate::sexp::envir::forcePromise(fun);
        }
        if TYPEOF(fun) != SEXPTYPE::CLOSXP {
            return;
        }
        let _source_body = if name == c"cacheMetaData" {
            methods_metadata_source_body(fun)
        } else {
            None
        };
        let call = if let Some(attach) = attach {
            crate::sexp::constructors::Rf_lang3(fun, where_env, attach)
        } else {
            crate::sexp::constructors::Rf_lang2(fun, where_env)
        };
        let _call = protect(call);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(call, ns)
        }));
    }
}

pub(super) unsafe fn purge_missing_arg_placeholders(env: SEXP) {
    unsafe {
        let missing = crate::sexp::globals::R_MissingArg();
        let mut doomed = Vec::new();
        for name in frame_binding_names(env, true) {
            let Ok(cname) = CString::new(name.as_str()) else {
                continue;
            };
            let symbol = Rf_install(cname.as_ptr());
            let value = crate::sexp::envir::R_findVarInFrame(env, symbol);
            if value.is_null() || value == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            let kind = TYPEOF(value);
            let empty_symbol =
                kind == SEXPTYPE::SYMSXP && symbol_name(value).is_none_or(|n| n.is_empty());
            let shadows_base = kind == SEXPTYPE::SYMSXP
                && (crate::eval::builtin::has_builtin_handler(&name)
                    || crate::eval::primitive::fun_tab_index_by_name(&name).is_some());
            if value == missing || empty_symbol || shadows_base {
                doomed.push(name);
            }
        }
        for name in doomed {
            let Ok(cname) = CString::new(name) else {
                continue;
            };
            crate::sexp::envir::remove_binding_raw(env, Rf_install(cname.as_ptr()));
        }
    }
}

pub(super) unsafe fn retarget_methods_generics(ns: SEXP) {
    unsafe {
        let methods_ns_sym = Rf_install(c".methodsNamespace".as_ptr());
        let old_ns = crate::sexp::envir::R_findVarInFrame(ns, methods_ns_sym);
        // `.InitRefClasses` captured this stub as envRefClass@refMethods$.objectParent.
        if !old_ns.is_null()
            && old_ns != crate::sexp::globals::R_UnboundValue()
            && TYPEOF(old_ns) == SEXPTYPE::ENVSXP
            && old_ns != ns
            && !env_chain_contains(ns, old_ns)
        {
            SET_ENCLOS(old_ns, ns);
        }

        crate::sexp::envir::defineVar(methods_ns_sym, ns, ns);
        for name in ["initialize", "new", "setClass", "getClass", "getClassDef"] {
            let symbol = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            let mut value = crate::sexp::envir::R_findVarInFrame(ns, symbol);
            if value.is_null() || value == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            if TYPEOF(value) == SEXPTYPE::PROMSXP {
                value = crate::sexp::accessors::PRVALUE(value);
            }
            if TYPEOF(value) != SEXPTYPE::CLOSXP {
                continue;
            }
            let cloenv = crate::sexp::accessors::CLOENV(value);
            if cloenv.is_null() || cloenv == ns {
                continue;
            }
            // Do not steal a generic's table environment; only enclose it.
            if crate::sexp::accessors::ENCLOS(cloenv) != ns {
                crate::sexp::accessors::SET_ENCLOS(cloenv, ns);
            }
        }
    }
}

pub(super) unsafe fn env_chain_contains(mut env: SEXP, target: SEXP) -> bool {
    unsafe {
        for _ in 0..64 {
            if env.is_null() || env == R_NilValue() {
                return false;
            }
            if env == target {
                return true;
            }
            if TYPEOF(env) != SEXPTYPE::ENVSXP {
                return false;
            }
            env = crate::sexp::accessors::ENCLOS(env);
        }
        false
    }
}

/// After methods `.onLoad`, the captured `.objectParent` stub must enclose
/// the methods namespace so ref methods see unexported helpers (`classLabel`).
pub(super) unsafe fn retarget_envref_object_parent(ns: SEXP) {
    unsafe {
        let class_def =
            crate::sexp::envir::R_findVarInFrame(ns, Rf_install(c".__C__envRefClass".as_ptr()));
        if class_def.is_null()
            || class_def == crate::sexp::globals::R_UnboundValue()
            || class_def == R_NilValue()
            || TYPEOF(class_def) == SEXPTYPE::PROMSXP
        {
            return;
        }
        retarget_envref_definition_parent(class_def, ns);
    }
}

/// Repair the restored reference-class definition when its lazy record is
/// actually requested, retaining GNU's deferred class-loading behavior.
pub(crate) unsafe fn retarget_envref_definition_parent(class_def: SEXP, ns: SEXP) {
    unsafe {
        let ref_methods =
            crate::eval::attrib_core::getAttrib(class_def, Rf_install(c"refMethods".as_ptr()));
        if TYPEOF(ref_methods) != SEXPTYPE::ENVSXP {
            return;
        }
        let object_parent = crate::sexp::envir::R_findVarInFrame(
            ref_methods,
            Rf_install(c".objectParent".as_ptr()),
        );
        if object_parent.is_null()
            || object_parent == crate::sexp::globals::R_UnboundValue()
            || TYPEOF(object_parent) != SEXPTYPE::ENVSXP
            || object_parent == ns
            || env_chain_contains(ns, object_parent)
        {
            return;
        }
        SET_ENCLOS(object_parent, ns);
    }
}

/// GNU `utils:::.onLoad` sets `options(str = strOptions())`.
pub(super) unsafe fn install_utils_str_option(env: SEXP) {
    unsafe {
        let src = crate::sexp::constructors::Rf_mkString(
            b"if (is.null(getOption(\"str\"))) options(str = strOptions())\0".as_ptr()
                as *const std::os::raw::c_char,
        );
        let _src = crate::sexp::protect::protect(src);
        let mut status = 0i32;
        let parsed = crate::mainutils::gram_main::R_ParseVector(
            src,
            -1,
            &mut status,
            crate::sexp::globals::R_NilValue(),
        );
        if status == 1 && !parsed.is_null() {
            let _parsed = crate::sexp::protect::protect(parsed);
            let expr = crate::sexp::accessors::VECTOR_ELT(parsed, 0);
            let _ = crate::eval::eval::Rf_eval(expr, env);
        }
    }
}

/// Pops a non-reentrant load from the session that pushed it.
pub(super) struct NamespaceLoadGuard {
    instance: *mut crate::sexp::instance::RInstance,
    package: String,
    pub(super) reentered: bool,
}

impl NamespaceLoadGuard {
    pub(super) fn enter(package: &str) -> Self {
        let Some(instance) = crate::sexp::instance::current_instance_ptr() else {
            return Self {
                instance: std::ptr::null_mut(),
                package: package.to_string(),
                reentered: false,
            };
        };
        // The pointer is copied out of the thread-local before the stack is
        // touched, so this does not re-borrow that cell. Drop pops the same
        // session even if another session is ambient.
        let reentered = unsafe {
            let stack = &mut (*instance).namespace_loads_in_progress;
            let reentered = stack.iter().any(|name| name == package);
            if !reentered {
                stack.push(package.to_string());
            }
            reentered
        };
        Self {
            instance,
            package: package.to_string(),
            reentered,
        }
    }
}

impl Drop for NamespaceLoadGuard {
    fn drop(&mut self) {
        if self.reentered || self.instance.is_null() {
            return;
        }
        unsafe {
            let stack = &mut (*self.instance).namespace_loads_in_progress;
            if let Some(index) = stack.iter().rposition(|name| name == &self.package) {
                stack.remove(index);
            }
        }
    }
}
