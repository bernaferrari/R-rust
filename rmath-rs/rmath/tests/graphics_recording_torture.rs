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

#[test]
fn record_replay_under_gc_torture_uses_owned_scene_backend() {
    let mut session = RSession::new();
    let mut scene = Scene::new(320, 240);
    let result = session.eval_script_with_renderplot_backend(
        "gctorture(TRUE); plot(1:3, c(1,4,9), col='blue'); saved <- serialize(recordPlot(), NULL); gctorture(FALSE); plot.new(); replayPlot(unserialize(saved))",
        &mut scene,
    );
    assert_ok(&result);
    assert!(
        !scene.operations().is_empty(),
        "record/replay should produce owned scene operations"
    );
}

#[test]
fn backend_guard_restores_after_eval_error() {
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
}

// Keep the trait import explicit: this test is intended to prove Scene remains
// usable as the public backend abstraction without a concrete renderer.
#[allow(dead_code)]
fn assert_backend_dimensions(target: &dyn DrawTarget) {
    assert_eq!(target.dimensions(), (320, 240));
}
