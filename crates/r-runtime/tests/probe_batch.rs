// Batch-probe conformance cases through the sweep pipeline — the epic's
// frozen failure list measured against the current tree in one pass.
#[test]
fn probe_batch() {
    let cases: Vec<String> = std::env::var("PROBE_CASES")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/conformance");
    let norm = |s: &str| s.lines().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n").trim_end().to_string();
    let mut report = String::new();
    for case in &cases {
        let source = match std::fs::read_to_string(root.join(format!("cases/{case}.R"))) {
            Ok(s) => s,
            Err(_) => {
                report.push_str(&format!("{case}: NOFILE\n"));
                continue;
            }
        };
        let golden = std::fs::read_to_string(root.join(format!("golden/{case}.out"))).unwrap_or_default();
        let source2 = source.clone();
        let golden2 = golden.clone();
        let outcome = std::panic::catch_unwind(move || {
            let mut session = r_runtime::value::session::Session::new();
            session.eval_script(&source2).map(|o| (o, golden2))
        });
        let verdict = match outcome {
            Ok(Ok((output, want))) => if norm(&output) == norm(&want) { "MATCH" } else { "DIFF" },
            Ok(Err(_)) => "ERROR",
            Err(_) => "PANIC",
        };
        report.push_str(&format!("{case}: {verdict}\n"));
    }
    std::fs::write("/tmp/probe_batch.txt", report).unwrap();
}
