//! gengc — extracted verbatim from gengc.rs.

use super::*;

/// Deferred alloc-time GC processing (see `memory::with_arena_in`).
///
/// Arena allocation methods run under a live `&mut RArena` borrow. Touching
/// instance state there would mean re-acquiring `&mut RInstance` from the
/// thread-local while the outer borrow (and the arena `&mut self` further
/// down) is still live and protected — aliasing UB under Stacked Borrows.
/// The arena therefore only records that its alloc-time hooks fired
/// (`alloc_gc_torture_ticks` / `alloc_gc_collect_requested`), and
/// `with_arena_in` feeds the recorded state through its own live instance
/// borrow into this function once the allocating closure has returned.
pub(crate) fn process_deferred_alloc_gc_in(
    instance: *mut instance::RInstance,
    torture_ticks: u32,
    collect_requested: bool,
) {
    unsafe {
        if collect_requested {
            (*instance).gc_state.gc_pending = true;
        }
        if torture_ticks > 0 {
            maybe_torture_gc_in(instance, torture_ticks);
        }
    }
}

/// gctorture/gctorture2 support: mirror of upstream `FORCE_GC`
/// (r-source/src/main/memory.c), evaluated at every arena allocation entry.
///
/// When torture is armed (`gc_force_gap > 0` via `gctorture()`/`gctorture2()`),
/// the `gc_force_wait` countdown delays the first forced collection, then
/// re-arms to `gc_force_gap` so every gap-th allocation forces a FULL
/// collection (`R_gc_internal(0)` upstream), run through the same
/// environment force-protect preamble `gc()` uses. When a collection is
/// already in progress the request defers via `gc_pending` (upstream's
/// `R_in_gc` path) instead of recursing. `ticks` is the number of deferred
/// allocation entries consumed since the last processing point; at most one
/// collection runs per call.
pub(crate) fn maybe_torture_gc_in(instance: *mut instance::RInstance, ticks: u32) {
    unsafe {
        if (*instance).memory_state.gc_force_gap <= 0 {
            // Not armed: default behavior is identical (single branch).
            return;
        }
        if (*instance).gc_state.in_progress || (*instance).memory_state.in_gc != 0 {
            // Mirrors upstream's R_in_gc deferral: don't recurse into a
            // collection from inside one; run it at the next safe point.
            (*instance).gc_state.gc_pending = true;
            return;
        }
        // FORCE_GC countdown: `--gc_force_wait` fires when it reaches zero,
        // then re-arms to `gc_force_gap`. Deferred ticks are consumed in one
        // step, firing at most one collection per processing point.
        let ticks: std::os::raw::c_int = ticks.try_into().unwrap_or(std::os::raw::c_int::MAX);
        if (*instance).memory_state.gc_force_wait > ticks {
            (*instance).memory_state.gc_force_wait -= ticks;
            return;
        }
        (*instance).memory_state.gc_force_wait = (*instance).memory_state.gc_force_gap;
        let owner = super::super::owner::OwnerToken::from_raw(instance);
        let pin = owner
            .pin()
            .and_then(|pin| pin.ok_or(super::super::object::SexpError::RootUnavailable))
            .unwrap_or_else(|failure| gc_entry_failure(failure));
        let _bindings = owned_environment_binding_values(owner)
            .unwrap_or_else(|failure| gc_entry_failure(failure));
        (*instance).gc_state.gc_pending = false;
        let _collection = crate::mainutils::memory_main::GcOperationGuard::enter(pin, true)
            .unwrap_or_else(|failure| gc_entry_failure(failure));
        run_gc_cycle_in(instance, do_torture_mark_sweep_in);
    }
}

pub(crate) fn eval_safe_point_gc_due_in(instance: *mut instance::RInstance) -> bool {
    unsafe { (*instance).gc_state.gc_pending || (*instance).arena.growth_warrants_gc() }
}

pub(crate) fn collect_environment_binding_values(instance: *mut instance::RInstance) -> Vec<SEXP> {
    unsafe {
        let mut values = Vec::new();
        let mut seen_envs = std::collections::HashSet::new();
        unsafe {
            let mut walk_env = |mut env: SEXP| {
                while !env.is_null() && seen_envs.insert(env as usize) {
                    if (*env).sxpinfo.type_of() != SEXPTYPE::ENVSXP {
                        break;
                    }
                    let mut frame = super::super::accessors::FRAME(env);
                    while !frame.is_null() && (*frame).sxpinfo.type_of() != SEXPTYPE::NILSXP {
                        values.push(frame);
                        let val = super::super::accessors::CAR(frame);
                        if !val.is_null() {
                            values.push(val);
                        }
                        frame = super::super::accessors::CDR(frame);
                    }
                    env = super::super::accessors::ENCLOS(env);
                }
            };
            for ctxt in &(*instance).context_stack {
                walk_env((*ctxt.get()).cloenv.as_raw());
            }
            walk_env((*instance).global_env);
            walk_env((*instance).base_env);
        }
        values
    }
}

/// Claim exact owning leases for the original environment binding values.
/// Root claims and header snapshots cannot invoke callbacks; only the completed
/// owning graph crosses collection, teardown or callback unwind.
pub(crate) fn owned_environment_binding_values(
    owner: super::super::owner::OwnerToken<'_>,
) -> super::super::object::SexpResult<Vec<super::super::object::Sexp<'static>>> {
    owner.require_active()?;
    collect_environment_binding_values(owner.as_ptr())
        .into_iter()
        .map(|value| owner.sexp(value)?.into_owned())
        .collect()
}

pub(crate) fn gc_entry_failure(failure: super::super::object::SexpError) -> ! {
    std::panic::panic_any(super::super::context::RError {
        message: failure.to_string(),
    });
}

/// Run an explicit collection with actual owning environment binding snapshots.
/// The leases release exactly these values on unwind or revocation; caller and
/// callback protection entries are unaffected.
pub fn collect_with_environment_protects(full: bool) -> (usize, usize) {
    instance::with_required_current_instance(|instance| unsafe {
        let owner = super::super::owner::OwnerToken::from_raw(instance);
        let _pin = owner
            .pin()
            .and_then(|pin| pin.ok_or(super::super::object::SexpError::RootUnavailable))
            .unwrap_or_else(|failure| gc_entry_failure(failure));
        let _bindings = owned_environment_binding_values(owner)
            .unwrap_or_else(|failure| gc_entry_failure(failure));
        (*instance).gc_state.gc_pending = false;
        if full {
            full_gc_in(instance)
        } else {
            minor_gc_in(instance)
        }
    })
}
