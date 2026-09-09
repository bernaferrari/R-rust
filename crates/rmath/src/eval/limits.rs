//! Per-session evaluator resource limits.

use std::os::raw::c_int;
use std::time::{Duration, Instant};

use crate::sexp::instance::{RInstance, with_required_current_instance};
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
    with_required_current_instance(|inst| unsafe { (*inst).eval_state.limits = limits });
}

/// Get the current evaluation limits for this thread.
pub fn get_eval_limits() -> EvalLimits {
    with_required_current_instance(|inst| unsafe { (*inst).eval_state.limits })
}

/// Reset evaluation limits to the default (500 depth, unlimited time/alloc).
pub fn reset_eval_limits() {
    set_eval_limits(EvalLimits::default());
}

pub struct EvalTimerGuard {
    started: bool,
    instance: *mut RInstance,
}

impl EvalTimerGuard {
    pub fn start_if_needed() -> Self {
        let (started, instance) = with_required_current_instance(|inst| unsafe {
            if (*inst).eval_state.start_time.is_some() {
                (false, inst)
            } else {
                // wasm32-unknown-unknown has no monotonic clock
                // (`Instant::now` panics): the wall-time execution limit is
                // inert there, like upstream R without setTimeLimit.
                #[cfg(target_arch = "wasm32")]
                let start = None;
                #[cfg(not(target_arch = "wasm32"))]
                let start = Some(Instant::now());
                (*inst).eval_state.start_time = start;
                (true, inst)
            }
        });
        EvalTimerGuard { started, instance }
    }
}

impl Drop for EvalTimerGuard {
    fn drop(&mut self) {
        if self.started {
            // The timer must be cleared from the same session that started it.
            // During unwinding or nested evaluation another compatibility
            // instance may be current, so do not dispatch through TLS here.
            unsafe {
                (*self.instance).eval_state.start_time = None;
            }
        }
    }
}

struct EvalLimitsOverrideGuard {
    instance: *mut RInstance,
    previous_limits: EvalLimits,
    previous_start_time: Option<Instant>,
}

impl EvalLimitsOverrideGuard {
    fn install(limits: EvalLimits) -> Self {
        with_required_current_instance(|inst| unsafe {
            let guard = EvalLimitsOverrideGuard {
                instance: inst,
                previous_limits: (*inst).eval_state.limits,
                previous_start_time: (*inst).eval_state.start_time,
            };
            #[cfg(target_arch = "wasm32")]
            let start = None;
            #[cfg(not(target_arch = "wasm32"))]
            let start = Some(Instant::now());
            (*inst).eval_state.start_time = start;
            guard
        })
    }
}

impl Drop for EvalLimitsOverrideGuard {
    fn drop(&mut self) {
        // Restore the exact session that installed the override, independent
        // of whichever compatibility instance is current at drop time.
        unsafe {
            (*self.instance).eval_state.limits = self.previous_limits;
            (*self.instance).eval_state.start_time = self.previous_start_time;
        }
    }
}

/// Depth guard that decrements R_EvalDepth when dropped.
pub struct DepthGuard {
    instance: *mut RInstance,
    depth: c_int,
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        // Match the decrement to the exact instance whose depth was
        // incremented. This keeps cleanup correct even if another session is
        // ambient when the guard drops.
        unsafe {
            (*self.instance).eval_state.eval_depth = self.depth - 1;
        }
    }
}

/// Check evaluation depth and time limits, returning a guard that decrements on drop.
pub fn check_eval_depth() -> Result<DepthGuard, String> {
    let (instance, limits, depth, elapsed) = with_required_current_instance(|inst| unsafe {
        (
            inst,
            (*inst).eval_state.limits,
            (*inst).eval_state.eval_depth + 1,
            (*inst).eval_state.start_time.map(|start| start.elapsed()),
        )
    });
    let max_depth = if limits.max_eval_depth > 0 {
        limits.max_eval_depth
    } else {
        500
    };
    if depth as usize > max_depth {
        return Err(EvalError::TooDeeplyNested.to_string());
    }

    if limits.max_execution_time_ms > 0 {
        if let Some(elapsed) = elapsed {
            if elapsed > Duration::from_millis(limits.max_execution_time_ms) {
                return Err(EvalError::TimeLimitExceeded.to_string());
            }
        }
    }

    unsafe {
        (*instance).eval_state.eval_depth = depth;
    }
    Ok(DepthGuard { instance, depth })
}

/// Cooperative checkpoint for long-running native computations. This checks
/// elapsed time without creating an evaluator frame; standalone kernels with
/// no active session retain their ordinary unrestricted behavior.
pub(crate) fn poll_computation() {
    crate::sexp::instance::check_cancellation();
    let expired = crate::sexp::instance::with_current_instance(|instance| unsafe {
        let state = &(*instance).eval_state;
        state.limits.max_execution_time_ms > 0
            && state.start_time.is_some_and(|start| {
                start.elapsed() > Duration::from_millis(state.limits.max_execution_time_ms)
            })
    })
    .unwrap_or(false);
    if expired {
        std::panic::panic_any(crate::sexp::context::RError {
            message: EvalError::TimeLimitExceeded.to_string(),
        });
    }
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
    let _guard = EvalLimitsOverrideGuard::install(limits);
    super::eval::eval_safe(expr, env)
}
