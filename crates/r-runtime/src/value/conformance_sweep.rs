//! Golden conformance sweep over the pure world's own pipeline.
//!
//! Every case in tests/conformance/cases evaluates through
//! value::session::Session — the same parse + eval + auto-print + deferred
//! warning discipline the consumers exercise — and compares byte-exactly
//! against the pinned GNU goldens. This harness is the strict conformance
//! measure; it replaced the cross-world bridge harness once the native
//! pipeline reached exact parity with it (493/2 at the migration).
#![cfg(test)]

use std::collections::BTreeMap;

fn normalize(s: &str) -> String {
    s.lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

fn classify_error(error: &crate::value::session::SessionError) -> String {
    let error = format!("{error:?}");
    let inner = error
        .strip_prefix("Eval(")
        .unwrap_or(&error)
        .trim_end_matches(')');
    let variant = inner
        .split('(')
        .next()
        .unwrap_or("unknown")
        .trim()
        .to_string();
    let detail = inner
        .split('"')
        .nth(1)
        .map(|s| format!(":{s}"))
        .unwrap_or_default();
    format!("{variant}{detail}")
}

#[test]
#[ignore = "conformance sweep — run explicitly with --ignored --nocapture"]
fn conformance_sweep_cross_world() {
    let cases_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/conformance/cases");
    let golden_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/conformance/golden");
    let mut files: Vec<_> = std::fs::read_dir(&cases_dir)
        .expect("conformance cases directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().map(|e| e == "R").unwrap_or(false))
        .collect();
    files.sort();
    // Series split: 001-499 semantic core; 500-999 legacy regression
    // stress; 1000+ stats builtins. RPORT_SWEEP_TO overrides the ceiling.
    if std::env::var("RPORT_SWEEP_FULL").is_err() {
        let upper: u32 = std::env::var("RPORT_SWEEP_TO")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(499);
        files.retain(|path| {
            path.file_name()
                .and_then(|n| n.to_string_lossy().split('_').next().and_then(|s| s.parse::<u32>().ok()))
                .map(|n| n <= upper)
                .unwrap_or(true)
        });
    }
    if let Ok(limit) = std::env::var("RPORT_SWEEP_LIMIT") {
        files.truncate(limit.parse().unwrap_or(usize::MAX));
    }
    let mut pass = 0usize;
    let mut output_mismatch = 0usize;
    let mut no_golden = 0usize;
    let mut mismatch_cases: Vec<String> = Vec::new();
    let mut pass_cases: Vec<String> = Vec::new();
    let mut failure_reasons: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut case_counter = 0usize;
    for path in &files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        case_counter += 1;
        let stem = name.trim_end_matches(".R").to_string();
        let golden_path = golden_dir.join(format!("{stem}.out"));
        let golden = match std::fs::read_to_string(&golden_path) {
            Ok(g) => g,
            Err(_) => {
                no_golden += 1;
                continue;
            }
        };
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut session = crate::value::session::Session::new();
        match session.eval_script(&source) {
            Ok(output) => {
                if normalize(&output) == normalize(&golden) {
                    pass += 1;
                    pass_cases.push(name.clone());
                } else {
                    output_mismatch += 1;
                    mismatch_cases.push(name.clone());
                }
            }
            Err(error) => {
                failure_reasons
                    .entry(classify_error(&error))
                    .or_default()
                    .push(name.clone());
            }
        }
        if case_counter % 100 == 0 {
            println!(
                "tally@{case_counter}: pass={pass} mismatch={output_mismatch} fails={}",
                failure_reasons.values().map(|v| v.len()).sum::<usize>()
            );
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
    }
    println!("=== conformance sweep (golden comparison) ===");
    println!("pass: {pass}  output-mismatch: {output_mismatch}  no-golden: {no_golden}  parse-skip: 0");
    println!("failure reasons by frequency:");
    let mut reasons: Vec<_> = failure_reasons.iter().collect();
    reasons.sort_by_key(|(_, cases)| std::cmp::Reverse(cases.len()));
    for (reason, cases) in reasons.iter().take(18) {
        let examples: Vec<_> = cases.iter().take(3).cloned().collect();
        println!("  {:>4} {}  e.g. {}", cases.len(), reason, examples.join(", "));
    }
    println!("pass cases: {}", pass_cases.iter().take(40).cloned().collect::<Vec<_>>().join(", "));
    println!("mismatch cases: {}", mismatch_cases.iter().take(40).cloned().collect::<Vec<_>>().join(", "));
}

/// Diagnostic probe: RPORT_SWEEP_CASE=027_list_print.R prints the golden
/// and the pure world's output side by side for diff inspection.
#[test]
#[ignore = "diagnostic probe — run explicitly with RPORT_SWEEP_CASE"]
fn sweep_case_diff_probe() {
    let name = std::env::var("RPORT_SWEEP_CASE").expect("RPORT_SWEEP_CASE");
    let stem = name.trim_end_matches(".R").to_string();
    let cases_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/conformance/cases");
    let golden_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/conformance/golden");
    let source = std::fs::read_to_string(cases_dir.join(&name)).expect("case source");
    let golden = std::fs::read_to_string(golden_dir.join(format!("{stem}.out"))).expect("golden");
    let mut session = crate::value::session::Session::new();
    match session.eval_script(&source) {
        Ok(output) => {
            println!("=== golden ===\n{golden}\n=== pure ===\n{output}\n=== end ===");
        }
        Err(error) => {
            println!("=== golden ===\n{golden}\n=== pure error ===\n{error:?}\n=== end ===");
        }
    }
}
