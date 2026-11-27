extern crate rmath;

use std::env;
use std::fs;
use std::io::{self, Write};
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

    // The pure-Rust Session owns parse/eval/print with no SEXP machinery —
    // the rport-c2v5f.11 runtime cutover. Mirrors Rscript: an uncaught
    // error prints the composed output to stderr and exits non-zero.
    let mut session = rmath::value::session::Session::new();
    let (output, failed) = match session.eval_script(&code) {
        Ok(output) => (output, false),
        Err(error) => (format!("Error: {error}\n"), true),
    };
    let emitted = if failed {
        io::stderr().lock().write_all(output.as_bytes())
    } else {
        io::stdout().lock().write_all(output.as_bytes())
    };
    if let Err(error) = emitted {
        eprintln!("failed to write conformance output: {error}");
        std::process::exit(2);
    }
    if failed {
        std::process::exit(1);
    }
}
