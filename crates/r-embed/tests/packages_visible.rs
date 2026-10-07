//! `.packages()` and `path.package()` are visible base functions.
//!
//! GNU R 4.6.1 `--vanilla` returns a character vector from `.packages()`
//! that includes `stats`, `graphics`, `grDevices`, `utils`, `datasets`,
//! `methods`, and `base` (search order is not pinned). `path.package("stats")`
//! is one non-empty string. `path.package("notapackage", quiet=TRUE)` is `NULL`.

use r_embed::{RSession, RValue, RuntimePathPolicy};

fn character_vector(value: &RValue) -> Vec<Option<String>> {
    match value {
        RValue::StringVector(values) => values.clone(),
        RValue::Attributed { value, .. } => character_vector(value),
        other => panic!("expected a character vector, got {other:?}"),
    }
}

fn assert_contains(label: &str, names: &[Option<String>], required: &[&str]) {
    for name in required {
        assert!(
            names.iter().any(|item| item.as_deref() == Some(*name)),
            "{label} missing {name} in {names:?}"
        );
    }
}

#[test]
fn packages_visible_matches_gnu_membership() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .expect("session");
        check_packages_visible(&mut session, portable);
    }
}

fn check_packages_visible(session: &mut RSession, portable: bool) {
    let attached = session
        .eval_result(".packages()")
        .unwrap_or_else(|err| panic!(".packages() failed: {err}"));
    let attached_names = character_vector(&attached.value);
    assert_contains(
        ".packages()",
        &attached_names,
        &["base", "stats", "methods"],
    );

    let explicit = session
        .eval_result(".packages(all.available = FALSE)")
        .unwrap_or_else(|err| panic!(".packages(all.available=FALSE) failed: {err}"));
    let explicit_names = character_vector(&explicit.value);
    assert_contains(
        ".packages(all.available=FALSE)",
        &explicit_names,
        &["base", "stats", "methods"],
    );

    let available = session
        .eval_result(".packages(all.available = TRUE)")
        .unwrap_or_else(|err| panic!(".packages(all.available=TRUE) failed: {err}"));
    let available_names = character_vector(&available.value);

    for package in [
        "stats",
        "graphics",
        "grDevices",
        "utils",
        "datasets",
        "methods",
    ] {
        let paths = session
            .eval_result(&format!("path.package('{package}')"))
            .unwrap_or_else(|err| panic!("path.package('{package}'); portable={portable}: {err}"));
        let paths = character_vector(&paths.value);
        assert_eq!(paths.len(), 1, "{package}; portable={portable}: {paths:?}");
        let path = paths[0].as_deref().expect("package path was NA");
        assert!(
            !path.is_empty(),
            "{package}; portable={portable}: empty path"
        );
        if portable {
            assert_eq!(path, format!("<builtin:{package}>"));
        }
        assert_eq!(
            session.eval(&format!("identical(path.package('{package}'), attr(as.environment('package:{package}'), 'path')) && identical(path.package('{package}'), getNamespaceInfo('{package}', 'path'))")).unwrap(),
            "[1] TRUE\n",
            "{package}; portable={portable}: attachment and namespace paths disagree"
        );
    }

    if portable {
        assert!(character_vector(&session.eval_result(".libPaths()").unwrap().value).is_empty());
        assert!(available_names.is_empty());
    }

    session
        .eval_result("path.package(\"notapackage\", quiet = TRUE)")
        .unwrap_or_else(|err| panic!("quiet unknown path.package threw: {err}"));
}
