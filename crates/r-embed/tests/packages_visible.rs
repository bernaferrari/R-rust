//! `.packages()` and `path.package()` are visible base functions.
//!
//! GNU R 4.6.1 `--vanilla` returns a character vector from `.packages()`
//! that includes `stats`, `graphics`, `grDevices`, `utils`, `datasets`,
//! `methods`, and `base` (search order is not pinned). `path.package("stats")`
//! is one non-empty string. `path.package("notapackage", quiet=TRUE)` is `NULL`.

use r_embed::{RSession, RValue};

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
    let mut session = RSession::new().expect("session");

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
    let _available_names = character_vector(&available.value);

    let stats_path = session
        .eval_result("path.package(\"stats\")")
        .unwrap_or_else(|err| panic!("path.package(\"stats\") failed: {err}"));
    let stats_paths = character_vector(&stats_path.value);
    assert_eq!(
        stats_paths.len(),
        1,
        "path.package(\"stats\") length, got {stats_paths:?}"
    );
    let path = stats_paths[0].as_deref().expect("path.package(\"stats\") was NA");
    assert!(
        !path.is_empty(),
        "path.package(\"stats\") was empty"
    );

    session
        .eval_result("path.package(\"notapackage\", quiet = TRUE)")
        .unwrap_or_else(|err| panic!("quiet unknown path.package threw: {err}"));
}
