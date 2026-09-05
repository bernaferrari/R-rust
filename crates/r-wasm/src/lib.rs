//! `wasm-bindgen` session boundary over [`r_embed::RSession`] (M2).
//!
//! This crate is the browser/Node entry point to the R interpreter:
//! [`WasmRSession`] exposes `eval` (display output), `is_input_complete`
//! (continuation-prompt probe), `global_binding_names` (owned snapshot), and
//! `close`. No UniFFI, no async runtime, and no raw `SEXP` crosses the
//! boundary — every value is an owned Rust/JS string.
//!
//! Console wiring: output capture is session-owned inside `r-embed` (the same
//! channel the native oracle test uses), so `eval` returns exactly the text a
//! native embed sees; no JS console callbacks are required.
//!
//! Native `cargo check -p r-wasm` type-checks the same session code through a
//! cfg-gated non-bindgen shim (`JsError` stand-in), since `wasm-bindgen` is a
//! wasm32-only dependency here.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(not(target_arch = "wasm32"))]
mod native_shim {
    //! Stand-in for `wasm_bindgen::JsError` so the crate type-checks on
    //! native targets (see the crate docs).

    #[derive(Debug, Clone)]
    pub struct JsError(pub String);

    impl std::fmt::Display for JsError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for JsError {}

    impl JsError {
        pub fn new(message: &str) -> Self {
            JsError(message.into())
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
use native_shim::JsError;

/// A wasm-boundary R session.
///
/// Thin wrapper around [`r_embed::RSession`]; one instance owns one isolated
/// interpreter (arena, environments, RNG, output capture). Create it on the
/// JS side with `new WasmRSession()`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct WasmRSession {
    inner: r_embed::RSession,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl WasmRSession {
    /// Create a session.
    ///
    /// Throws a `JsError` when interpreter initialization fails.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new() -> Result<Self, JsError> {
        let inner = r_embed::RSession::new().map_err(|e| JsError::new(&e.to_string()))?;
        Ok(WasmRSession { inner })
    }

    /// Evaluate R code and return its display output.
    ///
    /// The string is the same text a native embed sees (printed output, then
    /// the auto-printed value of the final visible expression). A failed
    /// evaluation returns the rendered error text as the string.
    pub fn eval(&mut self, code: &str) -> String {
        match self.inner.eval(code) {
            Ok(output) => output,
            Err(e) => render_error(e),
        }
    }

    /// Report whether `code` is syntactically complete R input.
    ///
    /// Incomplete input (`f <- function(x) {`) reports `false` so hosts show
    /// a continuation prompt. Complete-but-malformed input (a stray `)`)
    /// reports `true` and lets `eval` produce the upstream-shaped parse
    /// error. A closed or failed session reports `false`.
    pub fn is_input_complete(&mut self, code: &str) -> bool {
        self.inner.is_input_complete(code).unwrap_or(false)
    }

    /// Snapshot the global environment's binding names.
    ///
    /// Owned strings, sorted, `ls(all.names = TRUE)` semantics minus the
    /// engine-internal handle environment.
    pub fn global_binding_names(&mut self) -> Vec<String> {
        self.inner.global_binding_names().unwrap_or_default()
    }

    /// Close the session and release its interpreter resources.
    pub fn close(&mut self) {
        self.inner.close();
    }
}

/// Render an evaluation error the way the console would.
fn render_error(e: r_embed::RSessionError) -> String {
    let text = e.to_string();
    match text.strip_prefix("Evaluation error: ") {
        Some(inner) if inner.starts_with("Error") => inner.to_string(),
        _ => format!("Error: {text}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The native oracle the wasm boundary must satisfy (docs/web-architecture.md).
    #[test]
    fn wasm_m3_oracle_shape() {
        let mut session = WasmRSession::new().expect("session initializes");
        let out = session.eval("1+1");
        assert_eq!(out, "[1] 2");
        assert!(session.is_input_complete("1 + 1"));
        assert!(!session.is_input_complete("f <- function(x) {"));
        session.close();
    }

    #[test]
    fn global_binding_names_snapshot_is_owned_and_sorted() {
        let mut session = WasmRSession::new().expect("session initializes");
        session.eval("zzz <- 1; aaa <- 2");
        let names = session.global_binding_names();
        let pos_zzz = names.iter().position(|n| n == "zzz");
        let pos_aaa = names.iter().position(|n| n == "aaa");
        assert!(pos_zzz.is_some() && pos_aaa.is_some());
        assert!(pos_aaa.unwrap() < pos_zzz.unwrap(), "names are sorted");
        assert!(!names.iter().any(|n| n == "..rport_handles.."));
        session.close();
    }
}
