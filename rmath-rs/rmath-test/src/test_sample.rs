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

use rmath::mainutils::random::{
    FixupProb, GetRNGstate, PutRNGstate, set_session_seed64, walker_ProbSampleReplace_r,
};

pub fn run_tests() -> Result<(), String> {
    // --- API level: seeded walker stream matches trunk exactly ---
    let _session = rmath::sexp::RSession::new();
    unsafe {
        set_session_seed64(7);
        GetRNGstate();
        let n = 500usize;
        let mut p: Vec<f64> = (1..=n as i64).map(|i| (i * i) as f64).collect();
        if FixupProb(&mut p, 10_000, true).is_some() {
            return Err("FixupProb((1:500)^2) failed".to_string());
        }
        let ans = walker_ProbSampleReplace_r(n, &mut p, 10_000);
        PutRNGstate();

        let sum: i64 = ans.iter().map(|&v| v as i64).sum();
        if sum != 3_767_164 {
            return Err(format!(
                "walker sample(1:500, 1e4, prob=(1:500)^2), seed 7: sum {sum}, trunk 3767164"
            ));
        }
        let want_head = [
            298, 415, 449, 375, 442, 323, 417, 431, 471, 268, 336, 374, 397, 335, 407, 388, 358,
            215, 451, 381,
        ];
        if ans[..20] != want_head {
            return Err(format!(
                "walker head mismatch: {:?} vs trunk {want_head:?}",
                &ans[..20]
            ));
        }
    }

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
