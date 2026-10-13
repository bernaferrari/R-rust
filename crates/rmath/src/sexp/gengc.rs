//! Generational garbage collector with remembered-set write barriers.
//!
//! # Design parity with upstream R (`r-source/src/main/memory.c`)
//!
//! Upstream's `R_gc_internal`/`RunGenCollect` (memory.c:1681) is a
//! *non-compacting* generational mark-sweep collector. The "GC moving
//! parity" question — whether the port needs upstream's node moving —
//! resolves to: it does not, because upstream never relocates nodes in
//! memory either.
//!
//! Upstream mechanism (memory.c):
//! - Every node lives on one intrusive circular doubly-linked list per node
//!   class: `R_GenHeap[cls].New` / `.Old[0..NUM_OLD_GENERATIONS-1]` /
//!   `.OldToNew[gen]` / `.Free` (Baker's no-motion incremental design,
//!   memory.c:594-607). "Moving" a node between generations is an
//!   `UNSNAP_NODE`/`SNAP_NODE` re-link (or a `BULK_MOVE` of a whole list);
//!   the node's address never changes and no reference to it is ever
//!   rewritten. `SortNodes` only reorders *free* nodes for locality.
//! - `sxpinfo.gcgen` is a 1-bit generation counter (`NUM_OLD_GENERATIONS`
//!   = 2 lists, gcgen 0/1, plus the unmarked `New` space).
//!   `NODE_IS_OLDER` + `CHECK_OLD_TO_NEW` form the write barrier: storing a
//!   young (unmarked or younger-generation) child into a marked (old)
//!   parent puts the parent on its per-generation `OldToNew` remembered
//!   list (memory.c:559-561, 1313-1314).
//! - Collections come in levels 0/1/2 (collect `New` only / also `Old[0]`
//!   / everything), scheduled by `LEVEL_0_FREQ`=20 and `LEVEL_1_FREQ`=5
//!   counters with immediate escalation when a level frees too little
//!   (memory.c:280-296, 1691-1698, 1983-1992). Collected generations are
//!   unmarked and bulk-relinked into `New` (survivor generation bumped
//!   while `gen < NUM_OLD_GENERATIONS - 1`); marking then re-snaps
//!   reachable nodes into `Old[NODE_GENERATION(s)]` — that relinking *is*
//!   promotion (`FORWARD_NODE`/`PROCESS_ONE_NODE`, memory.c:789-804).
//!   Unmarked nodes become the free list; empty pages are released.
//! - Weak references/finalizers: `CheckFinalizers` (inside
//!   `RunGenCollect`, memory.c:1448) marks weakrefs whose key died
//!   ready-to-finalize and keeps them alive until run; `RunFinalizers`
//!   executes them from `R_gc`/`R_gc_lite` after the collection
//!   (memory.c:3092-3102) and from the eval-loop interrupt checks
//!   (`R_RunPendingFinalizers` in eval.c:1096, bc_check_sigint eval.c:6265)
//!   — finalizers run arbitrary R code, so they fire only at quiescent
//!   points, never mid-collection.
//!
//! The port is semantically equivalent with different plumbing:
//! - Generation is the same 1-bit counter (`sxpinfo.gcgen`, `Generation`):
//!   allocation is young; surviving a minor or full cycle promotes the node
//!   to old (`promote_to_old` and the sweeps below). Upstream's extra
//!   `Old[0]`/`Old[1]` distinction is reclamation-latency tuning, not
//!   observable behavior: both designs reclaim young garbage every minor
//!   cycle and old garbage on the periodic full pass (port: every
//!   `SAFE_POINT_FULL_COLLECTION_INTERVAL`-th safe-point collection, and
//!   every explicit `gc()`; upstream: the level counters).
//! - The write barrier (`write_barrier`) records old-parent/young-child
//!   edges in `RememberedSet` — the analog of upstream's `OldToNew` lists.
//!   Minor and full collections mark remembered parents so their young
//!   children stay reachable, then clear the set; survivors are promoted by
//!   the sweep, matching upstream's aging of `OldToNew` entries.
//! - The arena is a non-moving slab with a free list; `free_node` recycles
//!   slots exactly like upstream's `Free`-pointer reset. Node addresses are
//!   stable for the node's lifetime in BOTH implementations: R's C code and
//!   this port's translated evaluator hold raw `SEXP`s in machine-stack
//!   locals across allocations, so neither collector may relocate live
//!   objects. The formerly-present relocation machinery was removed for
//!   that reason; `compact_if_needed`/`force_compact` survive only as
//!   free-list normalization hooks that never move live objects.
//! - Dead-reference rewrite to `R_NilValue` in swept survivors is a port
//!   hardening measure; upstream leaves references to freed nodes in place
//!   (safe there because free nodes are never dereferenced).
//! - Weak references and finalizers exist (`R_MakeWeakRef`,
//!   `R_WeakRefKey`, `R_RegisterFinalizer(Ex)`, `R_RegisterCFinalizer(Ex)`
//!   in `mainutils::memory_main`). Sweeps mark ready finalizers via
//!   `mark_finalizers_ready_for_unreachable_in` and pin them until run;
//!   they execute after collections at the same quiescent points upstream
//!   uses — `R_gc`/`R_gc_lite`, eval safe points, the quiescent flush, and
//!   session exit (`R_RunExitFinalizers`).
//!
//! The collector is intentionally defensive: it scopes state to the active
//! `RInstance`, catches panics at public GC entry points, and uses a
//! non-moving mark/sweep collector with free-list recycling. Raw `SEXP`
//! internals still require careful auditing; do not document new
//! invariants here unless they are enforced by code and regression tests.

#[cfg(test)]
use std::ptr;
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use super::ffi::{SEXP, SEXPTYPE};
use super::heap::NodeLink;
use super::instance;
use super::memory::{RArena, with_arena_for_gc};
use super::protect::{RootValue, update_preserve_stack_refs_in, update_protect_stack_refs_in};

#[path = "gc_trace.rs"]
mod gc_trace;
#[path = "gc_trace_bridge.rs"]
mod gc_trace_bridge;

/// Generations for object aging.

/// Generations for object aging.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Generation {
    Young = 0,
    Old = 1,
}

// ---------------------------------------------------------------------------
// GC Statistics Tracking
// ---------------------------------------------------------------------------

/// Statistics collected during garbage collection cycles.
#[derive(Debug, Clone, Default)]
pub struct GcStats {
    pub collections: usize,
    pub promoted: usize,
    pub freed: usize,
    pub total_bytes_allocated: usize,
    pub total_bytes_freed: usize,
    pub peak_memory: usize,
}

/// Get a snapshot of the current GC statistics.
pub fn get_gc_stats() -> GcStats {
    with_gc_state(|state| state.stats.clone())
}

/// Reset all GC statistics to zero.
pub fn reset_gc_stats() {
    with_gc_state(|state| state.stats = GcStats::default());
}

fn record_collection_in(state: &mut GcState, promoted: usize, freed: usize) {
    let stats = &mut state.stats;
    stats.collections += 1;
    stats.promoted += promoted;
    stats.freed += freed;
}

// ---------------------------------------------------------------------------
// GC Callback Hooks
// ---------------------------------------------------------------------------

pub struct GcState {
    pub(crate) stats: GcStats,
    pub(crate) callbacks: Vec<GcCallbackLease>,
    notifying: Rc<Cell<bool>>,
    pub(crate) in_progress: bool,
    pub(crate) remembered_set: RememberedSet,
    /// Set when allocation would trigger GC during evaluation; flushed at quiescence.
    pub(crate) gc_pending: bool,
    /// Number of collections performed at evaluation safe points. Every
    /// [`SAFE_POINT_FULL_COLLECTION_INTERVAL`]-th one is a full collection so
    /// old-generation garbage cannot accumulate unbounded between explicit
    /// `gc()` calls (safe points otherwise collect the young generation only).
    pub(crate) safe_point_collections: u64,
}

impl GcState {
    pub fn new() -> Self {
        GcState {
            stats: GcStats::default(),
            callbacks: Vec::new(),
            notifying: Rc::new(Cell::new(false)),
            in_progress: false,
            remembered_set: RememberedSet::default(),
            gc_pending: false,
            safe_point_collections: 0,
        }
    }
}

impl Default for GcState {
    fn default() -> Self {
        Self::new()
    }
}

fn with_gc_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut GcState) -> R,
{
    instance::with_required_current_instance(|instance| with_gc_state_in(instance, f))
}

fn with_gc_state_in<F, R>(instance: *mut instance::RInstance, f: F) -> R
where
    F: FnOnce(&mut GcState) -> R,
{
    // P1: the `&mut` field lend is held only across the caller's local
    // state operations; GC callbacks are invoked outside any lend.
    unsafe { f(&mut (*instance).gc_state) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod sweep_remap_tests;

#[cfg(kani)]
mod kani_proofs {
    use super::{EDGE_ATTRIB, EDGE_CAR, EDGE_CDR, EDGE_PNAME, EDGE_VECTOR, child_mask};

    #[kani::proof]
    fn child_mask_matches_roles() {
        let type_code: i32 = kani::any();
        kani::assume((0..=32).contains(&type_code));
        let follow_weak_key: bool = kani::any();
        let mask = child_mask(type_code, follow_weak_key);
        assert_ne!(mask & EDGE_ATTRIB, 0);
        let vector = matches!(type_code, 16 | 19 | 20 | 21);
        assert_eq!(mask & EDGE_VECTOR != 0, vector);
        if type_code == 23 {
            assert_eq!(mask & EDGE_CAR != 0, follow_weak_key);
            assert_ne!(mask & EDGE_CDR, 0);
        }
        if type_code == 1 {
            assert_ne!(mask & EDGE_PNAME, 0);
            assert_eq!(mask & EDGE_CAR, 0);
        }
        if type_code == 2 || type_code == 6 || type_code == 17 {
            assert_ne!(mask & EDGE_CAR, 0);
        }
        kani::cover(type_code == 23 && !follow_weak_key, "weak key unmarked");
        kani::cover(type_code == 23 && follow_weak_key, "weak key forwarded");
        kani::cover(vector, "vector payload");
    }

    #[kani::proof]
    #[kani::unwind(6)]
    fn tiny_mark_is_the_reachable_set() {
        let mut child = [[4u8; 3]; 4];
        let mut index = 0usize;
        while index < 4 {
            let mut slot = 0usize;
            while slot < 3 {
                let next: u8 = kani::any();
                kani::assume(next <= 4);
                child[index][slot] = next;
                slot += 1;
            }
            index += 1;
        }
        let root: u8 = kani::any();
        kani::assume(root < 4);
        let mut seen = [false; 4];
        let mut stack = [0u8; 4];
        let mut sp = 1usize;
        stack[0] = root;
        seen[root as usize] = true;
        while sp > 0 {
            sp -= 1;
            let node = stack[sp] as usize;
            let mut slot = 0usize;
            while slot < 3 {
                let next = child[node][slot];
                if next < 4 && !seen[next as usize] {
                    seen[next as usize] = true;
                    stack[sp] = next;
                    sp += 1;
                }
                slot += 1;
            }
        }
        index = 0;
        while index < 4 {
            if seen[index] {
                let mut slot = 0usize;
                while slot < 3 {
                    let next = child[index][slot];
                    if next < 4 {
                        assert!(seen[next as usize]);
                    }
                    slot += 1;
                }
            }
            index += 1;
        }
        assert!(seen[root as usize]);
        kani::cover(seen[0] && seen[1] && seen[2] && seen[3], "all reachable");
        kani::cover(
            seen[root as usize] && !seen[((root as usize) + 1) % 4],
            "partial",
        );
    }
}

#[cfg(test)]
#[path = "gc_notification_tests.rs"]
mod notification_tests;

#[cfg(test)]
#[path = "gc_finalizer_graph_tests.rs"]
mod finalizer_graph_tests;

#[cfg(test)]
mod pressure_tests;

mod barrier;
mod callback;
mod collect;
mod finalizers;
mod mark;
mod remembered;
mod sweep;

pub(crate) use barrier::*;
pub(crate) use callback::*;
pub(crate) use collect::*;
pub(crate) use finalizers::*;
pub(crate) use mark::*;
pub(crate) use remembered::*;
pub(crate) use sweep::*;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests_a;
#[cfg(test)]
mod tests_b;
