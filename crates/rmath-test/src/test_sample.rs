//! Trunk-parity tests for `sample()` internals: the Walker alias-method
//! path (>200 categories with `n * p[i] > 0.1`) and the plain weighted
//! paths. Golden values generated with stock R (trunk):
//!
//!   set.seed(7); sum(sample(1:500, 10000, replace=TRUE, prob=(1:500)^2))
//!     -> 3767164   (walker, sample.kind="Rejection")
//!   RNGkind(sample.kind="Rounding"); set.seed(7); (same sample)
//!     -> 3761789   (walker, Rounding)
//!   set.seed(1); sum(sample(1:300, 5000, replace=TRUE,
//!                           prob=c(rep(1,150), rep(3,150)))) -> 933729
//!
//! The port's unif stream is stock-parity and the alias algorithm is
//! deterministic on that stream, so the sampled VALUES match exactly.

pub fn run_tests() -> Result<(), String> {
    // --- R level: dispatch wiring (nc > 200 -> walker, else plain path) ---
    // Values must match stock R exactly (see doc comment); n = 200/201 is
    // the dispatch boundary in random.c do_sample.
    let mut session = rmath::android::RSession::new();
    session.enable_host_process_capabilities();
    let cases: &[(&str, &str)] = &[
        (
            // walker, Rejection
            "set.seed(7); sum(sample(1:500, 10000, replace=TRUE, prob=(1:500)^2))",
            "3767164",
        ),
        (
            // walker, Rounding sample kind (consumes one unif per draw)
            "RNGkind(sample.kind='Rounding'); set.seed(7); sum(sample(1:500, 10000, replace=TRUE, prob=(1:500)^2))",
            "3761789",
        ),
        (
            // walker, mixed weights
            "RNGkind(sample.kind='Rejection'); set.seed(1); sum(sample(1:300, 5000, replace=TRUE, prob=c(rep(1,150), rep(3,150))))",
            "933729",
        ),
        (
            // boundary n=201: all categories -> walker
            "set.seed(11); sum(sample(1:201, 1000, replace=TRUE, prob=rep(1, 201)))",
            "101862",
        ),
        (
            // boundary n=200: plain ProbSampleReplace
            "set.seed(12); sum(sample(1:200, 1000, replace=TRUE, prob=rep(1, 200)))",
            "100104",
        ),
        (
            // plain path regression: single positive probability
            "set.seed(13); sum(sample(1:500, 1000, replace=TRUE, prob=c(1, rep(0, 499))))",
            "1000",
        ),
        (
            // without-replacement weighted (ProbSampleNoReplace, unchanged)
            "set.seed(14); sum(sample(1:500, 100, replace=FALSE, prob=(1:500)^2))",
            "36607",
        ),
    ];
    for (code, want) in cases {
        let result = session.eval(code);
        if matches!(result.typed, rmath::android::RValue::Error(_)) {
            return Err(format!("eval failed: {code} => {}", result.output));
        }
        let out = result.output.trim();
        let got = out.rsplit_once("[1]").map(|(_, v)| v.trim()).unwrap_or(out);
        if got != *want {
            return Err(format!("eval {code} => {got}, trunk {want}"));
        }
    }
    Ok(())
}
