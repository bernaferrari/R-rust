//! gengc — extracted verbatim from gengc.rs.

use super::*;

// ---------------------------------------------------------------------------
// GC Callback Hooks
// ---------------------------------------------------------------------------

/// Event callbacks run on their owning session's thread. They may capture
/// ordinary thread-confined state, including checked values and Rc cells.
pub type GcCallback = Box<dyn Fn(&GcStats)>;

// Notification snapshots own shared leases; no state borrow spans callback
// reentry or owner teardown, and no cross-thread dispatch is possible.
pub(crate) type GcCallbackLease = Rc<dyn Fn(&GcStats)>;

/// Register a callback to be invoked after each outer GC notification cycle.
/// Callbacks may collect or register callbacks. New registrations participate
/// in the next notification; nested collections update statistics without
/// recursively notifying the same callbacks.
pub fn register_gc_callback(cb: GcCallback) {
    with_gc_state(|state| state.callbacks.push(Rc::from(cb)));
}

pub(crate) struct NotificationGuard(Rc<Cell<bool>>);

impl Drop for NotificationGuard {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

pub(crate) fn notify_gc_callbacks_in(owner: *mut instance::RInstance) {
    // SAFETY: collection reaches notification with a live owner. This weak
    // token detects teardown; it does not pretend to keep the allocation live.
    let liveness = unsafe { instance::instance_liveness(owner) };
    // Only owned leases and copied statistics survive callback execution.
    // Registration or nested collection can mutate GcState freely.
    let snapshot = with_gc_state_in(owner, |state| {
        if state.notifying.get() {
            None
        } else {
            Some((
                state.callbacks.clone(),
                state.stats.clone(),
                state.notifying.clone(),
            ))
        }
    });
    let Some((callbacks, stats, notifying)) = snapshot else {
        return;
    };
    notifying.set(true);
    let _notification = NotificationGuard(notifying);
    for callback in callbacks {
        if !liveness.is_live() {
            break;
        }
        // SAFETY: the checked original owner is still live. No collector field
        // or payload reference is lent while arbitrary code executes. Scoped
        // activation also detects teardown before restoring an old owner.
        unsafe { super::super::session::with_instance_active(owner, || callback(&stats)) };
    }
}

/// Every N-th evaluation-safe-point collection also runs a full collection
/// (via the same environment force-protect preamble) so old-generation
/// garbage is reclaimed without waiting for an explicit `gc()`.
pub(crate) const SAFE_POINT_FULL_COLLECTION_INTERVAL: u64 = 64;

/// Run collection at a cooperative safe point during evaluation.
///
/// Call this after loop iterations and brace-block statements complete, when
/// no SEXP values from the just-finished evaluation remain only on Rust stack.
pub fn maybe_collect_at_eval_safe_point() {
    instance::with_required_current_instance(|instance| unsafe {
        if !eval_safe_point_gc_due_in(instance) {
            return;
        }
        let owner = super::super::owner::OwnerToken::from_raw(instance);
        let _pin = owner
            .pin()
            .and_then(|pin| pin.ok_or(super::super::object::SexpError::RootUnavailable))
            .unwrap_or_else(|failure| gc_entry_failure(failure));
        let bindings = owned_environment_binding_values(owner)
            .unwrap_or_else(|failure| gc_entry_failure(failure));
        (*instance).gc_state.gc_pending = false;
        // Full passes bound old-generation garbage between explicit gc() calls.
        (*instance).gc_state.safe_point_collections =
            (*instance).gc_state.safe_point_collections.wrapping_add(1);
        if (*instance).arena.budget_pressure_warrants_full_gc()
            || (*instance).gc_state.safe_point_collections % SAFE_POINT_FULL_COLLECTION_INTERVAL
                == 0
        {
            full_gc_in(instance);
        } else {
            minor_gc_in(instance);
        }
        drop(bindings);
        // A revoked owner stops this safe-point notification. A different active
        // runtime must never receive the original owner's pending finalizers.
        if owner.require_active().is_ok() {
            run_pending_finalizers_after_collection();
        }
    });
}

/// Run finalizers whose keys died in the collection that just completed.
///
/// Sweeps only mark weak references ready
/// (`mark_finalizers_ready_for_unreachable_in`); executing them is deferred
/// to quiescent points because finalizers run arbitrary R code. Upstream
/// runs `RunFinalizers` from exactly these sites: after `R_gc`/`R_gc_lite`
/// (memory.c:3092-3102) and from the eval-loop interrupt checks
/// (eval.c:1096, bc_check_sigint eval.c:6265). The `running_finalizers`
/// guard inside `R_RunPendingFinalizers` keeps a finalizer that allocates
/// (and so re-enters a collection) from recursing.
pub(crate) fn run_pending_finalizers_after_collection() {
    // SAFETY: requires an active instance (any collection entry point
    // implies one); the guard against finalizer re-entrancy lives inside.
    unsafe {
        crate::mainutils::memory_main::R_RunPendingFinalizers();
    }
}

/// Flush a deferred collection after a top-level expression completes, then
/// run any finalizers the collection made ready (upstream runs finalizers
/// at these same between-expression quiescent points).
pub fn run_pending_gc_if_quiescent() {
    instance::with_current_instance(|inst| unsafe {
        run_pending_gc_if_quiescent_in(inst);
    });
}

/// Flush deferred GC for an explicitly supplied, currently active owner.
///
/// # Safety
/// `inst` must point to a live instance for the duration of this call. No
/// Rust borrow of it or its fields may survive GC or finalizer reentry.
pub(crate) unsafe fn run_pending_gc_if_quiescent_in(inst: *mut instance::RInstance) {
    // Finalizers still use compatibility dispatch. Check its owner before
    // touching GC state so a caller cannot collect one session and execute
    // its finalizers against another session's bindings.
    assert_eq!(
        instance::with_current_instance(|active| active),
        Some(inst),
        "deferred GC requires activation of its owning session"
    );
    // SAFETY: the explicit owner is live before collection starts.
    let liveness = unsafe { instance::instance_liveness(inst) };
    let collected = unsafe {
        // Raw place accesses: minor_gc_in reenters instance bookkeeping.
        if (*inst).eval_state.eval_depth == 0 && (*inst).gc_state.gc_pending {
            (*inst).gc_state.gc_pending = false;
            minor_gc_in(inst);
            true
        } else {
            false
        }
    };
    if collected && liveness.is_live() {
        run_pending_finalizers_after_collection();
    }
}
