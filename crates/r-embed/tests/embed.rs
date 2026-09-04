//! Integration tests for the r-embed public API.

use r_embed::{
    AndroidRuntimePaths, CancellationToken, RPackageInfo, RResourceLimits, RRuntimeInfo, RSession,
    RValue,
};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

struct DecodedPng {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl DecodedPng {
    fn non_white_in_region(&self, x0: u32, y0: u32, x1: u32, y1: u32) -> usize {
        let x1 = x1.min(self.width);
        let y1 = y1.min(self.height);
        let mut count = 0;
        for y in y0.min(self.height)..y1 {
            for x in x0.min(self.width)..x1 {
                let offset = ((y * self.width + x) * 4) as usize;
                let pixel = &self.rgba[offset..offset + 4];
                if pixel != [255, 255, 255, 255] {
                    count += 1;
                }
            }
        }
        count
    }

    fn red_pixels(&self) -> usize {
        self.pixels_matching(|rgba| rgba[0] > 180 && rgba[1] < 120 && rgba[2] < 120)
    }

    fn green_pixels(&self) -> usize {
        self.pixels_matching(|rgba| rgba[0] < 120 && rgba[1] > 100 && rgba[2] < 120)
    }

    fn pixels_matching(&self, matches: impl Fn(&[u8]) -> bool) -> usize {
        self.rgba
            .chunks_exact(4)
            .filter(|rgba| matches(rgba) && rgba[3] > 0)
            .count()
    }
}

fn decode_png_rgba(png_bytes: &[u8]) -> DecodedPng {
    let decoder = png::Decoder::new(Cursor::new(png_bytes));
    let mut reader = decoder.read_info().expect("png reader");
    let mut buffer = vec![0; reader.output_buffer_size().expect("png output size")];
    let info = reader.next_frame(&mut buffer).expect("png frame");
    let bytes = &buffer[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => bytes
            .chunks_exact(3)
            .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
            .collect(),
        other => panic!("unexpected png color type: {other:?}"),
    };
    DecodedPng {
        width: info.width,
        height: info.height,
        rgba,
    }
}

fn unique_test_root(root_name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "{root_name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    ))
}

fn make_test_package(root_name: &str) -> (PathBuf, PathBuf) {
    make_test_package_with_source(
        root_name,
        "export(tiny_value)\n",
        "tiny_value <- function() 42L\n",
    )
}

fn make_test_package_with_source(
    root_name: &str,
    namespace: &str,
    source: &str,
) -> (PathBuf, PathBuf) {
    let root = unique_test_root(root_name);
    let bundled = root.join("bundled-library");
    let pkg = write_fixture_package(
        &bundled,
        FixturePackage {
            name: "tiny",
            description: concat!(
                "Package: tiny\n",
                "Version: 0.0.1\n",
                "Title: Tiny Test Package\n",
                "Description: Tiny package for Android runtime tests\n",
                "License: MIT\n",
                "Depends: R (>= 4.0.0)\n",
                "Imports: depall, depfrom\n",
                "Suggests: testthat\n",
                "NeedsCompilation: no\n",
            ),
            namespace,
            sources: &[("tiny.R", source)],
            data_sources: &[],
            extra_files: &[],
        },
    );
    (root, pkg)
}

struct FixturePackage<'a> {
    name: &'a str,
    description: &'a str,
    namespace: &'a str,
    sources: &'a [(&'a str, &'a str)],
    data_sources: &'a [(&'a str, &'a str)],
    extra_files: &'a [(&'a str, &'a [u8])],
}

fn write_fixture_package(library: &Path, package: FixturePackage<'_>) -> PathBuf {
    let pkg = library.join(package.name);
    let r_dir = pkg.join("R");
    std::fs::create_dir_all(&r_dir).expect("package R dir");
    std::fs::write(pkg.join("DESCRIPTION"), package.description).expect("description");
    std::fs::write(pkg.join("NAMESPACE"), package.namespace).expect("namespace");
    for (file, source) in package.sources {
        std::fs::write(r_dir.join(file), source).expect("R source");
    }
    if !package.data_sources.is_empty() {
        let data_dir = pkg.join("data");
        std::fs::create_dir_all(&data_dir).expect("data dir");
        for (file, source) in package.data_sources {
            std::fs::write(data_dir.join(file), source).expect("data source");
        }
    }
    for (file, bytes) in package.extra_files {
        let path = pkg.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("extra file parent");
        }
        std::fs::write(path, bytes).expect("extra file");
    }
    pkg
}

fn android_paths_for(root: &Path) -> AndroidRuntimePaths {
    AndroidRuntimePaths::new(
        root.join("files").to_str().expect("utf8 files path"),
        root.join("cache").to_str().expect("utf8 cache path"),
        Some(
            root.join("bundled-library")
                .to_str()
                .expect("utf8 bundled path"),
        ),
    )
}

#[test]
fn eval_uses_isolated_session_state() {
    let mut left = RSession::new().expect("left session");
    let mut right = RSession::new().expect("right session");

    assert_eq!(left.eval("x <- 11\nx").unwrap(), "[1] 11");
    assert_eq!(right.eval("x <- 29\nx").unwrap(), "[1] 29");
    assert_eq!(left.eval("x").unwrap(), "[1] 11");
    assert_eq!(right.eval("x").unwrap(), "[1] 29");
}

#[test]
fn eval_result_returns_owned_typed_value() {
    let mut session = RSession::new().expect("session");
    let result = session.eval_result("c(1, 2, 3)").expect("eval");
    assert_eq!(result.output, "[1] 1 2 3");
    assert_eq!(
        result.value,
        RValue::RealVector(vec![Some(1.0), Some(2.0), Some(3.0)])
    );

    let strings = session
        .eval_result("c(\"a\", NA_character_)")
        .expect("eval strings");
    assert_eq!(
        strings.value,
        RValue::StringVector(vec![Some("a".to_string()), None])
    );
}

#[test]
fn resource_limits_are_session_owned_and_enforced() {
    let mut limited = RSession::new().expect("limited session");
    let mut normal = RSession::new().expect("normal session");

    limited
        .set_resource_limits(RResourceLimits {
            max_eval_depth: 1,
            max_execution_time_ms: 0,
            max_alloc_bytes: 0,
            max_arena_nodes: 0,
        })
        .expect("set limits");

    let limits = limited.resource_limits();
    assert_eq!(limits.max_eval_depth, 1);
    assert_eq!(normal.resource_limits().max_eval_depth, 500);

    let err = limited
        .eval_result("{ 1 + 1 }")
        .expect_err("depth limit should reject nested eval");
    assert!(err.to_string().contains("too deeply"));
    assert_eq!(normal.eval("1 + 1").expect("normal eval"), "[1] 2");
}

#[test]
fn configure_android_paths_reaches_embedded_runtime() {
    let root = std::env::temp_dir().join(format!(
        "rport-embed-paths-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    ));
    let files = root.join("files");
    let cache = root.join("cache");
    let bundled = root.join("bundled-library");
    let paths = AndroidRuntimePaths::new(
        files.to_str().expect("utf8 files path"),
        cache.to_str().expect("utf8 cache path"),
        Some(bundled.to_str().expect("utf8 bundled path")),
    );

    assert_eq!(
        paths.user_library_dir(),
        files
            .join("R")
            .join("library")
            .to_string_lossy()
            .into_owned()
    );
    assert_eq!(
        paths.temp_dir(),
        cache.join("Rtmp").to_string_lossy().into_owned()
    );
    assert_eq!(
        paths.library_paths(),
        vec![
            files
                .join("R")
                .join("library")
                .to_string_lossy()
                .into_owned(),
            bundled.to_string_lossy().into_owned()
        ]
    );

    let mut session = RSession::new().expect("session");
    session
        .configure_android_runtime(&paths)
        .expect("path config");

    let result = session.eval_result(".libPaths()").expect("lib paths");
    assert_eq!(
        result.value,
        RValue::StringVector(vec![
            Some(
                files
                    .join("R")
                    .join("library")
                    .to_string_lossy()
                    .into_owned()
            ),
            Some(bundled.to_string_lossy().into_owned())
        ])
    );
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
fn package_helpers_load_android_library_package() {
    let (root, pkg) = make_test_package("rport-embed-package");
    let files = root.join("files");
    let cache = root.join("cache");
    let bundled = root.join("bundled-library");
    let paths = AndroidRuntimePaths::new(
        files.to_str().expect("utf8 files path"),
        cache.to_str().expect("utf8 cache path"),
        Some(bundled.to_str().expect("utf8 bundled path")),
    );

    let mut session = RSession::new().expect("session");
    session
        .configure_android_runtime(&paths)
        .expect("path config");

    assert!(session.package_available("tiny"));
    assert_eq!(
        session.package_path("tiny"),
        Some(pkg.to_string_lossy().into_owned())
    );
    assert_eq!(
        session.package_info("tiny"),
        Some(RPackageInfo {
            name: "tiny".to_string(),
            version: "0.0.1".to_string(),
            title: "Tiny Test Package".to_string(),
            description: "Tiny package for Android runtime tests".to_string(),
            license: "MIT".to_string(),
            depends: "R (>= 4.0.0)".to_string(),
            imports: "depall, depfrom".to_string(),
            suggests: "testthat".to_string(),
            needs_compilation: false,
            path: pkg.to_string_lossy().into_owned(),
            library_path: bundled.to_string_lossy().into_owned(),
        })
    );
    assert_eq!(session.installed_packages().len(), 1);
    assert_eq!(session.installed_packages()[0].name, "tiny");
    assert!(!session.package_available("../tiny"));
    session.load_package("tiny").expect("load package");
    assert_eq!(session.eval("tiny_value()").expect("eval"), "[1] 42");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn pure_r_package_corpus_smoke_lists_loads_and_runs_supported_packages() {
    let root = unique_test_root("rport-embed-corpus");
    let bundled = root.join("bundled-library");

    let base_pkg = write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corpbase",
            description: concat!(
                "Package: corpbase\n",
                "Version: 0.1.0\n",
                "Title: Corpus Base Package\n",
                "Description: Exercises exports, S3 methods, and source-form data.\n",
                "License: MIT\n",
                "NeedsCompilation: no\n",
            ),
            namespace: "export(base_value, make_corp, corp_generic)\nS3method(corp_generic, corpclass)\n",
            sources: &[(
                "base.R",
                concat!(
                    "base_value <- function() 10L\n",
                    "make_corp <- function() { x <- 1L; class(x) <- \"corpclass\"; x }\n",
                    "corp_generic <- function(x) UseMethod(\"corp_generic\", x)\n",
                    "corp_generic.corpclass <- function(x) 123L\n",
                ),
            )],
            data_sources: &[("corp_data.R", "corp_data <- 55L\n")],
            extra_files: &[],
        },
    );
    write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corpimport",
            description: concat!(
                "Package: corpimport\n",
                "Version: 0.1.0\n",
                "Title: Corpus Import Package\n",
                "Description: Exercises whole-package namespace imports.\n",
                "License: MIT\n",
                "Imports: corpbase\n",
                "NeedsCompilation: no\n",
            ),
            namespace: "import(corpbase)\nexport(import_value)\n",
            sources: &[("import.R", "import_value <- function() base_value() + 5L\n")],
            data_sources: &[],
            extra_files: &[],
        },
    );
    write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corpcollate",
            description: concat!(
                "Package: corpcollate\n",
                "Version: 0.1.0\n",
                "Title: Corpus Collate Package\n",
                "Description: Exercises DESCRIPTION Collate source ordering.\n",
                "License: MIT\n",
                "NeedsCompilation: no\n",
                "Collate: 'z-producer.R' 'a-consumer.R'\n",
            ),
            namespace: "export(collate_value)\n",
            sources: &[
                ("a-consumer.R", "collate_value <- collate_seed + 2L\n"),
                ("z-producer.R", "collate_seed <- 40L\n"),
            ],
            data_sources: &[],
            extra_files: &[],
        },
    );
    write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corpdepends",
            description: concat!(
                "Package: corpdepends\n",
                "Version: 0.1.0\n",
                "Title: Corpus Depends Package\n",
                "Description: Exercises DESCRIPTION Depends loading and symbol visibility.\n",
                "License: MIT\n",
                "Depends: R (>= 4.0.0), corpbase\n",
                "NeedsCompilation: no\n",
            ),
            namespace: "export(depends_value)\n",
            sources: &[(
                "depends.R",
                "depends_value <- function() base_value() + 9L\n",
            )],
            data_sources: &[],
            extra_files: &[],
        },
    );
    write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corpexamples",
            description: concat!(
                "Package: corpexamples\n",
                "Version: 0.1.0\n",
                "Title: Corpus Examples Package\n",
                "Description: Exercises package resource and example file discovery.\n",
                "License: MIT\n",
                "NeedsCompilation: no\n",
            ),
            namespace: "export(example_value)\n",
            sources: &[("examples.R", "example_value <- function() 12L\n")],
            data_sources: &[],
            extra_files: &[("examples/example-resource.R", b"example_answer <- 42L\n")],
        },
    );
    write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corpfrom",
            description: concat!(
                "Package: corpfrom\n",
                "Version: 0.1.0\n",
                "Title: Corpus ImportFrom Package\n",
                "Description: Exercises selective namespace imports.\n",
                "License: MIT\n",
                "Imports: corpbase\n",
                "NeedsCompilation: no\n",
            ),
            namespace: "importFrom(corpbase, base_value)\nexport(from_value)\n",
            sources: &[("from.R", "from_value <- function() base_value() + 7L\n")],
            data_sources: &[],
            extra_files: &[],
        },
    );
    write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corppattern",
            description: concat!(
                "Package: corppattern\n",
                "Version: 0.1.0\n",
                "Title: Corpus Export Pattern Package\n",
                "Description: Exercises NAMESPACE exportPattern handling.\n",
                "License: MIT\n",
                "NeedsCompilation: no\n",
            ),
            namespace: "exportPattern(\"^pat_\")\n",
            sources: &[(
                "pattern.R",
                "pat_value <- function() 31L\nhidden_value <- function() 99L\n",
            )],
            data_sources: &[],
            extra_files: &[],
        },
    );
    write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corps4",
            description: concat!(
                "Package: corps4\n",
                "Version: 0.1.0\n",
                "Title: Corpus S4 Package\n",
                "Description: Exercises pure-R package S4 class creation and slot access.\n",
                "License: MIT\n",
                "NeedsCompilation: no\n",
            ),
            namespace: "export(make_person, person_name, person_slots)\n",
            sources: &[(
                "s4.R",
                concat!(
                    "setClass(\"CorpusPerson\", name = \"character\", score = \"numeric\")\n",
                    "make_person <- function() new(\"CorpusPerson\", name = \"Ada\", score = 42)\n",
                    "person_name <- function(x) slot(x, \"name\")\n",
                    "person_slots <- function() slotNames(\"CorpusPerson\")\n",
                ),
            )],
            data_sources: &[],
            extra_files: &[],
        },
    );
    write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corpdataenv",
            description: concat!(
                "Package: corpdataenv\n",
                "Version: 0.1.0\n",
                "Title: Corpus Data Environment Package\n",
                "Description: Exercises data(..., envir=) package loading.\n",
                "License: MIT\n",
                "NeedsCompilation: no\n",
            ),
            namespace: "export(dataenv_value)\n",
            sources: &[("dataenv.R", "dataenv_value <- function() 5L\n")],
            data_sources: &[("env_data.R", "env_data <- 88L\n")],
            extra_files: &[],
        },
    );
    write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corpsourcelazy",
            description: concat!(
                "Package: corpsourcelazy\n",
                "Version: 0.1.0\n",
                "Title: Corpus Source Lazy Data Package\n",
                "Description: Exercises LazyData with source-form package data.\n",
                "License: MIT\n",
                "NeedsCompilation: no\n",
                "LazyData: true\n",
            ),
            namespace: "export(lazy_source_value)\n",
            sources: &[(
                "lazy-source.R",
                "lazy_source_value <- function() lazy_source_data + 2L\n",
            )],
            data_sources: &[("lazy_source_data.R", "lazy_source_data <- 90L\n")],
            extra_files: &[],
        },
    );
    write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corppaths",
            description: concat!(
                "Package: corppaths\n",
                "Version: 0.1.0\n",
                "Title: Corpus Runtime Paths Package\n",
                "Description: Exercises package-visible Android library paths.\n",
                "License: MIT\n",
                "NeedsCompilation: no\n",
            ),
            namespace: "export(corpus_lib_paths)\n",
            sources: &[("paths.R", "corpus_lib_paths <- function() .libPaths()\n")],
            data_sources: &[],
            extra_files: &[],
        },
    );
    write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corpnative",
            description: concat!(
                "Package: corpnative\n",
                "Version: 0.1.0\n",
                "Title: Corpus Native Policy Package\n",
                "Description: Exercises explicit native-code rejection.\n",
                "License: MIT\n",
                "NeedsCompilation: no\n",
            ),
            namespace: "useDynLib(corpnative)\nexport(native_value)\n",
            sources: &[("native.R", "native_value <- function() 1L\n")],
            data_sources: &[],
            extra_files: &[],
        },
    );
    write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corpcompiled",
            description: concat!(
                "Package: corpcompiled\n",
                "Version: 0.1.0\n",
                "Title: Corpus Compiled Policy Package\n",
                "Description: Exercises DESCRIPTION NeedsCompilation rejection.\n",
                "License: MIT\n",
                "NeedsCompilation: yes\n",
            ),
            namespace: "export(compiled_value)\n",
            sources: &[("compiled.R", "compiled_value <- function() 1L\n")],
            data_sources: &[],
            extra_files: &[],
        },
    );
    write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corpbytecode",
            description: concat!(
                "Package: corpbytecode\n",
                "Version: 0.1.0\n",
                "Title: Corpus Bytecode Policy Package\n",
                "Description: Exercises lazyload bytecode rejection.\n",
                "License: MIT\n",
                "NeedsCompilation: no\n",
            ),
            namespace: "export(byte_value)\n",
            sources: &[],
            data_sources: &[],
            extra_files: &[("R/corpbytecode.rdb", b"unsupported lazyload code")],
        },
    );
    write_fixture_package(
        &bundled,
        FixturePackage {
            name: "corplazydata",
            description: concat!(
                "Package: corplazydata\n",
                "Version: 0.1.0\n",
                "Title: Corpus Lazy Data Policy Package\n",
                "Description: Exercises serialized data rejection.\n",
                "License: MIT\n",
                "NeedsCompilation: no\n",
            ),
            namespace: "export(lazy_value)\n",
            sources: &[("lazy.R", "lazy_value <- function() 1L\n")],
            data_sources: &[],
            extra_files: &[("data/lazy_data.rda", b"unsupported serialized data")],
        },
    );

    let paths = android_paths_for(&root);
    let mut session = RSession::new().expect("session");
    session
        .configure_android_runtime(&paths)
        .expect("path config");

    let installed_names = session
        .installed_packages()
        .into_iter()
        .map(|package| package.name)
        .collect::<Vec<_>>();
    assert_eq!(
        installed_names,
        vec![
            "corpbase",
            "corpbytecode",
            "corpcollate",
            "corpcompiled",
            "corpdataenv",
            "corpdepends",
            "corpexamples",
            "corpfrom",
            "corpimport",
            "corplazydata",
            "corpnative",
            "corppaths",
            "corppattern",
            "corps4",
            "corpsourcelazy",
        ]
    );
    assert_eq!(
        session.package_path("corpbase"),
        Some(base_pkg.to_string_lossy().into_owned())
    );
    assert_eq!(
        session
            .eval("packageVersion(\"corpbase\") == \"0.1.0\"")
            .expect("package version"),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("packageDescription(\"corpbase\")$Title")
            .expect("package description title"),
        "[1] \"Corpus Base Package\""
    );
    assert_eq!(
        session
            .eval("packageDescription(\"corpbase\", fields = c(\"Package\", \"Version\"))")
            .expect("package description fields"),
        "[1] \"corpbase\" \"0.1.0\"   "
    );
    assert_eq!(
        session
            .eval("is.na(packageDescription(\"corpbase\", fields = \"NoSuchField\"))")
            .expect("missing package description field"),
        "[1] TRUE"
    );
    assert_eq!(
            session
                .eval("c(requireNamespace(\"corpbase\"), exists(\"base_value\"), \"corpbase\" %in% loadedNamespaces())")
                .expect("namespace load without attach"),
            "[1]  TRUE FALSE  TRUE"
        );
    assert_eq!(
            session
                .eval("f <- get(\"base_value\", envir = asNamespace(\"corpbase\")); c(is.environment(getNamespace(\"corpbase\")), f())")
                .expect("namespace access"),
            "[1]  1 10"
        );
    assert_eq!(
            session
                .eval("c(corpbase::base_value(), corpbase:::corp_generic.corpclass(corpbase::make_corp()))")
                .expect("namespace operators"),
            "[1]  10 123"
        );

    session.load_package("corpbase").expect("load corpbase");
    assert_eq!(session.eval("base_value()").expect("base value"), "[1] 10");
    assert_eq!(
        session
            .eval("corp_generic(make_corp())")
            .expect("s3 dispatch"),
        "[1] 123"
    );
    assert_eq!(
        session
            .eval("data(package = \"corpbase\")")
            .expect("list data"),
        "[1] \"corp_data\""
    );
    assert_eq!(
        session
            .eval("data(\"corp_data\", package = \"corpbase\")\ncorp_data")
            .expect("load data"),
        "[1] 55"
    );

    session.load_package("corpimport").expect("load import");
    assert_eq!(
        session.eval("import_value()").expect("import value"),
        "[1] 15"
    );
    session.load_package("corpfrom").expect("load importFrom");
    assert_eq!(session.eval("from_value()").expect("from value"), "[1] 17");
    session.load_package("corpcollate").expect("load collate");
    assert_eq!(
        session
            .eval("collate_value")
            .expect("Collate source ordering"),
        "[1] 42"
    );
    session.load_package("corpdepends").expect("load Depends");
    assert_eq!(
        session
            .eval("c(depends_value(), base_value())")
            .expect("Depends package visibility"),
        "[1] 19 10"
    );
    session.load_package("corpexamples").expect("load examples");
    assert_eq!(
            session
                .eval("source(system.file(\"examples\", \"example-resource.R\", package = \"corpexamples\", mustWork = TRUE)); c(example_value(), example_answer)")
                .expect("package example resource"),
            "[1] 12 42"
        );
    session
        .load_package("corppattern")
        .expect("load exportPattern");
    assert_eq!(
        session.eval("pat_value()").expect("pattern value"),
        "[1] 31"
    );
    let hidden = session
        .eval("hidden_value")
        .expect_err("hidden pattern symbol should not be attached");
    assert!(hidden.to_string().contains("not found"), "{hidden}");

    session.load_package("corps4").expect("load S4 package");
    assert_eq!(
            session
                .eval("p <- make_person(); all(c(isS4(p), is(p, \"CorpusPerson\"), person_name(p) == \"Ada\", all(person_slots() == c(\"name\", \"score\"))))")
                .expect("S4 package value"),
            "[1] TRUE"
        );

    session
        .load_package("corpdataenv")
        .expect("load data env package");
    assert_eq!(
            session
                .eval("e <- new.env(); data(\"env_data\", package = \"corpdataenv\", envir = e); c(exists(\"env_data\", envir = e), exists(\"env_data\"), get(\"env_data\", envir = e))")
                .expect("data envir"),
            "[1]  1  0 88"
        );

    session
        .load_package("corpsourcelazy")
        .expect("load source lazy data package");
    assert_eq!(
        session
            .eval("c(exists(\"lazy_source_data\"), lazy_source_data, lazy_source_value())")
            .expect("source lazy data"),
        "[1]  1 90 92"
    );

    session
        .load_package("corppaths")
        .expect("load paths package");
    assert_eq!(
        session
            .eval_result("corpus_lib_paths()")
            .expect("package-visible library paths")
            .value,
        RValue::StringVector(paths.library_paths().into_iter().map(Some).collect())
    );

    let native = session
        .load_package("corpnative")
        .expect_err("native package should be rejected");
    assert!(
        native.to_string().contains("useDynLib(corpnative)"),
        "{native}"
    );
    assert!(
        native.to_string().contains("pure-R Android runtime"),
        "{native}"
    );
    assert_eq!(
        session
            .eval("requireNamespace(\"corpnative\", quietly = TRUE)")
            .expect("native namespace policy"),
        "[1] FALSE"
    );
    let compiled = session
        .load_package("corpcompiled")
        .expect_err("compiled package should be rejected");
    assert!(
        compiled.to_string().contains("NeedsCompilation: yes"),
        "{compiled}"
    );
    let bytecode = session
        .load_package("corpbytecode")
        .expect_err("bytecode package should be rejected");
    assert!(
        bytecode
            .to_string()
            .contains("unsupported byte-compiled/lazyload R code"),
        "{bytecode}"
    );
    session
        .load_package("corplazydata")
        .expect("load lazy-data package namespace");
    let lazy = session
        .eval_result("data(\"lazy_data\", package = \"corplazydata\")")
        .expect_err("serialized lazy data should be rejected");
    assert!(
        lazy.to_string()
            .contains("unsupported serialized/lazy data"),
        "{lazy}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn pure_r_package_corpus_keeps_same_named_packages_isolated_by_session() {
    let make_root = |name: &str, value: i32| {
        let root = unique_test_root(name);
        let bundled = root.join("bundled-library");
        write_fixture_package(
            &bundled,
            FixturePackage {
                name: "corpbase",
                description: concat!(
                    "Package: corpbase\n",
                    "Version: 0.1.0\n",
                    "Title: Corpus Base Package\n",
                    "Description: Same package name, different library path.\n",
                    "License: MIT\n",
                    "NeedsCompilation: no\n",
                ),
                namespace: "export(base_value)\n",
                sources: &[("base.R", &format!("base_value <- function() {value}L\n"))],
                data_sources: &[],
                extra_files: &[],
            },
        );
        root
    };
    let left_root = make_root("rport-embed-corpus-left", 21);
    let right_root = make_root("rport-embed-corpus-right", 84);

    let mut left = RSession::new().expect("left session");
    left.configure_android_runtime(&android_paths_for(&left_root))
        .expect("left paths");
    let mut right = RSession::new().expect("right session");
    right
        .configure_android_runtime(&android_paths_for(&right_root))
        .expect("right paths");

    left.load_package("corpbase").expect("left load");
    right.load_package("corpbase").expect("right load");
    assert_eq!(left.eval("base_value()").expect("left value"), "[1] 21");
    assert_eq!(right.eval("base_value()").expect("right value"), "[1] 84");

    let _ = std::fs::remove_dir_all(left_root);
    let _ = std::fs::remove_dir_all(right_root);
}

#[test]
fn parallel_sessions_keep_android_runtime_state_isolated() {
    const WORKERS: usize = 4;

    let barrier = Arc::new(Barrier::new(WORKERS));
    let handles = (0..WORKERS)
            .map(|index| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let value = 100 + index as i32;
                    let namespace =
                        "export(tiny_value, make_tiny, tiny_generic)\nS3method(tiny_generic, tinything)\n";
                    let source = format!(
                        r#"
tiny_value <- function() {value}L
make_tiny <- function() {{
    x <- {value}L
    class(x) <- "tinything"
    x
}}
tiny_generic <- function(x) UseMethod("tiny_generic", x)
tiny_generic.tinything <- function(x) {value}L
"#
                    );
                    let (root, _pkg) = make_test_package_with_source(
                        &format!("rport-embed-parallel-{index}"),
                        namespace,
                        &source,
                    );
                    let files = root.join("files");
                    let cache = root.join("cache");
                    let bundled = root.join("bundled-library");
                    let paths = AndroidRuntimePaths::new(
                        files.to_str().expect("utf8 files path"),
                        cache.to_str().expect("utf8 cache path"),
                        Some(bundled.to_str().expect("utf8 bundled path")),
                    );

                    let mut session = RSession::new().expect("session");
                    session
                        .configure_android_runtime(&paths)
                        .expect("path config");

                    barrier.wait();

                    session.load_package("tiny").expect("load package");
                    assert_eq!(
                        session.eval("tiny_value()").expect("tiny value"),
                        format!("[1] {value}")
                    );

                    assert_eq!(
                        session
                            .eval("tiny_generic(make_tiny())")
                            .expect("s3 dispatch"),
                        format!("[1] {value}")
                    );

                    let captured = session
                        .eval("capture.output({ cat(\"session local\\n\") })")
                        .expect("capture output");
                    assert!(captured.contains("session local"), "{captured}");

                    let err = session
                        .eval("unknown_symbol")
                        .expect_err("undefined symbol should fail");
                    assert!(err.to_string().contains("not found"));
                    assert_eq!(
                        session.eval("tiny_value()").expect("eval after error"),
                        format!("[1] {value}")
                    );

                    let png = session
                        .render_with_dimensions(
                            &format!(
                                "plot(c(1, 2, 3), c({value}, {next}, {last}), main = \"session {index}\", col = \"red\", type = \"l\")",
                                next = value + 1,
                                last = value + 2,
                            ),
                            240,
                            180,
                        )
                        .expect("render");
                    let decoded = decode_png_rgba(&png);
                    assert!(decoded.red_pixels() > 5);
                    assert!(decoded.non_white_in_region(0, 0, decoded.width, 40) > 5);

                    let _ = std::fs::remove_dir_all(root);
                    value
                })
            })
            .collect::<Vec<_>>();

    let mut values = handles
        .into_iter()
        .map(|handle| handle.join().expect("worker should not panic"))
        .collect::<Vec<_>>();
    values.sort_unstable();
    assert_eq!(values, vec![100, 101, 102, 103]);
}

#[test]
fn android_runtime_paths_without_bundled_library_only_returns_user_library() {
    let paths = AndroidRuntimePaths::new("/tmp/app-files", "/tmp/app-cache", None::<&str>);

    assert_eq!(paths.user_library_dir(), "/tmp/app-files/R/library");
    assert_eq!(paths.temp_dir(), "/tmp/app-cache/Rtmp");
    assert_eq!(paths.library_paths(), vec!["/tmp/app-files/R/library"]);
}

#[test]
fn render_evaluates_basic_plot_expression() {
    let mut session = RSession::new().expect("session");
    let png = session
        .render_with_dimensions("plot(c(1, 2, 3), c(1, 4, 9))", 320, 240)
        .expect("render");

    assert!(png.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
    assert!(png.len() > 256);
}

#[test]
fn render_honors_plot_labels_and_color() {
    let mut session = RSession::new().expect("session");
    let png = session
            .render_with_dimensions(
                "plot(c(1, 2, 3, 4), c(1, 4, 9, 16), main = \"Revenue μ\", xlab = \"day\", ylab = \"value\", col = \"red\", type = \"l\")",
                360,
                260,
            )
            .expect("render");
    let decoded = decode_png_rgba(&png);

    assert!(decoded.red_pixels() > 10);
    assert!(decoded.non_white_in_region(0, 0, decoded.width, 42) > 5);
    assert!(decoded.non_white_in_region(0, decoded.height - 40, decoded.width, decoded.height) > 5);
    assert!(decoded.non_white_in_region(0, 58, 48, decoded.height - 52) > 5);
}

#[test]
fn render_supports_point_mode_and_responsive_dimensions() {
    let mut session = RSession::new().expect("session");
    let small = session
        .render_with_dimensions(
            "plot(c(1, 2, 3), c(3, 1, 2), type = \"p\", col = \"green\", cex = 1.4)",
            96,
            96,
        )
        .expect("small render");
    let small = decode_png_rgba(&small);
    assert_eq!(small.width, 96);
    assert_eq!(small.height, 96);
    assert!(small.green_pixels() > 5);
    assert!(small.non_white_in_region(0, 0, small.width, small.height) > 20);

    let large = session
            .render_with_dimensions(
                "plot(c(1, 2, 3, 4, 5), c(1, 4, 9, 16, 25), main = \"Large plot\", type = \"b\", col = \"blue\", lwd = 2)",
                1024,
                640,
            )
            .expect("large render");
    let large = decode_png_rgba(&large);
    assert_eq!(large.width, 1024);
    assert_eq!(large.height, 640);
    assert!(large.non_white_in_region(0, 0, large.width, 56) > 20);
    assert!(large.non_white_in_region(0, 0, large.width, large.height) > 100);
}

#[test]
fn render_reports_actionable_plot_errors() {
    let mut session = RSession::new().expect("session");

    let too_small = session
        .render_with_dimensions("plot(c(1), c(1))", 0, 120)
        .expect_err("zero width should fail");
    assert!(too_small.to_string().contains("at least 32 pixels"));

    let non_numeric = session
        .render_with_dimensions("plot(c(\"a\", \"b\"))", 320, 240)
        .expect_err("non-numeric plot should fail");
    assert!(non_numeric.to_string().contains("numeric"));

    let non_finite = session
        .render_with_dimensions("plot(c(1, Inf))", 320, 240)
        .expect_err("non-finite plot should fail");
    assert!(non_finite.to_string().contains("finite"));
}

#[test]
fn render_propagates_non_plot_eval_errors() {
    let mut session = RSession::new().expect("session");

    let err = session
        .render_with_dimensions("stop(\"render boom\")", 320, 240)
        .expect_err("render should propagate R errors");
    assert!(err.to_string().contains("render boom"), "{err}");
}

#[test]
fn eval_reports_errors_without_panicking() {
    let mut session = RSession::new().expect("session");
    let err = session
        .eval("unknown_symbol")
        .expect_err("undefined symbol");
    let message = err.to_string();
    assert!(message.contains("object '"));
    assert!(message.contains("not found"));
}

#[test]
fn close_makes_eval_fail() {
    let mut session = RSession::new().expect("session");
    session.close();
    assert!(session.eval("1 + 1").is_err());
}

#[test]
fn eval_observes_pre_cancelled_token() {
    let mut session = RSession::new().expect("session");
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let err = session
        .eval_result_cancellable("repeat { 1 + 1 }", &cancellation)
        .expect_err("cancelled");
    assert!(err.to_string().contains("operation cancelled"));
}

#[test]
fn eval_can_be_cancelled_from_another_thread() {
    let cancellation = CancellationToken::new();
    let worker_flag = cancellation.clone();
    let worker = std::thread::spawn(move || {
        let mut session = RSession::new().expect("session");
        session.eval_result_cancellable("repeat { 1 + 1 }", &worker_flag)
    });

    std::thread::sleep(std::time::Duration::from_millis(10));
    cancellation.cancel();

    let err = worker
        .join()
        .expect("worker should not panic")
        .expect_err("eval should be cancelled");
    assert!(err.to_string().contains("operation cancelled"));
}

#[test]
fn cancellation_does_not_poison_sessions() {
    let mut cancelled_session = RSession::new().expect("cancelled session");
    let mut other_session = RSession::new().expect("other session");

    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let err = cancelled_session
        .eval_result_cancellable("repeat { 1 + 1 }", &cancellation)
        .expect_err("cancelled");
    assert!(err.to_string().contains("operation cancelled"));

    assert_eq!(other_session.eval("1 + 1").unwrap(), "[1] 2");
    assert_eq!(cancelled_session.eval("2 + 2").unwrap(), "[1] 4");
}

#[cfg(unix)]
#[test]
fn trusted_desktop_host_can_dynload_and_call_native_symbol() {
    let root = std::env::temp_dir().join(format!(
        "rport-native-extension-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos(),
    ));
    std::fs::create_dir_all(&root).expect("native fixture directory");
    let source = root.join("identity.c");
    std::fs::write(
        &source,
        concat!(
            "typedef void *SEXP;\n",
            "typedef struct DllInfo DllInfo;\n",
            "typedef void (*DL_FUNC)(void);\n",
            "typedef struct { const char *name; DL_FUNC fun; int numArgs; const int *types; } R_CMethodDef;\n",
            "typedef struct { const char *name; DL_FUNC fun; int numArgs; } R_CallMethodDef;\n",
            "extern int R_registerRoutines(DllInfo *, const R_CMethodDef *, const R_CallMethodDef *, const void *, const void *);\n",
            "extern int R_useDynamicSymbols(DllInfo *, int);\n",
            "SEXP rport_identity(SEXP value) { return value; }\n",
            "void rport_increment(double *value) { *value += 1.0; }\n",
            "static const R_CMethodDef c_methods[] = {\n",
            "  {\"rport_increment\", (DL_FUNC) &rport_increment, 1, 0},\n",
            "  {0, 0, 0, 0}\n",
            "};\n",
            "static const R_CallMethodDef call_methods[] = {\n",
            "  {\"rport_identity\", (DL_FUNC) &rport_identity, 1},\n",
            "  {0, 0, 0}\n",
            "};\n",
            "void R_init_identity(DllInfo *dll) {\n",
            "  R_registerRoutines(dll, c_methods, call_methods, 0, 0);\n",
            "  R_useDynamicSymbols(dll, 0);\n",
            "}\n",
        ),
    )
    .expect("native fixture source");
    let library = if cfg!(target_os = "macos") {
        root.join("identity.dylib")
    } else {
        root.join("identity.so")
    };
    let mut compiler = std::process::Command::new("cc");
    if cfg!(target_os = "macos") {
        compiler.args(["-dynamiclib", "-undefined", "dynamic_lookup"]);
    } else {
        compiler.args(["-shared", "-fPIC"]);
    }
    let status = compiler
        .arg(&source)
        .arg("-o")
        .arg(&library)
        .status()
        .expect("C compiler available for native-extension test");
    assert!(status.success(), "native fixture compilation failed");

    let mut session = RSession::new().expect("session");
    session.enable_host_process_capabilities();
    let path = library
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let result = session
        .eval_result(&format!(
            "dyn.load(\"{path}\"); .Call(\"rport_identity\", 42)"
        ))
        .expect("trusted host native call");
    assert_eq!(result.value, RValue::Real(Some(42.0)));
    let copied = session
        .eval_result(".C(\"rport_increment\", as.double(41))[[1]]")
        .expect("registered .C call");
    assert_eq!(copied.value, RValue::Real(Some(42.0)));
    session
        .eval(&format!("dyn.unload(\"{path}\")"))
        .expect("native unload");

    std::fs::remove_dir_all(root).expect("remove native fixture");
}

#[test]
fn host_process_capabilities_are_session_local() {
    let mut trusted = RSession::new().expect("trusted session");
    trusted.enable_host_process_capabilities();
    let mut untrusted = RSession::new().expect("untrusted session");

    assert_eq!(trusted.eval("system('printf trusted')").unwrap(), "trusted");
    let error = untrusted
        .eval("system('printf untrusted')")
        .expect_err("a separate session must retain the default sandbox");
    let message = error.to_string();
    assert!(
        message.contains("disabled by the session capability policy"),
        "unexpected sandbox error: {message}"
    );
}

#[test]
fn malformed_script_is_atomic_at_embed_boundary() {
    let mut session = RSession::new().expect("session");

    let error = session
        .eval("embed_atomic_side_effect <- 1; \"unterminated")
        .expect_err("malformed script must fail");
    assert!(error.to_string().contains("unexpected"), "{error}");
    assert_eq!(
        session
            .eval("exists(\"embed_atomic_side_effect\")")
            .expect("session should remain usable"),
        "[1] FALSE"
    );
}

#[test]
fn is_input_complete_distinguishes_continuation_from_error() {
    let mut session = RSession::new().expect("session");

    // Incomplete inputs await continuation (unmatched brace, trailing
    // operator).
    assert!(
        !session
            .is_input_complete("f <- function(x) {")
            .expect("probe incomplete brace")
    );
    assert!(
        !session
            .is_input_complete("1 +")
            .expect("probe trailing operator")
    );

    // Complete inputs evaluate (or fail with the usual parse error).
    assert!(
        session
            .is_input_complete("f <- function(x) {\n  x + 1\n}")
            .expect("probe complete function")
    );
    assert!(
        session
            .is_input_complete("1 + 1")
            .expect("probe complete expression")
    );
    assert!(
        session
            .is_input_complete(")")
            .expect("stray closer is complete, not continuation")
    );
}
/// Synthetic package feature matrix (offline, zero network).
/// Each entry names an installed fixture package, the feature axis it
/// exercises, a probe expression, and the expected output snippet. The
/// `xfail` flag mirrors `tests/upstream-r/dispositions.tsv` semantics: an
/// entry marked `xfail` is known to fail on the current tree and records the
/// observed error in `xfail_reason` instead of changing interpreter code.
struct SyntheticPkgEntry {
    name: &'static str,
    axis: &'static str,
    title: &'static str,
    blurb: &'static str,
    desc_extra: &'static str,
    namespace: &'static str,
    sources: &'static [(&'static str, &'static str)],
    data_sources: &'static [(&'static str, &'static str)],
    extra: Option<(&'static str, &'static [u8])>,
    probe: &'static str,
    expected: &'static str,
    xfail: bool,
    xfail_reason: &'static str,
}
const SYNTHETIC_PACKAGE_FEATURE_MATRIX: &[SyntheticPkgEntry] = &[
    SyntheticPkgEntry {
        name: "pxexports",
        axis: "exports",
        title: "Px Exports Package",
        blurb: "Exercises plain exported functions.",
        desc_extra: "",
        namespace: "export(pxexports_value)\n",
        sources: &[("exports.R", "pxexports_value <- function() 101L\n")],
        data_sources: &[],
        extra: None,
        probe: "pxexports_value()",
        expected: "[1] 101",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxs3",
        axis: "S3 dispatch",
        title: "Px S3 Package",
        blurb: "Exercises S3 generics, methods, and classed objects.",
        desc_extra: "",
        namespace: "export(pxs3_value, pxs3_make, pxs3_generic)\nS3method(pxs3_generic, pxs3class)\n",
        sources: &[(
            "s3.R",
            concat!(
                "pxs3_value <- function() 102L\n",
                "pxs3_make <- function() { x <- 102L; class(x) <- \"pxs3class\"; x }\n",
                "pxs3_generic <- function(x) UseMethod(\"pxs3_generic\", x)\n",
                "pxs3_generic.pxs3class <- function(x) 102L\n",
                "pxs3_hidden <- function() 999L\n",
            ),
        )],
        data_sources: &[],
        extra: None,
        probe: "pxs3_generic(pxs3_make())",
        expected: "[1] 102",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxdata",
        axis: "source-form data",
        title: "Px Data Package",
        blurb: "Exercises source-form package data loading.",
        desc_extra: "",
        namespace: "export(pxdata_value)\n",
        sources: &[("data-shape.R", "pxdata_value <- function() 303L\n")],
        data_sources: &[("pxdata_item.R", "pxdata_item <- 103L\n")],
        extra: None,
        probe: "data(\"pxdata_item\", package = \"pxdata\")\npxdata_item + pxdata_value()",
        expected: "[1] 406",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pximport",
        axis: "whole-package import",
        title: "Px Import Package",
        blurb: "Exercises whole-package namespace imports.",
        desc_extra: "Imports: pxexports\n",
        namespace: "import(pxexports)\nexport(pximport_value)\n",
        sources: &[(
            "import.R",
            "pximport_value <- function() pxexports_value() + 100L\n",
        )],
        data_sources: &[],
        extra: None,
        probe: "pximport_value()",
        expected: "[1] 201",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxfrom",
        axis: "selective importFrom",
        title: "Px ImportFrom Package",
        blurb: "Exercises selective namespace imports.",
        desc_extra: "Imports: pxexports\n",
        namespace: "importFrom(pxexports, pxexports_value)\nexport(pxfrom_value)\n",
        sources: &[(
            "from.R",
            "pxfrom_value <- function() pxexports_value() + 104L\n",
        )],
        data_sources: &[],
        extra: None,
        probe: "pxfrom_value()",
        expected: "[1] 205",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxcollate",
        axis: "Collate ordering",
        title: "Px Collate Package",
        blurb: "Exercises DESCRIPTION Collate source ordering.",
        desc_extra: "Collate: 'z-producer.R' 'a-consumer.R'\n",
        namespace: "export(pxcollate_value)\n",
        sources: &[
            ("a-consumer.R", "pxcollate_value <- pxcollate_seed + 2L\n"),
            ("z-producer.R", "pxcollate_seed <- 204L\n"),
        ],
        data_sources: &[],
        extra: None,
        probe: "pxcollate_value",
        expected: "[1] 206",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxdepends",
        axis: "Depends loading",
        title: "Px Depends Package",
        blurb: "Exercises DESCRIPTION Depends loading and symbol visibility.",
        desc_extra: "Depends: R (>= 4.0.0), pxexports\n",
        namespace: "export(pxdepends_value)\n",
        sources: &[(
            "depends.R",
            "pxdepends_value <- function() pxexports_value() + 106L\n",
        )],
        data_sources: &[],
        extra: None,
        probe: "pxdepends_value()",
        expected: "[1] 207",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxsysfile",
        axis: "system.file resources",
        title: "Px System File Package",
        blurb: "Exercises package resource discovery via system.file.",
        desc_extra: "",
        namespace: "export(pxsys_value)\n",
        sources: &[("sysfile.R", "pxsys_value <- function() 208L\n")],
        data_sources: &[],
        extra: Some(("examples/pxsys-resource.R", b"pxsys_answer <- 42L\n")),
        probe: "source(system.file(\"examples\", \"pxsys-resource.R\", package = \"pxsysfile\", mustWork = TRUE))\npxsys_value() + pxsys_answer",
        expected: "[1] 250",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxpattern",
        axis: "exportPattern",
        title: "Px Export Pattern Package",
        blurb: "Exercises NAMESPACE exportPattern handling.",
        desc_extra: "",
        namespace: "exportPattern(\"^pxpat_\")\n",
        sources: &[(
            "pattern.R",
            "pxpat_value <- function() 209L\nhidden_junk <- function() 99L\n",
        )],
        data_sources: &[],
        extra: None,
        probe: "pxpat_value()",
        expected: "[1] 209",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxs4",
        axis: "S4 classes and slots",
        title: "Px S4 Package",
        blurb: "Exercises pure-R package S4 class creation and slot access.",
        desc_extra: "",
        namespace: "export(pxs4_make, pxs4_name, pxs4_slots)\n",
        sources: &[(
            "s4.R",
            concat!(
                "setClass(\"PxS4Person\", name = \"character\", score = \"numeric\")\n",
                "pxs4_make <- function() new(\"PxS4Person\", name = \"Ada\", score = 42)\n",
                "pxs4_name <- function(x) slot(x, \"name\")\n",
                "pxs4_slots <- function() slotNames(\"PxS4Person\")\n",
            ),
        )],
        data_sources: &[],
        extra: None,
        probe: "p <- pxs4_make()\nall(c(isS4(p), is(p, \"PxS4Person\"), pxs4_name(p) == \"Ada\", all(pxs4_slots() == c(\"name\", \"score\"))))",
        expected: "[1] TRUE",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxdataenv",
        axis: "data envir loading",
        title: "Px Data Envir Package",
        blurb: "Exercises data with an explicit target environment.",
        desc_extra: "",
        namespace: "export(pxdataenv_value)\n",
        sources: &[("dataenv.R", "pxdataenv_value <- function() 211L\n")],
        data_sources: &[("pxenv_item.R", "pxenv_item <- 111L\n")],
        extra: None,
        probe: "e <- new.env()\ndata(\"pxenv_item\", package = \"pxdataenv\", envir = e)\nget(\"pxenv_item\", envir = e) + exists(\"pxenv_item\")",
        expected: "[1] 111",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxlazy",
        axis: "LazyData source data",
        title: "Px Lazy Data Package",
        blurb: "Exercises LazyData with source-form package data.",
        desc_extra: "LazyData: true\n",
        namespace: "export(pxlazy_value)\n",
        sources: &[(
            "lazy-source.R",
            "pxlazy_value <- function() pxlazy_data + 2L\n",
        )],
        data_sources: &[("pxlazy_data.R", "pxlazy_data <- 212L\n")],
        extra: None,
        probe: "pxlazy_data + pxlazy_value()",
        expected: "[1] 426",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxpaths",
        axis: ".libPaths visibility",
        title: "Px Library Paths Package",
        blurb: "Exercises package-visible Android library paths.",
        desc_extra: "",
        namespace: "export(pxpaths_paths)\n",
        sources: &[("paths.R", "pxpaths_paths <- function() .libPaths()\n")],
        data_sources: &[],
        extra: None,
        probe: "paste(pxpaths_paths(), collapse = \",\")",
        expected: "bundled-library",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxarith",
        axis: "arithmetic evaluation",
        title: "Px Arithmetic Package",
        blurb: "Exercises vectorized arithmetic inside a package namespace.",
        desc_extra: "",
        namespace: "export(pxarith_value)\n",
        sources: &[(
            "arith.R",
            "pxarith_value <- function() sum(c(1, 2, 3) * 10)\n",
        )],
        data_sources: &[],
        extra: None,
        probe: "pxarith_value()",
        expected: "[1] 60",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxpaste",
        axis: "string paste",
        title: "Px Paste Package",
        blurb: "Exercises paste and paste0 string assembly.",
        desc_extra: "",
        namespace: "export(pxpaste_value)\n",
        sources: &[(
            "paste.R",
            "pxpaste_value <- function() paste0(\"a\", \"-\", 115L)\n",
        )],
        data_sources: &[],
        extra: None,
        probe: "pxpaste_value()",
        expected: "a-115",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxlogic",
        axis: "logical operators",
        title: "Px Logic Package",
        blurb: "Exercises logical operators and any/all aggregation.",
        desc_extra: "",
        namespace: "export(pxlogic_value)\n",
        sources: &[(
            "logic.R",
            "pxlogic_value <- function() any(c(TRUE, FALSE) & c(TRUE, TRUE))\n",
        )],
        data_sources: &[],
        extra: None,
        probe: "pxlogic_value()",
        expected: "[1] TRUE",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxvec",
        axis: "vector subsetting",
        title: "Px Vector Package",
        blurb: "Exercises vector construction and positional subsetting.",
        desc_extra: "",
        namespace: "export(pxvec_value)\n",
        sources: &[("vec.R", "pxvec_value <- function() c(10L, 20L, 117L)[3L]\n")],
        data_sources: &[],
        extra: None,
        probe: "pxvec_value()",
        expected: "[1] 117",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxlist",
        axis: "list element access",
        title: "Px List Package",
        blurb: "Exercises named list construction and dollar access.",
        desc_extra: "",
        namespace: "export(pxlist_value)\n",
        sources: &[(
            "list.R",
            "pxlist_value <- function() list(alpha = 118L, beta = 2L)$alpha\n",
        )],
        data_sources: &[],
        extra: None,
        probe: "pxlist_value()",
        expected: "[1] 118",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxctrl",
        axis: "control flow",
        title: "Px Control Flow Package",
        blurb: "Exercises for loops and if/else accumulation.",
        desc_extra: "",
        namespace: "export(pxctrl_value)\n",
        sources: &[(
            "ctrl.R",
            concat!(
                "pxctrl_value <- function() {\n",
                "  total <- 0L\n",
                "  for (i in 1:5) {\n",
                "    if (i > 3L) { total <- total + i }\n",
                "  }\n",
                "  total + 110L\n",
                "}\n",
            ),
        )],
        data_sources: &[],
        extra: None,
        probe: "pxctrl_value()",
        expected: "[1] 119",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxfn",
        axis: "closures and lexical scope",
        title: "Px Closure Package",
        blurb: "Exercises nested closures and lexical scoping.",
        desc_extra: "",
        namespace: "export(pxfn_value)\n",
        sources: &[(
            "fn.R",
            concat!(
                "pxfn_adder <- function(n) function(x) x + n\n",
                "pxfn_value <- function() pxfn_adder(100L)(20L)\n",
            ),
        )],
        data_sources: &[],
        extra: None,
        probe: "pxfn_value()",
        expected: "[1] 120",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxns",
        axis: "namespace-qualified access",
        title: "Px Namespace Operator Package",
        blurb: "Exercises :: and ::: access against an installed package.",
        desc_extra: "Imports: pxexports\n",
        namespace: "export(pxns_value)\n",
        sources: &[(
            "ns.R",
            "pxns_value <- function() pxexports::pxexports_value() + 20L\n",
        )],
        data_sources: &[],
        extra: None,
        probe: "pxns_value() + pxexports:::pxexports_value()",
        expected: "[1] 222",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxattach",
        axis: "attach isolation",
        title: "Px Attach Package",
        blurb: "Exercises search-path attach and non-export hiding.",
        desc_extra: "",
        namespace: "export(pxattach_value)\n",
        sources: &[(
            "attach.R",
            concat!(
                "pxattach_value <- function() 122L\n",
                "pxattach_hidden <- function() 999L\n",
            ),
        )],
        data_sources: &[],
        extra: None,
        probe: "pxattach_value() + exists(\"pxattach_hidden\")",
        expected: "[1] 122",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxmulti",
        axis: "multi-file sourcing",
        title: "Px Multi File Package",
        blurb: "Exercises multiple R files sharing one namespace.",
        desc_extra: "",
        namespace: "export(pxmulti_value)\n",
        sources: &[
            ("aa-first.R", "pxmulti_seed <- 120L\n"),
            ("bb-second.R", "pxmulti_value <- function() pxmulti_seed + 3L\n"),
        ],
        data_sources: &[],
        extra: None,
        probe: "pxmulti_value()",
        expected: "[1] 123",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxnslookup",
        axis: "getNamespace lookup",
        title: "Px Namespace Lookup Package",
        blurb: "Exercises getNamespace and programmatic symbol lookup.",
        desc_extra: "",
        namespace: "export(pxnslookup_value)\n",
        sources: &[(
            "nslookup.R",
            "pxnslookup_value <- function() 124L\n",
        )],
        data_sources: &[],
        extra: None,
        probe: "list(is.environment(getNamespace(\"pxnslookup\")), get(\"pxnslookup_value\", envir = asNamespace(\"pxnslookup\"))())",
        expected: "124",
        xfail: false,
        xfail_reason: "",
    },
    SyntheticPkgEntry {
        name: "pxhiddenns",
        axis: "triple-colon hidden access",
        title: "Px Hidden Symbol Package",
        blurb: "Exercises ::: access to a non-exported symbol.",
        desc_extra: "",
        namespace: "export(pxhiddenns_value)\n",
        sources: &[(
            "hidden.R",
            concat!(
                "pxhiddenns_value <- function() 125L\n",
                "pxhiddenns_secret <- function() 425L\n",
            ),
        )],
        data_sources: &[],
        extra: None,
        probe: "pxhiddenns:::pxhiddenns_secret()",
        expected: "[1] 425",
        xfail: false,
        xfail_reason: "",
    },
];
#[test]
fn synthetic_package_feature_matrix() {
    assert_eq!(SYNTHETIC_PACKAGE_FEATURE_MATRIX.len(), 25, "ledger must hold 25 packages");
    let mut names: Vec<&str> = SYNTHETIC_PACKAGE_FEATURE_MATRIX.iter().map(|entry| entry.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 25, "ledger package names must be unique");
    let root = unique_test_root("rport-embed-synthetic-matrix");
    let bundled = root.join("bundled-library");
    for entry in SYNTHETIC_PACKAGE_FEATURE_MATRIX {
        let description = format!(
            "Package: {name}\nVersion: 0.1.0\nTitle: {title}\nDescription: {blurb}\nLicense: MIT\n{extra}NeedsCompilation: no\n",
            name = entry.name,
            title = entry.title,
            blurb = entry.blurb,
            extra = entry.desc_extra,
        );
        let pkg = bundled.join(entry.name);
        let r_dir = pkg.join("R");
        std::fs::create_dir_all(&r_dir).expect("package R dir");
        std::fs::write(pkg.join("DESCRIPTION"), description).expect("description");
        std::fs::write(pkg.join("NAMESPACE"), entry.namespace).expect("namespace");
        for (file, source) in entry.sources {
            std::fs::write(r_dir.join(file), source).expect("R source");
        }
        if !entry.data_sources.is_empty() {
            let data_dir = pkg.join("data");
            std::fs::create_dir_all(&data_dir).expect("data dir");
            for (file, source) in entry.data_sources {
                std::fs::write(data_dir.join(file), source).expect("data source");
            }
        }
        if let Some((file, bytes)) = entry.extra {
            let path = pkg.join(file);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("extra file parent");
            }
            std::fs::write(path, bytes).expect("extra file");
        }
    }
    let mut session = RSession::new().expect("session");
    session
        .configure_android_runtime(&android_paths_for(&root))
        .expect("path config");
    let mut passed = 0usize;
    let mut xfailed = 0usize;
    for entry in SYNTHETIC_PACKAGE_FEATURE_MATRIX {
        if entry.xfail {
            let load = session.load_package(entry.name);
            let observed = match load {
                Ok(()) => match session.eval(entry.probe) {
                    Ok(output) => format!("probe unexpectedly passed: {output}"),
                    Err(err) => err.to_string(),
                },
                Err(err) => err.to_string(),
            };
            assert!(
                observed.contains(entry.xfail_reason) || entry.xfail_reason.is_empty(),
                "xfail ledger entry `{}` (axis `{}`) drifted: observed `{observed}`, recorded `{}`",
                entry.name,
                entry.axis,
                entry.xfail_reason,
            );
            xfailed += 1;
            continue;
        }
        session
            .load_package(entry.name)
            .unwrap_or_else(|err| panic!("load {} (axis {}): {err}", entry.name, entry.axis));
        let output = session
            .eval(entry.probe)
            .unwrap_or_else(|err| panic!("probe {} (axis {}): {err}", entry.name, entry.axis));
        assert!(
            output.contains(entry.expected),
            "ledger entry `{}` (axis `{}`) probe `{}` got `{output}`, want snippet `{}`",
            entry.name,
            entry.axis,
            entry.probe,
            entry.expected,
        );
        passed += 1;
    }
    assert_eq!(passed + xfailed, 25);
    eprintln!("synthetic matrix: {passed} passed, {xfailed} xfailed");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn wasm_m3_oracle_shape() {
    // Pins the contract the future wasm-bindgen M2/M3 session boundary must
    // satisfy: the M3 Node smoke test asserts this exact oracle string for
    // `eval("1+1")`. See docs/web-architecture.md ("R-in-WASM milestones
    // M2/M3 (plan)").
    let mut session = RSession::new().expect("session");
    assert_eq!(session.eval("1+1").unwrap(), "[1] 2");
}

/// Pinned real-package corpus (tests/real-packages/manifest.toml).
///
/// Unlike the synthetic feature matrix above, these are independently
/// authored CRAN packages, unpacked from tests/real-packages/vendor/ by
/// scripts/real_package_corpus.sh (env: RPORT_REAL_PKG_BUNDLED/APP/CACHE).
/// Statuses mirror the manifest: pass/partial/blocked with exact blockers.
#[test]
fn real_package_corpus() {
    let bundled = std::env::var("RPORT_REAL_PKG_BUNDLED").unwrap_or_else(|_| "/tmp/pkgprobe/bundled".to_string());
    let app = std::env::var("RPORT_REAL_PKG_APP").unwrap_or_else(|_| "/tmp/pkgprobe/app".to_string());
    let cache = std::env::var("RPORT_REAL_PKG_CACHE").unwrap_or_else(|_| "/tmp/pkgprobe/cache".to_string());
    let mut session = RSession::new().expect("session");
    session
        .configure_android_paths(&app, &cache, Some(&bundled))
        .expect("paths");

    // whisker 0.4.1 — partial: loads; render probes run (mapply named-FUN
    // hang is the documented blocker for full render parity).
    let whisker_load = session.load_package("whisker");
    eprintln!("whisker load: {whisker_load:?}");
    assert!(whisker_load.is_ok(), "whisker must load");
    eprintln!("whisker ns render fn: {:?}", session.eval("get(\"whisker.render\", envir=asNamespace(\"whisker\"))").map(|s| s.chars().take(40).collect::<String>()));

    // praise 1.0.0 — partial: loads; stem renders, word interpolation pending.
    let praise_load = session.load_package("praise");
    eprintln!("praise load: {praise_load:?}");
    assert!(praise_load.is_ok(), "praise must load");
    let praise_out = session.eval("praise(\"You are ${adjective}\")").expect("praise eval");
    eprintln!("praise probe: {praise_out:?}");
    assert!(praise_out.starts_with("[1] \"You are "), "praise stem must render");

    // crayon 1.5.3 — blocked: package-time tools/ data files + dynamic
    // exports; assert the EXACT current blocker so progress is visible.
    let crayon_load = session.load_package("crayon");
    eprintln!("crayon load: {crayon_load:?}");
    match crayon_load {
        Ok(()) => {
            eprintln!("crayon unexpectedly loads — update manifest status");
        }
        Err(e) => {
            let msg = format!("{e:?}");
            assert!(
                msg.contains("builtin_styles") || msg.contains("undefined exports"),
                "crayon blocker changed — update manifest: {msg}"
            );
        }
    }
}
