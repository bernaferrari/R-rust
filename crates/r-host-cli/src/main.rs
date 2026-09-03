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

fn read_input(
    prompt: &str,
    completer: &mut dyn FnMut(&str) -> Vec<String>,
) -> io::Result<Input> {
    use std::io::Read as _;
    let stdin = io::stdin();
    let mut lock = stdin.lock();
    // Raw byte assembly (instead of `read_line`) so a Tab byte (0x09) can be
    // intercepted as a completion request. Input without Tab bytes produces
    // exactly the same `Line` values as `read_line` did.
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        match lock.read(&mut byte) {
            Ok(0) => {
                if buf.is_empty() {
                    return Ok(Input::Eof);
                }
                // Final partial line without a trailing newline (piped input).
                return Ok(Input::Line(String::from_utf8_lossy(&buf).into_owned()));
            }
            Ok(_) => {
                let byte = byte[0];
                if byte == b'\n' {
                    buf.push(byte);
                    return Ok(Input::Line(String::from_utf8_lossy(&buf).into_owned()));
                }
                if byte == b'\t' {
                    handle_tab(prompt, &mut buf, completer);
                    continue;
                }
                buf.push(byte);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => return Ok(Input::Interrupted),
            Err(e) => return Err(e),
        }
    }
}

/// Handle one Tab byte: complete the trailing identifier prefix in `buf`.
///
/// Exactly one candidate appends the remainder and redraws the line;
/// several candidates are printed below and the pending line is redrawn
/// unchanged; no match (or an empty prefix) leaves the input untouched.
fn handle_tab(
    prompt: &str,
    buf: &mut Vec<u8>,
    completer: &mut dyn FnMut(&str) -> Vec<String>,
) {
    let current = String::from_utf8_lossy(buf).into_owned();
    let prefix = completion_prefix(&current).to_owned();
    if prefix.is_empty() {
        return;
    }
    let mut candidates = completer(&prefix);
    candidates.sort();
    candidates.dedup();
    match candidates.len() {
        0 => {}
        1 => {
            let remainder = candidates[0][prefix.len()..].to_owned();
            buf.extend_from_slice(remainder.as_bytes());
            redraw_line(prompt, buf);
        }
        _ => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            let _ = out.write_all(b"\n");
            let _ = out.write_all(candidates.join("  ").as_bytes());
            let _ = out.write_all(b"\n");
            let _ = out.write_all(prompt.as_bytes());
            // Raw bytes: non-UTF8 input is echoed through, never panics.
            let _ = out.write_all(buf);
            let _ = out.flush();
        }
    }
}

/// Reprint the prompt and the pending input bytes after a completion.
fn redraw_line(prompt: &str, buf: &[u8]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = out.write_all(b"\r\x1b[K");
    let _ = out.write_all(prompt.as_bytes());
    let _ = out.write_all(buf);
    let _ = out.flush();
}

/// The trailing identifier run (`[A-Za-z0-9._]+`) of the current line, or
/// `""` when the cursor does not sit behind an identifier character.
fn completion_prefix(line: &str) -> &str {
    let mut start = line.len();
    for (idx, ch) in line.char_indices().rev() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' {
            start = idx;
        } else {
            break;
        }
    }
    &line[start..]
}

/// Filter a completion pool down to the sorted, deduplicated names starting
/// with `prefix`. An empty prefix matches nothing so a bare Tab never dumps
/// the whole environment.
fn filter_completions(prefix: &str, pool: &[String]) -> Vec<String> {
    if prefix.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = pool
        .iter()
        .filter(|name| name.starts_with(prefix))
        .cloned()
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Static base builtins and keywords used for completion alongside the live
/// global-environment bindings.
const STATIC_COMPLETIONS: &[&str] = &[
    "c", "list", "names", "length", "lengths", "print", "paste", "paste0", "cat",
    "sprintf", "nchar", "substr", "substring", "strsplit", "gsub", "sub", "grep",
    "grepl", "regexpr", "tolower", "toupper", "trimws", "as.character", "as.numeric",
    "as.integer", "as.logical", "as.vector", "as.list", "as.data.frame", "data.frame",
    "matrix", "array", "vector", "factor", "levels", "table", "dim", "nrow", "ncol",
    "rownames", "colnames", "dimnames", "rbind", "cbind", "t", "diag", "mean", "sum",
    "prod", "min", "max", "range", "var", "sd", "median", "quantile", "sort", "order",
    "rank", "unique", "duplicated", "which", "any", "all", "cumsum", "cumprod",
    "cummax", "cummin", "diff", "rev", "seq", "rep", "seq_len", "seq_along", "sapply",
    "lapply", "apply", "tapply", "mapply", "vapply", "do.call", "eval", "parse",
    "quote", "substitute", "expression", "call", "as.call", "identical", "all.equal",
    "class", "unclass", "typeof", "mode", "storage.mode", "attributes", "attr",
    "is.na", "is.null", "is.numeric", "is.character", "is.logical", "is.list",
    "is.vector", "is.data.frame", "is.factor", "is.matrix", "na.omit", "na.exclude",
    "complete.cases", "head", "tail", "str", "summary", "plot", "lines", "points",
    "hist", "boxplot", "barplot", "ls", "rm", "exists", "get", "assign", "remove",
    "environment", "parent.env", "new.env", "search", "library", "require", "source",
    "file.exists", "list.files", "read.csv", "write.csv", "read.table", "write.table",
    "save", "load", "q", "quit", "history", "help", "options", "setwd", "getwd",
    "system", "file", "readLines", "writeLines", "message", "warning", "stop",
    "tryCatch", "invisible", "return", "on.exit", "missing", "match.arg", "match.call",
    "formals", "args", "body", "if", "else", "repeat", "while", "function", "for",
    "in", "next", "break", "TRUE", "FALSE", "NULL", "NA", "NA_integer_", "NA_real_",
    "NA_complex_", "NA_character_", "Inf", "NaN",
];

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
        let prompt = if pending.is_empty() {
            format!("[{line_num}]> ")
        } else {
            "+ ".to_string()
        };
        print!("{prompt}");
        io::stdout().flush()?;
        SIGINT_FLAG.store(false, Ordering::Relaxed);

        let line = match read_input(&prompt, &mut |prefix: &str| {
            let mut pool = session.global_binding_names();
            pool.extend(STATIC_COMPLETIONS.iter().map(|s| s.to_string()));
            filter_completions(prefix, &pool)
        })? {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_prefix_takes_trailing_identifier_run() {
        assert_eq!(completion_prefix("prin"), "prin");
        assert_eq!(completion_prefix("x <- my.va_r1"), "my.va_r1");
        assert_eq!(completion_prefix("paste(a, b"), "b");
        assert_eq!(completion_prefix(""), "");
        assert_eq!(completion_prefix("foo("), "");
        assert_eq!(completion_prefix("a + "), "");
    }

    #[test]
    fn filter_completions_matches_prefix_sorted_and_deduped() {
        let pool = vec![
            "print".to_string(),
            "paste".to_string(),
            "paste0".to_string(),
            "print".to_string(),
            "length".to_string(),
        ];
        assert_eq!(
            filter_completions("pa", &pool),
            vec!["paste".to_string(), "paste0".to_string()]
        );
        // Exact full name still matches itself.
        assert_eq!(
            filter_completions("print", &pool),
            vec!["print".to_string()]
        );
        assert!(filter_completions("zzz", &pool).is_empty());
        // Bare Tab never dumps the whole environment.
        assert!(filter_completions("", &pool).is_empty());
    }
}
