//! NAMESPACE file parsing and exports.

use super::*;

pub(crate) fn read_namespace_directives(
    package_dir: &Path,
) -> Result<Option<NamespaceDirectives>, String> {
    let owner = unsafe { crate::sexp::owner::OwnerToken::current() }.ok();
    let _pin = owner
        .map(|owner| owner.pin())
        .transpose()
        .map_err(|error| error.to_string())?;
    if let Some(owner) = owner
        && let Some(hit) = unsafe {
            (*owner.as_ptr())
                .namespace_directives_cache
                .get(package_dir)
                .cloned()
        }
    {
        return Ok(Some(hit));
    }
    let namespace = package_dir.join("NAMESPACE");
    if !namespace.is_file() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&namespace)
        .map_err(|err| format!("could not read {}: {err}", namespace.display()))?;
    let content = if parse_namespace_calls(&strip_namespace_comments(&content))
        .iter()
        .any(|(name, _)| name == "if")
    {
        namespace_conditions::selected_source(&content)?
    } else {
        content
    };
    let directives = parse_namespace_directives(&content);
    if let Some(owner) = owner {
        owner.require_active().map_err(|error| error.to_string())?;
        unsafe {
            (*owner.as_ptr())
                .namespace_directives_cache
                .insert(package_dir.to_owned(), directives.clone());
        }
    }
    Ok(Some(directives))
}

/// `::` checks exports on every lookup. Parsing `NAMESPACE` again each time
/// dominates `UseMethod` on bytecode. The file is immutable for a session.
pub(crate) fn cached_namespace_directives(package_dir: &Path) -> Option<NamespaceDirectives> {
    read_namespace_directives(package_dir).ok().flatten()
}

/// Public `::` checks the live exports table. Legacy namespaces without that
/// metadata use cached directives and test exportPattern against the live frame.
pub(crate) unsafe fn namespace_exports_contains(
    package_dir: &Path,
    package_env: SEXP,
    name: &str,
) -> bool {
    unsafe {
        // Portable namespaces have no host NAMESPACE file. Their original
        // exports environment is also the live authority for namespaceExport
        // additions; a leading dot does not make an explicit export private.
        if let Some(exports) = namespace_export_environment(package_env) {
            let Ok(name) = CString::new(name) else {
                return false;
            };
            let value = crate::sexp::envir::R_findVarInFrame(exports, Rf_install(name.as_ptr()));
            return !value.is_null() && value != crate::sexp::globals::R_UnboundValue();
        }
        let Some(directives) = cached_namespace_directives(package_dir) else {
            return frame_binding_names(package_env, false)
                .iter()
                .any(|n| n == name);
        };
        if directives.exports.iter().any(|export| export == name) {
            return true;
        }
        if !directives
            .export_patterns
            .iter()
            .any(|pattern| simple_namespace_pattern_matches(pattern, name))
        {
            return false;
        }
        let c_name = CString::new(name).unwrap_or_else(|_| CString::new("").unwrap());
        let value = crate::sexp::envir::R_findVarInFrame(package_env, Rf_install(c_name.as_ptr()));
        !value.is_null() && value != crate::sexp::globals::R_UnboundValue()
    }
}

pub(crate) fn parse_namespace_directives(content: &str) -> NamespaceDirectives {
    let mut directives = NamespaceDirectives::default();
    let uncommented = strip_namespace_comments(content);
    for (directive, args) in parse_namespace_calls(&uncommented) {
        match directive.as_str() {
            "export" | "exportMethods" => {
                for name in split_namespace_args(&args)
                    .into_iter()
                    .filter_map(clean_namespace_name)
                {
                    push_unique(&mut directives.exports, name);
                }
            }
            "exportClasses" => {
                for name in split_namespace_args(&args)
                    .into_iter()
                    .filter_map(clean_namespace_name)
                {
                    push_unique(&mut directives.exports, format!(".__C__{name}"));
                }
            }

            "exportPattern" => {
                if let Some(pattern) = split_namespace_args(&args)
                    .first()
                    .and_then(|arg| clean_namespace_name(arg))
                {
                    push_unique(&mut directives.export_patterns, pattern);
                }
            }
            "import" => {
                if let Some(package) = split_namespace_args(&args)
                    .first()
                    .and_then(|arg| clean_namespace_name(arg))
                {
                    directives.imports.push(NamespaceImport::All { package });
                }
            }
            "importFrom" => {
                let parts = split_namespace_args(&args);
                let Some(package) = parts.first().and_then(|arg| clean_namespace_name(arg)) else {
                    continue;
                };
                let names = parts
                    .iter()
                    .skip(1)
                    .filter_map(|arg| clean_namespace_name(arg))
                    .collect::<Vec<_>>();
                if !names.is_empty() {
                    directives
                        .imports
                        .push(NamespaceImport::From { package, names });
                }
            }
            "S3method" => {
                let parts = split_namespace_args(&args);
                let Some(generic) = parts.first().and_then(|arg| clean_namespace_name(arg)) else {
                    continue;
                };
                let Some(class) = parts.get(1).and_then(|arg| clean_namespace_name(arg)) else {
                    continue;
                };
                let method = parts.get(2).and_then(|arg| clean_namespace_name(arg));
                directives.s3_methods.push(S3MethodDirective {
                    generic,
                    class,
                    method,
                });
            }
            "useDynLib" => {
                for name in split_namespace_args(&args)
                    .into_iter()
                    .filter_map(clean_namespace_name)
                {
                    push_unique(&mut directives.native_libraries, name);
                }
            }
            _ => {}
        }
    }
    directives
}

pub(crate) fn reject_native_namespace_directives(
    package: &str,
    directives: &NamespaceDirectives,
) -> Result<(), String> {
    if directives.native_libraries.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "package '{}' requires native libraries via useDynLib({}); this pure-R Android runtime does not load native package code",
            package,
            directives.native_libraries.join(", ")
        ))
    }
}

pub(crate) enum S3MethodMetadata {
    Record,
    PreserveOriginal,
}

pub(crate) unsafe fn register_namespace_s3_methods(
    package: &str,
    package_env: SEXP,
    directives: &NamespaceDirectives,
    metadata: S3MethodMetadata,
) -> Result<(), String> {
    unsafe {
        if matches!(metadata, S3MethodMetadata::Record) {
            record_namespace_s3_metadata(package_env, directives);
        }
        for method in &directives.s3_methods {
            // Upstream loadNamespace strips a "pkg::" qualifier from the
            // generic when deriving the default method function name:
            // S3method(utils::.DollarNames, R6) registers the function
            // `.DollarNames.R6` for generic `.DollarNames`
            // (base/R/namespace.R: paste0(sub("^.*::", "", generic), ".", class)).
            let local_generic = method.generic.rsplit("::").next().unwrap_or("");
            if local_generic.is_empty() {
                continue;
            }
            let method_name = method
                .method
                .clone()
                .unwrap_or_else(|| format!("{}.{}", local_generic, method.class));
            let Ok(method_cstr) = CString::new(method_name.as_str()) else {
                return Err(format!(
                    "package '{}' has invalid S3 method name '{}'",
                    package, method_name
                ));
            };
            let method_sym = Rf_install(method_cstr.as_ptr());
            let method_value = crate::sexp::envir::R_findVarInFrame(package_env, method_sym);
            if method_value.is_null()
                || method_value == R_NilValue()
                || method_value == crate::sexp::globals::R_UnboundValue()
            {
                // GNU registerS3methods() skips methods that are not bound
                // in the namespace (Windows-only utils methods on Unix).
                continue;
            }
            // GNU registers namespace methods lazily. Dispatch forces the
            // promise; initialization must not deserialize every method body.
            define_s3_method(package_env, local_generic, &method.class, method_value)?;

            // GNU registerS3method admits group generics directly in the base
            // namespace; there need not be a callable function named "Ops".
            if matches!(
                local_generic,
                "Math" | "Ops" | "matrixOps" | "Summary" | "Complex"
            ) {
                define_s3_method(
                    crate::sexp::envir::R_BaseNamespace(),
                    local_generic,
                    &method.class,
                    method_value,
                )?;
                continue;
            }

            // GNU registerS3method: methods for a closure generic live in
            // environment(fdef); primitives use .BaseNamespaceEnv.
            if let Ok(generic_cstr) = CString::new(local_generic) {
                let generic_sym = Rf_install(generic_cstr.as_ptr());
                let fdef = crate::sexp::envir::findFun(generic_sym, package_env);
                if is_function_value(fdef) {
                    let defenv = if TYPEOF(fdef) == SEXPTYPE::CLOSXP {
                        crate::sexp::accessors::CLOENV(fdef)
                    } else {
                        crate::sexp::globals::R_BaseEnv()
                    };
                    if !defenv.is_null()
                        && TYPEOF(defenv) == SEXPTYPE::ENVSXP
                        && defenv != package_env
                    {
                        define_s3_method(defenv, local_generic, &method.class, method_value)?;
                    }
                }
                let base_fdef =
                    crate::sexp::envir::findFun(generic_sym, crate::sexp::globals::R_BaseEnv());
                if TYPEOF(base_fdef) == SEXPTYPE::BUILTINSXP
                    || TYPEOF(base_fdef) == SEXPTYPE::SPECIALSXP
                {
                    define_s3_method(
                        crate::sexp::globals::R_BaseEnv(),
                        local_generic,
                        &method.class,
                        method_value,
                    )?;
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn strip_namespace_comments(content: &str) -> String {
    let mut stripped = String::with_capacity(content.len());
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;
    let mut in_comment = false;

    for ch in content.chars() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
                stripped.push('\n');
            }
            continue;
        }

        if in_string {
            stripped.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' && quote != '`' {
                escaped = true;
            } else if ch == quote {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => {
                in_string = true;
                quote = ch;
                stripped.push(ch);
            }
            '#' => {
                in_comment = true;
            }
            _ => stripped.push(ch),
        }
    }

    stripped
}

pub(crate) fn parse_namespace_calls(content: &str) -> Vec<(String, String)> {
    let mut calls = Vec::new();
    let mut chars = content.char_indices().peekable();

    while let Some((start, ch)) = chars.next() {
        if !(ch.is_ascii_alphabetic() || ch == '.') {
            continue;
        }

        let mut end = start + ch.len_utf8();
        while let Some(&(idx, next)) = chars.peek() {
            if next.is_ascii_alphanumeric() || next == '.' {
                chars.next();
                end = idx + next.len_utf8();
            } else {
                break;
            }
        }

        let directive = content[start..end].trim();
        let mut scan = chars.clone();
        while let Some(&(_, whitespace)) = scan.peek() {
            if whitespace.is_whitespace() {
                scan.next();
            } else {
                break;
            }
        }
        let Some((open_idx, '(')) = scan.next() else {
            continue;
        };

        let Some((close_idx, args)) = find_namespace_call_args(content, open_idx) else {
            continue;
        };
        calls.push((directive.to_string(), args.to_string()));

        while let Some(&(idx, _)) = chars.peek() {
            if idx <= close_idx {
                chars.next();
            } else {
                break;
            }
        }
    }

    calls
}

pub(crate) fn find_namespace_call_args(content: &str, open_idx: usize) -> Option<(usize, &str)> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;

    for (idx, ch) in content[open_idx..].char_indices() {
        let absolute_idx = open_idx + idx;
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' && quote != '`' {
                escaped = true;
            } else if ch == quote {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => {
                in_string = true;
                quote = ch;
            }
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((absolute_idx, &content[open_idx + 1..absolute_idx]));
                }
            }
            _ => {}
        }
    }

    None
}

pub(crate) fn split_namespace_args(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;

    for (idx, ch) in args.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' && quote != '`' {
                escaped = true;
            } else if ch == quote {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => {
                in_string = true;
                quote = ch;
            }
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(args[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    parts.push(args[start..].trim());
    parts
}

pub(crate) fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

pub(crate) unsafe fn namespace_exports(
    namespace: Option<&NamespaceDirectives>,
    package_env: SEXP,
) -> Vec<String> {
    unsafe {
        let Some(directives) = namespace else {
            return frame_binding_names(package_env, false);
        };

        let mut exports = directives.exports.clone();
        for name in frame_binding_names(package_env, true) {
            if directives
                .export_patterns
                .iter()
                .any(|pattern| simple_namespace_pattern_matches(pattern, &name))
            {
                push_unique(&mut exports, name);
            }
        }

        exports
    }
}

pub(super) unsafe fn namespace_info_export_names(package_env: SEXP) -> Vec<String> {
    unsafe {
        namespace_export_environment(package_env)
            .map(|exports| frame_binding_names(exports, true))
            .unwrap_or_default()
    }
}

pub(crate) unsafe fn namespace_export_environment(package_env: SEXP) -> Option<SEXP> {
    unsafe {
        let mut info = crate::sexp::envir::R_findVarInFrame(
            package_env,
            Rf_install(c".__NAMESPACE__.".as_ptr()),
        );
        if TYPEOF(info) == SEXPTYPE::PROMSXP {
            info = crate::sexp::envir::forcePromise(info);
        }
        if info.is_null()
            || info == crate::sexp::globals::R_UnboundValue()
            || TYPEOF(info) != SEXPTYPE::ENVSXP
        {
            return None;
        }
        let mut exports =
            crate::sexp::envir::R_findVarInFrame(info, Rf_install(c"exports".as_ptr()));
        if TYPEOF(exports) == SEXPTYPE::PROMSXP {
            exports = crate::sexp::envir::forcePromise(exports);
        }
        if exports.is_null()
            || exports == crate::sexp::globals::R_UnboundValue()
            || TYPEOF(exports) != SEXPTYPE::ENVSXP
        {
            return None;
        }
        Some(exports)
    }
}

pub(crate) unsafe fn frame_binding_names(env: SEXP, include_hidden: bool) -> Vec<String> {
    unsafe { collect_frame_binding_names(env, include_hidden, false) }
}

/// Public environment-name enumeration must report every bound name when
/// `all.names=TRUE`, including names that look like namespace metadata.
pub(crate) unsafe fn frame_binding_names_for_listing(
    env: SEXP,
    include_hidden: bool,
) -> Vec<String> {
    unsafe { collect_frame_binding_names(env, include_hidden, true) }
}

unsafe fn collect_frame_binding_names(
    env: SEXP,
    include_hidden: bool,
    include_namespace_metadata: bool,
) -> Vec<String> {
    unsafe {
        let mut names = Vec::new();
        collect_binding_names(
            FRAME(env),
            include_hidden,
            include_namespace_metadata,
            &mut names,
        );
        let hashtab = HASHTAB(env);
        if !hashtab.is_null() && hashtab != R_NilValue() && TYPEOF(hashtab) == SEXPTYPE::VECSXP {
            for i in 0..XLENGTH(hashtab) {
                collect_binding_names(
                    VECTOR_ELT(hashtab, i),
                    include_hidden,
                    include_namespace_metadata,
                    &mut names,
                );
            }
        }
        names.sort();
        names.dedup();
        names
    }
}

pub(super) unsafe fn collect_binding_names(
    mut frame: SEXP,
    include_hidden: bool,
    include_namespace_metadata: bool,
    names: &mut Vec<String>,
) {
    unsafe {
        while !frame.is_null() && frame != R_NilValue() {
            let value = CAR(frame);
            if value != crate::sexp::globals::R_UnboundValue()
                && let Some(name) = symbol_name(TAG(frame))
                && (include_hidden || !name.starts_with('.'))
                && (include_namespace_metadata
                    || (name != ".packageName" && name != ".namespaceEnv"))
            {
                names.push(name);
            }
            frame = CDR(frame);
        }
    }
}

pub(crate) fn simple_namespace_pattern_matches(pattern: &str, name: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }

    let anchored_start = pattern.starts_with('^');
    let anchored_end = pattern.ends_with('$');
    let mut body = pattern;
    if anchored_start {
        body = &body[1..];
    }
    if anchored_end && !body.is_empty() {
        body = &body[..body.len() - 1];
    }

    let mut literal = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let Some(escaped) = chars.next() else {
                return false;
            };
            literal.push(escaped);
        } else if ".^$|?*+()[]{}".contains(ch) {
            return false;
        } else {
            literal.push(ch);
        }
    }

    if anchored_start && anchored_end {
        name == literal
    } else if anchored_start {
        name.starts_with(&literal)
    } else if anchored_end {
        name.ends_with(&literal)
    } else {
        name.contains(&literal)
    }
}

pub(crate) fn clean_namespace_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let quoted = (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2);
    let inner = if quoted {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed.trim_matches('`').trim()
    };
    if inner.is_empty() {
        return None;
    }
    Some(if quoted {
        unescape_r_string(inner)
    } else {
        inner.to_string()
    })
}

pub(super) fn unescape_r_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// GNU `exportPattern("^\\.__C__")` / `.__M__` / `.__T__`: class and method
/// metadata live in the methods namespace and must also be visible on the
/// attached `package:methods` env so `findClass` / `.findAll` / `exists(...,
/// inherits=FALSE)` see them. Without this, `setClass(..., contains="signature")`
/// copies `.__C__signature` into `.GlobalEnv`, and a later `removeClass` of
/// global classes uncaches `initialize,signature`.
pub(super) unsafe fn export_s4_metadata_to_package_env(namespace: SEXP, attach_env: SEXP) {
    unsafe {
        if namespace.is_null()
            || attach_env.is_null()
            || namespace == R_NilValue()
            || attach_env == R_NilValue()
        {
            return;
        }
        for name in frame_binding_names(namespace, true) {
            if !(name.starts_with(".__C__")
                || name.starts_with(".__M__")
                || name.starts_with(".__T__"))
            {
                continue;
            }
            let Ok(cname) = CString::new(name.as_str()) else {
                continue;
            };
            let symbol = Rf_install(cname.as_ptr());
            let value = crate::sexp::envir::R_findVarInFrame(namespace, symbol);
            if value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_UnboundValue()
            {
                continue;
            }
            crate::sexp::envir::defineVar(symbol, value, attach_env);
        }
    }
}

pub(crate) unsafe fn attach_package_env(package_env: SEXP) {
    unsafe {
        // Every attached package participates in S3 search, including small
        // export frames below the ordinary automatic-promotion threshold.
        // Admit the existing canonical frame index so repeated absent method
        // lookups do not walk those exports. The index still reads current
        // binding cells and uses the existing mutation/collection invalidation.
        crate::sexp::env_hash::promote_to_hash_table(package_env);
        let global = crate::sexp::globals::R_GlobalEnv();
        let old_enclos = crate::sexp::accessors::ENCLOS(global);
        crate::sexp::accessors::SET_ENCLOS(global, package_env);
        crate::sexp::accessors::SET_ENCLOS(package_env, old_enclos);
        // The installed methods image already contains its namespace metadata.
        // Re-running it on package:methods walks every methods generic.
        // Still copy GNU exportPattern class/method metadata onto the
        // attached env so findClass/exists(inherits=FALSE) see .__C__*.
        if attached_search_name(package_env).as_deref() == Some("package:methods") {
            let ns = crate::sexp::envir::R_findVarInFrame(package_env, namespace_env_symbol());
            if !ns.is_null()
                && ns != crate::sexp::globals::R_UnboundValue()
                && TYPEOF(ns) == SEXPTYPE::ENVSXP
            {
                export_s4_metadata_to_package_env(ns, package_env);
            } else if let Some(ns) = cached_namespace_by_name("methods") {
                export_s4_metadata_to_package_env(ns, package_env);
            }
            return;
        }
        CACHING_ATTACHED_S4.with(|flag| {
            if flag.get() {
                return;
            }
            flag.set(true);
            cache_attached_package_metadata(package_env);
            flag.set(false);
        });
    }
}

pub(super) unsafe fn attached_search_name(package_env: SEXP) -> Option<String> {
    unsafe {
        let name = crate::sexp::attrib_core::getAttrib(package_env, name_symbol());
        if TYPEOF(name) != SEXPTYPE::STRSXP || LENGTH(name) < 1 {
            return None;
        }
        let raw = CHAR(STRING_ELT(name, 0));
        if raw.is_null() {
            return None;
        }
        CStr::from_ptr(raw).to_str().ok().map(str::to_string)
    }
}

/// Try to find a demo file for a topic.
pub(crate) fn find_package_demo(topic: &str) -> String {
    let r_home = std::env::var("R_HOME").unwrap_or_else(|_| "/usr/lib/R".to_string());
    let paths = [
        format!("{}/library/*/demo/{}.R", r_home, topic),
        format!("/usr/local/lib/R/site-library/*/demo/{}.R", topic),
    ];
    // Simplified: check a few common locations
    for p in &paths {
        if std::path::Path::new(p).exists() {
            return p.clone();
        }
    }
    String::new()
}

/// Try to find an example file for a topic.
pub(crate) fn find_package_example(topic: &str) -> String {
    let r_home = std::env::var("R_HOME").unwrap_or_else(|_| "/usr/lib/R".to_string());
    let paths = [
        format!("{}/library/*/R-ex/{}.R", r_home, topic),
        format!("/usr/local/lib/R/site-library/*/R-ex/{}.R", topic),
    ];
    for p in &paths {
        if std::path::Path::new(p).exists() {
            return p.clone();
        }
    }
    String::new()
}
