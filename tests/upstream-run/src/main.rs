//! Run a single R file through the rport interpreter, mimicking
//! `Rscript --vanilla <file>` closely enough for differential testing:
//!
//! - stdout receives R-style display output (partial output is preserved
//!   even when the script aborts with an error);
//! - the first top-level error is reported on stderr as `Error: ...`;
//! - exit code 0 on clean completion, 1 on R-level error, 2 on harness
//!   failure (unreadable file, bad usage).

use std::io::Write;
use std::process::ExitCode;

use rmath::android::{RSession, RValue};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: rport-upstream-run <file.R>");
        return ExitCode::from(2);
    }
    let path = &args[1];
    let code = match std::fs::read_to_string(path) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("rport-upstream-run: cannot read {path}: {err}");
            return ExitCode::from(2);
        }
    };

    let mut session = RSession::new();
    session.enable_host_process_capabilities();

    let result = session.eval_script(&code);
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(result.output.as_bytes());
    let _ = stdout.flush();

    if let RValue::Error(message) = &result.typed {
        eprintln!("Error: {message}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
