//! Inventory mutable global declarations without approving them by file name.
#![forbid(unsafe_code)]
use quote::ToTokens;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};
use syn::{
    Attribute, ExprMacro, ForeignItemStatic, ImplItemFn, ItemFn, ItemImpl, ItemMacro, ItemMod,
    ItemStatic, Meta, StaticMutability, Type,
    visit::{self, Visit},
};

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct Key {
    path: String,
    kind: &'static str,
    name: String,
}

fn requires_test(meta: &Meta) -> bool {
    match meta {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) if list.path.is_ident("all") || list.path.is_ident("any") => {
            use syn::parse::Parser;
            let parser = syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated;
            let Ok(items) = parser.parse2(list.tokens.clone()) else {
                return false;
            };
            if list.path.is_ident("all") {
                items.iter().any(requires_test)
            } else {
                !items.is_empty() && items.iter().all(requires_test)
            }
        }
        _ => false,
    }
}
fn test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
            || (attr.path().is_ident("cfg")
                && attr.parse_args::<Meta>().is_ok_and(|m| requires_test(&m)))
    })
}
// `cfg_attr` may apply a path override under any production configuration.
// Without module-graph resolution, even an in-tree-looking path needs review.
fn may_override_module_path(meta: &Meta) -> bool {
    match meta {
        Meta::NameValue(value) if value.path.is_ident("path") => true,
        Meta::List(list) if list.path.is_ident("cfg_attr") => {
            use syn::parse::Parser;
            let parser = syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated;
            parser.parse2(list.tokens.clone()).map_or(true, |items| {
                items.iter().skip(1).any(may_override_module_path)
            })
        }
        _ => false,
    }
}
fn cfgs(attrs: &[Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("cfg"))
        .map(|a| a.meta.to_token_stream().to_string().replace(' ', ""))
        .collect()
}
fn sync_type(ty: &Type) -> bool {
    struct Find(bool);
    impl<'a> Visit<'a> for Find {
        fn visit_path_segment(&mut self, segment: &'a syn::PathSegment) {
            let name = segment.ident.to_string();
            self.0 |= matches!(name.as_str(), "OnceLock" | "LazyLock" | "Mutex" | "RwLock")
                || name.starts_with("Atomic");
            visit::visit_path_segment(self, segment);
        }
    }
    let mut find = Find(false);
    find.visit_type(ty);
    find.0
}

// Macro expansions are not available to syn. Refuse potentially hidden globals.
fn suspicious_macro_tokens(tokens: proc_macro2::TokenStream) -> bool {
    use proc_macro2::TokenTree;
    let mut previous_lifetime = false;
    let mut previous_special = false;
    for token in tokens {
        match token {
            TokenTree::Group(group) => {
                if suspicious_macro_tokens(group.stream()) {
                    return true;
                }
                previous_lifetime = false;
                previous_special = false;
            }
            TokenTree::Ident(ident) => {
                let word = ident.to_string();
                if word == "static" && !previous_lifetime {
                    return true;
                }
                previous_special =
                    matches!(word.as_str(), "thread_local" | "lazy_static" | "include");
                previous_lifetime = false;
            }
            TokenTree::Punct(punct) => {
                if punct.as_char() == '!' && previous_special {
                    return true;
                }
                previous_lifetime = punct.as_char() == '\'';
                previous_special = false;
            }
            TokenTree::Literal(_) => {
                previous_lifetime = false;
                previous_special = false;
            }
        }
    }
    false
}

struct Scanner {
    path: String,
    scope: Vec<String>,
    conditions: Vec<String>,
    found: BTreeMap<Key, usize>,
    errors: Vec<String>,
}
impl Scanner {
    fn new(path: String) -> Self {
        Self {
            path,
            scope: vec![],
            conditions: vec![],
            found: BTreeMap::new(),
            errors: vec![],
        }
    }
    fn scoped(&mut self, attrs: &[Attribute], name: Option<String>, f: impl FnOnce(&mut Self)) {
        if test_only(attrs) {
            return;
        }
        let n = self.conditions.len();
        self.conditions.extend(cfgs(attrs));
        if let Some(name) = &name {
            self.scope.push(name.clone());
        }
        f(self);
        if name.is_some() {
            self.scope.pop();
        }
        self.conditions.truncate(n);
    }
    fn record(&mut self, kind: &'static str, ident: &syn::Ident, attrs: &[Attribute]) {
        if test_only(attrs) {
            return;
        }
        let mut segments = self.scope.clone();
        segments.push(ident.to_string());
        let mut all_cfg = self.conditions.clone();
        all_cfg.extend(cfgs(attrs));
        all_cfg.sort();
        all_cfg.dedup();
        let mut name = segments.join("::");
        if !all_cfg.is_empty() {
            name.push('@');
            name.push_str(&all_cfg.join("&"));
        }
        *self
            .found
            .entry(Key {
                path: self.path.clone(),
                kind,
                name,
            })
            .or_default() += 1;
    }
    fn macro_body(&mut self, mac: &syn::Macro, attrs: &[Attribute]) {
        let Some(last) = mac.path.segments.last() else {
            return;
        };
        if last.ident != "thread_local" && last.ident != "lazy_static" {
            if !test_only(attrs)
                && (last.ident == "include" || suspicious_macro_tokens(mac.tokens.clone()))
            {
                self.errors.push(format!(
                    "{}: macro {} may declare global state; audit its expansion",
                    self.path,
                    mac.path.to_token_stream()
                ));
            }
            return;
        }
        if test_only(attrs) {
            return;
        }
        let is_tls = last.ident == "thread_local";
        let tokens = if is_tls {
            mac.tokens.clone()
        } else {
            // lazy_static accepts `static ref`, which is not an ItemStatic.
            // Rewrite only token pairs; nested initializers remain untouched.
            let mut out = proc_macro2::TokenStream::new();
            let mut iter = mac.tokens.clone().into_iter().peekable();
            while let Some(token) = iter.next() {
                let is_static =
                    matches!(&token, proc_macro2::TokenTree::Ident(id) if id == "static");
                out.extend([token]);
                if is_static
                    && matches!(iter.peek(), Some(proc_macro2::TokenTree::Ident(id)) if id == "ref")
                {
                    iter.next();
                }
            }
            out
        };
        match syn::parse2::<syn::File>(tokens) {
            Ok(file) => self.scoped(attrs, None, |s| {
                for item in &file.items {
                    if let syn::Item::Static(item) = item {
                        s.record(
                            if is_tls {
                                "thread-local"
                            } else {
                                "lazy-static"
                            },
                            &item.ident,
                            &item.attrs,
                        );
                        s.scoped(&item.attrs, Some(item.ident.to_string()), |s| {
                            s.visit_expr(&item.expr)
                        });
                    } else {
                        s.errors.push(format!(
                            "{}: unsupported {} declaration: {}",
                            s.path,
                            last.ident,
                            item.to_token_stream()
                        ));
                    }
                }
            }),
            Err(error) => self.errors.push(format!(
                "{}: cannot parse {} declarations: {error}",
                self.path, last.ident
            )),
        }
    }
}
impl<'a> Visit<'a> for Scanner {
    fn visit_file(&mut self, file: &'a syn::File) {
        self.scoped(&file.attrs, None, |s| visit::visit_file(s, file));
    }
    fn visit_item_foreign_mod(&mut self, item: &'a syn::ItemForeignMod) {
        self.scoped(&item.attrs, None, |s| {
            visit::visit_item_foreign_mod(s, item)
        });
    }
    fn visit_item_const(&mut self, item: &'a syn::ItemConst) {
        self.scoped(&item.attrs, Some(item.ident.to_string()), |s| {
            visit::visit_item_const(s, item)
        });
    }
    fn visit_impl_item_const(&mut self, item: &'a syn::ImplItemConst) {
        self.scoped(&item.attrs, Some(item.ident.to_string()), |s| {
            visit::visit_impl_item_const(s, item)
        });
    }
    fn visit_trait_item_fn(&mut self, item: &'a syn::TraitItemFn) {
        self.scoped(&item.attrs, Some(item.sig.ident.to_string()), |s| {
            visit::visit_trait_item_fn(s, item)
        });
    }

    fn visit_item_mod(&mut self, item: &'a ItemMod) {
        self.scoped(&item.attrs, Some(item.ident.to_string()), |s| {
            if item
                .attrs
                .iter()
                .any(|attr| may_override_module_path(&attr.meta))
            {
                s.errors.push(format!(
                    "{}: module {} has an unresolved path override; audit its source",
                    s.path,
                    s.scope.join("::")
                ));
            }
            visit::visit_item_mod(s, item)
        });
    }
    fn visit_item_fn(&mut self, item: &'a ItemFn) {
        self.scoped(&item.attrs, Some(item.sig.ident.to_string()), |s| {
            visit::visit_item_fn(s, item)
        });
    }
    fn visit_impl_item_fn(&mut self, item: &'a ImplItemFn) {
        self.scoped(&item.attrs, Some(item.sig.ident.to_string()), |s| {
            visit::visit_impl_item_fn(s, item)
        });
    }
    fn visit_item_impl(&mut self, item: &'a ItemImpl) {
        let name = format!(
            "impl({})",
            item.self_ty.to_token_stream().to_string().replace(' ', "")
        );
        self.scoped(&item.attrs, Some(name), |s| visit::visit_item_impl(s, item));
    }
    fn visit_item_static(&mut self, item: &'a ItemStatic) {
        if test_only(&item.attrs) {
            return;
        }
        if matches!(item.mutability, StaticMutability::Mut(_)) {
            self.record("static-mut", &item.ident, &item.attrs);
        } else if sync_type(&item.ty) {
            self.record("sync-static", &item.ident, &item.attrs);
        } else {
            // Aliases, imported types and user-defined wrappers may contain
            // interior mutability even when their spelling looks immutable.
            self.record("immutable-static", &item.ident, &item.attrs);
        }
        self.scoped(&item.attrs, Some(item.ident.to_string()), |s| {
            s.visit_expr(&item.expr)
        });
    }
    fn visit_foreign_item_static(&mut self, item: &'a ForeignItemStatic) {
        self.record(
            if matches!(item.mutability, StaticMutability::Mut(_)) {
                "static-mut"
            } else {
                "foreign-static"
            },
            &item.ident,
            &item.attrs,
        );
    }
    fn visit_item_macro(&mut self, item: &'a ItemMacro) {
        self.macro_body(&item.mac, &item.attrs);
    }
    fn visit_expr_macro(&mut self, item: &'a ExprMacro) {
        self.macro_body(&item.mac, &item.attrs);
    }
    fn visit_stmt_macro(&mut self, item: &'a syn::StmtMacro) {
        self.macro_body(&item.mac, &item.attrs);
    }
}
fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if file_type.is_symlink() {
            return Err(format!(
                "{}: source symlink requires review",
                path.display()
            ));
        }
        if file_type.is_dir() {
            if entry.file_name() != "support" {
                collect_files(&path, files)?;
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    Ok(())
}
fn scan(root: &Path) -> Result<Vec<Key>, String> {
    let mut files = vec![];
    collect_files(&root.join("crates/rmath/src"), &mut files)?;
    files.sort();
    let mut found = BTreeMap::new();
    let mut errors = vec![];
    for file in files {
        let path = file
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&file).map_err(|e| format!("{path}: {e}"))?;
        let syntax = match syn::parse_file(&source) {
            Ok(v) => v,
            Err(e) => {
                errors.push(format!("{path}: parse failed: {e}"));
                continue;
            }
        };
        let mut scanner = Scanner::new(path);
        scanner.visit_file(&syntax);
        errors.extend(scanner.errors);
        for (key, count) in scanner.found {
            *found.entry(key).or_insert(0usize) += count;
        }
    }
    for (key, count) in &found {
        if *count != 1 {
            errors.push(format!(
                "duplicate declaration identifier: {}\t{}\t{} ({count})",
                key.path, key.kind, key.name
            ));
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    Ok(found.into_keys().collect())
}
fn check_manifest(keys: &[Key], manifest: &str) -> Result<(), String> {
    let mut allowed = BTreeSet::new();
    let mut errors = Vec::new();
    for (index, line) in manifest.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 7 || fields.iter().any(|v| v.trim().is_empty()) {
            errors.push(format!(
                "manifest line {}: expected seven nonempty tab-separated fields",
                index + 1
            ));
            continue;
        }
        if !matches!(
            fields[1],
            "static-mut"
                | "sync-static"
                | "immutable-static"
                | "foreign-static"
                | "thread-local"
                | "lazy-static"
        ) {
            errors.push(format!(
                "manifest line {}: invalid declaration kind",
                index + 1
            ));
            continue;
        }
        if !matches!(
            fields[4],
            "allow-session-dispatch"
                | "allow-immutable"
                | "allow-test"
                | "allow-process"
                | "sessionize"
                | "platform-replace"
                | "disabled-android"
        ) {
            errors.push(format!("manifest line {}: invalid policy", index + 1));
            continue;
        }
        let identity = format!("{}\t{}\t{}", fields[0], fields[1], fields[2]);
        if !allowed.insert(identity.clone()) {
            errors.push(format!("duplicate manifest declaration: {identity}"));
        }
    }
    let found: BTreeSet<String> = keys
        .iter()
        .map(|k| format!("{}\t{}\t{}", k.path, k.kind, k.name))
        .collect();
    for key in found.difference(&allowed) {
        errors.push(format!("unclassified declaration: {key}"));
    }
    for key in allowed.difference(&found) {
        errors.push(format!("stale manifest declaration: {key}"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let result = match args.as_slice() {
        [root, output] => scan(Path::new(root)).and_then(|keys| {
            let text = keys
                .iter()
                .map(|k| format!("{}\t{}\t{}\n", k.path, k.kind, k.name))
                .collect::<String>();
            fs::write(output, text).map_err(|e| format!("cannot write inventory: {e}"))
        }),
        [flag, root, manifest] if flag == "--check" => scan(Path::new(root)).and_then(|keys| {
            let text =
                fs::read_to_string(manifest).map_err(|e| format!("cannot read manifest: {e}"))?;
            check_manifest(&keys, &text)
        }),
        _ => Err("usage: rport-android-global-audit ROOT OUTPUT | --check ROOT MANIFEST".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory(source: &str) -> Scanner {
        let file = syn::parse_file(source).unwrap();
        let mut scanner = Scanner::new("fixture.rs".into());
        scanner.visit_file(&file);
        scanner
    }

    #[test]
    fn identifies_each_global_and_qualifies_scopes_and_conditions() {
        let scanner = inventory(
            r#"
            static FIRST: AtomicU32 = init();
            static SECOND: OnceLock<usize> = init();
            thread_local! { static A: u32 = 0; static B: u32 = 0; }
            fn first() { static NEXT: AtomicU64 = init(); thread_local! { static LOCAL: u32 = 0; } }
            fn second() { static NEXT: AtomicU64 = init(); }
            #[cfg(unix)] unsafe extern "C" { static mut environ: i32; }
            #[cfg(unix)] static CONDITIONAL: AtomicBool = init();
            #[cfg(windows)] static CONDITIONAL: AtomicBool = init();
        "#,
        );
        assert!(scanner.errors.is_empty(), "{:?}", scanner.errors);
        let names: Vec<_> = scanner.found.keys().map(|k| k.name.as_str()).collect();
        assert_eq!(names.len(), 10);
        for name in [
            "FIRST",
            "SECOND",
            "A",
            "B",
            "first::NEXT",
            "first::LOCAL",
            "second::NEXT",
            "environ@cfg(unix)",
            "CONDITIONAL@cfg(unix)",
            "CONDITIONAL@cfg(windows)",
        ] {
            assert!(names.contains(&name), "missing {name}: {names:?}");
        }
    }

    #[test]
    fn requires_review_for_opaque_and_foreign_static_types() {
        let scanner = inventory(
            r#"
            use std::sync::atomic::AtomicUsize as Counter;
            type Alias = Counter;
            static FIRST: Alias = make();
            static SECOND: other_crate::Opaque = make();
            static SENTINEL: usize = 0;
            unsafe extern "C" { static FOREIGN: u32; }
        "#,
        );
        for (name, kind) in [
            ("FIRST", "immutable-static"),
            ("SECOND", "immutable-static"),
            ("SENTINEL", "immutable-static"),
            ("FOREIGN", "foreign-static"),
        ] {
            assert!(
                scanner
                    .found
                    .keys()
                    .any(|key| key.name == name && key.kind == kind),
                "missing {kind} {name}"
            );
        }
    }

    #[test]
    fn filters_only_conditions_that_require_tests() {
        let scanner = inventory(
            r#"
            #[cfg(test)] mod arbitrary { static HIDDEN: AtomicU32 = init(); }
            #[cfg(all(test, unix))] static HIDDEN2: AtomicU32 = init();
            #[cfg(not(test))] static VISIBLE: AtomicU32 = init();
            #[cfg(any(test, unix))] static ALSO_VISIBLE: AtomicU32 = init();
            #[test] fn check() { static HIDDEN3: AtomicU32 = init(); }
        "#,
        );
        assert_eq!(scanner.found.len(), 2);
        assert!(
            inventory("#![cfg(test)]\nstatic HIDDEN: AtomicU32 = init();")
                .found
                .is_empty()
        );
    }

    #[test]
    fn refuses_macros_that_may_hide_globals() {
        let bad = inventory("macro_rules! make_global { () => { static X: AtomicU32 = init(); } }");
        assert_eq!(bad.errors.len(), 1);
        let bad_tls = inventory("some_macro! { thread_local! { static X: u32 = 0; } }");
        assert_eq!(bad_tls.errors.len(), 1);
        let harmless = inventory("some_macro! { &'static str, \"static\" }");
        assert!(harmless.errors.is_empty(), "{:?}", harmless.errors);
    }

    #[test]
    fn rejects_rust_includes_that_could_escape_the_inventory() {
        for source in [
            "include!(\"../outside.rs\");",
            "fn f() { include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\")); }",
            "macro_rules! indirect { () => { include!(\"generated.rs\"); } }",
        ] {
            let scanner = inventory(source);
            assert!(
                !scanner.errors.is_empty(),
                "include must be reviewed: {source}"
            );
        }
        let scanner = inventory("const TEXT: &str = include_str!(\"data.txt\");");
        assert!(scanner.errors.is_empty());
    }

    #[test]
    fn rejects_unresolved_module_path_overrides() {
        for source in [
            "#[path = \"../outside.rs\"] mod outside;",
            "#[cfg_attr(target_os = \"android\", path = \"../outside.rs\")] mod outside;",
            "#[cfg_attr(unix, cfg_attr(target_os = \"android\", path = \"../outside.rs\"))] mod outside;",
        ] {
            let scanner = inventory(source);
            assert!(
                !scanner.errors.is_empty(),
                "module path must be reviewed: {source}"
            );
        }
        assert!(
            inventory("#[cfg(test)] #[path = \"../tests.rs\"] mod only_test;")
                .errors
                .is_empty()
        );
        assert!(
            inventory("#[cfg_attr(unix, allow(dead_code))] mod normal;")
                .errors
                .is_empty()
        );
    }

    #[test]
    fn manifest_checks_each_declaration_and_rejects_invalid_entries() {
        let scanner = inventory("static ONE: AtomicU32 = init(); static TWO: AtomicU32 = init();");
        let keys: Vec<_> = scanner.found.into_keys().collect();
        let one = "fixture.rs\tsync-static\tONE\tfixture\tallow-process\ttest\tverified\n";
        let two = "fixture.rs\tsync-static\tTWO\tfixture\tallow-process\ttest\tverified\n";
        assert!(check_manifest(&keys, &format!("{one}{two}")).is_ok());
        assert!(
            check_manifest(&keys, one)
                .unwrap_err()
                .contains("unclassified declaration: fixture.rs\tsync-static\tTWO")
        );
        assert!(
            check_manifest(&keys, &format!("{one}{two}{one}"))
                .unwrap_err()
                .contains("duplicate manifest")
        );
        assert!(check_manifest(&keys, &format!("{one}{two}fixture.rs\tsync-static\tGONE\tfixture\tallow-process\ttest\tverified\n")).unwrap_err().contains("stale manifest"));
        assert!(
            check_manifest(&keys, "fixture.rs\tlegacy-file-only\n")
                .unwrap_err()
                .contains("expected seven")
        );
        assert!(
            check_manifest(
                &keys,
                &format!("{one}fixture.rs\tsync-static\tTWO\tfixture\tunknown\ttest\tverified\n")
            )
            .unwrap_err()
            .contains("invalid policy")
        );
    }

    #[test]
    fn detects_duplicate_identifiers_and_parses_lazy_static() {
        let scanner = inventory(
            r#"
            lazy_static! { static ref ONE: Mutex<u32> = init(); static ref TWO: Mutex<u32> = init(); }
            fn f() { { static SAME: AtomicU32 = init(); } { static SAME: AtomicU32 = init(); } }
        "#,
        );
        assert!(scanner.errors.is_empty(), "{:?}", scanner.errors);
        assert_eq!(scanner.found.len(), 3);
        assert!(scanner.found.values().any(|count| *count == 2));
    }
}
