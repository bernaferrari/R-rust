use r_graphics_engine::Scene;

#[test]
fn failed_plot_new_preserves_mfrow_index_and_session_for_recovery() {
    let mut session = rmath::android::RSession::new();
    let mut scene = Scene::new(640, 480);

    let failed = session.eval_script_with_renderplot_backend(
        r#"
emit <- function(tag) cat(tag, "|", paste(par("mfg"), collapse=","),
                         "|new=", par("new"), "|page=", par("page"), "\n", sep="")
par(mar=rep(0,4)); plot.new(); emit("first")
par(mfrow=c(1,2)); emit("grid")
par(mar=rep(100,4))
tryCatch(plot.new(), error=function(e)
  cat("error|", paste(class(e), collapse=","), "|", conditionMessage(e), "\n", sep=""))
emit("failed")
"#,
        &mut scene,
    );
    assert!(
        !matches!(failed.typed, rmath::android::RValue::Error(_)),
        "{}",
        failed.output
    );
    // Re-enter the same session/device after the caught graphics error. This
    // verifies the failed plot did not revoke its owner or leave stale frame
    // state that prevents a valid recovery plot.
    let recovered = session.eval_script_with_renderplot_backend(
        r#"
par(mar=rep(0,4)); plot.new()
cat("recovered|", paste(par("mfg"), collapse=","),
    "|new=", par("new"), "|page=", par("page"), "\n", sep="")
plot.new()
cat("next|", paste(par("mfg"), collapse=","),
    "|new=", par("new"), "|page=", par("page"), "\n", sep="")
plot.new()
cat("wrapped|", paste(par("mfg"), collapse=","),
    "|new=", par("new"), "|page=", par("page"), "\n", sep="")
"#,
        &mut scene,
    );
    assert!(
        !matches!(recovered.typed, rmath::android::RValue::Error(_)),
        "{}",
        recovered.output
    );
    assert_eq!(
        recovered.output,
        include_str!("fixtures/graphics-plot-error-recovery-gnu.out")
    );
    assert_eq!(
        failed.output,
        include_str!("fixtures/graphics-plot-error-progression-gnu.out")
    );
}

#[test]
fn failed_first_plot_keeps_fresh_device_state_and_other_session_isolated() {
    let mut failed_session = rmath::android::RSession::new();
    let mut other_session = rmath::android::RSession::new();
    let mut failed_scene = Scene::new(640, 480);
    let mut other_scene = Scene::new(640, 480);

    let failed = failed_session.eval_script_with_renderplot_backend(
        r#"
emit <- function(tag) cat(tag, "|", paste(par("mfg"), collapse=","),
                         "|new=", par("new"), "|page=", par("page"), "\n", sep="")
par(mfrow=c(1,2)); emit("grid")
par(mar=rep(100,4))
tryCatch(plot.new(), error=function(e)
  cat("error|", paste(class(e), collapse=","), "|", conditionMessage(e), "\n", sep=""))
emit("failed")
"#,
        &mut failed_scene,
    );
    assert!(
        !matches!(failed.typed, rmath::android::RValue::Error(_)),
        "{}",
        failed.output
    );
    assert_eq!(
        failed.output,
        include_str!("fixtures/graphics-plot-error-fresh-gnu.out")
    );

    let other = other_session.eval_script_with_renderplot_backend(
        r#"
par(mfrow=c(1,2)); plot.new()
cat("other|", paste(par("mfg"), collapse=","),
    "|new=", par("new"), "|page=", par("page"), "\n", sep="")
"#,
        &mut other_scene,
    );
    assert!(
        !matches!(other.typed, rmath::android::RValue::Error(_)),
        "{}",
        other.output
    );
    assert_eq!(
        other.output,
        include_str!("fixtures/graphics-plot-error-other-session-gnu.out")
    );

    let recovered = failed_session.eval_script_with_renderplot_backend(
        r#"
par(mar=rep(0,4)); plot.new()
cat("recovered|", paste(par("mfg"), collapse=","),
    "|new=", par("new"), "|page=", par("page"), "\n", sep="")
plot.new()
cat("next|", paste(par("mfg"), collapse=","),
    "|new=", par("new"), "|page=", par("page"), "\n", sep="")
plot.new()
cat("wrapped|", paste(par("mfg"), collapse=","),
    "|new=", par("new"), "|page=", par("page"), "\n", sep="")
"#,
        &mut failed_scene,
    );
    assert!(
        !matches!(recovered.typed, rmath::android::RValue::Error(_)),
        "{}",
        recovered.output
    );
    assert_eq!(
        recovered.output,
        include_str!("fixtures/graphics-plot-error-fresh-recovery-gnu.out")
    );
}
