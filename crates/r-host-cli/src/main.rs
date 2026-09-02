//! Desktop CLI/REPL host for the R interpreter.
//!
//! Interactive features: multiline expression continuation (`+` prompt),
//! in-memory command history exposed to R via `history()`, and Ctrl-C
//! handling (clear the pending line at the prompt, cancel a running
//! evaluation).

use anyhow::Result;
use r_embed::CancellationToken;
use std::io::{self, Write};
use std::sync::{
    LazyLock,
    atomic::{AtomicBool, Ordering},
};
/// Set when SIGINT has been delivered and not yet consumed by the REPL loop.
static SIGINT_FLAG: AtomicBool = AtomicBool::new(false);
/// True while user code is being evaluated, so a SIGINT cancels the
/// evaluation instead of redrawing the prompt.
static EVAL_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
/// The token passed to the running evaluation, observed by the signal handler.
static EVAL_TOKEN: LazyLock<CancellationToken> = LazyLock::new(CancellationToken::new);
extern "C" fn handle_sigint(_sig: i32) {
    SIGINT_FLAG.store(true, Ordering::Relaxed);
    if EVAL_IN_PROGRESS.load(Ordering::Relaxed) {
        // Cooperatively cancel the running evaluation; the evaluator observes
        // the token at its next safe point.
        EVAL_TOKEN.cancel();
    } else {
        // At the prompt: abandon the pending input and draw a fresh prompt.
        // Only async-signal-safe calls here; write(2) qualifies, stdio does not.
        // The tty driver has already flushed the pending input queue.
        let prompt = b"\n> ";
        unsafe {
            libc::write(1, prompt.as_ptr().cast(), prompt.len());
        }
    }
}

fn install_sigint_handler() {
    unsafe {
        libc::signal(
            libc::SIGINT,
            handle_sigint as *const () as libc::sighandler_t,
        );
    }
}

enum Input {
    Line(String),
    /// SIGINT while blocked at the prompt; the handler already drew a fresh
    /// prompt line.
    Interrupted,
    Eof,
}

fn read_input() -> io::Result<Input> {
    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(0) => Ok(Input::Eof),
        Ok(_) => Ok(Input::Line(line)),
        Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(Input::Interrupted),
        Err(e) => Err(e),
    }
}

/// Render `s` as an R string literal. The port's lexer understands only
/// `\\n`, `\\t`, `\\r`, `\\\\` and the quote escapes, so stick to those.
fn r_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn main() -> Result<()> {
    println!("rport R interpreter (desktop host)");
    println!("Type R expressions. Enter 'q()' or Ctrl-D to quit.");
    println!("Ctrl-C clears the current line or cancels the running evaluation.\n");

    install_sigint_handler();

    let mut session = r_embed::RSession::new().map_err(|e| anyhow::anyhow!(e))?;
    session.enable_host_process_capabilities();

    // Backing store for `history()`: the Rust-side list is authoritative and
    // mirrored into the session after every executed command.
    let mut history: Vec<String> = Vec::new();
    session.eval(".rport.history <- character(0)\nhistory <- function() .rport.history")?;

    let mut line_num = 1;
    let mut pending = String::new();

    loop {
        if pending.is_empty() {
            print!("[{line_num}]> ");
        } else {
            print!("+ ");
        }
        io::stdout().flush()?;
        SIGINT_FLAG.store(false, Ordering::Relaxed);

        let line = match read_input()? {
            Input::Eof => {
                println!();
                break;
            }
            Input::Interrupted => {
                pending.clear();
                continue;
            }
            Input::Line(line) => line,
        };

        // A SIGINT while blocked at the prompt already redrew the prompt and
        // the tty flushed the queued input, so the line read here (if any) was
        // typed afterwards; process it as fresh input.
        if SIGINT_FLAG.swap(false, Ordering::Relaxed) {
            pending.clear();
            if line.trim().is_empty() {
                continue;
            }
        }

        let line = line.strip_suffix('\n').unwrap_or(&line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if pending.is_empty() && line.trim().is_empty() {
            continue;
        }
        if !pending.is_empty() {
            pending.push('\n');
        }
        pending.push_str(line);

        match session.is_input_complete(&pending) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                eprintln!("Error: {e}");
                pending.clear();
                continue;
            }
        }

        let command = std::mem::take(&mut pending);
        let trimmed = command.trim();
        if trimmed == "q()" || trimmed == "quit()" {
            break;
        }

        EVAL_TOKEN.reset();
        EVAL_IN_PROGRESS.store(true, Ordering::Relaxed);
        let result = session.eval_result_cancellable(&command, &EVAL_TOKEN);
        EVAL_IN_PROGRESS.store(false, Ordering::Relaxed);

        match result {
            Ok(output) => {
                if !output.output.is_empty() {
                    println!("{}", output.output);
                }
            }
            Err(e) => {
                eprintln!("Error: {e}");
            }
        }

        history.push(command);
        let update = format!(
            ".rport.history <- c(.rport.history, {})",
            r_string_literal(history.last().expect("just pushed"))
        );
        if session.eval(&update).is_err() {
            eprintln!("Error: failed to record command in history");
        }

        line_num += 1;
    }

    session.close();
    Ok(())
}
