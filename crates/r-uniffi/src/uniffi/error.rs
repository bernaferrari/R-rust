//! Errors surfaced across the UniFFI boundary.

/// Error type for every fallible operation on the [`crate::RSession`] surface.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum RError {
    #[error("Failed to initialize R session: {0}")]
    InitFailed(String),
    #[error("Evaluation error: {0}")]
    EvalError(String),
    #[error("Render error: {0}")]
    RenderError(String),
    #[error("Session is already closed")]
    SessionClosed,
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Internal error: {0}")]
    InternalError(String),
    #[error("Operation cancelled")]
    Cancelled,
    #[error("Session busy: {0}")]
    SessionBusy(String),
    /// The bounded command queue (capacity [`crate::uniffi::worker::QUEUE_CAPACITY`])
    /// was full: the interpreter worker is still occupied and too many
    /// operations are already pending. Callers should shed load and retry.
    #[error("Session command queue is full")]
    QueueFull,
    /// The synchronous request exceeded its deadline (`DEFAULT_REQUEST_TIMEOUT`,
    /// 120 s by default). The deadline expiry also requests cancellation of the
    /// affected operation so the worker unwinds promptly.
    #[error("Operation timed out after {after_ms} ms")]
    Timeout { after_ms: u64 },
}
