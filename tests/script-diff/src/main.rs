use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn normalize_output(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn run_rscript(script: &str) -> Result<String, String> {
    let rscript = std::env::var("RSCRIPT").unwrap_or_else(|_| "Rscript".to_string());
    let output = Command::new(&rscript)
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|err| format!("failed to launch {rscript}: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "Rscript failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(normalize_output(&String::from_utf8_lossy(&output.stdout)))
}

fn run_rport(script: &str) -> Result<String, String> {
    let mut session = r_embed::RSession::new().map_err(|err| err.to_string())?;
    session.enable_host_process_capabilities();
    session
        .eval_script(script)
        .map(|result| normalize_output(&result.output))
        .map_err(|err| err.to_string())
}

fn case_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .expect("cases directory should exist")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "R"))
        .collect();
    files.sort();
    files
}

fn usage() {
    eprintln!("usage: rport-script-diff [--allow-skip]");
    eprintln!();
    eprintln!("Exits 0 when every case runs and matches, 1 when any case fails,");
    eprintln!("2 when any case is skipped because stock R failed to run it");
    eprintln!("(suppressed by --allow-skip), and 3 on bad usage.");
}

fn parse_args() -> Result<bool, String> {
    let mut allow_skip = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--allow-skip" => allow_skip = true,
            "--help" | "-h" => {
                usage();
                std::process::exit(0);
            }
            other => return Err(other.to_string()),
        }
    }
    Ok(allow_skip)
}

fn main() {
    let allow_skip = match parse_args() {
        Ok(allow_skip) => allow_skip,
        Err(arg) => {
            usage();
            eprintln!("error: unknown argument: {arg}");
            std::process::exit(3);
        }
    };
    let cases_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("cases");
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;

    for path in case_files(&cases_dir) {
        let name = path.file_name().unwrap().to_string_lossy();
        let script = fs::read_to_string(&path).expect("case should be readable");

        let r_out = match run_rscript(&script) {
            Ok(output) => output,
            Err(err) => {
                eprintln!("SKIP {name}: {err}");
                skipped += 1;
                continue;
            }
        };

        let rport_out = match run_rport(&script) {
            Ok(output) => output,
            Err(err) => {
                eprintln!("FAIL {name}: rport error: {err}");
                failed += 1;
                continue;
            }
        };

        if r_out == rport_out {
            println!("PASS {name}");
            passed += 1;
        } else {
            eprintln!("FAIL {name}");
            eprintln!("  Rscript:\n{r_out}");
            eprintln!("  rport:\n{rport_out}");
            failed += 1;
        }
    }

    println!("script-diff: {passed} passed, {failed} failed, {skipped} skipped");
    if failed > 0 {
        std::process::exit(1);
    }
    if skipped > 0 && !allow_skip {
        eprintln!(
            "script-diff: {skipped} case(s) skipped because stock R failed to run them; \
             pass --allow-skip to tolerate"
        );
        std::process::exit(2);
    }
}
