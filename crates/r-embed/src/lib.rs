//! R interpreter embedding library.
//!
//! Provides the `RSession` type for embedding the R interpreter
//! into Rust applications. Uses the rmath crate as the backend
//! interpreter.

use std::ffi::CString;
use std::ptr;

use r_device_android_headless::AndroidHeadlessRenderer;
use r_graphics_engine::{Color, RenderPlot};

use rmath::sexp::ffi::{SEXP, SEXPTYPE};
use rmath::sexp::globals::{
    R_GlobalEnv, R_NilValue, set_R_BaseEnv, set_R_EmptyEnv, set_R_GlobalEnv,
};
use rmath::sexp::memory::RArena;
use rmath::sexp::output::{print_value, start_capture, stop_capture};
use rmath::sexp::safe::Sexp;

use thiserror::Error;

/// Errors that can occur during R session operations.
#[derive(Debug, Error)]
pub enum RSessionError {
    #[error("Failed to initialize R session: {0}")]
    InitFailed(String),
    #[error("Evaluation error: {0}")]
    EvalError(String),
    #[error("Render error: {0}")]
    RenderError(String),
}

/// An embedded R session.
///
/// This provides a handle to an R interpreter instance that can
/// evaluate expressions and render plots. Internally uses the rmath
/// crate for the interpreter backend.
pub struct RSession {
    active: bool,
    /// The arena allocator that owns all SEXP memory for this session.
    /// Must outlive any SEXP pointers derived from it.
    _arena: RArena,
}

impl RSession {
    /// Create a new R session.
    ///
    /// Initializes the rmath interpreter's global environment if it
    /// hasn't been set up yet.
    pub fn new() -> Result<Self, RSessionError> {
        let mut arena = RArena::new();

        // Initialize rmath's global environment hierarchy if needed.
        // R_EmptyEnv -> R_BaseEnv -> R_GlobalEnv
        unsafe {
            if R_GlobalEnv().is_null() {
                let global_env = arena.alloc_node(SEXPTYPE::ENVSXP);
                let base_env = arena.alloc_node(SEXPTYPE::ENVSXP);
                let empty_env = arena.alloc_node(SEXPTYPE::ENVSXP);

                if global_env.is_null() || base_env.is_null() || empty_env.is_null() {
                    return Err(RSessionError::InitFailed(
                        "failed to allocate environment nodes".into(),
                    ));
                }

                // R_EmptyEnv has no parent
                (*empty_env).data.envsxp.enclos = ptr::null_mut();
                (*empty_env).data.envsxp.frame = ptr::null_mut();
                (*empty_env).data.envsxp.hashtab = ptr::null_mut();

                // R_BaseEnv's parent is R_EmptyEnv
                (*base_env).data.envsxp.enclos = empty_env;
                (*base_env).data.envsxp.frame = ptr::null_mut();
                (*base_env).data.envsxp.hashtab = ptr::null_mut();

                // R_GlobalEnv's parent is R_BaseEnv
                (*global_env).data.envsxp.enclos = base_env;
                (*global_env).data.envsxp.frame = ptr::null_mut();
                (*global_env).data.envsxp.hashtab = ptr::null_mut();

                set_R_EmptyEnv(empty_env);
                set_R_BaseEnv(base_env);
                set_R_GlobalEnv(global_env);
            }
        }

        Ok(RSession {
            active: true,
            _arena: arena,
        })
    }

    /// Evaluate an R expression, returning the output as a string.
    ///
    /// Parses the code string using rmath's parser and evaluates it
    /// in the global environment. The result is formatted as a string
    /// using rmath's output subsystem.
    pub fn eval(&mut self, code: &str) -> Result<String, RSessionError> {
        if !self.active {
            return Err(RSessionError::EvalError("Session closed".into()));
        }

        let c_code = CString::new(code)
            .map_err(|e| RSessionError::EvalError(format!("invalid input: {e}")))?;

        let global_env = unsafe { R_GlobalEnv() };
        if global_env.is_null() {
            return Err(RSessionError::EvalError(
                "global environment not initialized".into(),
            ));
        }

        let result =
            unsafe { rmath::mainutils::gram_main::R_ParseEvalString(c_code.as_ptr(), global_env) };

        sexp_to_string(result)
    }

    /// Render an R expression as a plot, returning pixel data.
    ///
    /// Currently returns empty pixel data as the graphics engine
    /// has not yet been implemented.
    pub fn render_with_dimensions(
        &mut self,
        _code: &str,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, RSessionError> {
        if !self.active {
            return Err(RSessionError::RenderError("Session closed".into()));
        }
        let mut renderer = AndroidHeadlessRenderer::new(width, height);
        renderer.clear(Color::WHITE);
        Ok(renderer.finish())
    }

    /// Close the session.
    pub fn close(&mut self) {
        self.active = false;
    }
}

impl Default for RSession {
    fn default() -> Self {
        Self::new().expect("failed to create R session")
    }
}

impl Drop for RSession {
    fn drop(&mut self) {
        self.close();
    }
}

/// Convert a raw SEXP pointer to a human-readable string.
///
/// Uses rmath's output capture to format the value, with a fallback
/// to the Sexp Display impl.
fn sexp_to_string(sexp: SEXP) -> Result<String, RSessionError> {
    // Null or nil -> "NULL"
    if sexp.is_null() || std::ptr::eq(sexp, unsafe { R_NilValue() }) {
        return Ok("NULL".to_string());
    }

    let sexp = match Sexp::from_raw(sexp) {
        Some(s) => s,
        None => return Ok("NULL".to_string()),
    };

    // Use rmath's output capture to get a formatted representation.
    start_capture();
    print_value(sexp);
    let captured = stop_capture();

    let output = if captured.stdout.is_empty() {
        // Fallback: use the Sexp Display impl
        format!("{}", sexp)
    } else {
        captured.stdout.trim_end().to_string()
    };

    Ok(output)
}
