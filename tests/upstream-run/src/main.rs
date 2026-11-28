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

use r_runtime::host::pure_session::PureSession;

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
    if std::env::var_os("TZ").is_none() {
        unsafe {
            std::env::set_var("TZ", "UTC");
        }
    }
    if std::env::var_os("COPYFILE_DISABLE").is_none() {
        unsafe {
            std::env::set_var("COPYFILE_DISABLE", "1");
        }
    }
    if std::env::var_os("SRCDIR").is_none() {
        if let Some(dir) = std::path::Path::new(path).parent() {
            if let Some(dir) = dir.to_str() {
                if !dir.is_empty() {
                    unsafe {
                        std::env::set_var("SRCDIR", dir);
                    }
                }
            }
        }
    }


    // GNU evaluates with generous expression-stack headroom; the macOS
    // main thread's 8MB cannot hold the interpreter's deep recursion, so
    // evaluation runs on a worker thread with a large explicit stack —
    // the same headroom the conformance harness arranges through
    // RUST_MIN_STACK for its own threads.
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::Builder::new()
        .stack_size(1024 * 1024 * 1024)
        .spawn(move || {
            let mut session = PureSession::new();
            let result = session.eval_output(&code);
            let _ = tx.send(result);
        })
        .expect("spawn evaluation worker");
    let outcome = rx.recv().expect("evaluation worker result");
    worker.join().expect("evaluation worker join");
    match outcome {
        Ok(output) => {
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(output.as_bytes());
            let _ = stdout.flush();
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("Error: {message}");
            ExitCode::from(1)
        }
    }
}
