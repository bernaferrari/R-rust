//! Boundary stress: the fuzz invariant as a deterministic, CI-able test.
//!
//! The r-embed boundary contract: arbitrary input to `eval`,
//! `define_handle`, and the guards must never escape as a Rust panic —
//! only `RSessionError` crosses. `fuzz/` holds the coverage-guided
//! libFuzzer harnesses for the same invariant (see fuzz/README.md for
//! the sanitizer-budget runbook); this test proves it deterministically
//! for a fixed seed on every CI run.

use r_embed::RSession;

/// Deterministic xoshiro-style LCG so failures reproduce exactly.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const TOKENS: &[&str] = &[
    "function",
    "if",
    "else",
    "while",
    "repeat",
    "for",
    "in",
    "next",
    "break",
    "TRUE",
    "FALSE",
    "NULL",
    "NA",
    "Inf",
    "NaN",
    "local",
    "quote",
    "tryCatch",
    "stop",
    "warning",
    "return",
    "gc",
    "rm",
    "library",
    "x",
    "y",
    "f",
    "\"s\"",
    "\"",
    "'",
    "{",
    "}",
    "(",
    ")",
    "[",
    "]",
    "[[",
    "]]",
    "=",
    "<-",
    "->",
    "$",
    "@",
    "%>%",
    "%%",
    ",",
    ";",
    ":",
    "+",
    "-",
    "*",
    "/",
    "^",
    "!",
    "&",
    "|",
    "~",
    "?",
    "`",
    "\\",
    "\n",
    "\t",
    " ",
    "1",
    "2.5",
    "1e3",
    "0x1f",
    "1L",
    "NA_real_",
    "utf8: \u{e9}\u{4e2d}",
    "\u{1f600}",
];

fn script_for(rng: &mut Lcg) -> String {
    let n = rng.below(24);
    (0..n)
        .map(|_| TOKENS[rng.below(TOKENS.len())])
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn eval_boundary_never_panics() {
    let mut rng = Lcg(0x5eed_1234_abcd_ef01);
    let mut session = RSession::new().expect("session");
    // Generated grammar can express unbounded loops ("repeat", "while
    // TRUE"): bound execution like a host console would.
    session
        .set_resource_limits(r_embed::RResourceLimits {
            max_execution_time_ms: 200,
            ..r_embed::RResourceLimits::default()
        })
        .expect("limits");
    for i in 0..2000 {
        let script = script_for(&mut rng);
        // The invariant: this returns Ok/Err, never panics.
        let outcome = session.eval(&script);
        if i % 500 == 0 {
            let _ = session.eval("gc()");
        }
        std::hint::black_box(&outcome);
    }
}

#[test]
fn handle_boundary_never_panics() {
    let mut rng = Lcg(0xfeed_0bad_c0de_1234);
    let mut session = RSession::new().expect("session");
    session
        .set_resource_limits(r_embed::RResourceLimits {
            max_execution_time_ms: 200,
            ..r_embed::RResourceLimits::default()
        })
        .expect("limits");
    for i in 0..500 {
        let payload = script_for(&mut rng);
        if let Ok(handle) = session.define_handle(&payload) {
            if let Ok(guard) = session.read_handle(&handle) {
                std::hint::black_box(guard.value());
            }
            if let Ok(mut writer) = session.write_handle(&handle) {
                let _ = writer.set(&payload);
                let _ = writer.update("length(.)");
            }
            let _ = session.remove_handle(&handle);
            // Stale use must error, never panic.
            let stale = session.read_handle(&handle);
            assert!(stale.is_err(), "stale handle must error (iteration {i})");
        }
        if i % 250 == 0 {
            let _ = session.eval("gc()");
        }
    }
}
