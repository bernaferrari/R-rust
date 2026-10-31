#![allow(deprecated)] // translated tests exercise the Sexp compat setters
use super::*;
use std::path::Path;

fn console_owner_fixture() -> RSession {
    RSession {
        core: CoreRSession::new_without_default_packages(),
        result_limit: None,
    }
}

#[test]
fn owned_retained_console_closed_error_preserves_other_owner_warnings() {
    let mut closed = console_owner_fixture();
    closed.close();
    let other = console_owner_fixture();
    other.core.with_active_in(|instance| {
        unsafe {
            (*instance).error_state.collect_warnings = 1;
        }
        let result = closed.eval_script("7L");
        assert!(matches!(result.typed, RValue::Error(_)));
        assert_eq!(result.stdout, "");
        assert!(result.stderr.contains("session is closed"));
        assert_eq!(unsafe { (*instance).error_state.collect_warnings }, 1);
        unsafe {
            (*instance).error_state.collect_warnings = 0;
        }
    });
}

#[test]
fn owned_retained_console_revoked_print_keeps_bytes_and_original_cleanup() {
    for code in ["x", "x; 7L"] {
        let mut session = console_owner_fixture();
        let setup = session.eval(
                "print.zz <- function(x, ...) { cat('partial  '); gc(); invisible(x) }; x <- structure(1L, class='zz')",
            );
        assert!(!matches!(setup.typed, RValue::Error(_)), "{setup:?}");
        let owner = session.core.owner_token().unwrap().weak_owner().unwrap();
        let pin = owner.pin().unwrap();
        let revoked = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed = revoked.clone();
        session.core.with_active_in(|instance| {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !observed.replace(true) {
                    unsafe {
                        crate::sexp::instance::revoke_instance_availability(instance);
                    }
                }
            }));
        });
        let result = session.eval(code);
        assert!(revoked.get(), "{code}");
        assert!(matches!(result.typed, RValue::Error(_)), "{result:?}");
        assert_eq!(result.stdout, "partial  ", "{code}");
        assert!(result.output.starts_with("partial  "));
        assert!(owner.pin().is_err());
        assert!(!session.is_active());
        let subsequent = session.eval("cat('must not run')");
        assert!(
            matches!(subsequent.typed, RValue::Error(_)),
            "{subsequent:?}"
        );
        assert_eq!(subsequent.stdout, "");
        assert!(!subsequent.output.contains("must not run"));
        assert_eq!(unsafe { (*pin.as_ptr()).error_state.toplevel_expr_no }, 0);
        assert!(!unsafe { (*pin.as_ptr()).output_capture.borrow().is_capturing() });
    }
}

#[test]
fn owned_retained_console_active_last_value_error_is_typed_and_recovers() {
    let mut session = console_owner_fixture();
    let (setup, _, _) = session.core.eval_code_with_output_capture(
        "makeActiveBinding('.Last.value', function(value) stop('last value failed'), globalenv())",
    );
    assert!(setup.is_ok(), "{setup:?}");
    drop(setup);
    // GNU 4.7 rememberLastValue stores via SET_SYMVALUE on the binding
    // cell, bypassing user active-binding functions: `7L; 8L` succeeds
    // even with an erroring active binding installed on .Last.value.
    let result = session.eval("7L; 8L");
    assert_eq!(result.stdout, "[1] 7\n[1] 8\n", "{result:?}");
    assert_eq!(result.typed, RValue::Integer(Some(8)), "{result:?}");
    // The binding itself is still active: reading it invokes the function.
    let read = session.eval(".Last.value");
    assert!(
        matches!(&read.typed, RValue::Error(message) if message.contains("last value failed")),
        "{read:?}"
    );
    let (removed, _, _) = session
        .core
        .eval_code_with_output_capture("rm('.Last.value')");
    assert!(removed.is_ok(), "{removed:?}");
    drop(removed);
    assert_eq!(session.eval("7L").stdout, "[1] 7\n");
}

#[test]
fn owned_retained_console_live_print_panic_preserves_payload_and_cleanup() {
    let mut session = console_owner_fixture();
    let setup = session.eval(
        "print.zz <- function(x, ...) { gc(); invisible(x) }; x <- structure(1L, class='zz')",
    );
    assert!(!matches!(setup.typed, RValue::Error(_)), "{setup:?}");
    let pin = session
        .core
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap()
        .pin()
        .unwrap();
    session.core.with_active(|| {
        crate::sexp::gengc::register_gc_callback(Box::new(|_| std::panic::panic_any(991_u32)));
    });
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.eval("x; 7L")));
    let payload = outcome.expect_err("live-owner Rust panic must preserve its payload");
    assert_eq!(payload.downcast_ref::<u32>(), Some(&991));
    assert_eq!(unsafe { (*pin.as_ptr()).error_state.toplevel_expr_no }, 0);
    assert!(!unsafe { (*pin.as_ptr()).output_capture.borrow().is_capturing() });
    session
        .core
        .with_active_in(|instance| unsafe { (*instance).gc_state.callbacks.clear() });
    assert_eq!(session.eval("7L").stdout, "[1] 7\n");
}
pub(crate) fn string_vector(values: Vec<String>) -> RValue {
    RValue::StringVector(values.into_iter().map(Some).collect())
}

pub(crate) fn literal_string_vector(values: &[&str]) -> RValue {
    string_vector(values.iter().map(|value| (*value).to_string()).collect())
}

pub(crate) fn unique_test_root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "rport-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    ))
}

fn write_package(root: &Path, name: &str, namespace: &str, r_source: &str) -> std::path::PathBuf {
    let pkg = root.join(name);
    let r_dir = pkg.join("R");
    std::fs::create_dir_all(&r_dir).expect("package R dir");
    std::fs::write(
        pkg.join("DESCRIPTION"),
        format!("Package: {name}\nVersion: 0.0.1\n"),
    )
    .expect("description");
    std::fs::write(pkg.join("NAMESPACE"), namespace).expect("namespace");
    std::fs::write(r_dir.join(format!("{name}.R")), r_source).expect("R source");
    pkg
}

#[test]
fn source_echo_truncation_keeps_trailing_space_on_script_error() {
    let mut session = RSession::new();
    let result = session.eval_script(
            r#"
f <- tempfile()
writeLines("hasReal <- function(x) { if (is.double(x) || is.complex(x)) !all((x == round(x, 3)) | is.na(x)) else if (is.logical(x) || is.integer(x) || is.symbol(x) || is.call(x) || is.environment(x) || is.character(x)) FALSE else FALSE }", f)
source(f, echo = TRUE)
stop("after-echo")
"#,
        );
    let idx = result
        .stdout
        .rfind("[TRUNCATED]")
        .expect("source(echo=TRUE) must truncate");
    assert!(
        result.stdout[idx..].starts_with("[TRUNCATED] "),
        "GNU keeps a trailing space after [TRUNCATED] when the script later errors; stdout={:?}",
        result.stdout
    );
}

#[test]
fn eval_script_stdout_ends_with_newline_like_rscript() {
    let mut session = RSession::new();
    let result = session.eval_script("1 + 1");
    assert!(
        result.stdout.ends_with('\n'),
        "GNU Rscript ends printed output with a newline; stdout={:?}",
        result.stdout
    );
    assert!(result.stdout.contains("[1] 2"));
}

#[test]
fn eval_script_concatenates_cat_without_newline_onto_next_print() {
    let mut session = RSession::new();
    let result = session.eval_script("cat('x'); TRUE");
    assert_eq!(result.stdout, "x[1] TRUE\n");
}

#[test]
fn test_session_new() {
    let mut session = RSession::new();
    assert!(
        session
            .core
            .with_arena(|arena| arena.node_count() > 0)
            .unwrap_or(false)
    );
    assert_eq!(session.eval("1 + 1").output, "[1] 2\n");
}

#[test]
fn test_android_session_constructor_does_not_replace_current_instance() {
    let _current = crate::sexp::session::RSession::new();
    let current = crate::sexp::instance::current_instance_ptr();
    assert!(current.is_some());

    let mut android = RSession::new();
    assert_eq!(crate::sexp::instance::current_instance_ptr(), current);
    assert_eq!(android.eval("1 + 1").output, "[1] 2\n");
    assert_eq!(crate::sexp::instance::current_instance_ptr(), current);
}

#[test]
fn test_android_path_policy_drives_libpaths_find_package_and_tempdir() {
    let root = std::env::temp_dir().join(format!(
        "rport-android-paths-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    ));
    let files = root.join("files");
    let cache = root.join("cache");
    let bundled = root.join("bundled-library");
    let base_pkg = bundled.join("base");
    std::fs::create_dir_all(&base_pkg).expect("package dir");
    std::fs::write(base_pkg.join("DESCRIPTION"), "Package: base\n").expect("description");

    let mut session = RSession::new();
    session
        .configure_paths(
            files.to_str().expect("utf8 files path"),
            cache.to_str().expect("utf8 cache path"),
            Some(bundled.to_str().expect("utf8 bundled path")),
        )
        .expect("configure paths");

    let lib_paths = session.eval(".libPaths()");
    assert_eq!(
        lib_paths.typed,
        string_vector(vec![
            files
                .join("R")
                .join("library")
                .to_string_lossy()
                .into_owned(),
            bundled.to_string_lossy().into_owned()
        ])
    );

    let base_path = session.eval("find.package(\"base\")");
    assert_eq!(
        base_path.typed,
        string_vector(vec![base_pkg.to_string_lossy().into_owned()])
    );
    let installed = session.eval("installed.packages(noCache = TRUE)");
    match installed.typed {
        RValue::Attributed { value, metadata } => {
            // GNU .readPkgDesc only reads Meta/package.rds, so a
            // DESCRIPTION-only bundled package contributes no rows; the
            // empty matrix keeps all 18 columns (Archs is dropped only
            // when rows exist). Oracle: dim c(0, 18).
            assert_eq!(metadata.dim, Some(vec![0, 18]));
            match *value {
                RValue::StringVector(values) => {
                    assert!(values.is_empty(), "{values:?}");
                }
                other => panic!("expected installed package string matrix, got {other:?}"),
            }
        }
        other => panic!("expected attributed installed package matrix, got {other:?}"),
    }

    let tempdir = session.eval("tempdir()");
    assert_eq!(
        tempdir.typed,
        string_vector(vec![cache.join("Rtmp").to_string_lossy().into_owned()])
    );
    assert_eq!(session.eval("file.exists(tempdir())").output, "[1] TRUE\n");
    assert_eq!(
        session.runtime_info(),
        RRuntimeInfo {
            is_active: true,
            library_paths: vec![
                files
                    .join("R")
                    .join("library")
                    .to_string_lossy()
                    .into_owned(),
                bundled.to_string_lossy().into_owned()
            ],
            temp_dir: cache.join("Rtmp").to_string_lossy().into_owned(),
        }
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn test_library_loads_pure_r_package_from_android_paths() {
    let root = unique_test_root("android-package");
    let files = root.join("files");
    let cache = root.join("cache");
    let bundled = root.join("bundled-library");
    let pkg = write_package(
        &bundled,
        "tiny",
        "export(tiny_value, tiny_label)\n",
        "tiny_secret <- function() 42L\ntiny_value <- function() tiny_secret()\ntiny_label <- \"loaded\"\n",
    );
    let data_dir = pkg.join("data");
    std::fs::create_dir_all(&data_dir).expect("data dir");
    std::fs::write(data_dir.join("tiny_data.R"), "tiny_data <- 314L\n").expect("data file");
    std::fs::write(
        pkg.join("DESCRIPTION"),
        "Package: tiny\nVersion: 0.0.1\nLazyData: true\n",
    )
    .expect("description");
    std::fs::write(data_dir.join("lazy_data.R"), "lazy_data <- 271L\n").expect("lazy data file");

    let mut session = RSession::new();
    session
        .configure_paths(
            files.to_str().expect("utf8 files path"),
            cache.to_str().expect("utf8 cache path"),
            Some(bundled.to_str().expect("utf8 bundled path")),
        )
        .expect("configure paths");

    let require = session.eval("require(\"tiny\")");
    // GNU require(): packageStartupMessage on the not-yet-attached path.
    assert_eq!(require.output, "Loading required package: tiny\n");
    assert_eq!(require.typed, RValue::Logical(Some(true)));
    assert_eq!(session.eval("tiny_value()").output, "[1] 42\n");
    assert_eq!(session.eval("lazy_data").output, "[1] 271\n");
    // GNU data(package=) visibly returns a packageIQR object whose
    // print method renders the dataset table via file.show.
    let listing = session.eval("data(package = \"tiny\")");
    // GNU quotes the package name with sQuote: fancy quotes in UTF-8
    // locales, ASCII quotes in the C locale this session runs in.
    assert!(
        listing.stdout.contains("Data sets in package ‘tiny’")
            || listing.stdout.contains("Data sets in package 'tiny'"),
        "{listing:?}"
    );
    assert!(
        listing.stdout.contains("lazy_data") && listing.stdout.contains("tiny_data"),
        "{listing:?}"
    );
    let shape =
        session.eval("y <- data(package = \"tiny\")\nc(class(y), y$results[, \"Item\"], y$title)");
    assert_eq!(
        shape.typed,
        string_vector(vec![
            "packageIQR".to_string(),
            "lazy_data".to_string(),
            "tiny_data".to_string(),
            "Data sets".to_string()
        ])
    );
    assert_eq!(
        session
            .eval("data(\"tiny_data\", package = \"tiny\")\ntiny_data")
            .output,
        "[1] 314\n"
    );
    assert_eq!(
        session.eval("tiny_label").typed,
        string_vector(vec!["loaded".to_string()])
    );
    let secret = session.eval("tiny_secret");
    assert!(matches!(secret.typed, RValue::Error(_)), "{secret:?}");
    assert_eq!(session.eval("library(\"tiny\")").output, "");
    assert_eq!(
        session.eval("find.package(\"tiny\")").typed,
        string_vector(vec![pkg.to_string_lossy().into_owned()])
    );
    let search = session.eval("search()");
    match search.typed {
        RValue::StringVector(values) => {
            assert!(
                values.contains(&Some("package:tiny".to_string())),
                "{values:?}"
            );
        }
        other => panic!("expected search path string vector, got {other:?}"),
    }
    assert_eq!(session.eval("detach(\"package:tiny\")").typed, RValue::Null);
    let search = session.eval("search()");
    match search.typed {
        RValue::StringVector(values) => {
            assert!(
                !values.contains(&Some("package:tiny".to_string())),
                "{values:?}"
            );
        }
        other => panic!("expected search path string vector, got {other:?}"),
    }
    let after_detach = session.eval("tiny_value");
    assert!(
        matches!(after_detach.typed, RValue::Error(_)),
        "{after_detach:?}"
    );
    assert_eq!(
        session.eval("require(\"tiny\")").output,
        "Loading required package: tiny\n"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn test_library_supports_simple_namespace_imports_and_export_patterns() {
    let root = unique_test_root("android-namespace");
    let files = root.join("files");
    let cache = root.join("cache");
    let bundled = root.join("bundled-library");

    write_package(
        &bundled,
        "depall",
        "export(dep_value, dep_helper)\n",
        "dep_value <- function() 7L\ndep_helper <- function(x) x + 1L\ndep_hidden <- function() 99L\n",
    );
    write_package(
        &bundled,
        "depfrom",
        "export(dep_pick, dep_extra)\n",
        "dep_pick <- function() 11L\ndep_extra <- function() 100L\n",
    );
    let tiny = write_package(
        &bundled,
        "tiny",
        "import(depall)\nimportFrom(depfrom, dep_pick)\nexport(tiny_value)\nexportPattern(\"^tiny_\")\n",
        "tiny_value <- function() dep_helper(dep_value()) + dep_pick()\ntiny_label <- \"namespace\"\ntiny_hidden <- function() dep_hidden()\ntiny_imported <- function() dep_pick()\n",
    );

    let mut session = RSession::new();
    session
        .configure_paths(
            files.to_str().expect("utf8 files path"),
            cache.to_str().expect("utf8 cache path"),
            Some(bundled.to_str().expect("utf8 bundled path")),
        )
        .expect("configure paths");

    assert_eq!(session.eval("library(\"tiny\")").output, "");
    assert_eq!(session.eval("tiny_value()").output, "[1] 19\n");
    assert_eq!(
        session.eval("tiny_label").typed,
        string_vector(vec!["namespace".to_string()])
    );
    assert_eq!(session.eval("tiny_imported()").output, "[1] 11\n");

    let dep_value = session.eval("dep_value");
    assert!(matches!(dep_value.typed, RValue::Error(_)), "{dep_value:?}");
    let dep_pick = session.eval("dep_pick");
    assert!(matches!(dep_pick.typed, RValue::Error(_)), "{dep_pick:?}");
    let dep_hidden = session.eval("dep_hidden");
    assert!(
        matches!(dep_hidden.typed, RValue::Error(_)),
        "{dep_hidden:?}"
    );

    assert_eq!(
        session.eval("find.package(\"tiny\")").typed,
        string_vector(vec![tiny.to_string_lossy().into_owned()])
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn test_s3_method_registry_and_namespace_directives() {
    let root = unique_test_root("android-s3");
    let files = root.join("files");
    let cache = root.join("cache");
    let bundled = root.join("bundled-library");
    write_package(
        &bundled,
        "tiny",
        "export(tiny_generic)\nS3method(tiny_generic, myclass)\n",
        "tiny_generic <- function(x) UseMethod(\"tiny_generic\", x)\ntiny_generic.myclass <- function(x) 77L\n",
    );

    let mut session = RSession::new();
    session
        .configure_paths(
            files.to_str().expect("utf8 files path"),
            cache.to_str().expect("utf8 cache path"),
            Some(bundled.to_str().expect("utf8 bundled path")),
        )
        .expect("configure paths");

    assert_eq!(session.eval("library(\"tiny\")").output, "");
    assert_eq!(
        session
            .eval("getS3method(\"tiny_generic\", \"myclass\")(1L)")
            .output,
        "[1] 77\n"
    );
    assert_eq!(
        session
            .eval("x <- 1L\nclass(x) <- \"myclass\"\ntiny_generic(x)")
            .output,
        "[1] 77\n"
    );

    let private_method = session.eval("tiny_generic.myclass");
    assert!(
        matches!(private_method.typed, RValue::Error(_)),
        "{private_method:?}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn test_library_rejects_native_code_packages_explicitly() {
    let root = unique_test_root("android-native-policy");
    let files = root.join("files");
    let cache = root.join("cache");
    let bundled = root.join("bundled-library");
    write_package(
        &bundled,
        "tiny",
        "useDynLib(tiny)\nexport(tiny_value)\n",
        "tiny_value <- function() 42L\n",
    );

    let mut session = RSession::new();
    session
        .configure_paths(
            files.to_str().expect("utf8 files path"),
            cache.to_str().expect("utf8 cache path"),
            Some(bundled.to_str().expect("utf8 bundled path")),
        )
        .expect("configure paths");

    let result = session.eval("library(\"tiny\")");
    match result.typed {
        RValue::Error(message) => {
            assert!(message.contains("useDynLib(tiny)"), "{message}");
            assert!(message.contains("pure-R Android runtime"), "{message}");
        }
        other => panic!("expected native package load error, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn test_native_entrypoints_fail_with_android_policy_errors() {
    let root = unique_test_root("android-native-policy");
    let mut session = RSession::new();
    session
        .configure_paths(
            root.join("files").to_str().expect("utf8 files"),
            root.join("cache").to_str().expect("utf8 cache"),
            None,
        )
        .expect("android paths");
    for (expr, entrypoint) in [
        (".Call(\"native_symbol\")", ".Call"),
        (".C(\"native_symbol\", 1L)", ".C"),
        (".Fortran(\"native_symbol\", 1L)", ".Fortran"),
        (".External(\"native_symbol\")", ".External"),
        (".External2(\"native_symbol\")", ".External"),
        ("dyn.load(\"libnative.so\")", "dyn.load"),
        ("dyn.unload(\"libnative.so\")", "dyn.unload"),
        ("library.dynam(\"nativepkg\")", "library.dynam"),
    ] {
        let result = session.eval(expr);
        match result.typed {
            RValue::Error(message) => {
                assert!(message.contains(entrypoint), "{entrypoint}: {message}");
                assert!(message.contains("native extension code"), "{message}");
                assert!(message.contains("pure-R Android runtime"), "{message}");
            }
            other => panic!("expected native policy error for {expr}, got {other:?}"),
        }
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn test_data_rejects_serialized_package_data_explicitly() {
    let root = unique_test_root("android-data-policy");
    let files = root.join("files");
    let cache = root.join("cache");
    let bundled = root.join("bundled-library");
    let pkg = write_package(
        &bundled,
        "tiny",
        "export(tiny_value)\n",
        "tiny_value <- function() 42L\n",
    );
    let data_dir = pkg.join("data");
    std::fs::create_dir_all(&data_dir).expect("data dir");
    std::fs::write(data_dir.join("tiny_data.rda"), b"unsupported").expect("rda file");

    let mut session = RSession::new();
    session
        .configure_paths(
            files.to_str().expect("utf8 files path"),
            cache.to_str().expect("utf8 cache path"),
            Some(bundled.to_str().expect("utf8 bundled path")),
        )
        .expect("configure paths");

    let result = session.eval("data(\"tiny_data\", package = \"tiny\")");
    match result.typed {
        RValue::Error(message) => {
            // Explicit .rda files go through the restore reader even
            // under the pure-R policy (GNU data() semantics; the embed
            // corpus smoke test pins the same contract). Lazy databases
            // (Rdata.rdb) are what the package-data layer rejects.
            assert!(
                message.contains("bad restore file magic number"),
                "{message}"
            );
        }
        other => panic!("expected data policy error, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn test_session_eval_integer() {
    let mut session = RSession::new();
    let result = session.eval_integer(42);
    assert_eq!(result.value, 42.0);
    assert!(result.output.contains("42"));
    assert_eq!(result.typed, RValue::Integer(Some(42)));
}

#[test]
fn test_session_eval_real() {
    let mut session = RSession::new();
    let result = session.eval_real(3.14);
    assert!((result.value - 3.14).abs() < f64::EPSILON);
    assert!(result.output.contains("3.14"));
}

#[test]
fn test_session_eval_int_vector() {
    let mut session = RSession::new();
    let result = session.eval_int_vector(&[1, 2, 3]);
    assert!(result.output.contains('1'));
    assert_eq!(
        result.typed,
        RValue::IntegerVector(vec![Some(1), Some(2), Some(3)])
    );
}

#[test]
fn test_eval_returns_owned_typed_values() {
    let mut session = RSession::new();
    let strings = session.eval("c(\"a\", \"b\")");
    let strings_with_na = session.eval("c(\"a\", NA_character_)");
    let logical = session.eval("TRUE");
    let list = session.eval("list(1, \"x\")");

    assert_eq!(strings.typed, literal_string_vector(&["a", "b"]));
    assert_eq!(
        strings_with_na.typed,
        RValue::StringVector(vec![Some("a".to_string()), None])
    );
    assert_eq!(logical.typed, RValue::Logical(Some(true)));
    assert_eq!(
        list.typed,
        RValue::List(vec![RValue::Real(Some(1.0)), literal_string_vector(&["x"])])
    );
}

#[test]
fn test_eval_preserves_metadata_in_owned_typed_values() {
    // Detached RArena fixtures can no longer cross the graph-ownership
    // boundary enforced at mutation sites (SET_ATTRIB etc.); build the
    // attributed value inside the session instead.
    let mut session = RSession::new();
    let result = session.eval("structure(c(1L, 2L), names = c('a', 'b'), class = 'foo')");
    let RValue::Attributed { value, metadata } = result.typed else {
        panic!("expected attributed value");
    };

    assert_eq!(*value, RValue::IntegerVector(vec![Some(1), Some(2)]));
    assert_eq!(
        metadata.names,
        Some(vec![Some("a".to_string()), Some("b".to_string())])
    );
    assert_eq!(metadata.class, Some(vec![Some("foo".to_string())]));
    assert!(
        metadata
            .attributes
            .iter()
            .any(|attribute| attribute.name == "names")
    );
    assert_eq!(value.numeric_scalar_value(), 1.0);
}

#[test]
fn test_typed_values_expose_raw_and_complex_without_print_parsing() {
    let mut session = RSession::new();
    let raw = session.eval("as.raw(c(65, 90))");
    assert_eq!(raw.typed, RValue::RawVector(vec![0x41, 0x5a]));

    let mut arena = crate::sexp::memory::RArena::new();
    let complex = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(arena.alloc_vector(SEXPTYPE::CPLXSXP, 2)) }.unwrap();
    unsafe {
        /* SAFETY: fixture has no outstanding payload borrows. */
        complex
            .clone()
            .try_set_complex_elt(0, crate::sexp::Rcomplex { r: 1.0, i: -2.0 })
    }
    .unwrap();
    unsafe {
        /* SAFETY: fixture has no outstanding payload borrows. */
        complex.clone().try_set_complex_elt(
            1,
            crate::sexp::Rcomplex {
                r: crate::sexp::NA_REAL,
                i: 0.0,
            },
        )
    }
    .unwrap();

    assert_eq!(
        RValue::from_sexp(complex),
        RValue::ComplexVector(vec![
            Some(RComplexValue {
                real: 1.0,
                imaginary: -2.0,
            }),
            None,
        ])
    );
}

#[test]
fn test_unsupported_typed_values_only_carry_type_name() {
    let mut arena = crate::sexp::memory::RArena::new();
    let closure = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(arena.alloc_node(SEXPTYPE::CLOSXP)) }.unwrap();

    assert_eq!(
        RValue::from_sexp(closure),
        RValue::Unsupported {
            type_name: "closure".to_string(),
        }
    );
}

#[test]
fn test_session_math_unary() {
    let session = RSession::new();
    assert!((session.math_unary(RMathFunc::Sqrt, 4.0) - 2.0).abs() < 1e-10);
    assert!((session.math_unary(RMathFunc::Exp, 0.0) - 1.0).abs() < 1e-10);
    assert!(session.math_unary(RMathFunc::Log, 1.0).abs() < 1e-10);
    assert!(session.math_unary(RMathFunc::Sin, 0.0).abs() < 1e-10);
}

#[test]
fn test_session_bessel() {
    let session = RSession::new();
    let j0 = session.bessel(RBesselFunc::J, 0.0, 0.0, false);
    assert!((j0 - 1.0).abs() < 1e-10);

    let i0 = session.bessel(RBesselFunc::I, 0.0, 0.0, false);
    assert!((i0 - 1.0).abs() < 1e-10);
}

#[test]
fn bessel_warning_uses_its_detached_session_owner() {
    let mut session = RSession::new();
    let other = RSession::new();
    let first = session.bessel(RBesselFunc::J, 1.0, 500.0, false);
    assert_eq!(first, 0.0);
    assert_eq!(
        session
            .core
            .with_active_in(|p| unsafe { (*p).error_state.collect_warnings }),
        1
    );
    other.core.with_active_in(|p| {
        assert_eq!(unsafe { (*p).error_state.collect_warnings }, 0);
        let value = session.bessel(RBesselFunc::J, 1.0, 500.0, false);
        assert_eq!(value, 0.0);
        assert_eq!(unsafe { (*p).error_state.collect_warnings }, 0);
        assert_eq!(
            unsafe { crate::sexp::owner::OwnerToken::current().unwrap().as_ptr() },
            p
        );
    });
    assert_eq!(
        session
            .core
            .with_active_in(|p| unsafe { (*p).error_state.collect_warnings }),
        2
    );
    let setup = session.eval("options(warn=2)");
    assert!(!matches!(setup.typed, RValue::Error(_)), "{setup:?}");
    other.core.with_active_in(|p| {
        for (func, x, alpha) in [(RBesselFunc::J, 1.0, 500.0), (RBesselFunc::K, 1e-320, 1.0)] {
            let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                session.bessel(func, x, alpha, false)
            }));
            assert!(error.is_err(), "warn=2 must stop the {func:?} calculation");
            assert_eq!(
                unsafe { crate::sexp::owner::OwnerToken::current().unwrap().as_ptr() },
                p
            );
            assert_eq!(unsafe { (*p).error_state.collect_warnings }, 0);
        }
    });
    let reset = session.eval("options(warn=0)");
    assert!(!matches!(reset.typed, RValue::Error(_)), "{reset:?}");
    assert_eq!(session.bessel(RBesselFunc::J, 1.0, 500.0, false), 0.0);
    for func in [
        RBesselFunc::J,
        RBesselFunc::I,
        RBesselFunc::Y,
        RBesselFunc::K,
    ] {
        let before = session
            .core
            .with_active_in(|p| unsafe { (*p).error_state.collect_warnings });
        other.core.with_active_in(|p| {
            assert!(session.bessel(func, -1.0, 0.0, false).is_nan());
            assert_eq!(unsafe { (*p).error_state.collect_warnings }, 0);
        });
        assert_eq!(
            session
                .core
                .with_active_in(|p| unsafe { (*p).error_state.collect_warnings }),
            before + 1
        );
    }
}

#[test]
fn bessel_warning_stops_after_owner_revocation() {
    for (func, x, alpha) in [
        (RBesselFunc::J, 1.0, -500.25),
        (RBesselFunc::K, 1e-320, 1.0),
        (RBesselFunc::I, 1e-320, -1.25),
    ] {
        let session = RSession {
            core: CoreRSession::construct_detached(CoreRSession::new_for_gc_tests),
            result_limit: None,
        };
        let other = RSession {
            core: CoreRSession::construct_detached(CoreRSession::new_for_gc_tests),
            result_limit: None,
        };
        let owner = session.core.owner_token().unwrap().weak_owner().unwrap();
        let pin = owner.pin().unwrap();
        let ledger = unsafe { (*pin.as_ptr()).heap_identity.byte_ledger() };
        let workspace_before = ledger.workspace_bytes_for_test();
        let revoked = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = revoked.clone();
        session.core.with_active_in(|p| {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !seen.replace(true) {
                    unsafe { crate::sexp::instance::revoke_instance_availability(p) };
                }
            }));
            unsafe {
                (*p).memory_state.gc_force_gap = 1;
                (*p).memory_state.gc_force_wait = 1;
            }
        });
        other.core.with_active_in(|p| {
            let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                session.bessel(func, x, alpha, false)
            }))
            .unwrap_err();
            assert_eq!(
                error
                    .downcast_ref::<crate::sexp::context::RError>()
                    .unwrap()
                    .message,
                crate::sexp::object::SexpError::RootUnavailable.to_string()
            );
            assert!(revoked.get());
            assert!(!owner.is_live());
            assert_eq!(
                unsafe { crate::sexp::owner::OwnerToken::current().unwrap().as_ptr() },
                p
            );
            assert_eq!(unsafe { (*p).error_state.collect_warnings }, 0);
        });
        assert_eq!(ledger.workspace_bytes_for_test(), workspace_before);
    }
}

#[test]
fn bessel_workspace_obeys_session_budget() {
    let session = RSession {
        core: CoreRSession::construct_detached(CoreRSession::new_for_gc_tests),
        result_limit: None,
    };
    session.core.with_active(|| {
        let owner = session.core.owner_token().unwrap();
        let (bytes, workspace) = owner
            .with_arena(|arena| (arena.total_bytes_allocated(), arena.workspace_bytes()))
            .unwrap();
        owner
            .with_arena(|arena| {
                arena.set_budget(crate::sexp::memory::ArenaBudget::new(bytes + workspace, 0))
            })
            .unwrap();
        for func in [
            RBesselFunc::J,
            RBesselFunc::I,
            RBesselFunc::Y,
            RBesselFunc::K,
        ] {
            let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                session.bessel(func, 30.0, 10.0, false)
            }))
            .unwrap_err();
            assert!(
                error
                    .downcast_ref::<crate::sexp::context::RError>()
                    .unwrap()
                    .message
                    .contains("Bessel workspace exceeds session allocation limit"),
                "{func:?}"
            );
            assert_eq!(
                owner.with_arena(|a| a.workspace_bytes()).unwrap(),
                workspace
            );
        }
        owner
            .with_arena(|arena| arena.set_budget(crate::sexp::memory::ArenaBudget::unlimited()))
            .unwrap();
        for func in [
            RBesselFunc::J,
            RBesselFunc::I,
            RBesselFunc::Y,
            RBesselFunc::K,
        ] {
            assert!(
                session.bessel(func, 30.0, 10.0, false).is_finite(),
                "{func:?}"
            );
        }
    });
}

#[test]
fn bessel_workspace_matches_scalar_values() {
    let session = RSession {
        core: CoreRSession::construct_detached(CoreRSession::new_for_gc_tests),
        result_limit: None,
    };
    session.core.with_active(|| {
        for func in [
            RBesselFunc::J,
            RBesselFunc::I,
            RBesselFunc::Y,
            RBesselFunc::K,
        ] {
            for (x, alpha) in [
                (0.0, 0.0),
                (0.0, -2.25),
                (1e-300, 5.5),
                (2.0, 0.0),
                (2.0, 2.25),
                (2.0, -2.25),
                (2.0, -2.5),
                (30.0, 10.0),
                (1.0, 500.0),
                (1.0, -500.25),
                (f64::INFINITY, 0.0),
            ] {
                for scaled in [false, true] {
                    let expected = match func {
                        RBesselFunc::J => crate::special::bessel::bessel_j(x, alpha),
                        RBesselFunc::Y => crate::special::bessel::bessel_y(x, alpha),
                        RBesselFunc::I => crate::special::bessel::bessel_i(x, alpha, scaled),
                        RBesselFunc::K => crate::special::bessel::bessel_k(x, alpha, scaled),
                    };
                    let actual = session.bessel(func, x, alpha, scaled);
                    assert_eq!(
                        actual.to_bits(),
                        expected.to_bits(),
                        "{func:?} x={x} alpha={alpha} scaled={scaled}"
                    );
                }
            }
        }
    });
}

#[test]
fn bessel_workspace_remains_charged_during_warning_gc() {
    thread_local! {
        static OBSERVED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    fn inspect(_: &str) {
        let owner = unsafe { crate::sexp::owner::OwnerToken::current().unwrap() };
        let bytes = owner.with_arena(|arena| arena.workspace_bytes()).unwrap();
        OBSERVED.with(|value| value.set(bytes));
        unsafe { crate::sexp::gengc::full_gc() };
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            bytes
        );
    }
    struct ResetHook;
    impl Drop for ResetHook {
        fn drop(&mut self) {
            crate::nmath::error::set_warning_hook(Some(
                crate::mainutils::errors::nmath_warning_hook,
            ));
        }
    }
    let session = RSession {
        core: CoreRSession::construct_detached(CoreRSession::new_for_gc_tests),
        result_limit: None,
    };
    let baseline = session.core.with_active(|| {
        session
            .core
            .owner_token()
            .unwrap()
            .with_arena(|a| a.workspace_bytes())
            .unwrap()
    });
    OBSERVED.with(|value| value.set(0));
    crate::nmath::error::set_warning_hook(Some(inspect));
    let _reset = ResetHook;
    assert_eq!(session.bessel(RBesselFunc::J, 1.0, 500.0, false), 0.0);
    assert!(OBSERVED.with(|value| value.get()) >= baseline + 501 * size_of::<f64>());
    assert_eq!(
        session.core.with_active(|| {
            session
                .core
                .owner_token()
                .unwrap()
                .with_arena(|a| a.workspace_bytes())
                .unwrap()
        }),
        baseline
    );
}

#[test]
fn bessel_k_warning_reentry_keeps_session_workspace_charged() {
    thread_local! {
        static EVENTS: std::cell::RefCell<Vec<(String, usize)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    fn inspect(message: &str) {
        let owner = unsafe { crate::sexp::owner::OwnerToken::current().unwrap() };
        let bytes = owner.with_arena(|arena| arena.workspace_bytes()).unwrap();
        EVENTS.with(|events| events.borrow_mut().push((message.to_owned(), bytes)));
        unsafe { crate::sexp::gengc::full_gc() };
        assert_eq!(crate::special::bessel_j::bessel_j(0.0, 0.0), 1.0);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            bytes
        );
    }
    struct ResetHook;
    impl Drop for ResetHook {
        fn drop(&mut self) {
            crate::nmath::error::set_warning_hook(Some(
                crate::mainutils::errors::nmath_warning_hook,
            ));
        }
    }
    let session = RSession {
        core: CoreRSession::construct_detached(CoreRSession::new_for_gc_tests),
        result_limit: None,
    };
    let other = RSession {
        core: CoreRSession::construct_detached(CoreRSession::new_for_gc_tests),
        result_limit: None,
    };
    let baseline = session.core.with_active(|| {
        session
            .core
            .owner_token()
            .unwrap()
            .with_arena(|a| a.workspace_bytes())
            .unwrap()
    });
    EVENTS.with(|events| events.borrow_mut().clear());
    crate::nmath::error::set_warning_hook(Some(inspect));
    let _reset = ResetHook;
    other.core.with_active_in(|p| {
        assert_eq!(session.bessel(RBesselFunc::K, 1e-320, 1.0, false), 0.0);
        let _ = session.bessel(RBesselFunc::I, 1e-320, -2.25, false);
        assert_eq!(
            unsafe { crate::sexp::owner::OwnerToken::current().unwrap().as_ptr() },
            p
        );
        assert_eq!(unsafe { (*p).error_state.collect_warnings }, 0);
    });
    EVENTS.with(|events| {
        let events = events.borrow();
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0].0,
            "bessel_k(9.99989e-321): ncalc (=-1) != nb (=2); alpha=0. Arg. out of range?\n"
        );
        assert_eq!(
            events[1].0,
            "full precision may not have been achieved in 'bessel_i'\n"
        );
        assert_eq!(
            events[2].0,
            "bessel_k(9.99989e-321): ncalc (=-1) != nb (=3); alpha=0.25. Arg. out of range?\n"
        );
        assert!(events[0].1 >= baseline + 2 * size_of::<f64>());
        assert!(events[1].1 >= baseline + 3 * size_of::<f64>());
        assert!(events[2].1 >= baseline + 3 * size_of::<f64>());
    });
    assert_eq!(
        session.core.with_active(|| session
            .core
            .owner_token()
            .unwrap()
            .with_arena(|a| a.workspace_bytes())
            .unwrap()),
        baseline
    );
}

#[test]
fn test_session_dnorm_pnorm() {
    let session = RSession::new();
    let d = session.dnorm(0.0, 0.0, 1.0, false);
    assert!((d - 0.3989422804014327).abs() < 1e-10);

    let p = session.pnorm(0.0, 0.0, 1.0, true, false);
    assert!((p - 0.5).abs() < 1e-10);
}

#[test]
fn test_free_functions() {
    assert!((dnorm_free(0.0, 0.0, 1.0) - 0.3989422804014327).abs() < 1e-10);
    assert!((pnorm_free(0.0, 0.0, 1.0) - 0.5).abs() < 1e-10);
}

#[test]
fn test_eval_integer_literal() {
    let mut session = RSession::new();
    let result = session.eval("42");
    assert!(
        (result.value - 42.0).abs() < 1e-10,
        "value: {}",
        result.value
    );
}

#[test]
fn test_eval_real_literal() {
    let mut session = RSession::new();
    let result = session.eval("3.14");
    assert!(
        (result.value - 3.14).abs() < 1e-10,
        "value: {}",
        result.value
    );
}

#[test]
fn test_eval_addition() {
    let mut session = RSession::new();
    let result = session.eval("1 + 2");
    assert!(
        (result.value - 3.0).abs() < 1e-10,
        "value: {}",
        result.value
    );
}

#[test]
fn test_eval_honors_visibility_for_assignment_and_invisible() {
    let mut session = RSession::new();

    let assigned = session.eval("x <- 42");
    assert_eq!(assigned.output, "");
    assert_eq!(assigned.typed, RValue::Real(Some(42.0)));

    let invisible = session.eval("invisible(7)");
    assert_eq!(invisible.output, "");
    assert_eq!(invisible.typed, RValue::Real(Some(7.0)));

    let visible = session.eval("x");
    assert_eq!(visible.output, "[1] 42\n");
}

#[test]
fn test_eval_captures_explicit_output_without_implicit_reprint() {
    let mut session = RSession::new();

    let cat = session.eval("cat(\"hello\")");
    assert_eq!(cat.output, "hello");
    assert_eq!(cat.typed, RValue::Null);

    let printed = session.eval("print(1)");
    assert_eq!(printed.output, "[1] 1\n");
    assert_eq!(printed.typed, RValue::Real(Some(1.0)));
}

#[test]
fn test_stopifnot_is_invisible_on_success_and_errors_on_failure() {
    let mut session = RSession::new();

    let ok = session.eval("stopifnot(TRUE)");
    assert_eq!(ok.output, "");
    assert_eq!(ok.typed, RValue::Null);

    let err = session.eval("stopifnot(FALSE)");
    assert!(matches!(err.typed, RValue::Error(_)));
    assert!(err.output.contains("FALSE is not TRUE"));
}

#[test]
fn test_with_visible_returns_visible_list_with_captured_flag() {
    let mut session = RSession::new();

    let visible = session.eval("withVisible(1)");
    assert_eq!(
        visible.output.trim_end(),
        "$value\n[1] 1\n\n$visible\n[1] TRUE"
    );

    let RValue::Attributed { value, metadata } = visible.typed else {
        panic!("expected attributed withVisible result");
    };
    assert_eq!(
        *value,
        RValue::List(vec![RValue::Real(Some(1.0)), RValue::Logical(Some(true))])
    );
    assert_eq!(
        metadata.names,
        Some(vec![Some("value".to_string()), Some("visible".to_string())])
    );

    let invisible = session.eval("withVisible(invisible(1))");
    assert_eq!(
        invisible.output.trim_end(),
        "$value\n[1] 1\n\n$visible\n[1] FALSE"
    );

    let RValue::Attributed { value, metadata } = invisible.typed else {
        panic!("expected attributed withVisible result");
    };
    assert_eq!(
        *value,
        RValue::List(vec![RValue::Real(Some(1.0)), RValue::Logical(Some(false))])
    );
    assert_eq!(
        metadata.names,
        Some(vec![Some("value".to_string()), Some("visible".to_string())])
    );
}

#[test]
fn test_capture_output_evaluates_expression_under_capture() {
    let mut session = RSession::new();

    let printed = session.eval("capture.output(print(1))");
    assert_eq!(printed.output, "[1] \"[1] 1\"\n");
    assert_eq!(printed.typed, literal_string_vector(&["[1] 1"]));

    let cat = session.eval("capture.output(cat(\"hello\"))");
    assert_eq!(cat.output, "[1] \"hello\"\n");
    assert_eq!(cat.typed, literal_string_vector(&["hello"]));
}

#[test]
fn test_stop_warning_message_and_suppression() {
    let mut session = RSession::new();

    let stopped = session.eval("stop(\"boom\")");
    assert!(matches!(stopped.typed, RValue::Error(_)));
    assert_eq!(stopped.output.trim_end(), "Error: boom");

    let warned = session.eval("warning(\"careful\"); 1");
    assert_eq!(warned.output, "Warning message:\ncareful \n[1] 1\n");

    let messaged = session.eval("message(\"hi\"); 1");
    assert_eq!(messaged.output, "hi\n[1] 1\n");

    let suppress_warning = session.eval("suppressWarnings(warning(\"careful\")); 1");
    assert_eq!(suppress_warning.output, "[1] 1\n");

    let suppress_message = session.eval("suppressMessages(message(\"hi\")); 1");
    assert_eq!(suppress_message.output, "[1] 1\n");
}

#[test]
fn test_structure_deprecation_warning_preserves_condition_class() {
    let mut session = RSession::new();

    let result = session.eval(
        r#"
            caught <- NULL
            withCallingHandlers(
                structure(1:4, .Dim = c(2L, 2L)),
                warning = function(w) {
                    caught <<- w
                    invokeRestart("muffleWarning")
                }
            )
            inherits(caught, "deprecatedWarning")
            "#,
    );

    assert_eq!(result.typed, RValue::Logical(Some(true)));
}

#[test]
fn test_regexpr_reports_match_attributes() {
    let mut session = RSession::new();

    let result = session.eval("regexpr(\"a\", c(\"cat\", \"dog\"))");
    assert_eq!(
        result.output,
        "[1]  2 -1\nattr(,\"match.length\")\n[1]  1 -1\nattr(,\"index.type\")\n[1] \"chars\"\nattr(,\"useBytes\")\n[1] TRUE\n"
    );

    let match_length = session.eval("attr(regexpr(\"a\", c(\"cat\", \"dog\")), \"match.length\")");
    assert_eq!(match_length.output, "[1]  1 -1\n");

    let use_bytes = session.eval("attr(regexpr(\"a\", \"cat\"), \"useBytes\")");
    assert_eq!(use_bytes.output, "[1] TRUE\n");
}

#[test]
fn test_connection_seekability_uses_connection_capability() {
    let mut session = RSession::new();

    let file = session
            .eval("con <- file(tempfile(), \"w+\", TRUE, \"native.enc\", \"default\", FALSE); isSeekable(con)");
    assert_eq!(file.typed, RValue::Logical(Some(true)));
}

#[test]
fn test_connection_pushback_is_session_owned_and_readable() {
    let mut session = RSession::new();

    let length = session.eval(
            "con <- textConnection(\"pb\", c(\"underlying\"), \"r\", FALSE); pushBack(c(\"first\", \"second\"), con); pushBackLength(con)",
        );
    assert_eq!(length.output, "[1] 2\n");

    let lines = session.eval("readLines(con, 3, TRUE, TRUE, \"\", FALSE)");
    assert_eq!(
        lines.typed,
        RValue::StringVector(vec![
            Some("first".to_string()),
            Some("second".to_string()),
            Some("underlying".to_string()),
        ])
    );

    let drained = session.eval("pushBackLength(con)");
    assert_eq!(drained.output, "[1] 0\n");
}

#[test]
fn test_socket_connection_errors_explicitly() {
    let mut session = RSession::new();
    let result = session.eval("socketConnection(\"127.0.0.1\", 9)");
    assert!(
        matches!(result.typed, RValue::Error(message) if message.contains("socketConnection is not supported"))
    );
}

#[test]
fn test_sample_int_uniform_shape_and_errors() {
    let mut session = RSession::new();

    let permutation = session.eval("all(sort(sample.int(5)) == 1:5)");
    assert_eq!(permutation.output, "[1] TRUE\n");

    let replace = session.eval("length(sample.int(3, 7, TRUE))");
    assert_eq!(replace.output, "[1] 7\n");

    let weighted_replace = session.eval("sample.int(3, 5, TRUE, c(0, 0, 1))");
    assert_eq!(weighted_replace.output, "[1] 3 3 3 3 3\n");

    let weighted_no_replace = session.eval("length(sample.int(3, 2, FALSE, c(0, 1, 1)))");
    assert_eq!(weighted_no_replace.output, "[1] 2\n");

    let too_large = session.eval("sample.int(3, 4, FALSE)");
    assert!(matches!(too_large.typed, RValue::Error(_)));
    assert!(
        too_large
            .output
            .contains("cannot take a sample larger than the population when 'replace = FALSE'")
    );
}

#[test]
fn test_sample_matches_core_r_shape_and_type_rules() {
    let mut session = RSession::new();

    let shortcut = session.eval("all(sort(sample(5)) == 1:5)");
    assert_eq!(shortcut.output, "[1] TRUE\n");

    let integer_type = session.eval("is.integer(sample(5, 3))");
    assert_eq!(integer_type.typed, RValue::Logical(Some(true)));

    let character_type = session.eval("is.character(sample(c(\"a\", \"b\", \"c\"), 2))");
    assert_eq!(character_type.typed, RValue::Logical(Some(true)));

    let logical_type = session.eval("is.logical(sample(c(TRUE, FALSE), 2))");
    assert_eq!(logical_type.typed, RValue::Logical(Some(true)));

    let names = session.eval(
            "x <- c(a = 10, b = 20, c = 30)\ny <- sample(x, 2)\nall(c(length(names(y)) == 2, names(y) %in% names(x)))",
        );
    assert_eq!(names.typed, RValue::Logical(Some(true)));

    let too_large = session.eval("sample(c(\"a\", \"b\"), 3, FALSE)");
    assert!(matches!(too_large.typed, RValue::Error(_)));
    assert!(
        too_large
            .output
            .contains("cannot take a sample larger than the population when 'replace = FALSE'")
    );

    let weighted_replace =
        session.eval("all(sample(c(\"a\", \"b\", \"c\"), 5, TRUE, c(0, 0, 1)) == \"c\")");
    assert_eq!(weighted_replace.typed, RValue::Logical(Some(true)));
    let weighted_replace_length =
        session.eval("length(sample(c(\"a\", \"b\", \"c\"), 5, TRUE, c(0, 0, 1)))");
    assert_eq!(weighted_replace_length.output, "[1] 5\n");

    let weighted_no_replace = session.eval("all(sample(1:3, 2, FALSE, c(0, 1, 1)) != 1L)");
    assert_eq!(weighted_no_replace.typed, RValue::Logical(Some(true)));
    let weighted_no_replace_length = session.eval("length(sample(1:3, 2, FALSE, c(0, 1, 1)))");
    assert_eq!(weighted_no_replace_length.output, "[1] 2\n");

    let impossible = session.eval("sample(1:3, 2, FALSE, c(1, 0, 0))");
    assert!(matches!(impossible.typed, RValue::Error(_)));
    assert!(impossible.output.contains("too few positive probabilities"));
}

#[test]
#[ignore = "diagnostic; complete package graph census, no performance claim"]
fn complete_producer_graph_census() {
    let mut session = RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp"));
    session.enable_browser_files();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for path in [
        "tests/conformance/cases/260_length_replacement.R",
        "tests/conformance/cases/492_rep_int_len_helpers.R",
        "tests/conformance/cases/288_with_calling_handlers.R",
        "tests/conformance/cases/388_no_print_summary_method_aliases.R",
        "tests/conformance/cases/1094_cat_complex_fft.R",
        "tests/conformance/cases/544_complex_constructor_format_str_parity.R",
        "tests/conformance/cases/551_round_signif_complex_parity.R",
        "crates/r-embed/tests/fixtures/complex-print-public-contract.R",
        "crates/r-embed/tests/fixtures/stats-namespace-public-contract.R",
        "crates/r-embed/tests/fixtures/get-lazy-mode-public-contract.R",
        "crates/r-embed/tests/fixtures/correlation-public-contract.R",
        "crates/r-embed/tests/fixtures/palette-public-contract.R",
        "crates/r-embed/tests/fixtures/tempfile-public-contract.R",
        "crates/r-embed/tests/fixtures/bincode-public-contract.R",
        "crates/r-embed/tests/fixtures/print-gap-public-contract.R",
        "crates/r-embed/tests/fixtures/arima0-public-contract.R",
        "crates/r-embed/tests/fixtures/utils-console-public-contract.R",
        "crates/r-embed/tests/fixtures/mapply-public-contract.R",
        "crates/r-embed/tests/fixtures/sample-condition-contract.R",
        "crates/r-embed/tests/fixtures/rep-len-admission-contract.R",
        "crates/r-embed/tests/fixtures/calling-error-handler-public-contract.R",
        "crates/r-embed/tests/fixtures/calling-warning-handler-public-contract.R",
        "crates/r-embed/tests/fixtures/s3-methods-public-contract.R",
        "crates/r-embed/tests/fixtures/namespace-s3-startup-contract.R",
        "crates/r-embed/tests/fixtures/base-print-methods-public-contract.R",
        "crates/r-embed/tests/fixtures/compiled-dots-public-contract.R",
        "crates/r-embed/tests/fixtures/vector-print-limits-public-contract.R",
        "crates/r-embed/tests/fixtures/print-digits-public-contract.R",
        "crates/r-embed/tests/fixtures/prmatrix-public-contract.R",
        "crates/r-embed/tests/fixtures/condition-handler-stack-public-contract.R",
        "crates/r-embed/tests/fixtures/attribute-print-public-contract.R",
    ] {
        let result = session.eval(&std::fs::read_to_string(root.join(path)).unwrap());
        assert!(
            !matches!(result.typed, RValue::Error(_)),
            "{path}: {result:?}"
        );
        session.eval("invisible(gc())");
        println!("CENSUS {path}: {:?}", session.arena_stats());
    }
    session.core.with_active_in(|instance| unsafe {
        use crate::sexp::accessors::{TYPEOF,XLENGTH,CHAR,INTEGER_ELT};
        use crate::sexp::ffi::SEXPTYPE;
        let mut kinds = std::collections::BTreeMap::new();
        let mut strings = std::collections::HashSet::new();
        let mut total_strings = 0;
        let mut scalars = std::collections::BTreeMap::new();
        for node in (*instance).arena.active_nodes() {
            let kind = TYPEOF(node);
            *kinds.entry(kind as i32).or_insert(0usize) += 1;
            if kind == SEXPTYPE::CHARSXP {
                total_strings += 1;
                let bytes=std::slice::from_raw_parts(CHAR(node).cast::<u8>(),XLENGTH(node) as usize);
                strings.insert(bytes.to_vec());
            }
            if (kind == SEXPTYPE::INTSXP || kind == SEXPTYPE::LGLSXP) && XLENGTH(node)==1 {
                *scalars.entry((kind as i32,INTEGER_ELT(node,0))).or_insert(0usize)+=1;
            }
        }
        println!("CENSUS kinds={kinds:?}; strings={total_strings}; unique_strings={}; scalar_ints={scalars:?}", strings.len());
    });
}
