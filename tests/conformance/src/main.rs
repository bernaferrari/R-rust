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

    if result.output.starts_with("Error:") {
        eprintln!("{}", result.output);
        std::process::exit(1);
    }

    println!("{}", result.output);
}
