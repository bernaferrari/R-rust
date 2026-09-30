//! Lightweight backend coverage for recordPlot/replayPlot and backend guards.
//!
//! This uses the owned Scene itself as the DrawTarget, so the test exercises
//! the public session/backend path without pulling in a font or raster device.

#![cfg(feature = "renderplot-device")]

use r_graphics_engine::{DrawTarget, Scene};
use rmath::android::{RSession, RValue};

fn assert_ok(result: &rmath::android::RResult) {
    assert!(
        !matches!(result.typed, RValue::Error(_)),
        "{}",
        result.output
    );
}

fn default_pdf_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("Rplots.pdf"));
    }
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Rplots.pdf");
    if !paths.iter().any(|path| path == &manifest) {
        paths.push(manifest);
    }
    paths
}

fn remove_default_pdf() {
    for path in default_pdf_paths() {
        let _ = std::fs::remove_file(path);
    }
}

fn assert_no_default_pdf() {
    for path in default_pdf_paths() {
        assert!(
            !path.exists(),
            "renderplot evaluation wrote {}",
            path.display()
        );
    }
}

#[test]
fn record_replay_serializes_owned_scene_backend() {
    remove_default_pdf();
    let mut session = RSession::new();
    let mut scene = Scene::new(320, 240);
    let result = session.eval_script_with_renderplot_backend(
        "plot(1:3, c(1,4,9), col='blue'); saved <- serialize(recordPlot(), NULL); plot.new(); replayPlot(unserialize(saved))",
        &mut scene,
    );
    assert_ok(&result);
    assert!(
        !scene.operations().is_empty(),
        "record/replay should produce owned scene operations"
    );
    assert_no_default_pdf();
}

#[test]
fn record_replay_under_gc_torture_uses_owned_scene_backend() {
    remove_default_pdf();
    let mut session = RSession::new();
    let mut scene = Scene::new(320, 240);
    // Torture the snapshot and replay, not every allocation inside plot().
    // gctorture around plot() itself collects for hours.
    let result = session.eval_script_with_renderplot_backend(
        "plot(1:3, c(1,4,9), col='blue'); gctorture(TRUE); saved <- serialize(recordPlot(), NULL); plot.new(); replayPlot(unserialize(saved)); gctorture(FALSE)",
        &mut scene,
    );
    assert_ok(&result);
    assert!(
        !scene.operations().is_empty(),
        "record/replay should produce owned scene operations"
    );
    assert_no_default_pdf();
}

#[test]
fn renderplot_catch_script_errors_continues_and_draws() {
    remove_default_pdf();
    let mut session = RSession::new();
    let mut scene = Scene::new(320, 240);
    let result = session.eval_script_with_renderplot_backend(
        r#"
        options(catch.script.errors = TRUE)
        stop("boom-render")
        setClass("Foo", slots = c(x = "numeric"))
        setMethod("show", "Foo", function(object) stop("boom-show"))
        new("Foo", x = 1)
        plot(1:3, c(1, 4, 9))
        "#,
        &mut scene,
    );
    assert_ok(&result);
    assert!(
        result.stderr.contains("boom-render"),
        "stderr={}",
        result.stderr
    );
    assert!(
        result.stderr.contains("boom-show"),
        "stderr={}",
        result.stderr
    );
    assert!(
        scene
            .operations()
            .iter()
            .any(|operation| matches!(operation, r_graphics_engine::DrawOperation::Path(_))),
        "plot after a caught script error should draw; stderr={}; ops={:?}",
        result.stderr,
        scene.operations()
    );
    assert_no_default_pdf();
}

#[test]
fn renderplot_catch_script_errors_then_plots() {
    remove_default_pdf();
    let mut session = RSession::new();
    let mut scene = Scene::new(320, 240);
    let result = session.eval_script_with_renderplot_backend(
        r#"
        options(catch.script.errors = TRUE)
        stop("boom-render")
        plot(1:3, c(1, 4, 9))
        7
        "#,
        &mut scene,
    );
    assert_ok(&result);
    assert!(
        result.stderr.contains("boom-render"),
        "stderr={}",
        result.stderr
    );
    assert!(
        result.stdout.contains("[1] 7"),
        "stdout={}; stderr={}",
        result.stdout,
        result.stderr
    );
    assert!(
        scene
            .operations()
            .iter()
            .any(|operation| matches!(operation, r_graphics_engine::DrawOperation::Path(_))),
        "stderr={}; ops={:?}",
        result.stderr,
        scene.operations()
    );
    assert_no_default_pdf();
}

#[test]
fn backend_guard_restores_after_eval_error() {
    remove_default_pdf();
    let mut session = RSession::new();
    let mut scene = Scene::new(320, 240);
    let error = session.eval_script_with_renderplot_backend(
        "plot(1:3, c(1,4,9)); stop('recording backend error')",
        &mut scene,
    );
    assert!(matches!(error.typed, RValue::Error(_)));

    let mut recovered = Scene::new(320, 240);
    let result = session.eval_script_with_renderplot_backend(
        "plot(1:3, c(3,1,2), col='red'); invisible(gc())",
        &mut recovered,
    );
    assert_ok(&result);
    assert!(
        recovered
            .operations()
            .iter()
            .any(|operation| { matches!(operation, r_graphics_engine::DrawOperation::Path(_)) })
    );
    assert_no_default_pdf();
}

// Keep the trait import explicit: this test is intended to prove Scene remains
// usable as the public backend abstraction without a concrete renderer.
#[allow(dead_code)]
fn assert_backend_dimensions(target: &dyn DrawTarget) {
    assert_eq!(target.dimensions(), (320, 240));
}
