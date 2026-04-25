//! Per-session evaluator resource limits.

use std::os::raw::c_int;
use std::time::{Duration, Instant};

use crate::sexp::globals::{R_EvalDepth, set_R_EvalDepth};
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::object::Sexp;

use super::error::EvalError;

/// Limits for expression evaluation to prevent runaway computation.
///
/// A limit of `0` means unlimited for that dimension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EvalLimits {
    /// Maximum evaluation recursion depth (0 = default of 500).
    pub max_eval_depth: usize,
    /// Maximum execution time in milliseconds (0 = unlimited).
    pub max_execution_time_ms: u64,
    /// Maximum total allocations in bytes during evaluation (0 = unlimited).
    pub max_alloc_bytes: usize,
}

impl EvalLimits {
    /// Default limits matching historic R behavior.
    pub const fn default() -> Self {
        EvalLimits {
            max_eval_depth: 500,
            max_execution_time_ms: 0,
            max_alloc_bytes: 0,
        }
    }

    /// No limits at all.
    pub const fn none() -> Self {
        EvalLimits {
            max_eval_depth: 0,
            max_execution_time_ms: 0,
            max_alloc_bytes: 0,
        }
    }
}

/// Set evaluation limits for the current thread.
pub fn set_eval_limits(limits: EvalLimits) {
    with_required_current_instance(|inst| inst.eval_state.limits = limits);
}

/// Get the current evaluation limits for this thread.
pub fn get_eval_limits() -> EvalLimits {
    with_required_current_instance(|inst| inst.eval_state.limits)
}

/// Reset evaluation limits to the default (500 depth, unlimited time/alloc).
pub fn reset_eval_limits() {
    set_eval_limits(EvalLimits::default());
}

pub struct EvalTimerGuard {
    started: bool,
}

impl EvalTimerGuard {
    pub fn start_if_needed() -> Self {
        let started = with_required_current_instance(|inst| {
            if inst.eval_state.start_time.is_some() {
                false
            } else {
                inst.eval_state.start_time = Some(Instant::now());
                true
            }
        });
        EvalTimerGuard { started }
    }
}

impl Drop for EvalTimerGuard {
    fn drop(&mut self) {
        if self.started {
            with_required_current_instance(|inst| inst.eval_state.start_time = None);
        }
    }
}

/// Depth guard that decrements R_EvalDepth when dropped.
pub struct DepthGuard(c_int);

impl Drop for DepthGuard {
    fn drop(&mut self) {
        unsafe { set_R_EvalDepth(self.0 - 1) };
    }
}

/// Check evaluation depth and time limits, returning a guard that decrements on drop.
pub fn check_eval_depth() -> Result<DepthGuard, String> {
    let limits = get_eval_limits();
    let depth = unsafe { R_EvalDepth() } + 1;
    let max_depth = if limits.max_eval_depth > 0 {
        limits.max_eval_depth
    } else {
        500
    };
    if depth as usize > max_depth {
        return Err(EvalError::TooDeeplyNested.to_string());
    }

    if limits.max_execution_time_ms > 0 {
        let elapsed = with_required_current_instance(|inst| {
            inst.eval_state.start_time.map(|start| start.elapsed())
        });
        if let Some(elapsed) = elapsed {
            if elapsed > Duration::from_millis(limits.max_execution_time_ms) {
                return Err(EvalError::TimeLimitExceeded.to_string());
            }
        }
    }

    unsafe { set_R_EvalDepth(depth) };
    Ok(DepthGuard(depth))
}

/// Evaluate an R expression with custom limits.
///
/// Sets the thread-local evaluation limits for the duration of this call, then
/// restores the previous limits afterward.
pub fn eval_with_limits<'a>(
    expr: Sexp<'a>,
    env: Sexp<'a>,
    limits: EvalLimits,
) -> Result<Sexp<'a>, String> {
    let previous = get_eval_limits();
    set_eval_limits(limits);
    with_required_current_instance(|inst| inst.eval_state.start_time = Some(Instant::now()));
    let result = super::eval::eval_safe(expr, env);
    with_required_current_instance(|inst| inst.eval_state.start_time = None);
    set_eval_limits(previous);
    result
}
