//! Behavior tests for the exported session surface (ported from the original
//! single-file test suite).

use std::sync::Arc;
use std::time::Duration;

use super::support::{CallbackEvent, RecordingCallback, make_test_package, wait_for_callback};
use crate::uniffi::conversion::{
    PackageInfo, RComplexValue, RValue, RValueKind, ResourceLimits, RuntimeInfo,
    android_runtime_paths,
};
use crate::uniffi::error::RError;
use crate::uniffi::session::RSession;

#[test]
fn cancel_without_active_eval_does_not_poison_next_eval() {
    let session = RSession::new().expect("session");

    session.cancel_current_operation();

    assert_eq!(session.eval("1 + 1".to_string()).unwrap(), "[1] 2");
}

#[test]
fn data_frame_page_slices_before_crossing_the_public_boundary() {
    let session = RSession::new().expect("session");
    session
        .eval("paged <- data.frame(id = 1:1000, label = rep(c(\"a\", \"b\"), 500))".into())
        .expect("create source table");

    let page = session
        .data_frame_page("paged".into(), 400, 25)
        .expect("fetch page");

    assert_eq!(page.total_rows, 1000);
    assert_eq!(page.offset, 400);
    assert_eq!(page.value.kind, RValueKind::List);
    assert_eq!(page.value.list_values.len(), 2);
    assert_eq!(page.value.list_values[0].integer_values.len(), 25);
    assert_eq!(page.value.list_values[0].integer_values[0], Some(401));
}

#[test]
fn data_frame_page_validates_bounds_and_table_shape() {
    let session = RSession::new().expect("session");
    assert!(matches!(
        session.data_frame_page("x".into(), 0, 0),
        Err(RError::InvalidInput(message)) if message.contains("page size")
    ));
    session.eval("x <- 1".into()).expect("create scalar");
    let scalar_page = session.data_frame_page("x".into(), 0, 10);
    assert!(
        matches!(
        &scalar_page,
        Err(RError::InvalidInput(message)) if message.contains("rectangular")
        ),
        "unexpected scalar page result: {scalar_page:?}",
    );
}

#[test]
fn shutdown_worker_closes_session() {
    let session = RSession::new().expect("session");

    assert!(session.is_active());
    session.shutdown_worker();
    assert!(!session.is_active());
    assert!(matches!(
        session.eval("1 + 1".to_string()),
        Err(RError::SessionClosed)
    ));
}

#[test]
fn validates_android_facing_inputs() {
    let session = RSession::new().expect("session");

    assert!(matches!(
        session.package_available("   ".to_string()),
        Err(RError::InvalidInput(message)) if message.contains("package")
    ));
    assert!(matches!(
        session.render("plot(c(1), c(1))".to_string(), 0, 120),
        Err(RError::InvalidInput(message)) if message.contains("width")
    ));
    assert!(matches!(
        session.render("plot(c(1), c(1))".to_string(), 31, 120),
        Err(RError::InvalidInput(message)) if message.contains("32 pixels")
    ));
}

#[test]
fn eval_result_returns_owned_value() {
    let session = RSession::new().expect("session");

    let result = session.eval_result("1:3".to_string()).expect("eval");

    assert_eq!(result.output, "[1] 1 2 3");
    assert_eq!(result.value.kind, RValueKind::IntegerVector);
    assert_eq!(result.value.integer_values, vec![Some(1), Some(2), Some(3)]);

    let strings = session
        .eval_result("c(\"a\", NA_character_)".to_string())
        .expect("eval strings");
    assert_eq!(strings.value.kind, RValueKind::StringVector);
    assert_eq!(
        strings.value.string_values,
        vec![Some("a".to_string()), None]
    );
}

#[test]
fn eval_result_preserves_value_metadata() {
    let value = RValue::from(r_embed::RValue::Attributed {
        value: Box::new(r_embed::RValue::IntegerVector(vec![Some(1), Some(2)])),
        metadata: r_embed::RMetadata {
            names: Some(vec![Some("a".to_string()), Some("b".to_string())]),
            class: Some(vec![Some("foo".to_string())]),
            ..r_embed::RMetadata::default()
        },
    });

    assert_eq!(value.kind, RValueKind::IntegerVector);
    assert_eq!(value.integer_values, vec![Some(1), Some(2)]);
    assert_eq!(
        value.metadata.names,
        Some(vec![Some("a".to_string()), Some("b".to_string())])
    );
    assert_eq!(value.metadata.class, Some(vec![Some("foo".to_string())]));
}

#[test]
fn eval_result_preserves_raw_and_complex_values() {
    let session = RSession::new().expect("session");

    let raw = session
        .eval_result("as.raw(c(65, 90))".to_string())
        .expect("eval");
    assert_eq!(raw.value.kind, RValueKind::RawVector);
    assert_eq!(raw.value.raw_values, vec![0x41, 0x5a]);

    let complex = RValue::from(r_embed::RValue::ComplexVector(vec![
        Some(r_embed::RComplexValue {
            real: 1.0,
            imaginary: -2.0,
        }),
        None,
    ]));
    assert_eq!(complex.kind, RValueKind::ComplexVector);
    assert_eq!(
        complex.complex_values,
        vec![
            Some(RComplexValue {
                real: 1.0,
                imaginary: -2.0,
            }),
            None,
        ]
    );
}

#[test]
fn unsupported_values_carry_type_name_only() {
    let value = RValue::from(r_embed::RValue::Unsupported {
        type_name: "closure".to_string(),
    });

    assert_eq!(value.kind, RValueKind::Unsupported);
    assert_eq!(value.type_name, "closure");
}

#[test]
fn configure_android_paths_runs_on_worker_session() {
    let root = std::env::temp_dir().join(format!(
        "rport-uniffi-paths-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    ));
    let files = root.join("files");
    let cache = root.join("cache");
    let bundled = root.join("bundled-library");
    let session = RSession::new().expect("session");
    let paths = android_runtime_paths(
        files.to_string_lossy().into_owned(),
        cache.to_string_lossy().into_owned(),
        Some(bundled.to_string_lossy().into_owned()),
    );

    assert_eq!(
        paths.user_library_dir,
        files
            .join("R")
            .join("library")
            .to_string_lossy()
            .into_owned()
    );
    assert_eq!(
        paths.temp_dir,
        cache.join("Rtmp").to_string_lossy().into_owned()
    );
    assert_eq!(
        paths.library_paths,
        vec![
            files
                .join("R")
                .join("library")
                .to_string_lossy()
                .into_owned(),
            bundled.to_string_lossy().into_owned(),
        ]
    );

    session
        .configure_android_runtime(paths)
        .expect("configure paths");

    let result = session
        .eval_result(".libPaths()".to_string())
        .expect("lib paths");
    assert_eq!(result.value.kind, RValueKind::StringVector);
    assert_eq!(
        result.value.string_values,
        vec![
            Some(
                files
                    .join("R")
                    .join("library")
                    .to_string_lossy()
                    .into_owned()
            ),
            Some(bundled.to_string_lossy().into_owned())
        ]
    );
    assert_eq!(
        session.runtime_info().expect("runtime info"),
        RuntimeInfo {
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
fn resource_limits_run_on_worker_session() {
    let session = RSession::new().expect("session");
    session
        .set_resource_limits(ResourceLimits {
            max_eval_depth: 1,
            max_execution_time_ms: 0,
            max_alloc_bytes: 0,
            max_arena_nodes: 0,
        })
        .expect("set limits");

    assert_eq!(session.resource_limits().expect("limits").max_eval_depth, 1);
    let err = session
        .eval_result("{ 1 + 1 }".to_string())
        .expect_err("nested eval should hit depth limit");
    assert!(err.to_string().contains("too deeply"));
}

#[test]
fn package_helpers_run_on_worker_session() {
    let (root, pkg) = make_test_package("rport-uniffi-package");
    let files = root.join("files");
    let cache = root.join("cache");
    let bundled = root.join("bundled-library");
    let session = RSession::new().expect("session");

    session
        .configure_android_runtime(android_runtime_paths(
            files.to_string_lossy().into_owned(),
            cache.to_string_lossy().into_owned(),
            Some(bundled.to_string_lossy().into_owned()),
        ))
        .expect("configure paths");

    assert!(
        session
            .package_available("tiny".to_string())
            .expect("available")
    );
    assert_eq!(
        session
            .package_path("tiny".to_string())
            .expect("package path"),
        Some(pkg.to_string_lossy().into_owned())
    );
    assert_eq!(
        session
            .package_info("tiny".to_string())
            .expect("package info"),
        Some(PackageInfo {
            name: "tiny".to_string(),
            version: "0.0.1".to_string(),
            title: "Tiny Test Package".to_string(),
            description: "Tiny package for Android runtime tests".to_string(),
            license: "MIT".to_string(),
            depends: "R (>= 4.0.0)".to_string(),
            imports: "base".to_string(),
            suggests: "testthat".to_string(),
            needs_compilation: false,
            path: pkg.to_string_lossy().into_owned(),
            library_path: bundled.to_string_lossy().into_owned(),
        })
    );
    assert_eq!(
        session
            .installed_packages()
            .expect("installed packages")
            .into_iter()
            .map(|package| package.name)
            .collect::<Vec<_>>(),
        vec!["tiny".to_string()]
    );
    assert!(
        !session
            .package_available("../tiny".to_string())
            .expect("invalid package unavailable")
    );
    session
        .load_package("tiny".to_string())
        .expect("load package");
    assert_eq!(
        session.eval("tiny_value()".to_string()).expect("eval"),
        "[1] 42"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn parallel_worker_sessions_keep_state_isolated() {
    const WORKERS: usize = 4;

    let barrier = Arc::new(std::sync::Barrier::new(WORKERS));
    let handles = (0..WORKERS)
        .map(|index| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let session = RSession::new().expect("session");
                let value = 200 + index as i32;

                barrier.wait();

                assert_eq!(
                    session
                        .eval(format!("x <- {value}L\nx"))
                        .expect("assign global"),
                    format!("[1] {value}")
                );
                assert_eq!(
                    session.eval("x".to_string()).expect("read global"),
                    format!("[1] {value}")
                );

                let plot = session
                    .render(
                        format!(
                            "plot(c(1, 2, 3), c({value}, {next}, {last}), main = \"worker {index}\", col = \"red\", type = \"l\")",
                            next = value + 1,
                            last = value + 2,
                        ),
                        220,
                        160,
                    )
                    .expect("render");
                assert_eq!(plot.width, 220);
                assert_eq!(plot.height, 160);
                assert!(plot.png_bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
                assert!(plot.png_bytes.len() > 256);

                value
            })
        })
        .collect::<Vec<_>>();

    let mut values = handles
        .into_iter()
        .map(|handle| handle.join().expect("worker should not panic"))
        .collect::<Vec<_>>();
    values.sort_unstable();
    assert_eq!(values, vec![200, 201, 202, 203]);
}

#[test]
fn android_runtime_paths_omits_missing_bundled_library() {
    let paths = android_runtime_paths(
        "/tmp/app-files".to_string(),
        "/tmp/app-cache".to_string(),
        None,
    );

    assert_eq!(paths.user_library_dir, "/tmp/app-files/R/library");
    assert_eq!(paths.temp_dir, "/tmp/app-cache/Rtmp");
    assert_eq!(paths.library_paths, vec!["/tmp/app-files/R/library"]);
}

#[test]
fn render_passes_plot_code_to_worker_session() {
    let session = RSession::new().expect("session");

    let plot = session
        .render("plot(c(1, 2, 3), c(3, 2, 5))".to_string(), 320, 240)
        .expect("render");

    assert_eq!(plot.width, 320);
    assert_eq!(plot.height, 240);
    assert!(plot.png_bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
    assert!(plot.png_bytes.len() > 256);
}

#[test]
fn cancel_stops_running_eval() {
    let session = Arc::new(RSession::new().expect("session"));
    let worker_session = session.clone();
    let worker = std::thread::spawn(move || worker_session.eval("repeat { 1 + 1 }".to_string()));

    std::thread::sleep(Duration::from_millis(10));
    session.cancel();

    let err = worker
        .join()
        .expect("worker should not panic")
        .expect_err("eval should be cancelled");
    assert!(matches!(err, RError::Cancelled));
}

#[test]
fn async_operations_report_callbacks_and_recover_after_cancel() {
    let session = RSession::new().expect("session");
    let (callback, events) = RecordingCallback::new();
    session.set_callback(Box::new(callback));

    assert_eq!(session.eval_async("1 + 1".to_string()).expect("eval id"), 0);
    match wait_for_callback(
        &events,
        |event| matches!(event, CallbackEvent::EvalComplete { output, .. } if output == "[1] 2"),
    ) {
        CallbackEvent::EvalComplete { kind, .. } => assert_eq!(kind, RValueKind::Real),
        event => panic!("unexpected callback event: {event:?}"),
    }
    match wait_for_callback(
        &events,
        |event| matches!(event, CallbackEvent::Output(line) if line == "[1] 2"),
    ) {
        CallbackEvent::Output(line) => assert_eq!(line, "[1] 2"),
        event => panic!("unexpected callback event: {event:?}"),
    }

    assert_eq!(
        session
            .render_async(
                "plot(c(1, 2, 3), c(3, 1, 4), col = \"green\", type = \"p\")".to_string(),
                240,
                180,
            )
            .expect("render id"),
        1
    );
    match wait_for_callback(&events, |event| {
        matches!(
            event,
            CallbackEvent::PlotReady {
                width: 240,
                height: 180,
                bytes
            } if *bytes > 256
        )
    }) {
        CallbackEvent::PlotReady { bytes, .. } => assert!(bytes > 256),
        event => panic!("unexpected callback event: {event:?}"),
    }

    assert_eq!(
        session
            .eval_async("repeat { 1 + 1 }".to_string())
            .expect("cancelled eval id"),
        2
    );
    std::thread::sleep(Duration::from_millis(10));
    session.cancel_current_operation();
    match wait_for_callback(
        &events,
        |event| matches!(event, CallbackEvent::Error(message) if message.contains("cancelled")),
    ) {
        CallbackEvent::Error(message) => assert!(message.contains("cancelled")),
        event => panic!("unexpected callback event: {event:?}"),
    }

    assert_eq!(
        session.eval("2 + 2".to_string()).expect("recovered eval"),
        "[1] 4"
    );
}
