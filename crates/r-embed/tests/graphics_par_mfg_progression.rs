use r_graphics_engine::Scene;

fn run(code: &str) -> String {
    let mut session = rmath::android::RSession::new();
    let mut scene = Scene::new(640, 480);
    let result = session.eval_script_with_renderplot_backend(code, &mut scene);
    assert!(
        !matches!(result.typed, rmath::android::RValue::Error(_)),
        "{}",
        result.output
    );
    result.output
}

#[test]
fn plot_new_advances_mfrow_and_mfcol_but_respects_explicit_mfg() {
    let output = run(r#"
emit <- function(tag) {
  cat(tag, "|", paste(par("mfg"), collapse=","), "|",
      paste(sprintf("%.3f", par("fig")), collapse=","), "\n", sep="")
}
emit("fresh")
par(mfrow=c(2,3)); emit("mfrow-initial")
for (i in 1:7) { plot.new(); emit(paste0("mfrow-", i)) }
par(mfcol=c(2,3)); emit("mfcol-initial")
for (i in 1:7) { plot.new(); emit(paste0("mfcol-", i)) }
par(mfrow=c(2,3)); par(mfg=c(1,2)); emit("explicit-mfg")
plot.new(); emit("explicit-new-1")
plot.new(); emit("explicit-new-2")
par(new=FALSE); plot.new(); emit("after-new-false")
plot(1:2); emit("after-plot-1")
plot(1:2); emit("after-plot-2")
"#);
    assert_eq!(output, include_str!("fixtures/graphics-par-mfg-gnu.out"));
}

#[test]
fn physical_par_values_couple_after_changes_and_new_frames() {
    let output = run(r#"
emit <- function(tag) {
  cat(tag, "|", paste(sprintf("%.5f", par("din")), collapse=","), "|",
      paste(sprintf("%.5f", par("fin")), collapse=","), "|",
      paste(sprintf("%.5f", par("fig")), collapse=","), "|",
      paste(sprintf("%.5f", par("mai")), collapse=","), "|",
      paste(sprintf("%.5f", par("omi")), collapse=","), "|",
      paste(sprintf("%.5f", par("pin")), collapse=","), "|",
      paste(sprintf("%.5f", par("plt")), collapse=","), "\n", sep="")
}
emit("fresh")
par(mai=c(1,2,3,4)); emit("mai")
par(omi=c(.1,.2,.3,.4)); emit("omi")
par(fig=c(.1,.8,.2,.9)); emit("fig")
par(plt=c(.2,.7,.1,.8)); emit("plt")
plot.new(); emit("first plot")
par(mai=c(.5,.6,.7,.8)); emit("postplot mai")
par(omi=c(.25,.3,.35,.4)); emit("postplot omi")
plot.new(); emit("second plot")
par(mfrow=c(2,2)); plot.new(); emit("mfrow1")
par(mai=c(.4,.5,.6,.7)); emit("mfrow postplot mai")
plot.new(); emit("mfrow2")
"#);
    assert_eq!(
        output,
        include_str!("fixtures/graphics-par-physical-browser.out")
    );
}

#[test]
fn first_device_size_query_uses_the_renderplot_backend_geometry() {
    let output = run("cat(paste(sprintf('%.5f', par('din')), collapse=','))");
    assert_eq!(output.trim(), "8.88889,6.66667");
}

#[test]
fn device_size_query_tracks_a_replaced_renderplot_backend() {
    let mut session = rmath::android::RSession::new();
    let mut first = Scene::new(640, 480);
    let snapshot = r#"
par(mai=c(.5,.6,.7,.8)); par(omi=c(.25,.3,.35,.4))
cat(paste(sprintf("%.5f", par("din")), collapse=","), "|",
    paste(sprintf("%.5f", par("fin")), collapse=","), "|",
    paste(sprintf("%.5f", par("mai")), collapse=","), "|",
    paste(sprintf("%.5f", par("omi")), collapse=","), "|",
    paste(sprintf("%.5f", par("pin")), collapse=","), "|",
    paste(sprintf("%.5f", par("plt")), collapse=","), "\n", sep="")
"#;
    let first_result = session.eval_script_with_renderplot_backend(snapshot, &mut first);
    assert!(
        !matches!(first_result.typed, rmath::android::RValue::Error(_)),
        "{}",
        first_result.output
    );
    assert_eq!(
        first_result.output,
        "8.88889,6.66667|8.18889,6.06667|0.50000,0.60000,0.70000,0.80000|0.25000,0.30000,0.35000,0.40000|6.78889,4.86667|0.07327,0.90231,0.08242,0.88462\n"
    );

    let mut replacement = Scene::new(320, 240);
    let next_result = session.eval_script_with_renderplot_backend(snapshot, &mut replacement);
    assert!(
        !matches!(next_result.typed, rmath::android::RValue::Error(_)),
        "{}",
        next_result.output
    );
    assert_eq!(
        next_result.output,
        "4.44444,3.33333|3.74444,2.73333|0.50000,0.60000,0.70000,0.80000|0.25000,0.30000,0.35000,0.40000|2.34444,1.53333|0.16024,0.78635,0.18293,0.74390\n"
    );
}
