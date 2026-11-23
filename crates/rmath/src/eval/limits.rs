//! Per-session evaluator resource limits.

use std::os::raw::c_int;
use std::time::{Duration, Instant};

use crate::sexp::instance::{RInstance, with_execution_state_in, with_required_current_instance};
use crate::sexp::object::Sexp;

use super::error::EvalError;

/// Limits for expression evaluation to prevent runaway computation.
///
/// A limit of `0` means unlimited for that dimension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EvalLimits {
    /// Maximum evaluation recursion depth (0 = R's expressions option).
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
    with_required_current_instance(|inst| unsafe {
        with_execution_state_in(inst, |state| state.limits = limits)
    });
}

/// Get the current evaluation limits for this thread.
pub fn get_eval_limits() -> EvalLimits {
    with_required_current_instance(|inst| unsafe {
        with_execution_state_in(inst, |state| state.limits)
    })
}

/// Reset evaluation limits to the default (500 depth, unlimited time/alloc).
pub fn reset_eval_limits() {
    set_eval_limits(EvalLimits::default());
}

/// Cleanup retains the sole original runtime allocation, without keeping a
/// session operational after close. Standalone raw fixtures use only their
/// availability witness and skip cleanup once that fixture has been destroyed.
struct EvaluationOwner {
    pointer: *mut RInstance,
    pin: Option<crate::sexp::owner::OwnerPin>,
    availability: crate::sexp::instance::InstanceLiveness,
}

impl EvaluationOwner {
    unsafe fn capture(pointer: *mut RInstance) -> Self {
        let pin = unsafe { (*pointer).runtime_owner.clone() }
            .map(|owner| owner.pin().expect("live evaluation owner"));
        Self {
            pointer,
            pin,
            availability: unsafe { crate::sexp::instance::instance_liveness(pointer) },
        }
    }

    fn cleanup_pointer(&self) -> Option<*mut RInstance> {
        match &self.pin {
            Some(pin) => Some(pin.as_ptr()),
            None => self.availability.is_live().then_some(self.pointer),
        }
    }
}

pub struct EvalTimerGuard {
    started: bool,
    owner: EvaluationOwner,
}

impl EvalTimerGuard {
    pub fn start_if_needed() -> Self {
        let (started, owner) = with_required_current_instance(|inst| unsafe {
            let owner = EvaluationOwner::capture(inst);
            let started = with_execution_state_in(inst, |state| {
                if state.start_time.is_some() {
                    return false;
                }
                // wasm32-unknown-unknown has no monotonic clock
                // (`Instant::now` panics): the wall-time execution limit is
                // inert there, like upstream R without setTimeLimit.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    state.start_time = Some(Instant::now());
                }
                true
            });
            (started, owner)
        });
        EvalTimerGuard { started, owner }
    }
}

impl Drop for EvalTimerGuard {
    fn drop(&mut self) {
        if self.started
            && let Some(instance) = self.owner.cleanup_pointer()
        {
            // The timer must be cleared from the same session that started it.
            // During unwinding or nested evaluation another compatibility
            // instance may be current, so do not dispatch through TLS here.
            unsafe {
                with_execution_state_in(instance, |state| state.start_time = None);
            }
        }
    }
}

struct EvalLimitsOverrideGuard {
    owner: EvaluationOwner,
    previous_limits: EvalLimits,
    previous_start_time: Option<Instant>,
}

impl EvalLimitsOverrideGuard {
    fn install(limits: EvalLimits) -> Self {
        with_required_current_instance(|inst| unsafe {
            let owner = EvaluationOwner::capture(inst);
            #[cfg(target_arch = "wasm32")]
            let start = None;
            #[cfg(not(target_arch = "wasm32"))]
            let start = Some(Instant::now());
            let (previous_limits, previous_start_time) = with_execution_state_in(inst, |state| {
                (
                    std::mem::replace(&mut state.limits, limits),
                    std::mem::replace(&mut state.start_time, start),
                )
            });
            Self {
                owner,
                previous_limits,
                previous_start_time,
            }
        })
    }
}

impl Drop for EvalLimitsOverrideGuard {
    fn drop(&mut self) {
        // Restore the exact session that installed the override, independent
        // of whichever compatibility instance is current at drop time.
        if let Some(instance) = self.owner.cleanup_pointer() {
            unsafe {
                with_execution_state_in(instance, |state| {
                    state.limits = self.previous_limits;
                    state.start_time = self.previous_start_time;
                });
            }
        }
    }
}

/// Depth guard that decrements R_EvalDepth when dropped.
pub struct DepthGuard {
    owner: EvaluationOwner,
    depth: c_int,
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        // Match the decrement to the exact instance whose depth was
        // incremented. This keeps cleanup correct even if another session is
        // ambient when the guard drops.
        if let Some(instance) = self.owner.cleanup_pointer() {
            unsafe {
                with_execution_state_in(instance, |state| state.eval_depth = self.depth - 1);
            }
        }
    }
}
fn current_call_hint() -> String {
    unsafe {
        let mut names = Vec::new();
        let mut ctx = crate::sexp::context::R_GlobalContext();
        let mut hops = 0;
        while !ctx.is_null() && hops < 8 {
            let call = (*ctx).call.as_raw();
            if !call.is_null()
                && call != crate::sexp::globals::R_NilValue()
                && crate::sexp::accessors::TYPEOF(call) == crate::sexp::ffi::SEXPTYPE::LANGSXP
            {
                let head = crate::sexp::accessors::CAR(call);
                if crate::sexp::accessors::TYPEOF(head) == crate::sexp::ffi::SEXPTYPE::SYMSXP {
                    let pname = crate::sexp::accessors::PRINTNAME(head);
                    if !pname.is_null() {
                        let chars = crate::sexp::accessors::CHAR(pname);
                        if !chars.is_null() {
                            let name = std::ffi::CStr::from_ptr(chars).to_string_lossy();
                            if names.last().map(String::as_str) != Some(name.as_ref()) {
                                names.push(name.into_owned());
                            }
                        }
                    }
                }
            }
            ctx = (*ctx).nextcontext;
            hops += 1;
        }
        if names.is_empty() {
            String::new()
        } else {
            format!(" [{}]", names.join(" -> "))
        }
    }
}

fn effective_eval_depth_limit(configured: usize, r_expressions: c_int) -> usize {
    let option_limit = if r_expressions > 0 {
        r_expressions as usize
    } else {
        5000
    };
    if configured > 0 {
        configured.min(option_limit)
    } else {
        option_limit
    }
}

/// Check evaluation depth and time limits, returning a guard that decrements on drop.
pub fn check_eval_depth() -> Result<DepthGuard, String> {
    let (owner, limits, depth, start) = with_required_current_instance(|inst| unsafe {
        let owner = EvaluationOwner::capture(inst);
        let (limits, depth, start) = with_execution_state_in(inst, |state| {
            (
                state.limits,
                state.eval_depth.checked_add(1),
                state.start_time,
            )
        });
        (owner, limits, depth, start)
    });
    let elapsed = start.map(|start| start.elapsed());
    let depth = depth.ok_or_else(|| "evaluation depth overflow".to_string())?;
    let max_depth = effective_eval_depth_limit(
        limits.max_eval_depth,
        crate::mainutils::errors::R_Expressions(),
    );
    if depth as usize > max_depth {
        let hint = { current_call_hint() };
        return Err(format!(
            "evaluation nested too deeply: infinite recursion / options(expressions=)?{hint}"
        ));
    }

    if limits.max_execution_time_ms > 0 {
        if let Some(elapsed) = elapsed {
            if elapsed > Duration::from_millis(limits.max_execution_time_ms) {
                return Err(EvalError::TimeLimitExceeded.to_string());
            }
        }
    }

    let instance = owner.cleanup_pointer().expect("live evaluation owner");
    unsafe { with_execution_state_in(instance, |state| state.eval_depth = depth) };
    Ok(DepthGuard { owner, depth })
}

#[cfg(test)]
mod depth_limit_tests {
    use super::{effective_eval_depth_limit, with_execution_state_in};

    #[test]
    fn owned_evaluation_guards_restore_closed_owner_after_callback_unwind() {
        use crate::sexp::instance::with_required_current_instance;
        use crate::sexp::session::RSession;
        use std::panic::{AssertUnwindSafe, catch_unwind};

        for inject_panic in [false, true] {
            let mut session = Some(RSession::new_for_gc_tests());
            let (pin, previous_limits, previous_depth) =
                with_required_current_instance(|instance| unsafe {
                    let pin = (*instance).runtime_owner.as_ref().unwrap().pin().unwrap();
                    let (limits, depth) =
                        with_execution_state_in(instance, |state| (state.limits, state.eval_depth));
                    (pin, limits, depth)
                });
            let requested = super::EvalLimits {
                max_eval_depth: 3,
                max_execution_time_ms: 30_000,
                max_alloc_bytes: 1024,
            };
            let result = catch_unwind(AssertUnwindSafe(|| {
                let _timer = super::EvalTimerGuard::start_if_needed();
                let _override = super::EvalLimitsOverrideGuard::install(requested);
                assert_eq!(super::get_eval_limits(), requested);
                let _depth = super::check_eval_depth().expect("bounded evaluation depth");
                assert_eq!(
                    unsafe { with_execution_state_in(pin.as_ptr(), |state| state.eval_depth) },
                    previous_depth + 1
                );
                // A user callback destroys the session while its guards remain
                // alive. Cleanup retains only the original physical allocation.
                drop(session.take());
                assert!(pin.require_live().is_err());
                if inject_panic {
                    panic!("injected evaluation callback unwind");
                }
            }));
            assert_eq!(result.is_err(), inject_panic);
            let instance = pin.as_ptr();
            unsafe {
                with_execution_state_in(instance, |state| {
                    assert_eq!(state.limits, previous_limits);
                    assert_eq!(state.eval_depth, previous_depth);
                    assert!(state.start_time.is_none());
                });
            }
        }
    }

    #[test]
    fn evaluation_guards_skip_destroyed_standalone_fixture() {
        let mut instance = Box::new(crate::sexp::instance::RInstance::new_for_gc_tests());
        let pointer = &raw mut *instance;
        unsafe { crate::sexp::instance::set_current_instance(pointer) };
        let timer = super::EvalTimerGuard::start_if_needed();
        let override_guard = super::EvalLimitsOverrideGuard::install(super::EvalLimits::none());
        let depth = super::check_eval_depth().expect("fixture evaluation depth");
        drop(instance);
        assert!(crate::sexp::instance::current_instance_ptr().is_none());
        drop(depth);
        drop(override_guard);
        drop(timer);
        crate::sexp::instance::clear_current_instance();
    }

    #[test]
    fn evaluation_depth_overflow_preserves_original_depth() {
        let _session = crate::sexp::session::RSession::new_for_gc_tests();
        crate::sexp::instance::with_required_current_instance(|instance| unsafe {
            with_execution_state_in(instance, |state| state.eval_depth = i32::MAX);
        });
        assert_eq!(
            super::check_eval_depth().err().as_deref(),
            Some("evaluation depth overflow")
        );
        crate::sexp::instance::with_required_current_instance(|instance| unsafe {
            with_execution_state_in(instance, |state| {
                assert_eq!(state.eval_depth, i32::MAX);
                state.eval_depth = 0;
            });
        });
    }

    #[test]
    fn execution_state_rejects_same_owner_reentry_and_releases_after_unwind() {
        use crate::sexp::session::RSession;
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let left = RSession::new_for_gc_tests();
        let right = RSession::new_for_gc_tests();
        let left_ptr = left.instance_ptr();
        left.with_active(|| {
            let right_depth = std::cell::Cell::new(-1);
            let result = catch_unwind(AssertUnwindSafe(|| unsafe {
                with_execution_state_in(left_ptr, |state| {
                    state.eval_depth = 3;
                    right_depth.set(right.with_active(crate::sexp::globals::R_EvalDepth));
                    crate::sexp::globals::set_R_EvalDepth(4);
                });
            }));
            assert_eq!(right_depth.get(), 0);
            assert!(result.is_err());
            assert_eq!(crate::sexp::globals::R_EvalDepth(), 3);
            crate::sexp::globals::set_R_EvalDepth(0);
        });
    }

    #[test]
    fn session_limit_cannot_be_relaxed_by_r_expressions() {
        assert_eq!(effective_eval_depth_limit(7, 5000), 7);
        assert_eq!(effective_eval_depth_limit(500, 100), 100);
        assert_eq!(effective_eval_depth_limit(0, 200), 200);
    }
}

/// Cooperative checkpoint for long-running native computations. This checks
/// elapsed time without creating an evaluator frame; standalone kernels with
/// no active session retain their ordinary unrestricted behavior.
pub(crate) fn poll_computation() {
    crate::sexp::instance::check_cancellation();
    let expired = crate::sexp::instance::with_current_instance(|instance| unsafe {
        let (limit, start) = with_execution_state_in(instance, |state| {
            (state.limits.max_execution_time_ms, state.start_time)
        });
        limit > 0 && start.is_some_and(|start| start.elapsed() > Duration::from_millis(limit))
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
    {
        /* SAFETY: caller supplies the active owner, rooted inputs and no payload loan. */
        super::eval::eval_safe(expr, env)
    }
}
