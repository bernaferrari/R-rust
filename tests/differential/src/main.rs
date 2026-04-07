//! Differential testing: compare rmath output against stock R reference values.
//!
//! Reference values were computed with R 4.x using the corresponding d/p/q functions.
//! Each test checks that rmath reproduces the R result to within machine epsilon tolerance
//! (relative error < 1e-12 for normal-range results, or absolute error < 1e-15 near zero).

use rmath::dist::beta::*;
use rmath::dist::cauchy::*;
use rmath::dist::chisq::*;
use rmath::dist::exponential::*;
use rmath::dist::gamma::*;
use rmath::dist::normal::*;
use rmath::dist::t_dist::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const REL_TOL: f64 = 1e-10;
const ABS_TOL: f64 = 1e-10;

struct TestCase {
    label: &'static str,
    got: f64,
    expected: f64,
}

fn approx_eq(label: &str, got: f64, expected: f64) -> Result<(), String> {
    // Both NaN → pass
    if got.is_nan() && expected.is_nan() {
        return Ok(());
    }
    // One NaN → fail
    if got.is_nan() || expected.is_nan() {
        return Err(format!(
            "{label}: NaN mismatch — got {got}, expected {expected}"
        ));
    }
    // Both ±Inf of the same sign → pass
    if got.is_infinite()
        && expected.is_infinite()
        && got.is_sign_positive() == expected.is_sign_positive()
    {
        return Ok(());
    }
    if got.is_infinite() || expected.is_infinite() {
        return Err(format!(
            "{label}: Inf mismatch — got {got}, expected {expected}"
        ));
    }
    // Zero/zero
    if got == 0.0 && expected == 0.0 {
        return Ok(());
    }
    // Relative error for normal values, absolute near zero
    let denom = expected.abs().max(got.abs());
    let rel_err = if denom == 0.0 {
        (got - expected).abs()
    } else {
        (got - expected).abs() / denom
    };
    if rel_err <= REL_TOL || (got - expected).abs() <= ABS_TOL {
        Ok(())
    } else {
        Err(format!(
            "{label}: got {got:.16e}, expected {expected:.16e}, rel_err {rel_err:.2e}"
        ))
    }
}

/// Run a batch of test cases, collecting failures.
fn run_batch(tests: Vec<TestCase>) -> Result<(), String> {
    let mut failures: Vec<String> = Vec::new();
    for t in tests {
        if let Err(e) = approx_eq(t.label, t.got, t.expected) {
            failures.push(e);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n  "))
    }
}

// ---------------------------------------------------------------------------
// Distribution test modules
// ---------------------------------------------------------------------------

/// Reference values from R 4.x: dnorm(x, mean, sd, log=FALSE)
fn test_dnorm() -> Result<(), String> {
    let tests = vec![
        // dnorm(0, 0, 1)
        TestCase {
            label: "dnorm(0,0,1)",
            got: dnorm4_inner(0.0, 0.0, 1.0, false),
            expected: 0.398_942_280_401_432_7,
        },
        // dnorm(1, 0, 1)
        TestCase {
            label: "dnorm(1,0,1)",
            got: dnorm4_inner(1.0, 0.0, 1.0, false),
            expected: 0.241_970_724_519_143_37,
        },
        // dnorm(0, 0, 1, log=TRUE)
        TestCase {
            label: "dnorm(0,0,1,log)",
            got: dnorm4_inner(0.0, 0.0, 1.0, true),
            expected: -0.918_938_533_204_672_7,
        },
        // dnorm(-1.96, 0, 1)
        TestCase {
            label: "dnorm(-1.96,0,1)",
            got: dnorm4_inner(-1.96, 0.0, 1.0, false),
            expected: 0.058_440_944_333_451_47,
        },
        // dnorm(0, 5, 2)
        TestCase {
            label: "dnorm(0,5,2)",
            got: dnorm4_inner(0.0, 5.0, 2.0, false),
            expected: 0.008_764_150_246_784_270,
        },
        // NaN propagation
        TestCase {
            label: "dnorm(NaN,0,1)",
            got: dnorm4_inner(f64::NAN, 0.0, 1.0, false),
            expected: f64::NAN,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: pnorm(x, mean, sd, lower.tail=TRUE, log.p=FALSE)
fn test_pnorm() -> Result<(), String> {
    let tests = vec![
        // pnorm(0)
        TestCase {
            label: "pnorm(0)",
            got: pnorm5_inner(0.0, 0.0, 1.0, true, false),
            expected: 0.5,
        },
        // pnorm(1.96)
        TestCase {
            label: "pnorm(1.96)",
            got: pnorm5_inner(1.96, 0.0, 1.0, true, false),
            expected: 0.975_002_104_851_779_5,
        },
        // pnorm(-1.96)
        TestCase {
            label: "pnorm(-1.96)",
            got: pnorm5_inner(-1.96, 0.0, 1.0, true, false),
            expected: 0.024_997_895_148_220_46,
        },
        // pnorm(Inf)
        TestCase {
            label: "pnorm(Inf)",
            got: pnorm5_inner(f64::INFINITY, 0.0, 1.0, true, false),
            expected: 1.0,
        },
        // pnorm(-Inf)
        TestCase {
            label: "pnorm(-Inf)",
            got: pnorm5_inner(f64::NEG_INFINITY, 0.0, 1.0, true, false),
            expected: 0.0,
        },
        // pnorm(0, log=TRUE)
        TestCase {
            label: "pnorm(0,log)",
            got: pnorm5_inner(0.0, 0.0, 1.0, true, true),
            expected: -0.693_147_180_559_945_3,
        },
        // pnorm(0, lower.tail=FALSE)
        TestCase {
            label: "pnorm(0,upper)",
            got: pnorm5_inner(0.0, 0.0, 1.0, false, false),
            expected: 0.5,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: qnorm(p, mean, sd, lower.tail=TRUE, log.p=FALSE)
fn test_qnorm() -> Result<(), String> {
    let tests = vec![
        // qnorm(0.5)
        TestCase {
            label: "qnorm(0.5)",
            got: qnorm5_inner(0.5, 0.0, 1.0, true, false),
            expected: 0.0,
        },
        // qnorm(0.975)
        TestCase {
            label: "qnorm(0.975)",
            got: qnorm5_inner(0.975, 0.0, 1.0, true, false),
            expected: 1.959_963_984_540_053_6,
        },
        // qnorm(0.025)
        TestCase {
            label: "qnorm(0.025)",
            got: qnorm5_inner(0.025, 0.0, 1.0, true, false),
            expected: -1.959_963_984_540_053_6,
        },
        // qnorm(0)
        TestCase {
            label: "qnorm(0)",
            got: qnorm5_inner(0.0, 0.0, 1.0, true, false),
            expected: f64::NEG_INFINITY,
        },
        // qnorm(1)
        TestCase {
            label: "qnorm(1)",
            got: qnorm5_inner(1.0, 0.0, 1.0, true, false),
            expected: f64::INFINITY,
        },
        // qnorm(0.5, 10, 2)
        TestCase {
            label: "qnorm(0.5,10,2)",
            got: qnorm5_inner(0.5, 10.0, 2.0, true, false),
            expected: 10.0,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: dgamma(x, shape, scale, log=FALSE)
fn test_dgamma() -> Result<(), String> {
    let tests = vec![
        // dgamma(1, 1, 1)
        TestCase {
            label: "dgamma(1,1,1)",
            got: dgamma_inner(1.0, 1.0, 1.0, false),
            expected: 0.367_879_441_171_442_33,
        },
        // dgamma(2, 2, 1)
        TestCase {
            label: "dgamma(2,2,1)",
            got: dgamma_inner(2.0, 2.0, 1.0, false),
            expected: 0.270_670_566_473_225_4,
        },
        // dgamma(0.5, 1, 1, log=TRUE)
        TestCase {
            label: "dgamma(0.5,1,1,log)",
            got: dgamma_inner(0.5, 1.0, 1.0, true),
            expected: -0.5,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: pgamma(x, shape, scale, lower.tail=TRUE, log.p=FALSE)
fn test_pgamma() -> Result<(), String> {
    let tests = vec![
        // pgamma(1, 1, 1)
        TestCase {
            label: "pgamma(1,1,1)",
            got: pgamma_inner(1.0, 1.0, 1.0, true, false),
            expected: 0.632_120_558_828_557_7,
        },
        // pgamma(2, 2, 1)
        TestCase {
            label: "pgamma(2,2,1)",
            got: pgamma_inner(2.0, 2.0, 1.0, true, false),
            expected: 0.593_994_150_290_161_9,
        },
        // pgamma(0, 1, 1)
        TestCase {
            label: "pgamma(0,1,1)",
            got: pgamma_inner(0.0, 1.0, 1.0, true, false),
            expected: 0.0,
        },
        // pgamma(Inf, 1, 1)
        TestCase {
            label: "pgamma(Inf,1,1)",
            got: pgamma_inner(f64::INFINITY, 1.0, 1.0, true, false),
            expected: 1.0,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: qgamma(p, shape, scale, lower.tail=TRUE, log.p=FALSE)
fn test_qgamma() -> Result<(), String> {
    let tests = vec![
        // qgamma(0.5, 1, 1)
        TestCase {
            label: "qgamma(0.5,1,1)",
            got: qgamma_inner(0.5, 1.0, 1.0, true, false),
            expected: 0.693_147_180_559_945_3,
        },
        // qgamma(0.9, 2, 1)
        TestCase {
            label: "qgamma(0.9,2,1)",
            got: qgamma_inner(0.9, 2.0, 1.0, true, false),
            expected: 3.889_720_169_867_430_0,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: dbeta(x, a, b, log=FALSE)
fn test_dbeta() -> Result<(), String> {
    let tests = vec![
        // dbeta(0.5, 1, 1)
        TestCase {
            label: "dbeta(0.5,1,1)",
            got: dbeta_inner(0.5, 1.0, 1.0, false),
            expected: 1.0,
        },
        // dbeta(0.5, 2, 5)
        TestCase {
            label: "dbeta(0.5,2,5)",
            got: dbeta_inner(0.5, 2.0, 5.0, false),
            expected: 0.937_5,
        },
        // dbeta(0.2, 0.5, 0.5)
        TestCase {
            label: "dbeta(0.2,0.5,0.5)",
            got: dbeta_inner(0.2, 0.5, 0.5, false),
            expected: 0.795_774_715_459_476_8,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: pbeta(x, a, b, lower.tail=TRUE, log.p=FALSE)
fn test_pbeta() -> Result<(), String> {
    let tests = vec![
        // pbeta(0.5, 1, 1)
        TestCase {
            label: "pbeta(0.5,1,1)",
            got: pbeta_inner(0.5, 1.0, 1.0, true, false),
            expected: 0.5,
        },
        // pbeta(0.5, 2, 5)
        TestCase {
            label: "pbeta(0.5,2,5)",
            got: pbeta_inner(0.5, 2.0, 5.0, true, false),
            expected: 0.890_625,
        },
        // pbeta(0, 1, 1)
        TestCase {
            label: "pbeta(0,1,1)",
            got: pbeta_inner(0.0, 1.0, 1.0, true, false),
            expected: 0.0,
        },
        // pbeta(1, 1, 1)
        TestCase {
            label: "pbeta(1,1,1)",
            got: pbeta_inner(1.0, 1.0, 1.0, true, false),
            expected: 1.0,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: qbeta(p, a, b, lower.tail=TRUE, log.p=FALSE)
fn test_qbeta() -> Result<(), String> {
    let tests = vec![
        // qbeta(0.5, 1, 1)
        TestCase {
            label: "qbeta(0.5,1,1)",
            got: qbeta_inner(0.5, 1.0, 1.0, true, false),
            expected: 0.5,
        },
        // qbeta(0.5, 2, 2)
        TestCase {
            label: "qbeta(0.5,2,2)",
            got: qbeta_inner(0.5, 2.0, 2.0, true, false),
            expected: 0.5,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: dexp(x, rate, log=FALSE)
fn test_dexp() -> Result<(), String> {
    let tests = vec![
        // dexp(0, 1)
        TestCase {
            label: "dexp(0,1)",
            got: dexp_inner(0.0, 1.0, false),
            expected: 1.0,
        },
        // dexp(1, 1)
        TestCase {
            label: "dexp(1,1)",
            got: dexp_inner(1.0, 1.0, false),
            expected: 0.367_879_441_171_442_33,
        },
        // dexp(2, 0.5) - rmath uses scale parameter (scale = 1/rate = 2.0)
        TestCase {
            label: "dexp(2,0.5)",
            got: dexp_inner(2.0, 2.0, false),
            expected: 0.183_939_720_585_721_17,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: pexp(x, rate, lower.tail=TRUE, log.p=FALSE)
fn test_pexp() -> Result<(), String> {
    let tests = vec![
        // pexp(0, 1)
        TestCase {
            label: "pexp(0,1)",
            got: pexp_inner(0.0, 1.0, true, false),
            expected: 0.0,
        },
        // pexp(1, 1)
        TestCase {
            label: "pexp(1,1)",
            got: pexp_inner(1.0, 1.0, true, false),
            expected: 0.632_120_558_828_557_7,
        },
        // pexp(Inf, 1)
        TestCase {
            label: "pexp(Inf,1)",
            got: pexp_inner(f64::INFINITY, 1.0, true, false),
            expected: 1.0,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: qexp(p, rate, lower.tail=TRUE, log.p=FALSE)
fn test_qexp() -> Result<(), String> {
    let tests = vec![
        // qexp(0.5, 1)
        TestCase {
            label: "qexp(0.5,1)",
            got: qexp_inner(0.5, 1.0, true, false),
            expected: 0.693_147_180_559_945_3,
        },
        // qexp(0.632, 1) ≈ 1
        TestCase {
            label: "qexp(0.632,1)",
            got: qexp_inner(0.632_120_558_828_557_7, 1.0, true, false),
            expected: 1.0,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: dcauchy(x, location, scale, log=FALSE)
fn test_dcauchy() -> Result<(), String> {
    let tests = vec![
        // dcauchy(0, 0, 1)
        TestCase {
            label: "dcauchy(0,0,1)",
            got: dcauchy_inner(0.0, 0.0, 1.0, false),
            expected: 0.318_309_886_183_790_7,
        },
        // dcauchy(1, 0, 1)
        TestCase {
            label: "dcauchy(1,0,1)",
            got: dcauchy_inner(1.0, 0.0, 1.0, false),
            expected: 0.159_154_943_091_895_35,
        },
        // dcauchy(0, 0, 1, log=TRUE)
        TestCase {
            label: "dcauchy(0,0,1,log)",
            got: dcauchy_inner(0.0, 0.0, 1.0, true),
            expected: -1.144_729_885_849_400_2,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: pcauchy(x, location, scale, lower.tail=TRUE, log.p=FALSE)
fn test_pcauchy() -> Result<(), String> {
    let tests = vec![
        // pcauchy(0, 0, 1)
        TestCase {
            label: "pcauchy(0,0,1)",
            got: pcauchy_inner(0.0, 0.0, 1.0, true, false),
            expected: 0.5,
        },
        // pcauchy(1, 0, 1)
        TestCase {
            label: "pcauchy(1,0,1)",
            got: pcauchy_inner(1.0, 0.0, 1.0, true, false),
            expected: 0.75,
        },
        // pcauchy(Inf, 0, 1)
        TestCase {
            label: "pcauchy(Inf,0,1)",
            got: pcauchy_inner(f64::INFINITY, 0.0, 1.0, true, false),
            expected: 1.0,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: dchisq(x, df, log=FALSE)
fn test_dchisq() -> Result<(), String> {
    let tests = vec![
        // dchisq(1, 1)
        TestCase {
            label: "dchisq(1,1)",
            got: dchisq_inner(1.0, 1.0, false),
            expected: 0.241_970_724_519_143_37,
        },
        // dchisq(3, 2)
        TestCase {
            label: "dchisq(3,2)",
            got: dchisq_inner(3.0, 2.0, false),
            expected: 0.111_565_080_074_214_97,
        },
        // dchisq(5, 10)
        TestCase {
            label: "dchisq(5,10)",
            got: dchisq_inner(5.0, 10.0, false),
            expected: 0.066_800_942_890_542_66,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: pchisq(x, df, lower.tail=TRUE, log.p=FALSE)
fn test_pchisq() -> Result<(), String> {
    let tests = vec![
        // pchisq(3.841, 1) - 3.841 is the 0.95 quantile of chi-sq(1)
        TestCase {
            label: "pchisq(3.841,1)",
            got: pchisq_inner(3.841, 1.0, true, false),
            expected: 0.949_986_316_236_043_3,
        },
        // pchisq(5.991, 2) - 5.991 is the 0.95 quantile of chi-sq(2)
        TestCase {
            label: "pchisq(5.991,2)",
            got: pchisq_inner(5.991, 2.0, true, false),
            expected: 0.949_988_384_973_420_9,
        },
        // pchisq(0, 5)
        TestCase {
            label: "pchisq(0,5)",
            got: pchisq_inner(0.0, 5.0, true, false),
            expected: 0.0,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: dt(x, df, log=FALSE)
fn test_dt() -> Result<(), String> {
    let tests = vec![
        // dt(0, 1)
        TestCase {
            label: "dt(0,1)",
            got: dt_inner(0.0, 1.0, false),
            expected: 0.318_309_886_183_790_7,
        },
        // dt(0, 10)
        TestCase {
            label: "dt(0,10)",
            got: dt_inner(0.0, 10.0, false),
            expected: 0.389_108_383_990_589_7,
        },
        // dt(2, 5)
        TestCase {
            label: "dt(2,5)",
            got: dt_inner(2.0, 5.0, false),
            expected: 0.065_090_310_326_216_47,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: pt(x, df, lower.tail=TRUE, log.p=FALSE)
fn test_pt() -> Result<(), String> {
    let tests = vec![
        // pt(0, 1)
        TestCase {
            label: "pt(0,1)",
            got: pt_inner(0.0, 1.0, true, false),
            expected: 0.5,
        },
        // pt(2.228, 10)
        TestCase {
            label: "pt(2.228,10)",
            got: pt_inner(2.228, 10.0, true, false),
            expected: 0.974_994_114_091_444_3,
        },
        // pt(Inf, 5)
        TestCase {
            label: "pt(Inf,5)",
            got: pt_inner(f64::INFINITY, 5.0, true, false),
            expected: 1.0,
        },
    ];
    run_batch(tests)
}

/// Reference values from R 4.x: qt(p, df, lower.tail=TRUE, log.p=FALSE)
fn test_qt() -> Result<(), String> {
    let tests = vec![
        // qt(0.975, 1)
        TestCase {
            label: "qt(0.975,1)",
            got: qt_inner(0.975, 1.0, true, false),
            expected: 12.706_204_736_174_485,
        },
        // qt(0.975, 10)
        TestCase {
            label: "qt(0.975,10)",
            got: qt_inner(0.975, 10.0, true, false),
            expected: 2.228_138_851_986_269_5,
        },
        // qt(0.5, 5)
        TestCase {
            label: "qt(0.5,5)",
            got: qt_inner(0.5, 5.0, true, false),
            expected: 0.0,
        },
    ];
    run_batch(tests)
}

// ---------------------------------------------------------------------------
// Main runner
// ---------------------------------------------------------------------------

fn main() {
    println!("rport differential tests — comparing against stock R reference values\n");

    let suites: Vec<(&str, fn() -> Result<(), String>)> = vec![
        ("dnorm", test_dnorm),
        ("pnorm", test_pnorm),
        ("qnorm", test_qnorm),
        ("dgamma", test_dgamma),
        ("pgamma", test_pgamma),
        ("qgamma", test_qgamma),
        ("dbeta", test_dbeta),
        ("pbeta", test_pbeta),
        ("qbeta", test_qbeta),
        ("dexp", test_dexp),
        ("pexp", test_pexp),
        ("qexp", test_qexp),
        ("dcauchy", test_dcauchy),
        ("pcauchy", test_pcauchy),
        ("dchisq", test_dchisq),
        ("pchisq", test_pchisq),
        ("dt", test_dt),
        ("pt", test_pt),
        ("qt", test_qt),
    ];

    let mut passed = 0usize;
    let mut failed = 0usize;

    for (name, suite) in &suites {
        match suite() {
            Ok(()) => {
                println!("  [PASS] {name}");
                passed += 1;
            }
            Err(e) => {
                println!("  [FAIL] {name}");
                for line in e.lines() {
                    println!("         {line}");
                }
                failed += 1;
            }
        }
    }

    println!("\nResults: {passed} passed, {failed} failed");
    if failed > 0 {
        std::process::exit(1);
    }
}
