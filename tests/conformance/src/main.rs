extern crate rmath;

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let path = match env::args_os().nth(1) {
        Some(arg) => PathBuf::from(arg),
        None => {
            eprintln!("usage: rport-conformance-runner <case-file>");
            std::process::exit(2);
        }
    };

    let code = match fs::read_to_string(&path) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("failed to read case file {}: {}", path.display(), err);
            std::process::exit(2);
        }
    };

    let mut session = rmath::android::RSession::new();
    session.enable_host_process_capabilities();
    let result = session.eval(&code);

    // Mirror Rscript: an uncaught error prints the composed output (prior
    // prints plus the rendered "Error in <call> : ..." text) to stderr and
    // exits non-zero; the error text may not be the first line of the
    // output, so key off the typed result, not the output prefix.
    if matches!(result.typed, rmath::android::RValue::Error(_)) {
        eprintln!("{}", result.output);
        std::process::exit(1);
    }

    println!("{}", result.output);
}
