use rmath::android::{RSession, RValue};
use std::{hint::black_box, time::Instant};
fn main() {
    let iterations = std::env::var("RMATH_BENCH_ITERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1000);
    for (name, code) in [
        ("alloc_integer_vector", "integer(1000)"),
        ("alloc_real_vector", "numeric(1000)"),
        ("alloc_string_vector", "character(100)"),
        ("eval_self_integer", "42L"),
        ("eval_self_real", "3.141592653589793"),
        ("eval_null", "NULL"),
        ("output_capture", "cat(42)"),
        ("math_dnorm", "dnorm(0)"),
        ("math_pnorm", "pnorm(1.96)"),
        ("math_qnorm", "qnorm(0.975)"),
        ("math_rnorm", "rnorm(1)"),
        ("unif_rand", "runif(1)"),
    ] {
        let mut session = RSession::new();
        let start = Instant::now();
        for _ in 0..iterations {
            let result = session.eval(black_box(code));
            assert!(
                !matches!(result.typed, RValue::Error(_)),
                "{}",
                result.output
            );
            black_box(result);
        }
        eprintln!(
            "{name}: {:?} for {iterations} iterations (owned embedding API)",
            start.elapsed()
        );
    }
}
