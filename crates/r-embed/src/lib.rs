//! R interpreter embedding library.
//!
//! Provides the `RSession` type for embedding the R interpreter
//! into Rust applications.

use thiserror::Error;

/// Errors that can occur during R session operations.
#[derive(Debug, Error)]
pub enum RSessionError {
    #[error("Failed to initialize R session")]
    InitFailed,
    #[error("Evaluation error: {0}")]
    EvalError(String),
    #[error("Render error: {0}")]
    RenderError(String),
}

/// An embedded R session.
///
/// This provides a handle to an R interpreter instance that can
/// evaluate expressions and render plots.
pub struct RSession {
    active: bool,
}

impl RSession {
    /// Create a new R session.
    pub fn new() -> Result<Self, RSessionError> {
        Ok(RSession { active: true })
    }

    /// Evaluate an R expression, returning the output as a string.
    pub fn eval(&mut self, _code: &str) -> Result<String, RSessionError> {
        if !self.active {
            return Err(RSessionError::EvalError("Session closed".into()));
        }
        // In a full implementation, this would call into rmath
        Ok(String::new())
    }

    /// Render an R expression as a plot, returning pixel data.
    pub fn render_with_dimensions(
        &mut self,
        _code: &str,
        _width: u32,
        _height: u32,
    ) -> Result<Vec<u8>, RSessionError> {
        if !self.active {
            return Err(RSessionError::RenderError("Session closed".into()));
        }
        // In a full implementation, this would call r-graphics-engine
        Ok(Vec::new())
    }

    /// Close the session.
    pub fn close(&mut self) {
        self.active = false;
    }
}

impl Drop for RSession {
    fn drop(&mut self) {
        self.close();
    }
}
