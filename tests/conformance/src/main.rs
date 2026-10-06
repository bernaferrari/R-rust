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

    let mut session = match env::var("RPORT_RUNTIME_PACKAGE_POLICY").as_deref() {
        Ok("portable") => rmath::android::RSession::new_with_path_policy(
            rmath::android::RuntimePathPolicy::new(Vec::new(), env::temp_dir()),
        ),
        Ok("native") | Err(_) => rmath::android::RSession::new(),
        Ok(other) => {
            eprintln!("invalid runtime package policy: {other}");
            std::process::exit(2);
        }
    };
    session.enable_host_process_capabilities();
    session.set_top_level_evaluation_mode(rmath::android::TopLevelEvaluationMode::Script);
    if let Some(receipt) = env::var_os("RPORT_RUNTIME_RECEIPT") {
        if let Err(error) = fs::write(
            receipt,
            format!(
                "top_level_evaluation_mode: Script\n{:#?}\n",
                session.runtime_info()
            ),
        ) {
            eprintln!("failed to record initialized runtime policy: {error}");
            std::process::exit(2);
        }
    }
    #[cfg(rport_renderplot)]
    let result = {
        // A real device with bundled font metrics, not a success-only stub.
        // 504 device units at 72 dpi matches the default PDF's 7-inch extent.
        let mut device = r_graphics_engine::Scene::new(504, 504);
        session.eval_script_with_renderplot_backend(&code, &mut device)
    };
    #[cfg(not(rport_renderplot))]
    let result = session.eval(&code);

    // Mirror Rscript: an uncaught error prints the composed output (prior
    // prints plus the rendered "Error in <call> : ..." text) to stderr and
    // exits non-zero; the error text may not be the first line of the
    // output, so key off the typed result, not the output prefix.
    let failed = matches!(result.typed, rmath::android::RValue::Error(_));
    let emitted = if failed {
        io::stderr().lock().write_all(result.output.as_bytes())
    } else {
        io::stdout().lock().write_all(result.output.as_bytes())
    };
    if let Err(error) = emitted {
        eprintln!("failed to write conformance output: {error}");
        std::process::exit(2);
    }
    if failed {
        std::process::exit(1);
    }
}
