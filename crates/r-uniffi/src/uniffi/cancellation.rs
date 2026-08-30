//! Per-operation cancellation.
//!
//! # Policy
//!
//! Every operation submitted to the worker — synchronous requests and async
//! operations alike — owns a **fresh, private** [`CancellationToken`]. The
//! token is created at enqueue time, travels with the command, and is dropped
//! with the operation. There is no shared session token and no
//! reset-around-requests dance: cancelling one operation can never poison,
//! cancel, or otherwise affect any other operation (past or future).
//!
//! Cancellation entry points:
//!
//! * [`super::session::RSession::cancel`] — requests cancellation of every
//!   active (queued or running) operation.
//! * `cancel_current_operation` — compatibility alias for [`super::session::RSession::cancel`].
//! * [`super::session::RSession::cancel_operation`] — cancels one operation by id.
//!
//! Tokens are cooperative: `eval` aborts through the interpreter's
//! cancellation hook. `render` has no mid-flight hook, so it honors
//! cancellation only when the token is already cancelled before the render
//! starts (the worker checks at dequeue).

use r_embed::RSessionError;

use super::error::RError;

/// Create a fresh cancellation token for one operation.
pub(crate) fn new_token() -> r_embed::CancellationToken {
    r_embed::CancellationToken::new()
}

/// r-embed reports cooperative cancellation as an evaluation error whose
/// message is exactly `"operation cancelled"`. Match that contract here so
/// cancellation keeps mapping to [`RError::Cancelled`].
pub(crate) fn is_cancellation(err: &RSessionError) -> bool {
    err.to_string().contains("operation cancelled")
}

/// Map an `r_embed` evaluation error onto the UniFFI error surface,
/// preserving cancellation as a distinct variant.
pub(crate) fn map_eval_error(err: RSessionError) -> RError {
    if is_cancellation(&err) {
        RError::Cancelled
    } else {
        RError::EvalError(err.to_string())
    }
}
