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

/// Event callbacks run on their owning session's thread. They may capture
/// ordinary thread-confined state, including checked values and Rc cells.
pub type GcCallback = Box<dyn Fn(&GcStats)>;
// Notification snapshots own shared leases; no state borrow spans callback
// reentry or owner teardown, and no cross-thread dispatch is possible.
type GcCallbackLease = Rc<dyn Fn(&GcStats)>;

/// Register a callback to be invoked after each outer GC notification cycle.
/// Callbacks may collect or register callbacks. New registrations participate
/// in the next notification; nested collections update statistics without
/// recursively notifying the same callbacks.
pub fn register_gc_callback(cb: GcCallback) {
    with_gc_state(|state| state.callbacks.push(Rc::from(cb)));
}

struct NotificationGuard(Rc<Cell<bool>>);
impl Drop for NotificationGuard {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

fn notify_gc_callbacks_in(owner: *mut instance::RInstance) {
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
        unsafe { super::session::with_instance_active(owner, || callback(&stats)) };
    }
}

// ---------------------------------------------------------------------------
// GC Invariants Checking
// ---------------------------------------------------------------------------

fn verify_gc_invariants_in(instance: *mut instance::RInstance) {
    unsafe {
        (*instance).legacy_protect.with_entries(|entries| {
            for &obj in entries.iter() {
                if !obj.is_null() {
                    // Debug-only: verify object is within arena bounds.
                    // Full validation would require classifying singleton roots too.
                }
            }
        });
        (*instance).root_table.with_entries(|entries| {
            for &obj in entries.iter() {
                if !obj.is_null() {
                    // As above: tombstoned slots hold null and are skipped.
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Root tracing
// ---------------------------------------------------------------------------

// Checked allocation tokens and collection epochs replace mutable header
// marks. Raw projection reads remain confined to the snapshot bridge.
const EDGE_PNAME: u32 = 1 << 0;
const EDGE_SYM_VALUE: u32 = 1 << 1;
const EDGE_INTERNAL: u32 = 1 << 2;
const EDGE_CAR: u32 = 1 << 3;
const EDGE_CDR: u32 = 1 << 4;
const EDGE_TAG: u32 = 1 << 5;
const EDGE_FORMALS: u32 = 1 << 6;
const EDGE_BODY: u32 = 1 << 7;
const EDGE_CLOENV: u32 = 1 << 8;
const EDGE_FRAME: u32 = 1 << 9;
const EDGE_ENCLOS: u32 = 1 << 10;
const EDGE_HASHTAB: u32 = 1 << 11;
const EDGE_PROM_VALUE: u32 = 1 << 12;
const EDGE_PROM_EXPR: u32 = 1 << 13;
const EDGE_PROM_ENV: u32 = 1 << 14;
const EDGE_EXT_TAG: u32 = 1 << 15;
const EDGE_EXT_PROT: u32 = 1 << 16;
const EDGE_ATTRIB: u32 = 1 << 17;
const EDGE_VECTOR: u32 = 1 << 18;
const EDGE_VECTOR_METADATA: u32 = 1 << 19;

/// Which pointer slots a node owns.
///
/// Mark and update both use this mask. Mark passes `follow_weak_key = false`
/// so a weak reference's key stays unmarked; update passes `true` so a live
/// key is still rewritten when its node is forwarded.
fn child_mask(type_code: i32, follow_weak_key: bool) -> u32 {
    let vector = if vector_payload_has_sexp_refs(SEXPTYPE(type_code)) {
        EDGE_VECTOR
    } else {
        0
    };
    let body = match type_code {
        1 => EDGE_PNAME | EDGE_SYM_VALUE | EDGE_INTERNAL,
        2 | 6 | 17 => EDGE_CAR | EDGE_CDR | EDGE_TAG,
        3 => EDGE_FORMALS | EDGE_BODY | EDGE_CLOENV,
        4 => EDGE_FRAME | EDGE_ENCLOS | EDGE_HASHTAB,
        5 => EDGE_PROM_VALUE | EDGE_PROM_EXPR | EDGE_PROM_ENV,
        22 => EDGE_EXT_TAG | EDGE_EXT_PROT,
        23 => EDGE_CDR | EDGE_TAG | if follow_weak_key { EDGE_CAR } else { 0 },
        _ => 0,
    };
    let private = if matches!(type_code, 9 | 10 | 13 | 14 | 15 | 16 | 19 | 20 | 21 | 24) {
        EDGE_VECTOR_METADATA
    } else {
        0
    };
    body | EDGE_ATTRIB | vector | private
}

#[cfg(test)]
fn each_child(obj: SEXP, follow_weak_key: bool, visit: impl FnMut(&mut NodeLink)) {
    let (_, parent) =
        super::memory::checked_projection(obj).expect("GC parent belongs to checked storage");
    gc_trace_bridge::rewrite_children(&parent, follow_weak_key, visit);
}

#[inline(always)]
fn mark_reachable(obj: SEXP) {
    if obj.is_null() {
        return;
    }
    mark_reachable_traced(obj);
}

thread_local! {
    static MARK_WHERE: std::cell::Cell<&'static str> = const { std::cell::Cell::new("unlabeled") };
}

#[inline(always)]
fn mark_reachable_traced(obj: SEXP) {
    let mut pending = gc_trace::TraceWorklist::new(gc_trace::TraceScope::active());
    trace_result(pending.enqueue(obj));
    drain_trace_worklist(pending);
}

fn trace_result<T>(result: Result<T, gc_trace::TraceError>) -> T {
    result.unwrap_or_else(|error| {
        let where_ = MARK_WHERE.with(|label| label.get());
        panic!("invalid GC graph while marking {where_}: {error:?}");
    })
}

fn drain_trace_worklist(mut pending: gc_trace::TraceWorklist) {
    // Graph depth consumes owned tasks, never the Rust call stack. Each task
    // retains an exact generation and canonical Cell projection until read.
    while let Some(node) = trace_result(pending.next_marked()) {
        // SAFETY: GC is quiescent and the owner remains live through marking.
        // Published payloads retain their native span invariants. The bridge
        // revalidates the token, copies edges, and lends no mutable fields.
        let children =
            trace_result(unsafe { gc_trace_bridge::snapshot_children(&node, pending.context()) });
        for child in children.into_edges() {
            trace_result(pending.enqueue_link(child));
        }
    }
}

#[inline(always)]
fn mark_checked_root_snapshot(roots: Vec<RootValue>) {
    let mut pending = gc_trace::TraceWorklist::new(gc_trace::TraceScope::active());
    for root in roots {
        match root {
            RootValue::Checked {
                projection,
                allocation,
            } => {
                trace_result(pending.enqueue_checked(projection, allocation));
            }
            RootValue::Static { projection } => {
                trace_result(pending.enqueue(projection));
            }
        }
    }
    drain_trace_worklist(pending);
}

fn mark_instance_roots(instance: *mut instance::RInstance) {
    unsafe {
        mark_reachable((*instance).empty_env);
        mark_reachable((*instance).base_env);
        mark_reachable((*instance).global_env);
        // Permanent owned headers include scalar/name storage that is not
        // present in the legacy environment, symbol, or cons projections.
        for node in (*instance).persistent_nodes.projections() {
            mark_reachable(node);
        }

        // Fresh results remain rooted through lend cleanup, warning dispatch,
        // deferred collection and notification callbacks. The ledger snapshot
        // owns exact allocation identities and ends its TLS borrow before trace.
        let identity = &(*instance).heap_identity;
        let fresh = super::memory::fresh_allocation_roots(identity)
            .into_iter()
            .map(|(projection, allocation)| RootValue::Checked {
                projection,
                allocation,
            })
            .collect();
        MARK_WHERE.with(|w| w.set("fresh_allocations"));
        mark_checked_root_snapshot(fresh);

        let automatic = super::memory::automatic_roots(identity)
            .into_iter()
            .map(|(projection, allocation)| RootValue::Checked {
                projection,
                allocation,
            })
            .collect();
        MARK_WHERE.with(|w| w.set("automatic_handles"));
        mark_checked_root_snapshot(automatic);

        // Every native and Rust protection channel carries original checked
        // allocation identities; raw address reuse never creates a new root.
        MARK_WHERE.with(|w| w.set("legacy_protect"));
        mark_checked_root_snapshot((*instance).legacy_protect.checked_entries_snapshot());
        MARK_WHERE.with(|w| w.set("root_table"));
        mark_checked_root_snapshot((*instance).root_table.checked_entries_snapshot());
        MARK_WHERE.with(|w| w.set("preserve_stack"));
        mark_checked_root_snapshot((*instance).preserve_stack.checked_entries_snapshot());
        // Context and bytecode values own exact-generation automatic roots.

        // Error-state values own exact-generation automatic roots.

        MARK_WHERE.with(|w| w.set("eval_state"));
        mark_reachable((*instance).eval_state.printvector.na_string);
        mark_reachable((*instance).eval_state.printvector.na_string_noquote);
        mark_reachable((*instance).eval_state.print.data.na_string);
        mark_reachable((*instance).eval_state.print.data.na_string_noquote);
        mark_reachable((*instance).eval_state.print.data.env);
        mark_reachable((*instance).eval_state.print.data.callArgs);

        MARK_WHERE.with(|w| w.set("symbols"));
        for &obj in (*instance).symbols.values() {
            mark_reachable(obj);
        }
        for &node in &(*instance).symbol_nodes {
            mark_reachable(node);
        }
        for &node in &(*instance).env_nodes {
            mark_reachable(node);
        }
        for &obj in &(*instance).names_state.ddval_symbols {
            mark_reachable(obj);
        }
        mark_reachable((*instance).bind_state.blank_string);

        // Options and cached base wrappers own exact-generation automatic roots.
        // Task callbacks own exact-generation automatic roots.
        // S4 method and inheritance caches own automatic roots as well.
        // Namespace cache values may be reachable only through the cache: a
        // pure-R package namespace has no other root once attach-time references
        // die. Untraced, a collection swept the namespace env and left a dangling
        // raw pointer in the cache.
        for &(_, namespace) in (*instance).package_namespace_cache.values() {
            mark_reachable(namespace);
        }
        mark_reachable((*instance).unwrap_methods_ns);
        for &clos in &(*instance).unwrap_methods_closures {
            mark_reachable(clos);
        }

        // Active-binding functions must live as long as their entry. The (env,
        // symbol) key addresses are deliberately not marked: bindings belong to
        // their environment, so entries whose keyed node is reclaimed this cycle
        // are swept in update_instance_roots_in instead of pinning the env.
        for value in (*instance).active_bindings.values() {
            mark_reachable(*value);
        }

        for finalizer in &(*instance).memory_state.pending_finalizers {
            if finalizer.is_ready() {
                mark_reachable(finalizer.obj());
            }
            if let crate::mainutils::memory_main::PendingFinalizer::R { fun, .. } = finalizer {
                mark_reachable(*fun);
            }
        }
        mark_reachable((*instance).dynload_state.dll_info_eptrs);
        mark_reachable((*instance).dynload_state.symbol_eptrs);
        mark_reachable((*instance).dynload_state.c_entry_table);

        #[cfg(not(target_arch = "wasm32"))]
        (*instance)
            .httpd_state
            .visit_roots(|obj| mark_reachable(*obj));

        mark_reachable((*instance).grid_runtime_state.current_grid_state);
        mark_reachable((*instance).grid_runtime_state.eval_env);

        for &obj in &(*instance).raw_cons {
            mark_reachable(obj);
        }
    }
}

// ---------------------------------------------------------------------------
// Remembered Set
// ---------------------------------------------------------------------------

/// Remembered set tracking old objects with references to young objects.
///
/// Membership lives in owned state (`members`), never in the SEXP mark bit.
/// The mark bit belongs to the collector's mark phase: setting it at barrier
/// time made the remembered-set scans in `do_minor_gc_in` /
/// `mark_from_all_roots_in` early-return on `mark_reachable`, so the young
/// children of a remembered old object were never traced and could be swept
/// while still reachable (plans/001-separate-remembered-set-membership.md).
#[derive(Default)]
pub struct RememberedSet {
    entries: Vec<(SEXP, super::heap::CheckedNode)>,
    members: HashSet<super::heap::NodeId>,
    #[cfg(test)]
    fail_next_reservation: bool,
}

impl RememberedSet {
    #[inline]
    pub fn add(&mut self, obj: SEXP) {
        if !self.try_add(obj) {
            // Legacy raw setters may already have published the pointer.
            // Continuing after a missing barrier could leave dangling edges;
            // these infallible callers follow Rust's fatal OOM contract.
            std::alloc::handle_alloc_error(std::alloc::Layout::new::<SEXP>());
        }
    }

    /// Record an edge before publishing it. On failure, membership and
    /// entries remain unchanged so the caller can reject the write.
    #[inline]
    pub(crate) fn try_add(&mut self, obj: SEXP) -> bool {
        let Some((projection, allocation)) = super::memory::checked_projection(obj) else {
            return true;
        };
        let Some(header) = super::memory::checked_snapshot(projection, &allocation) else {
            return true;
        };
        if header.sxpinfo.gcgen() == Generation::Young as u8 {
            return true;
        }
        if self.members.contains(allocation.id()) {
            return true;
        }
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_reservation) {
            return false;
        }
        // Reserve both collections before mutating either, so an allocation
        // failure cannot leave entries and membership out of sync.
        if self.entries.try_reserve(1).is_err() || self.members.try_reserve(1).is_err() {
            return false;
        }
        self.members.insert(allocation.id().clone());
        self.entries.push((projection, allocation));
        true
    }

    #[cfg(test)]
    pub(crate) fn fail_next_reservation_for_test(&mut self) {
        self.fail_next_reservation = true;
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.members.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = SEXP> + '_ {
        self.entries
            .iter()
            .filter_map(|(projection, allocation)| allocation.is_live().then_some(*projection))
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Remap entries through a relocation map (non-moving sweep redirects
    /// freed addresses to `R_NilValue`) and rebuild membership so a later
    /// barrier cannot deduplicate against a stale address.
    pub fn remap(&mut self, old_to_new: &HashMap<usize, SEXP>) {
        self.entries.retain_mut(|(projection, allocation)| {
            if !allocation.is_live() {
                return false;
            }
            if let Some(&replacement) = old_to_new.get(&(*projection as usize)) {
                let Some((canonical, current)) = super::memory::checked_projection(replacement)
                else {
                    return false;
                };
                if !allocation.same_heap(&current) {
                    return false;
                }
                *projection = canonical;
                *allocation = current;
            }
            true
        });
        self.rebuild_membership();
    }

    fn retain_live(&mut self, freed: &HashSet<usize>) {
        self.entries.retain(|(projection, allocation)| {
            allocation.is_live() && !freed.contains(&(*projection as usize))
        });
        self.rebuild_membership();
    }

    fn rebuild_membership(&mut self) {
        self.members.clear();
        self.members.extend(
            self.entries
                .iter()
                .map(|(_, allocation)| allocation.id().clone()),
        );
    }

    fn checked_roots(&self) -> Vec<RootValue> {
        self.entries
            .iter()
            .filter(|(_, allocation)| allocation.is_live())
            .map(|(projection, allocation)| RootValue::Checked {
                projection: *projection,
                allocation: allocation.clone(),
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// GC State
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
// Write Barriers
// ---------------------------------------------------------------------------

#[inline(always)]
pub fn write_barrier(parent: SEXP, child: SEXP) {
    if parent.is_null() || child.is_null() {
        return;
    }

    let Some(instance) = instance::current_instance_ptr() else {
        return;
    };
    // Inputs are address lookups only; the checked barrier copies canonical
    // cells after validating both allocation identities and their owner.
    if !unsafe { write_barrier_in(instance, parent, child) } {
        // A valid edge may have been published by an infallible setter.
        // Invalid inputs are rejected before any reservation is attempted.
        if checked_barrier_pair(instance, parent, child).is_some() {
            std::alloc::handle_alloc_error(std::alloc::Layout::new::<SEXP>());
        }
    }
}

fn checked_barrier_pair(
    instance: *mut instance::RInstance,
    parent: SEXP,
    child: SEXP,
) -> Option<(SEXP, u8, u8)> {
    let (parent, parent_id) = super::memory::checked_projection(parent)?;
    let (child, child_id) = super::memory::checked_projection(child)?;
    // SAFETY: only callers with a live owner invoke this local helper. The
    // immutable identity is independent of an outstanding arena lend.
    let identity = unsafe { &(*instance).heap_identity };
    if !parent_id.belongs_to(identity) || !child_id.belongs_to(identity) {
        return None;
    }
    let parent_header = super::memory::checked_snapshot(parent, &parent_id)?;
    let child_header = super::memory::checked_snapshot(child, &child_id)?;
    Some((
        parent,
        parent_header.sxpinfo.gcgen(),
        child_header.sxpinfo.gcgen(),
    ))
}

/// Record an old-to-young edge in the original owner's remembered set.
/// Does not activate a session, collect, or call the evaluator.
///
/// # Safety
/// `instance` is live; parent and child are live nodes owned by that instance
/// or immutable storage. No borrow of its GC state overlaps this operation.
#[inline]
pub(crate) unsafe fn write_barrier_in(
    instance: *mut instance::RInstance,
    parent: SEXP,
    child: SEXP,
) -> bool {
    if parent.is_null() || child.is_null() {
        return true;
    }
    // Immutable singleton edges never introduce a young allocation.
    if super::globals::immutable_singleton_projection(parent).is_some()
        || super::globals::immutable_singleton_projection(child).is_some()
    {
        return true;
    }
    let Some((parent, parent_gen, child_gen)) = checked_barrier_pair(instance, parent, child)
    else {
        return false;
    };
    if parent_gen == Generation::Old as u8 && child_gen == Generation::Young as u8 {
        with_gc_state_in(instance, |state| state.remembered_set.try_add(parent))
    } else {
        true
    }
}

#[inline(always)]
pub fn vector_write_barrier(vec: SEXP, index: usize, value: SEXP) {
    write_barrier(vec, value);
}

#[inline(always)]
pub fn list_write_barrier(list: SEXP, field: u8, value: SEXP) {
    write_barrier(list, value);
}

#[inline(always)]
pub fn attrib_write_barrier(obj: SEXP, value: SEXP) {
    write_barrier(obj, value);
}

// ---------------------------------------------------------------------------
// Generation Promotion
// ---------------------------------------------------------------------------

#[inline]
pub unsafe fn promote_to_old(obj: SEXP) {
    if obj.is_null() || super::globals::immutable_singleton_projection(obj).is_some() {
        return;
    }
    unsafe {
        debug_assert!((*obj).sxpinfo.gcgen() == Generation::Young as u8);
        (*obj).sxpinfo.set_gcgen(Generation::Old as u8);
    }
}

// ---------------------------------------------------------------------------
// Reference Updating
// ---------------------------------------------------------------------------

#[inline]
fn update_field(field: &mut SEXP, old_to_new: &HashMap<usize, SEXP>) {
    if field.is_null() {
        return;
    }
    let addr = *field as usize;
    if let Some(&new_ptr) = old_to_new.get(&addr) {
        *field = new_ptr;
    }
}

#[inline]
fn vector_payload_has_sexp_refs(t: SEXPTYPE) -> bool {
    // BCODESXP (21) is included: its payload holds the instruction stream,
    // constant pool, and stack-depth vector as SEXP references. Without
    // tracing them, a collection frees a compiled closure's code while the
    // BCODESXP itself survives (e.g. `f <- function(x) x+1; gc(); f(1)`).
    matches!(t.0, 16 | 19 | 20 | 21) // STRSXP, VECSXP, EXPRSXP, BCODESXP
}

fn update_protect_stack_in(instance: *mut instance::RInstance, old_to_new: &HashMap<usize, SEXP>) {
    unsafe {
        update_protect_stack_refs_in(instance, |ptr| {
            let addr = ptr as usize;
            old_to_new.get(&addr).copied().unwrap_or(ptr)
        });
    }
}

fn update_preserve_stack_in(instance: *mut instance::RInstance, old_to_new: &HashMap<usize, SEXP>) {
    unsafe {
        update_preserve_stack_refs_in(instance, |ptr| {
            let addr = ptr as usize;
            old_to_new.get(&addr).copied().unwrap_or(ptr)
        });
    }
}

fn update_remembered_set(old_to_new: &HashMap<usize, SEXP>) {
    instance::with_required_current_instance(|instance| {
        update_remembered_set_in(instance, old_to_new)
    });
}

fn update_remembered_set_in(instance: *mut instance::RInstance, old_to_new: &HashMap<usize, SEXP>) {
    unsafe {
        with_gc_state_in(instance, |state| {
            state.remembered_set.remap(old_to_new);
        });
    }
}

fn update_references_in_object(obj: SEXP, old_to_new: &HashMap<NodeLink, NodeLink>) {
    if obj.is_null() || super::globals::immutable_singleton_projection(obj).is_some() {
        return;
    }
    let (_, parent) =
        super::memory::checked_projection(obj).expect("GC parent belongs to checked storage");
    gc_trace_bridge::remap_children(&parent, old_to_new);
}

#[cfg(test)]
fn update_object_references(old_to_new: &HashMap<NodeLink, NodeLink>) {
    instance::with_required_current_instance(|owner| {
        update_object_references_in(owner, old_to_new);
    });
}

#[cfg(test)]
fn update_object_references_in(
    instance: *mut instance::RInstance,
    old_to_new: &HashMap<NodeLink, NodeLink>,
) {
    // End the arena/persistent registry loan before any link visitor runs.
    let nodes: Vec<SEXP> = unsafe { (*instance).arena.active_nodes().collect() };
    let permanent: Vec<SEXP> = unsafe { (*instance).persistent_nodes.projections().collect() };
    for obj in nodes.into_iter().chain(permanent) {
        update_references_in_object(obj, old_to_new);
    }
}

/// This shortcut is exclusive to nonmoving sweep, after the complete strong
/// mark worklist and ready-finalizer graph have drained. Epoch metadata remains
/// valid after sxpinfo.mark is cleared. Marked strong children cannot be swept;
/// the weak key is deliberately unmarked and must still be cleared. Unmarked
/// old/young survivors of partial collection still require all-edge cleanup.
fn update_swept_object_references_in(
    instance: *mut instance::RInstance,
    old_to_nil: &HashMap<NodeLink, NodeLink>,
) {
    let nodes: Vec<SEXP> = unsafe { (*instance).arena.active_nodes().collect() };
    let permanent: Vec<SEXP> = unsafe { (*instance).persistent_nodes.projections().collect() };
    for obj in nodes.into_iter().chain(permanent) {
        if super::memory::arena_node_marked(obj) {
            let (_, parent) = super::memory::checked_projection(obj)
                .expect("marked GC parent belongs to checked storage");
            let header = parent
                .heap_identity()
                .node_snapshot(&parent)
                .expect("marked GC parent remains live");
            if header.sxpinfo.type_of() == SEXPTYPE::WEAKREFSXP {
                gc_trace_bridge::remap_weak_key(&parent, old_to_nil);
            }
        } else {
            update_references_in_object(obj, old_to_nil);
        }
    }
}

fn sweep_link_remap(
    instance: *mut instance::RInstance,
    freed: &[SEXP],
    nil: SEXP,
) -> HashMap<NodeLink, NodeLink> {
    let heap = unsafe { (*instance).heap_identity.clone() };
    let nil = heap
        .link_from_projection(nil)
        .expect("collector retains canonical nil");
    freed
        .iter()
        .map(|&projection| {
            let token = unsafe { (*instance).arena.node_token(projection) }
                .expect("swept node remains live before deactivation");
            (
                token
                    .link()
                    .expect("swept node has an exact allocation link"),
                nil,
            )
        })
        .collect()
}

fn remap_addr(addr: usize, old_to_new: &HashMap<usize, SEXP>) -> usize {
    old_to_new
        .get(&addr)
        .copied()
        .map(|ptr| ptr as usize)
        .unwrap_or(addr)
}

fn update_instance_roots_in(instance: *mut instance::RInstance, old_to_new: &HashMap<usize, SEXP>) {
    unsafe {
        update_field(&mut (*instance).empty_env, old_to_new);
        update_field(&mut (*instance).base_env, old_to_new);
        update_field(&mut (*instance).global_env, old_to_new);

        update_protect_stack_in(instance, old_to_new);
        update_preserve_stack_in(instance, old_to_new);

        update_field(
            &mut (*instance).eval_state.printvector.na_string,
            old_to_new,
        );
        update_field(
            &mut (*instance).eval_state.printvector.na_string_noquote,
            old_to_new,
        );
        update_field(&mut (*instance).eval_state.print.data.na_string, old_to_new);
        update_field(
            &mut (*instance).eval_state.print.data.na_string_noquote,
            old_to_new,
        );
        update_field(&mut (*instance).eval_state.print.data.env, old_to_new);
        update_field(&mut (*instance).eval_state.print.data.callArgs, old_to_new);

        for obj in (*instance).symbols.values_mut() {
            update_field(obj, old_to_new);
        }
        for obj in &mut (*instance).names_state.ddval_symbols {
            update_field(obj, old_to_new);
        }
        update_field(&mut (*instance).bind_state.blank_string, old_to_new);

        let old_cache = std::mem::take(&mut (*instance).package_namespace_cache);
        (*instance).package_namespace_cache = old_cache
            .into_iter()
            .map(|(package, (dir, mut namespace))| {
                update_field(&mut namespace, old_to_new);
                (package, (dir, namespace))
            })
            .collect();
        update_field(&mut (*instance).unwrap_methods_ns, old_to_new);
        for clos in &mut (*instance).unwrap_methods_closures {
            update_field(clos, old_to_new);
        }

        // The binding tables are keyed by raw node addresses. Entries whose keyed
        // node was reclaimed this cycle must be dropped before `free_node` puts
        // the address back on the LIFO free list — a recycled address would
        // otherwise alias the stale entry (a fresh environment reporting locks or
        // active bindings it never had). `old_to_new` maps exactly the reclaimed
        // addresses to R_NilValue, so key membership identifies them; live keys
        // keep their address (the collector never moves nodes).
        (*instance).active_bindings.retain(|key, value| {
            update_field(value, old_to_new);
            !old_to_new.contains_key(&key.0) && !old_to_new.contains_key(&key.1)
        });
        (*instance)
            .locked_environments
            .retain(|env| !old_to_new.contains_key(env));
        (*instance)
            .env_hash_sizes
            .retain(|env, _| !old_to_new.contains_key(env));
        (*instance).locked_bindings.retain(|(env, symbol)| {
            !old_to_new.contains_key(env) && !old_to_new.contains_key(symbol)
        });

        for finalizer in &mut (*instance).memory_state.pending_finalizers {
            update_field(finalizer.obj_mut(), old_to_new);
            if let Some(fun) = finalizer.fun_mut() {
                update_field(fun, old_to_new);
            }
        }
        update_field(&mut (*instance).dynload_state.dll_info_eptrs, old_to_new);
        update_field(&mut (*instance).dynload_state.symbol_eptrs, old_to_new);
        update_field(&mut (*instance).dynload_state.c_entry_table, old_to_new);

        #[cfg(not(target_arch = "wasm32"))]
        (*instance)
            .httpd_state
            .visit_roots(|obj| update_field(obj, old_to_new));

        update_field(
            &mut (*instance).grid_runtime_state.current_grid_state,
            old_to_new,
        );
        update_field(&mut (*instance).grid_runtime_state.eval_env, old_to_new);

        for obj in &mut (*instance).raw_cons {
            let mut sexp = *obj as SEXP;
            update_field(&mut sexp, old_to_new);
            *obj = sexp;
        }
    }
}

fn clear_swept_references_in(
    instance: *mut instance::RInstance,
    old_to_new: &HashMap<usize, SEXP>,
    old_links: &HashMap<NodeLink, NodeLink>,
) {
    unsafe {
        update_instance_roots_in(instance, old_to_new);
        update_remembered_set_in(instance, old_to_new);
        update_swept_object_references_in(instance, old_links);
    }
}

// ---------------------------------------------------------------------------
// Minor GC
// ---------------------------------------------------------------------------

/// Run a minor garbage collection cycle.
///
/// This collects young generation objects and promotes surviving objects
/// to the old generation. This function is panic-free and handles all
/// errors gracefully.
///
/// Returns (promoted_count, freed_count).
pub fn minor_gc() -> (usize, usize) {
    instance::with_required_current_instance(|owner| unsafe { minor_gc_in(owner) })
}

fn run_gc_cycle_in<F>(instance: *mut instance::RInstance, collect: F) -> (usize, usize)
where
    F: FnOnce(*mut instance::RInstance) -> (usize, usize),
{
    unsafe {
        // Defer direct requests before touching an arena with an active mutable lend.
        if super::memory::is_arena_lent(instance) {
            (*instance).gc_state.gc_pending = true;
            return (0, 0);
        }
        if (*instance).gc_state.in_progress {
            return (0, 0);
        }

        // Native Rust resources detached by sweep may have destructors that
        // reenter R. Release them after tracing, arena lends, and collection
        // bookkeeping have ended, including when collection unwinds.
        let _resource_drops = super::memory::resource_drop_guard_in(instance);
        (*instance).gc_state.in_progress = true;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            verify_gc_invariants_in(instance);
            // Managed and permanent headers share epoch metadata; immutable
            // process singletons are recognized without marking their bytes.
            super::memory::begin_gc_epoch();
            let _trace_scope = gc_trace::TraceScope::enter(gc_trace::TraceContext::new(
                (*instance).arena.heap_identity(),
                super::memory::current_gc_epoch(),
            ));
            collect(instance)
        }));
        (*instance).gc_state.in_progress = false;
        // Sweep has retired exact generations. This metadata-only pass drops
        // dead env indexes without rooting any cell. Live indexes remain warm;
        // canonical structural writes invalidate their membership separately.
        (*instance).heap_identity.prune_binding_indexes();

        match result {
            Ok((promoted, freed)) => {
                record_collection_in(&mut (*instance).gc_state, promoted, freed);
                (*instance).arena.note_gc_completed();
                notify_gc_callbacks_in(instance);
                (promoted, freed)
            }

            // A panic mid-collection leaves the heap in an indeterminate state.
            // Swallowing it (the old `=> (0, 0)`) risks silent memory corruption:
            // callers would keep using a partially-marked/swept heap. Make the
            // panic propagate (fatal to the session/eval) instead. `in_progress`
            // was already reset above, so a panic caught higher up does not leave
            // GC permanently disabled.
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

/// Collect in an explicitly selected owner.
///
/// # Safety
/// `instance` is the original writable pointer to a live, active RInstance.
/// No whole-instance borrow or Rust payload loan may overlap collection or
/// callbacks. Arena lends are detected and defer the collection request.
pub(crate) unsafe fn minor_gc_in(instance: *mut instance::RInstance) -> (usize, usize) {
    unsafe { run_gc_cycle_in(instance, do_minor_gc_in) }
}

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
fn maybe_torture_gc_in(instance: *mut instance::RInstance, ticks: u32) {
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
        let owner = super::owner::OwnerToken::from_raw(instance);
        let pin = owner
            .pin()
            .and_then(|pin| pin.ok_or(super::object::SexpError::RootUnavailable))
            .unwrap_or_else(|failure| gc_entry_failure(failure));
        let _bindings = owned_environment_binding_values(owner)
            .unwrap_or_else(|failure| gc_entry_failure(failure));
        (*instance).gc_state.gc_pending = false;
        let _collection = crate::mainutils::memory_main::GcOperationGuard::enter(pin, true)
            .unwrap_or_else(|failure| gc_entry_failure(failure));
        run_gc_cycle_in(instance, do_torture_mark_sweep_in);
    }
}
fn eval_safe_point_gc_due_in(instance: *mut instance::RInstance) -> bool {
    unsafe { (*instance).gc_state.gc_pending || (*instance).arena.growth_warrants_gc() }
}

fn collect_environment_binding_values(instance: *mut instance::RInstance) -> Vec<SEXP> {
    unsafe {
        let mut values = Vec::new();
        let mut seen_envs = std::collections::HashSet::new();
        unsafe {
            let mut walk_env = |mut env: SEXP| {
                while !env.is_null() && seen_envs.insert(env as usize) {
                    if (*env).sxpinfo.type_of() != SEXPTYPE::ENVSXP {
                        break;
                    }
                    let mut frame = super::accessors::FRAME(env);
                    while !frame.is_null() && (*frame).sxpinfo.type_of() != SEXPTYPE::NILSXP {
                        values.push(frame);
                        let val = super::accessors::CAR(frame);
                        if !val.is_null() {
                            values.push(val);
                        }
                        frame = super::accessors::CDR(frame);
                    }
                    env = super::accessors::ENCLOS(env);
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
fn owned_environment_binding_values(
    owner: super::owner::OwnerToken<'_>,
) -> super::object::SexpResult<Vec<super::object::Sexp<'static>>> {
    owner.require_active()?;
    collect_environment_binding_values(owner.as_ptr())
        .into_iter()
        .map(|value| owner.sexp(value)?.into_owned())
        .collect()
}

fn gc_entry_failure(failure: super::object::SexpError) -> ! {
    std::panic::panic_any(super::context::RError {
        message: failure.to_string(),
    });
}

/// Run an explicit collection with actual owning environment binding snapshots.
/// The leases release exactly these values on unwind or revocation; caller and
/// callback protection entries are unaffected.
pub fn collect_with_environment_protects(full: bool) -> (usize, usize) {
    instance::with_required_current_instance(|instance| unsafe {
        let owner = super::owner::OwnerToken::from_raw(instance);
        let _pin = owner
            .pin()
            .and_then(|pin| pin.ok_or(super::object::SexpError::RootUnavailable))
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
/// Every N-th evaluation-safe-point collection also runs a full collection
/// (via the same environment force-protect preamble) so old-generation
/// garbage is reclaimed without waiting for an explicit `gc()`.
const SAFE_POINT_FULL_COLLECTION_INTERVAL: u64 = 64;

/// Run collection at a cooperative safe point during evaluation.
///
/// Call this after loop iterations and brace-block statements complete, when
/// no SEXP values from the just-finished evaluation remain only on Rust stack.
pub fn maybe_collect_at_eval_safe_point() {
    instance::with_required_current_instance(|instance| unsafe {
        if !eval_safe_point_gc_due_in(instance) {
            return;
        }
        let owner = super::owner::OwnerToken::from_raw(instance);
        let _pin = owner
            .pin()
            .and_then(|pin| pin.ok_or(super::object::SexpError::RootUnavailable))
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
fn run_pending_finalizers_after_collection() {
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

fn do_minor_gc_in(instance: *mut instance::RInstance) -> (usize, usize) {
    unsafe {
        // No traceable HashSet: use mark bit visited. (Perf + addresses review complaint
        // about allocating HashSets and hashing every edge.)
        mark_instance_roots(instance);
        mark_checked_root_snapshot((*instance).gc_state.remembered_set.checked_roots());

        let mut freed_count = 0;
        let mut promoted_count = 0;
        let mut to_free = Vec::new();

        {
            let arena = &mut (*instance).arena;
            // Iterate directly; only allocate to_free vec (not full snapshot of actives).
            // Reduces temp memory/alloc pressure during GC (perf + memory win, especially on constrained Android/WASM).
            for obj in arena.active_nodes() {
                if obj.is_null() {
                    continue;
                }
                unsafe {
                    let obj_gen = (*obj).sxpinfo.gcgen();
                    let marked = super::memory::arena_node_marked(obj);

                    if obj_gen == Generation::Young as u8 {
                        if marked {
                            (*obj).sxpinfo.set_gcgen(Generation::Old as u8);
                            (*obj).sxpinfo.set_mark(false);
                            promoted_count += 1;
                        } else {
                            to_free.push(obj);
                        }
                    } else {
                        if marked {
                            (*obj).sxpinfo.set_mark(false);
                        }
                    }
                }
            }
        }

        promoted_count += retain_newly_ready_finalizer_graph(instance, &mut to_free, true);

        if !to_free.is_empty() {
            let nil = unsafe { crate::sexp::globals::R_NilValue() };
            let old_to_nil: HashMap<usize, SEXP> =
                to_free.iter().map(|&obj| (obj as usize, nil)).collect();
            let old_links = sweep_link_remap(instance, &to_free, nil);
            clear_swept_references_in(instance, &old_to_nil, &old_links);

            for obj in to_free {
                (*instance).arena.free_node(obj);
                freed_count += 1;
            }
        }

        (*instance).gc_state.remembered_set.clear();

        (promoted_count, freed_count)
    }
}

/// Newly ready finalizers become roots during this collection, after the
/// initial mark pass. Trace their complete graph before deciding what to free.
/// Keys alone are insufficient: a child can have been queued for sweeping too.
fn retain_newly_ready_finalizer_graph(
    instance: *mut instance::RInstance,
    to_free: &mut Vec<SEXP>,
    promote_young: bool,
) -> usize {
    if to_free.is_empty() {
        return 0;
    }
    unsafe {
        let unreachable: HashSet<usize> = to_free.iter().map(|&obj| obj as usize).collect();
        let keys = crate::mainutils::memory_main::mark_finalizers_ready_for_unreachable_in(
            &mut (*instance).memory_state,
            &unreachable,
        );
        if keys.is_empty() {
            return 0;
        }
        // Copy original pointers, preserving provenance and ending the memory
        // state borrow before recursive traversal touches the owning graph.
        let ready_roots: Vec<SEXP> = (*instance)
            .memory_state
            .pending_finalizers
            .iter()
            .filter(|finalizer| finalizer.is_ready())
            .map(|finalizer| finalizer.obj())
            .collect();
        for root in ready_roots {
            mark_reachable(root);
        }
        let mut promoted = 0;
        to_free.retain(|obj| {
            if !super::memory::arena_node_marked(*obj) {
                return true;
            }
            if promote_young && (**obj).sxpinfo.gcgen() == Generation::Young as u8 {
                (**obj).sxpinfo.set_gcgen(Generation::Old as u8);
                promoted += 1;
            }
            (**obj).sxpinfo.set_mark(false);
            false
        });
        promoted
    }
}

// ---------------------------------------------------------------------------
// Full GC
// ---------------------------------------------------------------------------

/// Run full garbage collection.
///
/// This performs non-moving mark-sweep over all generations. It deliberately
/// does not move live objects because translated evaluator code routinely
/// holds raw `SEXP` pointers in Rust stack locals.
///
/// Returns (promoted_count, freed_count).
pub fn full_gc() -> (usize, usize) {
    instance::with_required_current_instance(|owner| unsafe { full_gc_in(owner) })
}

/// Collect in an explicitly selected owner.
///
/// # Safety
/// `instance` is the original writable pointer to a live, active RInstance.
/// No whole-instance borrow or Rust payload loan may overlap collection or
/// callbacks. Arena lends are detected and defer the collection request.
pub(crate) unsafe fn full_gc_in(instance: *mut instance::RInstance) -> (usize, usize) {
    unsafe { run_gc_cycle_in(instance, do_full_mark_sweep_in) }
}

/// gctorture collection: full mark from all roots, but only OLD-generation
/// garbage is swept.
///
/// Upstream `FORCE_GC` runs `R_gc_internal(0)` — a full sweep — at arbitrary
/// allocation points, safely, because R's collector conservatively scans the
/// C stack and every partially built structure in a local survives. This
/// port has no stack scan, so an alloc-time full sweep would reclaim young
/// nodes that translated code legitimately holds only in Rust locals between
/// two allocations (e.g. the CHARSXP held across the STRSXP allocation in
/// `Rf_mkString`). Young nodes therefore survive torture collections and are
/// reclaimed, as usual, by the safe-point/quiescent collections that never
/// run mid-construction. Old-generation garbage — the accumulation gctorture
/// exists to exercise — is still reclaimed on every forced cycle.
fn do_torture_mark_sweep_in(instance: *mut instance::RInstance) -> (usize, usize) {
    unsafe {
        mark_from_all_roots_in(instance);

        let mut freed_count = 0;
        let mut to_free = Vec::new();

        {
            let arena = &mut (*instance).arena;
            // Young nodes are not reclaimed here and are not promoted, so the
            // only nodes this pass can free are old. Walking the whole slab
            // on every allocation is most of a long `gctorture(TRUE)` run.
            for obj in arena.old_nodes() {
                if obj.is_null() {
                    continue;
                }
                unsafe {
                    if super::memory::arena_node_marked(obj) {
                        // Marked nodes stay in their generation: promotion to
                        // the old generation here would let the very next forced
                        // collection (only `gc_force_gap` allocations later)
                        // sweep an in-flight value that is momentarily
                        // reachable only from a Rust local. Promotion stays
                        // the safe-point collectors' job.
                        (*obj).sxpinfo.set_mark(false);
                    } else {
                        // Old-generation garbage: reclaim now.
                        to_free.push(obj);
                    }
                    // Unmarked young nodes survive alloc-time collections; the
                    // next safe-point collection sweeps whichever stay dead.
                }
            }
        }

        let mut freed_set: HashSet<usize> = HashSet::new();
        retain_newly_ready_finalizer_graph(instance, &mut to_free, false);

        if !to_free.is_empty() {
            freed_set = to_free.iter().map(|&obj| obj as usize).collect();
            let nil = unsafe { crate::sexp::globals::R_NilValue() };
            let old_to_nil: HashMap<usize, SEXP> =
                to_free.iter().map(|&obj| (obj as usize, nil)).collect();
            let old_links = sweep_link_remap(instance, &to_free, nil);
            clear_swept_references_in(instance, &old_to_nil, &old_links);

            for obj in to_free {
                (*instance).arena.free_node(obj);
                freed_count += 1;
            }
        }

        // The remembered set cannot be cleared wholesale (unlike a true full
        // collection, reachable young nodes were not promoted, so live
        // old-to-young edges still exist). Drop only entries whose old parent
        // was reclaimed this cycle.
        (*instance).gc_state.remembered_set.retain_live(&freed_set);

        (0, freed_count)
    }
}

fn mark_from_all_roots_in(instance: *mut instance::RInstance) {
    unsafe {
        // No traceable HashSet: use mark bit visited. (Perf + addresses review complaint
        // about allocating HashSets and hashing every edge.)
        mark_instance_roots(instance);
        mark_checked_root_snapshot((*instance).gc_state.remembered_set.checked_roots());
    }
}

fn do_full_mark_sweep_in(instance: *mut instance::RInstance) -> (usize, usize) {
    unsafe {
        mark_from_all_roots_in(instance);

        let mut freed_count = 0;
        let mut promoted_count = 0;
        let mut to_free = Vec::new();

        {
            let arena = &mut (*instance).arena;
            // Direct iter, only to_free alloc (less GC-time memory pressure).
            for obj in arena.active_nodes() {
                if obj.is_null() {
                    continue;
                }
                unsafe {
                    if super::memory::arena_node_marked(obj) {
                        if (*obj).sxpinfo.gcgen() == Generation::Young as u8 {
                            (*obj).sxpinfo.set_gcgen(Generation::Old as u8);
                            promoted_count += 1;
                        }
                        (*obj).sxpinfo.set_mark(false);
                    } else {
                        to_free.push(obj);
                    }
                }
            }
        }

        promoted_count += retain_newly_ready_finalizer_graph(instance, &mut to_free, true);

        if !to_free.is_empty() {
            let nil = unsafe { crate::sexp::globals::R_NilValue() };
            let old_to_nil: HashMap<usize, SEXP> =
                to_free.iter().map(|&obj| (obj as usize, nil)).collect();
            let old_links = sweep_link_remap(instance, &to_free, nil);
            clear_swept_references_in(instance, &old_to_nil, &old_links);

            for obj in to_free {
                (*instance).arena.free_node(obj);
                freed_count += 1;
            }
        }

        (*instance).gc_state.remembered_set.clear();

        (promoted_count, freed_count)
    }
}

// ---------------------------------------------------------------------------
// Legacy relocation hooks.
//
// Per architecture review, a moving collector is incompatible with raw `SEXP`
// pointers held in Rust stack frames across allocations (the dominant coding
// style in the ported evaluator). R itself uses a non-moving GC for exactly
// this reason. We retain only mark-sweep + free-list recycling.
//
// All moving logic (snapshot_live_objects, LiveObject, do_relocate, the two-space
// copy + root rewrite) has been removed. Reference rewriting is kept only for
// non-moving sweep to redirect refs-to-dead -> R_NilValue in survivor objects.
// ---------------------------------------------------------------------------

/// Legacy hook retained for callers that used to request arena relocation.
///
/// The current collector never relocates live `SEXP` objects. This function
/// performs a normal minor GC and returns `false`.
///
/// Returns true if live objects were relocated. Always false.
pub fn compact_if_needed(_frag_threshold: f64) -> bool {
    let (_promoted, _freed) = minor_gc();
    false
}

/// Normalize the free list without moving live objects.
fn normalize_free_list(arena: &mut RArena) {
    arena.normalize_free_list();
}

/// Get the current fragmentation ratio of the arena.
pub fn get_fragmentation_ratio() -> f64 {
    unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        with_arena_for_gc(|arena| arena.fragmentation_ratio())
    }
}

/// Force non-moving free-list cleanup.
///
/// This does not move live objects. It only normalizes the arena free list.
pub fn force_compact() {
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        with_arena_for_gc(|arena| {
            normalize_free_list(arena);
        })
    });
}

// ---------------------------------------------------------------------------
// Barrier Enforcement Wrappers
// ---------------------------------------------------------------------------

/// Guarded vector slot reference that automatically runs write barrier on assignment.
pub struct VectorSlot<'a> {
    vec: SEXP,
    slot: &'a mut SEXP,
}

impl<'a> VectorSlot<'a> {
    #[inline]
    pub fn new(vec: SEXP, slot: &'a mut SEXP) -> Self {
        VectorSlot { vec, slot }
    }

    #[inline]
    pub fn set(&mut self, value: SEXP) {
        vector_write_barrier(self.vec, 0, value);
        *self.slot = value;
    }

    #[inline]
    pub fn get(&self) -> SEXP {
        *self.slot
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod sweep_remap_tests;

#[cfg(test)]
mod tests {
    use crate::sexp::protect::push_protect_in;
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::super::memory::{ArenaBudget, with_arena};
    use super::super::protect::with_protected_objects;
    use crate::sexp::session::RSession;

    use super::*;

    fn preamble_unwind_detaches_incidental_roots(safe_point: bool) {
        use std::{cell::RefCell, rc::Rc};
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let pin = owner.weak_owner().unwrap().pin().unwrap();
        let instance = pin.as_ptr();
        let allocate = || {
            owner
                .node_factory()
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
                .unwrap()
        };
        let binding = allocate();
        let binding_node = binding.allocation().unwrap().clone();
        let callback_binding_node = binding_node.clone();
        assert!(session.define_var("owned_gc_preamble_binding", binding));
        let caller_value = allocate();
        unsafe { super::super::protect::protect_raw_pointer(caller_value.as_raw()) };
        drop(caller_value);
        let expected = Rc::new(RefCell::new(unsafe {
            (*instance).legacy_protect.checked_entries_snapshot()
        }));
        let callback_value = Rc::new(RefCell::new(Some(allocate().into_owned().unwrap())));
        let callback_expected = expected.clone();
        register_gc_callback(Box::new(move |_| {
            // The global binding and the caller protection are sole roots here;
            // this checked node is an observer, not another owning handle.
            assert!(callback_binding_node.is_live());
            unsafe {
                super::super::accessors::SET_FRAME(
                    (*instance).global_env,
                    super::super::globals::R_NilValue(),
                );
                super::super::owner::OwnerToken::from_raw(instance)
                    .full_gc()
                    .unwrap();
            }
            // The detached binding now survives only in the original preamble's
            // actual owning snapshot, not in an environment or fixture handle.
            assert!(callback_binding_node.is_live());
            let value = callback_value.borrow_mut().take().unwrap();
            unsafe { super::super::protect::protect_raw_pointer(value.as_raw()) };
            drop(value);
            let entries = unsafe { (*instance).legacy_protect.checked_entries_snapshot() };
            // The preamble must add no manual roots that could accidentally be
            // confused with this callback's independently retained protection.
            assert_eq!(
                &entries[..entries.len() - 1],
                callback_expected.borrow().as_slice()
            );
            *callback_expected.borrow_mut() = entries;
            std::panic::panic_any(91_u32);
        }));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if safe_point {
                unsafe { (*instance).gc_state.gc_pending = true };
                maybe_collect_at_eval_safe_point();
            } else {
                collect_with_environment_protects(true);
            }
        }));
        assert_eq!(*outcome.unwrap_err().downcast::<u32>().unwrap(), 91);
        assert_eq!(
            unsafe { (*instance).legacy_protect.checked_entries_snapshot() },
            *expected.borrow()
        );
        unsafe { (*instance).gc_state.callbacks.clear() };
        owner.full_gc().unwrap();
        assert!(
            !binding_node.is_live(),
            "preamble retained its binding after unwind"
        );
        assert_eq!(
            unsafe { (*instance).legacy_protect.checked_entries_snapshot() },
            *expected.borrow()
        );
        unsafe { super::super::protect::unprotect_count_in(instance, 2) };
    }

    #[test]
    fn owned_gc_preamble_unwind_restores_original_protection_depth() {
        preamble_unwind_detaches_incidental_roots(false);
    }

    #[test]
    fn owned_gc_safe_point_unwind_retains_and_releases_detached_binding() {
        preamble_unwind_detaches_incidental_roots(true);
    }

    #[test]
    fn owned_gc_safe_point_revocation_stops_original_finalizer_continuation() {
        use std::{
            cell::{Cell, RefCell},
            rc::Rc,
        };
        let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
        let original = facade
            .borrow()
            .as_ref()
            .unwrap()
            .owner_token()
            .unwrap()
            .weak_owner()
            .unwrap();
        let pin = original.pin().unwrap();
        let instance = pin.as_ptr();
        let binding = original
            .node_factory()
            .unwrap()
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
            .unwrap();
        let binding_node = binding.allocation().unwrap().clone();
        assert!(
            facade
                .borrow()
                .as_ref()
                .unwrap()
                .define_var("owned_safe_point_binding", binding)
        );
        let callback_facade = Rc::downgrade(&facade);
        let calls = Rc::new(Cell::new(0));
        let callback_calls = calls.clone();
        register_gc_callback(Box::new(move |_| unsafe {
            callback_calls.set(callback_calls.get() + 1);
            super::super::accessors::SET_FRAME(
                (*instance).global_env,
                super::super::globals::R_NilValue(),
            );
            super::super::owner::OwnerToken::from_raw(instance)
                .full_gc()
                .unwrap();
            assert!(binding_node.is_live());
            assert_eq!((*instance).legacy_protect.len(), 0);
            drop(callback_facade.upgrade().unwrap().borrow_mut().take());
        }));
        unsafe { (*instance).gc_state.gc_pending = true };
        // Actual minor collection must stop before consulting a dead ambient
        // owner for finalizers or cleanup.
        maybe_collect_at_eval_safe_point();
        assert_eq!(calls.get(), 1);
        assert!(facade.borrow().is_none());
        assert!(pin.require_live().is_err());
        assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
        assert_eq!(unsafe { (*instance).legacy_protect.len() }, 0);
    }

    #[test]
    fn collected_node_resources_reenter_only_after_collection_and_arena_lends_end() {
        use std::{cell::Cell, rc::Rc};
        struct ReentrantDrop(Rc<Cell<usize>>);
        impl Drop for ReentrantDrop {
            fn drop(&mut self) {
                instance::with_required_current_instance(|owner| unsafe {
                    assert!(!(*owner).gc_state.in_progress);
                    assert!(!super::super::memory::is_arena_lent(owner));
                });
                // SAFETY: the enclosing session is live throughout collection
                // and this destructor; its exclusive arena lend has ended.
                let owner = unsafe { super::super::owner::OwnerToken::current().unwrap() };
                let value = owner
                    .node_factory()
                    .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
                    .unwrap();
                full_gc();
                assert!(value.is_live());
                self.0.set(self.0.get() + 1);
            }
        }
        let session = RSession::new_for_gc_tests();
        let drops = Rc::new(Cell::new(0));
        let value = session
            .owner_token()
            .unwrap()
            .node_factory()
            .allocate(|arena| {
                let pointer = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
                let token = arena.node_token(pointer)?;
                arena
                    .heap_identity()
                    .attach_resource(&token, Rc::new(ReentrantDrop(drops.clone())))?;
                Some(pointer)
            })
            .unwrap();
        let original = super::super::memory::checked_projection(value.as_raw())
            .unwrap()
            .1;
        drop(value);
        session.gc();
        assert!(!original.is_live());
        assert_eq!(drops.get(), 1);
        session.gc();
        assert_eq!(drops.get(), 1);
    }

    /// Persistent environment sentinels live outside the arena, so sweep
    /// never resets their mark bits. Before `clear_persistent_node_marks_in`
    /// ran at cycle start, the mark left by cycle one made
    /// `mark_reachable_traced` short-circuit on every later cycle: global
    /// frame bindings were not re-traced and got swept while still
    /// reachable, leaving dangling frame chains that later collections (or
    /// frame walks) dereferenced as garbage.
    #[test]
    fn test_persistent_env_roots_retraced_every_cycle() {
        let _session = RSession::new_without_default_packages();

        let sym =
            unsafe { crate::sexp::symbol::Rf_install(b"retrace_probe\0".as_ptr() as *const _) };
        let value = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
        };
        unsafe {
            *(crate::sexp::accessors::INTEGER(value)) = 42;
        }
        unsafe {
            crate::sexp::envir::defineVar(sym, value, crate::sexp::globals::R_GlobalEnv());
        }

        // Cycle one marks the persistent global env; cycle two must clear
        // the stale mark and walk the frame again instead of sweeping it.
        full_gc();
        let found = unsafe {
            crate::sexp::envir::R_findVarInFrame(crate::sexp::globals::R_GlobalEnv(), sym)
        };
        assert!(found != unsafe { crate::sexp::globals::R_UnboundValue() });
        assert!(
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena(|arena| arena.contains(found))
            })
        );

        full_gc();
        let found = unsafe {
            crate::sexp::envir::R_findVarInFrame(crate::sexp::globals::R_GlobalEnv(), sym)
        };
        assert!(
            found != unsafe { crate::sexp::globals::R_UnboundValue() },
            "global binding swept after second cycle: persistent env mark went stale"
        );
        assert!(
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena(|arena| arena.contains(found))
            })
        );
        assert_eq!(unsafe { *crate::sexp::accessors::INTEGER(found) }, 42);

        // The quiescent path has no force-protect preamble; with the fix the
        // global env root alone must keep the binding alive.
        let (_p, _f) = minor_gc();
        let found = unsafe {
            crate::sexp::envir::R_findVarInFrame(crate::sexp::globals::R_GlobalEnv(), sym)
        };
        assert!(
            found != unsafe { crate::sexp::globals::R_UnboundValue() },
            "global binding swept by preamble-less minor gc"
        );
        assert_eq!(unsafe { *crate::sexp::accessors::INTEGER(found) }, 42);
    }

    fn reset_gc_test_arena(arena: &mut RArena) {
        *arena = RArena::fresh_with_identity(arena.heap_identity());
        let nil = unsafe { crate::sexp::globals::R_NilValue() };
        unsafe {
            // Base bootstrap inserts heap-owned Autoloads (and a full session
            // inserts package environments) into this chain. They were freed
            // with the old arena; keep only the persistent sentinels.
            (*crate::sexp::globals::R_GlobalEnv())
                .data
                .environment_mut()
                .enclos = arena
                .link_from_projection(crate::sexp::globals::R_BaseEnv())
                .unwrap();
        }
        instance::with_required_current_instance(|instance| unsafe {
            // Test-harness bulk reset via raw place accesses: BOTH
            // protection storages go back to empty.
            (*instance).legacy_protect.clear();
            (*instance).root_table.clear();
            (*instance).preserve_stack.clear();
            (*instance).base_wrappers.borrow_mut().clear();
            (*instance).package_namespace_cache.clear();
            (*instance).unwrap_methods_ns = nil;
            (*instance).unwrap_methods_closures.clear();
            (*instance).active_bindings.clear();
            (*instance).objects_state.deferred_default_object = instance::RuntimeValue::empty();
            (*instance).objects_state.s4_validity.clear();
            (*instance).objects_state.s4_extends_table = instance::RuntimeValue::empty();
            (*instance).eval_state.bc_stack.set_depth(0);
            (*instance).context_stack.clear();
            (*instance).gc_state.remembered_set.clear();
            (*instance).error_state.warnings = instance::RuntimeValue::empty();
            (*instance).error_state.handler_stack = instance::RuntimeValue::empty();
            (*instance).error_state.restart_stack = instance::RuntimeValue::empty();
            (*instance).error_state.global_calling_handlers = instance::RuntimeValue::empty();
            (*instance).error_state.signalled_condition = instance::RuntimeValue::empty();
            (*instance).error_state.warning_call = instance::RuntimeValue::empty();
            (*instance).error_state.last_error_call = instance::RuntimeValue::empty();
            (*instance).error_state.last_error_call_explicit = false;
            (*instance).error_state.last_error_nframe = 0;
            (*instance).error_state.try_catch_nframes.clear();
            (*instance).error_state.mathlib_warning_call = instance::RuntimeValue::empty();
            (*instance).eval_state.current_expr = instance::RuntimeValue::empty();
            (*instance).eval_state.parse_error_file = instance::RuntimeValue::empty();
            (*instance).eval_state.exec_token = instance::RuntimeValue::empty();
            (*instance).eval_state.profiling.sref = instance::RuntimeValue::empty();
            (*instance).eval_state.profiling.srcfiles.clear();
            (*instance).eval_state.profiling.srcfile_bytes_used = 0;
            (*instance).eval_state.profiling.srcfiles_buffer = instance::RuntimeValue::empty();
            (*instance).eval_state.printvector.na_string = nil;
            (*instance).eval_state.printvector.na_string_noquote = nil;
            (*instance).eval_state.print.data.na_string = nil;
            (*instance).eval_state.print.data.na_string_noquote = nil;
            (*instance).eval_state.print.data.env = nil;
            (*instance).eval_state.print.data.callArgs = nil;
            (*instance).symbols.clear();
            (*instance).symbol_nodes.clear();
            (*instance).names_state.ddval_symbols.clear();
            (*instance).bind_state.blank_string = nil;
            (*instance).options.clear();
            (*instance).main_state.task_callbacks.clear();
            (*instance).objects_state.prim_generics.clear();
            (*instance).objects_state.prim_mlist.clear();
            (*instance).heap_identity.clear_binding_indexes();
            (*instance).memory_state.pending_finalizers.clear();
            (*instance).dynload_state.dll_info_eptrs = nil;
            (*instance).dynload_state.symbol_eptrs = nil;
            (*instance).dynload_state.c_entry_table = nil;
            (*instance).grid_runtime_state.current_grid_state = nil;
            (*instance).grid_runtime_state.eval_env = nil;
            (*instance).raw_cons.clear();
            // Retain only the three environment sentinels. Permanent owned
            // names, scalar buffers, promises, and cons headers can contain
            // edges into the discarded arena even when no legacy projection
            // list contains them. Removing each owner also invalidates its
            // checked tokens before the underlying cell storage is dropped.
            let sentinels = [
                (*instance).empty_env,
                (*instance).base_env,
                (*instance).global_env,
            ];
            let discarded: Vec<_> = (*instance)
                .persistent_nodes
                .projections()
                .filter(|node| !sentinels.contains(node))
                .collect();
            for node in discarded {
                assert!((*instance).persistent_nodes.remove(node));
            }
            (*instance)
                .env_nodes
                .retain(|node| sentinels.contains(node));
        });
        for (env, enclos) in [
            (unsafe { crate::sexp::globals::R_EmptyEnv() }, nil),
            (unsafe { crate::sexp::globals::R_BaseEnv() }, unsafe {
                crate::sexp::globals::R_EmptyEnv()
            }),
            (unsafe { crate::sexp::globals::R_GlobalEnv() }, unsafe {
                crate::sexp::globals::R_BaseEnv()
            }),
        ] {
            if !env.is_null() {
                unsafe {
                    (*env).attrib = arena.link_from_projection(nil).unwrap();
                    (*env).data.environment_mut().frame = arena.link_from_projection(nil).unwrap();
                    (*env).data.environment_mut().hashtab =
                        arena.link_from_projection(nil).unwrap();
                    (*env).data.environment_mut().enclos =
                        arena.link_from_projection(enclos).unwrap();
                }
            }
        }
    }

    #[test]
    fn test_bulk_reset_detaches_discarded_heap_roots() {
        let _session = RSession::new_without_default_packages();
        let global = unsafe { crate::sexp::globals::R_GlobalEnv() };
        let base = unsafe { crate::sexp::globals::R_BaseEnv() };
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                assert!(arena.contains(unsafe { super::super::accessors::ENCLOS(global) }));
                let discarded = arena.alloc_node(SEXPTYPE::LISTSXP);
                let arena_token = arena.node_token(discarded).unwrap();
                let (sentinel_token, permanent_token) =
                    instance::with_required_current_instance(|inst| unsafe {
                        let mut header = crate::sexp::ffi::SexprecCore::new(SEXPTYPE::PROMSXP);
                        header.attrib = arena.link_from_projection(discarded).unwrap();
                        let permanent = (*inst).persistent_nodes.allocate_header(header).unwrap();
                        (
                            (*inst).persistent_nodes.token(global).unwrap(),
                            (*inst).persistent_nodes.token(permanent).unwrap(),
                        )
                    });
                assert!(arena_token.same_heap(&sentinel_token));
                reset_gc_test_arena(arena);
                assert_eq!(unsafe { super::super::accessors::ENCLOS(global) }, base);
                assert!(!arena_token.is_live());
                assert!(!permanent_token.is_live());
                assert!(sentinel_token.is_live());
            })
        });
        instance::with_required_current_instance(|inst| unsafe {
            assert!((*inst).preserve_stack.is_empty());
            assert!((*inst).base_wrappers.borrow().is_empty());
            assert!((*inst).package_namespace_cache.is_empty());
            assert_eq!((*inst).persistent_nodes.len(), 3);
        });
        assert_eq!(full_gc(), (0, 0));
    }

    #[test]
    fn test_live_base_runtime_survives_repeated_collections() {
        let mut session = RSession::new_without_default_packages();
        let (result, _, _) = session.eval_script_with_output_capture(
            "kept <- list(v = 1:8, f = function(x) x + 1, e = new.env()); kept$e$answer <- 42L",
        );
        result.expect("initialize live runtime fixture");
        for _ in 0..20 {
            full_gc();
            let (result, _, _) = session.eval_script_with_output_capture(
                "identical(kept$v, 1:8) && identical(kept$f(9), 10) && identical(kept$e$answer, 42L)",
            );
            let value = result.expect("evaluate after collection");
            assert_eq!(value.logical_elt(0), Some(1));
        }
    }

    #[test]
    fn test_write_barrier_detects_old_to_young() {
        let _session = RSession::new_without_default_packages();
        let previous = with_gc_state(|state| state.remembered_set.len());

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                let old_obj = arena.alloc_node(SEXPTYPE::LISTSXP);
                let young_obj = arena.alloc_node(SEXPTYPE::INTSXP);

                unsafe {
                    (*old_obj).sxpinfo.set_gcgen(Generation::Old as u8);
                    (*young_obj).sxpinfo.set_gcgen(Generation::Young as u8);
                }

                write_barrier(old_obj, young_obj);

                assert_eq!(
                    with_gc_state(|state| state.remembered_set.len()),
                    previous + 1
                );
            })
        });
    }

    #[test]
    fn test_gc_root_updates_can_target_instance_explicitly() {
        let mut left = instance::RInstance::new();
        let mut right = instance::RInstance::new();
        let old = left.arena.alloc_node(SEXPTYPE::INTSXP);
        let new = left.arena.alloc_node(SEXPTYPE::REALSXP);
        let right_obj = right.arena.alloc_node(SEXPTYPE::INTSXP);
        let mut old_to_new = HashMap::new();
        old_to_new.insert(old as usize, new);
        unsafe {
            (*old).sxpinfo.set_gcgen(Generation::Old as u8);
            (*new).sxpinfo.set_gcgen(Generation::Old as u8);
            (*right_obj).sxpinfo.set_gcgen(Generation::Old as u8);
        }

        let old_root = unsafe { RootValue::from_owner(std::ptr::addr_of_mut!(left), old) }.unwrap();
        let right_root =
            unsafe { RootValue::from_owner(std::ptr::addr_of_mut!(right), right_obj) }.unwrap();
        left.legacy_protect.push(old_root.clone(), "test");
        let (projection, allocation) = super::super::memory::checked_projection(old).unwrap();
        left.root_table.claim(
            RootValue::Checked {
                projection,
                allocation,
            },
            "test",
        );
        left.preserve_stack.push_for_test(old_root);
        left.gc_state.remembered_set.add(old);
        right.legacy_protect.push(right_root.clone(), "test");
        right.preserve_stack.push_for_test(right_root);
        right.gc_state.remembered_set.add(right_obj);

        update_protect_stack_in(&mut left, &old_to_new);
        update_preserve_stack_in(&mut left, &old_to_new);
        update_remembered_set_in(&mut left, &old_to_new);

        left.legacy_protect
            .with_entries(|entries| assert_eq!(entries[0], new));
        left.root_table
            .with_entries(|entries| assert_eq!(entries[0], new));
        assert_eq!(
            left.preserve_stack.entries_snapshot().last().copied(),
            Some(new)
        );
        assert!(left.gc_state.remembered_set.iter().any(|obj| obj == new));
        assert!(!left.gc_state.remembered_set.iter().any(|obj| obj == old));

        right
            .legacy_protect
            .with_entries(|entries| assert_eq!(entries[0], right_obj));
        assert_eq!(
            right.preserve_stack.entries_snapshot().last().copied(),
            Some(right_obj)
        );
        assert!(
            right
                .gc_state
                .remembered_set
                .iter()
                .any(|obj| obj == right_obj)
        );
    }

    #[test]
    fn test_gc_with_empty_arena() {
        let _session = RSession::new_without_default_packages();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
            })
        });
        let (promoted, freed) = minor_gc();
        assert_eq!(promoted, 0);
        assert_eq!(freed, 0);
    }

    #[test]
    fn test_gc_with_only_young_objects() {
        let _session = RSession::new_without_default_packages();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                arena.alloc_node(SEXPTYPE::INTSXP);
                arena.alloc_node(SEXPTYPE::REALSXP);
            })
        });
        let (promoted, freed) = minor_gc();
        assert_eq!(promoted, 0);
        assert_eq!(freed, 2);
    }

    #[test]
    fn test_gc_with_only_old_objects() {
        let _session = RSession::new_without_default_packages();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                let obj1 = arena.alloc_node(SEXPTYPE::INTSXP);
                let obj2 = arena.alloc_node(SEXPTYPE::REALSXP);
                unsafe {
                    (*obj1).sxpinfo.set_gcgen(Generation::Old as u8);
                    (*obj2).sxpinfo.set_gcgen(Generation::Old as u8);
                }
            })
        });
        let (promoted, freed) = minor_gc();
        assert_eq!(promoted, 0);
        assert_eq!(freed, 0);
    }

    #[test]
    fn test_gc_with_mixed_objects() {
        let _session = RSession::new_without_default_packages();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                let old_obj = arena.alloc_node(SEXPTYPE::INTSXP);
                let young_obj = arena.alloc_node(SEXPTYPE::REALSXP);
                unsafe {
                    (*old_obj).sxpinfo.set_gcgen(Generation::Old as u8);
                    (*young_obj).sxpinfo.set_gcgen(Generation::Young as u8);
                }
            })
        });
        let (promoted, freed) = minor_gc();
        assert_eq!(promoted, 0);
        assert_eq!(freed, 1);
    }

    #[test]
    fn test_minor_gc_traces_global_environment_bindings() {
        let session = RSession::new_without_default_packages();

        let value_raw = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                let value = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
                unsafe {
                    *(crate::sexp::accessors::INTEGER(value)) = 123;
                }
                value
            })
        };
        let value = session.sexp(value_raw).expect("value belongs to session");
        assert!(session.define_var("kept_by_global_env", value));

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                let garbage = arena.alloc_node(SEXPTYPE::REALSXP);
                assert!(!garbage.is_null());
            })
        });
        let (_, freed) = minor_gc();
        assert!(freed >= 1);
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                for _ in 0..256 {
                    assert!(!arena.alloc_node(SEXPTYPE::REALSXP).is_null());
                }
            })
        });

        let found = session.find_var("kept_by_global_env").unwrap();
        assert_eq!(found.as_raw(), value_raw);
        unsafe {
            assert_eq!((*value_raw).sxpinfo.type_of(), SEXPTYPE::INTSXP);
            assert_eq!((*value_raw).vecsxp_length(), 1);
            assert_eq!(*(crate::sexp::accessors::INTEGER(value_raw)), 123);
        }
    }

    #[test]
    fn test_gc_reentrancy_guard() {
        let _session = RSession::new_without_default_packages();

        instance::with_required_current_instance(|instance| unsafe {
            (*instance).gc_state.in_progress = true;
            assert_eq!(minor_gc_in(instance), (0, 0));
            assert!((*instance).gc_state.in_progress);
            (*instance).gc_state.in_progress = false;
        });
    }

    #[test]
    fn test_gc_stats_tracking() {
        let _session = RSession::new_without_default_packages();

        reset_gc_stats();
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                arena.alloc_node(SEXPTYPE::INTSXP);
            })
        });
        minor_gc();
        let stats = get_gc_stats();
        assert_eq!(stats.collections, 1);
        assert_eq!(stats.freed, 1);
    }

    #[test]
    fn test_gc_callback_invocation() {
        let _session = RSession::new_without_default_packages();

        reset_gc_stats();
        let (tx, rx) = std::sync::mpsc::channel();

        register_gc_callback(Box::new(move |_| {
            let _ = tx.send(());
        }));

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                arena.alloc_node(SEXPTYPE::INTSXP);
            })
        });
        minor_gc();

        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn test_remembered_set_null_handling() {
        let mut rs = RememberedSet::default();
        rs.add(ptr::null_mut());
        assert_eq!(rs.len(), 0);
    }

    #[test]
    fn test_write_barrier_null_handling() {
        write_barrier(ptr::null_mut(), ptr::null_mut());
        write_barrier(ptr::null_mut(), 0x1 as SEXP);
        write_barrier(0x1 as SEXP, ptr::null_mut());
    }

    #[test]
    fn safe_barriers_ignore_unregistered_inputs_without_reading_them() {
        let _session = RSession::new_for_gc_tests();
        let invalid = std::ptr::without_provenance_mut::<super::super::ffi::SexprecCore>(1);
        let mut remembered = RememberedSet::default();
        remembered.add(invalid);
        remembered.add(std::ptr::dangling_mut());
        assert_eq!(remembered.len(), 0);
        write_barrier(invalid, invalid);
        vector_write_barrier(invalid, 0, invalid);
        list_write_barrier(invalid, 0, invalid);
        attrib_write_barrier(invalid, invalid);
        assert_eq!(with_gc_state(|state| state.remembered_set.len()), 0);
    }

    #[test]
    fn remembered_identity_does_not_root_a_reused_generation() {
        let mut arena = RArena::new();
        let original = arena.alloc_node(SEXPTYPE::LISTSXP);
        unsafe {
            (*original).sxpinfo.set_gcgen(Generation::Old as u8);
        }
        let original_id = arena.node_token(original).unwrap();
        let mut remembered = RememberedSet::default();
        // Address-only inputs must recover the canonical cell's provenance.
        remembered.add(std::ptr::without_provenance_mut(original.addr()));
        assert_eq!(remembered.len(), 1);
        unsafe {
            arena.free_node(original);
        }
        let replacement = arena.alloc_node(SEXPTYPE::LISTSXP);
        assert_eq!(replacement, original);
        assert!(!original_id.is_live());
        assert!(remembered.checked_roots().is_empty());
        assert_eq!(remembered.iter().count(), 0);
        super::super::memory::begin_gc_epoch();
        let _trace = gc_trace::TraceScope::enter(gc_trace::TraceContext::new(
            arena.heap_identity(),
            super::super::memory::current_gc_epoch(),
        ));
        mark_checked_root_snapshot(remembered.checked_roots());
        assert!(!super::super::memory::arena_node_marked(replacement));
        unsafe {
            (*replacement).sxpinfo.set_gcgen(Generation::Old as u8);
        }
        remembered.add(replacement);
        assert_eq!(remembered.iter().count(), 1);
        remembered.remap(&HashMap::new());
        assert_eq!(remembered.len(), 1);
        remembered.add(replacement);
        assert_eq!(remembered.len(), 1);
    }

    #[test]
    fn checked_write_barrier_rejects_foreign_allocation_domains() {
        let mut left = instance::RInstance::new_for_gc_tests();
        let mut right = instance::RInstance::new_for_gc_tests();
        let parent = left.arena.alloc_node(SEXPTYPE::LISTSXP);
        let child = left.arena.alloc_node(SEXPTYPE::INTSXP);
        let foreign = right.arena.alloc_node(SEXPTYPE::INTSXP);
        unsafe {
            (*parent).sxpinfo.set_gcgen(Generation::Old as u8);
            let owner = &raw mut left;
            assert!(!write_barrier_in(owner, parent, foreign));
            assert!(!write_barrier_in(owner, foreign, child));
            assert!(!write_barrier_in(owner, parent, 1 as SEXP));
            assert_eq!((*owner).gc_state.remembered_set.len(), 0);
            assert!(write_barrier_in(owner, parent, child));
            assert_eq!((*owner).gc_state.remembered_set.len(), 1);
        }
    }

    #[test]
    fn test_full_gc_empty_arena() {
        let _session = RSession::new_without_default_packages();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
            })
        });
        let (promoted, freed) = full_gc();
        assert_eq!(promoted, 0);
        assert_eq!(freed, 0);
    }

    #[test]
    fn test_full_gc_collects_unreachable_old_objects() {
        let _session = RSession::new_without_default_packages();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                let obj = arena.alloc_node(SEXPTYPE::INTSXP);
                unsafe {
                    (*obj).sxpinfo.set_gcgen(Generation::Old as u8);
                }
            })
        });

        let (promoted, freed) = full_gc();
        assert_eq!(promoted, 0);
        assert_eq!(freed, 1);
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                assert_eq!(arena.node_count(), 0);
                assert_eq!(arena.free_count(), 1);
            })
        });
    }

    #[test]
    fn test_full_gc_preserves_protected_old_objects() {
        let _session = RSession::new_without_default_packages();

        use super::super::protect::protect;

        let mut obj = ptr::null_mut();
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                obj = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
                unsafe {
                    (*obj).sxpinfo.set_gcgen(Generation::Old as u8);
                    *(crate::sexp::accessors::INTEGER(obj)) = 42;
                }
                std::mem::forget(protect(obj));
            })
        });

        let (promoted, freed) = full_gc();
        assert_eq!(promoted, 0);
        assert_eq!(freed, 0);

        let protected_obj = with_protected_objects(|_legacy, roots| roots[0]);
        assert_eq!(protected_obj, obj);
        unsafe {
            assert_eq!((*protected_obj).sxpinfo.type_of(), SEXPTYPE::INTSXP);
            assert_eq!((*protected_obj).vecsxp_length(), 1);
            assert_eq!(*(crate::sexp::accessors::INTEGER(protected_obj)), 42);
        }
    }

    #[test]
    fn test_full_gc_never_moves_protected_objects() {
        let _session = RSession::new_without_default_packages();

        use super::super::protect::protect;

        let mut obj = ptr::null_mut();
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                obj = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
                unsafe {
                    (*obj).sxpinfo.set_gcgen(Generation::Old as u8);
                    *(crate::sexp::accessors::INTEGER(obj)) = 99;
                }
                std::mem::forget(protect(obj));
                arena.set_budget(ArenaBudget::new(1, 0));
            })
        });

        let (promoted, freed) = full_gc();
        assert_eq!((promoted, freed), (0, 0));

        let protected_obj = with_protected_objects(|_legacy, roots| roots[0]);
        assert_eq!(protected_obj, obj);
        unsafe {
            assert_eq!((*protected_obj).sxpinfo.type_of(), SEXPTYPE::INTSXP);
            assert_eq!(*(crate::sexp::accessors::INTEGER(protected_obj)), 99);
        }
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                assert!(arena.contains(obj));
            })
        });
    }

    /// GC soundness stress: many protected real vectors must retain their
    /// exact sentinel data across repeated full collections while unprotected
    /// garbage is churned underneath. This exercises the invariant the
    /// conformance suite does not cover: that GC never collects or corrupts a
    /// live (protected) object. A failure here would indicate premature
    /// collection or a mark/sweep bug.
    #[test]
    fn gc_stress_protected_vectors_retain_data_across_collections() {
        stress_protected_vectors(RSession::new_for_gc_tests());
    }

    #[test]
    fn gc_stress_protected_vectors_with_default_packages() {
        stress_protected_vectors(RSession::new());
    }

    fn stress_protected_vectors(_session: RSession) {
        use super::super::protect::protect;

        const N: usize = 64;
        const LEN: usize = 8;
        let mut keepers: Vec<SEXP> = Vec::with_capacity(N);
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                for i in 0..N {
                    let v = arena.alloc_vector(SEXPTYPE::REALSXP, LEN as i64);
                    unsafe {
                        let data = crate::sexp::accessors::REAL(v);
                        for j in 0..LEN {
                            *data.add(j) = (i as f64) * 1000.0 + j as f64;
                        }
                        std::mem::forget(protect(v));
                    }
                    keepers.push(v);
                }
            })
        });

        // Churn unprotected garbage and collect repeatedly; prove real work
        // happens (freed > 0) and no protected vector is touched.
        let mut any_freed = 0usize;
        for _ in 0..20 {
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena(|arena| {
                    for _ in 0..200 {
                        let _ = arena.alloc_vector(SEXPTYPE::REALSXP, 4);
                    }
                })
            });
            let (_promoted, freed) = full_gc();
            any_freed |= freed;
            for (i, v) in keepers.iter().enumerate() {
                unsafe {
                    assert_eq!((**v).sxpinfo.type_of(), SEXPTYPE::REALSXP);
                    let data = crate::sexp::accessors::REAL(*v);
                    for j in 0..LEN {
                        let expected = (i as f64) * 1000.0 + j as f64;
                        assert_eq!(*data.add(j), expected, "vector {i}[{j}] corrupted after GC");
                    }
                }
            }
        }
        assert!(any_freed > 0, "stress did not actually free any garbage");
    }

    #[test]
    fn test_full_gc_preserves_external_pointer_edges_without_moving() {
        let _session = RSession::new_without_default_packages();

        use super::super::protect::protect;

        let payload = 0x1234usize as *mut std::ffi::c_void;
        let mut ext = ptr::null_mut();
        let mut tag = ptr::null_mut();
        let mut prot = ptr::null_mut();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                tag = arena.alloc_node(SEXPTYPE::INTSXP);
                prot = arena.alloc_node(SEXPTYPE::REALSXP);
                ext = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
                unsafe {
                    (*ext).sxpinfo.set_gcgen(Generation::Old as u8);
                    *(*ext).data.extptr_mut() = crate::sexp::ffi::ExtPtrBody {
                        address: payload,
                        protected: arena.link_from_projection(prot).unwrap(),
                        tag: arena.link_from_projection(tag).unwrap(),
                    };
                }
                std::mem::forget(protect(ext));
            })
        });

        let (_, freed) = full_gc();
        assert_eq!(freed, 0);

        let protected_ext = with_protected_objects(|_legacy, roots| roots[0]);
        assert_eq!(protected_ext, ext);
        unsafe {
            assert_eq!((*protected_ext).sxpinfo.type_of(), SEXPTYPE::EXTPTRSXP);
            assert_eq!((*protected_ext).data.extptr().address, payload);
            let linked_prot = crate::mainutils::memory_main::R_ExternalPtrProtected(protected_ext);
            let linked_tag = crate::mainutils::memory_main::R_ExternalPtrTag(protected_ext);
            assert_eq!(linked_prot, prot);
            assert_eq!(linked_tag, tag);
            assert_eq!((*linked_prot).sxpinfo.type_of(), SEXPTYPE::REALSXP);
            assert_eq!((*linked_tag).sxpinfo.type_of(), SEXPTYPE::INTSXP);
            with_arena(|arena| {
                assert!(arena.contains(protected_ext));
                assert!(arena.contains(linked_prot));
                assert!(arena.contains(linked_tag));
            });
        }
    }

    #[test]
    fn test_full_gc_weakref_traces_value_and_finalizer_but_not_key() {
        let _session = RSession::new_without_default_packages();

        use super::super::protect::protect;

        let mut weak = ptr::null_mut();
        let mut key = ptr::null_mut();
        let mut value = ptr::null_mut();
        let mut finalizer = ptr::null_mut();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                key = arena.alloc_node(SEXPTYPE::INTSXP);
                value = arena.alloc_node(SEXPTYPE::REALSXP);
                finalizer = arena.alloc_node(SEXPTYPE::LISTSXP);
                weak = arena.alloc_node(SEXPTYPE::WEAKREFSXP);
                unsafe {
                    for obj in [key, value, finalizer, weak] {
                        (*obj).sxpinfo.set_gcgen(Generation::Old as u8);
                    }
                    (*weak).data.list_mut().carval = arena.link_from_projection(key).unwrap();
                    (*weak).data.list_mut().cdrval = arena.link_from_projection(value).unwrap();
                    (*weak).data.list_mut().tagval = arena.link_from_projection(finalizer).unwrap();
                }
                std::mem::forget(protect(weak));
            })
        });

        let (_, freed) = full_gc();
        assert_eq!(freed, 1);

        let protected_weak = with_protected_objects(|_legacy, roots| roots[0]);
        assert_eq!(protected_weak, weak);
        unsafe {
            assert_eq!((*protected_weak).sxpinfo.type_of(), SEXPTYPE::WEAKREFSXP);
            assert_eq!(
                super::super::accessors::CAR(protected_weak),
                crate::sexp::globals::R_NilValue()
            );
            let linked_value = super::super::accessors::CDR(protected_weak);
            let linked_finalizer = super::super::accessors::TAG(protected_weak);
            assert_eq!(linked_value, value);
            assert_eq!(linked_finalizer, finalizer);
            assert_eq!((*linked_value).sxpinfo.type_of(), SEXPTYPE::REALSXP);
            assert_eq!((*linked_finalizer).sxpinfo.type_of(), SEXPTYPE::LISTSXP);
            with_arena(|arena| {
                assert!(arena.contains(protected_weak));
                assert!(arena.contains(linked_value));
                assert!(arena.contains(linked_finalizer));
                assert!(!arena.contains(key));
            });
        }
    }

    #[test]
    fn test_full_gc_weakref_forwards_live_key_without_marking_it() {
        let _session = RSession::new_without_default_packages();

        use super::super::protect::protect;

        let mut weak = ptr::null_mut();
        let mut key = ptr::null_mut();
        let mut value = ptr::null_mut();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                key = arena.alloc_node(SEXPTYPE::INTSXP);
                value = arena.alloc_node(SEXPTYPE::REALSXP);
                weak = arena.alloc_node(SEXPTYPE::WEAKREFSXP);
                unsafe {
                    for obj in [key, value, weak] {
                        (*obj).sxpinfo.set_gcgen(Generation::Old as u8);
                    }
                    (*weak).data.list_mut().carval = arena.link_from_projection(key).unwrap();
                    (*weak).data.list_mut().cdrval = arena.link_from_projection(value).unwrap();
                    (*weak).data.list_mut().tagval = arena
                        .link_from_projection(crate::sexp::globals::R_NilValue())
                        .unwrap();
                }
                std::mem::forget(protect(weak));
                std::mem::forget(protect(key));
            })
        });

        let (_, freed) = full_gc();
        assert_eq!(freed, 0);

        let (protected_weak, protected_key) =
            with_protected_objects(|_legacy, roots| (roots[0], roots[1]));
        assert_eq!(protected_weak, weak);
        assert_eq!(protected_key, key);
        unsafe {
            assert_eq!((*protected_weak).sxpinfo.type_of(), SEXPTYPE::WEAKREFSXP);
            let linked_key = super::super::accessors::CAR(protected_weak);
            let linked_value = super::super::accessors::CDR(protected_weak);
            assert_eq!(linked_key, protected_key);
            assert_eq!(linked_key, key);
            assert_eq!(linked_value, value);
            assert_eq!((*linked_key).sxpinfo.type_of(), SEXPTYPE::INTSXP);
            assert_eq!((*linked_value).sxpinfo.type_of(), SEXPTYPE::REALSXP);
            with_arena(|arena| {
                assert!(arena.contains(protected_weak));
                assert!(arena.contains(linked_key));
                assert!(arena.contains(linked_value));
            });
        }
    }

    #[test]
    fn test_gc_stats_reset() {
        let _session = RSession::new_without_default_packages();

        reset_gc_stats();
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                arena.alloc_node(SEXPTYPE::INTSXP);
                arena.alloc_node(SEXPTYPE::REALSXP);
            })
        });
        minor_gc();
        let stats = get_gc_stats();
        assert!(stats.collections > 0);

        reset_gc_stats();
        let stats = get_gc_stats();
        assert_eq!(stats.collections, 0);
        assert_eq!(stats.freed, 0);
    }

    #[test]
    fn test_session_gc_stats_are_local_on_same_thread() {
        let mut left = RSession::new_without_default_packages();
        let mut right = RSession::new_without_default_packages();
        let right_initial_collections = right.with_active(|| get_gc_stats().collections);

        left.with_arena(|arena| {
            reset_gc_stats();
            reset_gc_test_arena(arena);
            arena.alloc_node(SEXPTYPE::INTSXP);
        })
        .unwrap();
        left.with_active(|| {
            let (_, freed) = minor_gc();
            assert_eq!(freed, 1);
            assert_eq!(get_gc_stats().collections, 1);
        });

        right
            .with_arena(|arena| {
                assert_eq!(get_gc_stats().collections, right_initial_collections);
                reset_gc_stats();
                reset_gc_test_arena(arena);
                arena.alloc_node(SEXPTYPE::INTSXP);
                arena.alloc_node(SEXPTYPE::REALSXP);
            })
            .unwrap();
        right.with_active(|| {
            let (_, freed) = minor_gc();
            assert_eq!(freed, 2);
            assert_eq!(get_gc_stats().collections, 1);
        });

        left.with_active(|| {
            let stats = get_gc_stats();
            assert_eq!(stats.collections, 1);
            assert_eq!(stats.freed, 1);
        });
    }

    #[test]
    fn test_session_remembered_sets_are_local_on_same_thread() {
        let mut left = RSession::new_without_default_packages();
        let mut right = RSession::new_without_default_packages();

        left.with_arena(|arena| {
            reset_gc_test_arena(arena);
            let old_obj = arena.alloc_node(SEXPTYPE::LISTSXP);
            let young_obj = arena.alloc_node(SEXPTYPE::INTSXP);
            unsafe {
                (*old_obj).sxpinfo.set_gcgen(Generation::Old as u8);
                (*young_obj).sxpinfo.set_gcgen(Generation::Young as u8);
            }
            write_barrier(old_obj, young_obj);
            assert_eq!(with_gc_state(|state| state.remembered_set.len()), 1);
        })
        .unwrap();

        right
            .with_arena(|arena| {
                reset_gc_test_arena(arena);
                assert_eq!(with_gc_state(|state| state.remembered_set.len()), 0);
                let old_obj = arena.alloc_node(SEXPTYPE::LISTSXP);
                let young_obj = arena.alloc_node(SEXPTYPE::INTSXP);
                unsafe {
                    (*old_obj).sxpinfo.set_gcgen(Generation::Old as u8);
                    (*young_obj).sxpinfo.set_gcgen(Generation::Young as u8);
                }
                write_barrier(old_obj, young_obj);
                assert_eq!(with_gc_state(|state| state.remembered_set.len()), 1);
            })
            .unwrap();
        right.with_active(|| {
            minor_gc();
            assert_eq!(with_gc_state(|state| state.remembered_set.len()), 0);
        });

        left.with_active(|| {
            assert_eq!(with_gc_state(|state| state.remembered_set.len()), 1);
            minor_gc();
            assert_eq!(with_gc_state(|state| state.remembered_set.len()), 0);
        });
    }

    #[test]
    fn test_promote_to_old_null_handling() {
        unsafe {
            promote_to_old(ptr::null_mut());
        }
    }

    #[test]
    fn test_vector_slot_null_handling() {
        let mut slot: SEXP = ptr::null_mut();
        let vec_slot = VectorSlot::new(ptr::null_mut(), &mut slot);
        assert!(vec_slot.get().is_null());
    }

    #[test]
    fn test_gc_deterministic_behavior() {
        let _session = RSession::new_without_default_packages();

        for _ in 0..5 {
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena(|arena| {
                    reset_gc_test_arena(arena);
                    let obj1 = arena.alloc_node(SEXPTYPE::INTSXP);
                    let obj2 = arena.alloc_node(SEXPTYPE::REALSXP);
                    unsafe {
                        (*obj1).sxpinfo.set_gcgen(Generation::Young as u8);
                        (*obj2).sxpinfo.set_gcgen(Generation::Young as u8);
                    }
                })
            });
            let (promoted, freed) = minor_gc();
            assert_eq!(promoted, 0);
            assert_eq!(freed, 2);
        }
    }

    #[test]
    fn test_gc_with_protected_objects() {
        let _session = RSession::new_without_default_packages();

        use super::super::protect::protect;

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                let obj = arena.alloc_node(SEXPTYPE::INTSXP);
                unsafe {
                    (*obj).sxpinfo.set_gcgen(Generation::Young as u8);
                }
                std::mem::forget(protect(obj));
            })
        });
        let (promoted, freed) = minor_gc();
        assert_eq!(promoted, 1);
        assert_eq!(freed, 0);
    }

    #[test]
    fn test_compact_if_needed_runs_gc_but_never_moves() {
        let _session = RSession::new_without_default_packages();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
            })
        });
        let result = compact_if_needed(0.0);
        assert!(!result);
    }

    #[test]
    fn test_get_fragmentation_ratio_empty() {
        let _session = RSession::new_without_default_packages();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
            })
        });
        let ratio = get_fragmentation_ratio();
        assert_eq!(ratio, 0.0);
    }

    #[test]
    fn test_force_compact_only_normalizes_free_list() {
        let _session = RSession::new_without_default_packages();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
            })
        });
        force_compact();
    }

    #[test]
    fn test_minor_gc_does_not_refree_nodes_on_free_list() {
        let _session = RSession::new_without_default_packages();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                arena.alloc_vector(SEXPTYPE::REALSXP, 2);
            })
        });

        let (_, freed1) = minor_gc();
        assert_eq!(freed1, 1);
        let free_after_first = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| arena.free_count())
        };

        let (_, freed2) = minor_gc();
        let free_after_second = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| arena.free_count())
        };
        assert_eq!(freed2, 0);
        assert_eq!(free_after_second, free_after_first);
    }

    #[test]
    fn test_update_object_references_skips_atomic_vector_payloads() {
        let _session = RSession::new_without_default_packages();

        let mut marker: SEXP = ptr::null_mut();
        let mut vec: SEXP = ptr::null_mut();
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                marker = arena.alloc_node(SEXPTYPE::LISTSXP);
                vec = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
            })
        });

        let original_bits = marker as usize as u64;
        unsafe {
            *(crate::sexp::accessors::DATAPTR(vec) as *mut f64) = f64::from_bits(original_bits);
        }

        let replacement = unsafe { crate::sexp::globals::R_NilValue() };
        let mut map = HashMap::new();
        let (_, marker_token) = super::super::memory::checked_projection(marker).unwrap();
        let heap = marker_token.heap_identity();
        map.insert(
            marker_token.link().unwrap(),
            heap.link_from_projection(replacement).unwrap(),
        );
        update_object_references(&map);

        let after_bits = unsafe { *(crate::sexp::accessors::DATAPTR(vec) as *const f64) }.to_bits();
        assert_eq!(after_bits, original_bits);
    }

    #[test]
    fn test_update_object_references_updates_pointer_vector_payloads() {
        let _session = RSession::new_without_default_packages();

        let mut marker: SEXP = ptr::null_mut();
        let mut vec: SEXP = ptr::null_mut();
        let mut replacement: SEXP = ptr::null_mut();
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                marker = arena.alloc_node(SEXPTYPE::LISTSXP);
                vec = arena.alloc_vector(SEXPTYPE::VECSXP, 1);
                replacement = arena.alloc_node(SEXPTYPE::LISTSXP);
                arena
                    .set_reference_element(vec, 0, marker)
                    .expect("fixture reference slot");
            })
        });
        let mut map = HashMap::new();
        let (_, marker_token) = super::super::memory::checked_projection(marker).unwrap();
        let heap = marker_token.heap_identity();
        map.insert(
            marker_token.link().unwrap(),
            heap.link_from_projection(replacement).unwrap(),
        );
        update_object_references(&map);

        let (_, node) = super::super::memory::checked_projection(vec).expect("fixture vector");
        let after_ptr = node
            .heap_identity()
            .reference_elt(&node, 0)
            .expect("fixture reference slot");
        assert_eq!(after_ptr, replacement);
    }

    #[test]
    fn graph_rewrite_distinguishes_recycled_allocation_generations() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let (parent, vector, replacement, old_link, nil_link) = unsafe {
                with_arena(|arena| {
                    let old = arena.alloc_node(SEXPTYPE::LISTSXP);
                    let old_link = arena.link_from_projection(old).unwrap();
                    let parent = arena.alloc_node(SEXPTYPE::LISTSXP);
                    (*parent).data.list_mut().carval = old_link;
                    let vector = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
                    arena.set_reference_element(vector, 0, old).unwrap();
                    arena.free_node(old);
                    let replacement = arena.alloc_node(SEXPTYPE::LISTSXP);
                    assert_eq!(replacement, old);
                    let current_link = arena.link_from_projection(replacement).unwrap();
                    assert_ne!(current_link, old_link);
                    (*parent).data.list_mut().cdrval = current_link;
                    arena.set_reference_element(vector, 1, replacement).unwrap();
                    let nil_link = arena
                        .link_from_projection(crate::sexp::globals::R_NilValue())
                        .unwrap();
                    (parent, vector, replacement, old_link, nil_link)
                })
            };
            let map = HashMap::from([(old_link, nil_link)]);
            update_references_in_object(parent, &map);
            update_references_in_object(vector, &map);
            unsafe {
                assert_eq!(
                    super::super::accessors::CAR(parent),
                    crate::sexp::globals::R_NilValue()
                );
                assert_eq!(super::super::accessors::CDR(parent), replacement);
                assert_eq!(
                    super::super::accessors::VECTOR_ELT(vector, 0),
                    crate::sexp::globals::R_NilValue()
                );
                assert_eq!(super::super::accessors::VECTOR_ELT(vector, 1), replacement);
            }
        });
    }

    #[test]
    fn copied_child_visitors_preserve_reentrant_header_updates() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let (parent, child_link, replacement_link) = unsafe {
                with_arena(|arena| {
                    let child = arena.alloc_node(SEXPTYPE::LISTSXP);
                    let replacement = arena.alloc_node(SEXPTYPE::LISTSXP);
                    let parent = arena.alloc_node(SEXPTYPE::LISTSXP);
                    let child_link = arena.link_from_projection(child).unwrap();
                    (*parent).data.list_mut().carval = child_link;
                    (
                        parent,
                        child_link,
                        arena.link_from_projection(replacement).unwrap(),
                    )
                })
            };
            let (_, token) = super::super::memory::checked_projection(parent).unwrap();
            let heap = token.heap_identity();
            each_child(parent, true, |link| {
                if *link == child_link {
                    let mut header = heap.node_snapshot(&token).unwrap();
                    header.sxpinfo.set_named(2);
                    heap.replace_node(&token, header).unwrap();
                    *link = replacement_link;
                }
            });
            let header = heap.node_snapshot(&token).unwrap();
            assert_eq!(header.sxpinfo.named(), 2);
            assert_eq!(header.data.list().carval, replacement_link);
        });
    }

    #[test]
    fn checked_gc_clears_dead_key_in_unlisted_permanent_weak_header() {
        for collect in [minor_gc as fn() -> (usize, usize), full_gc] {
            let session = RSession::new_for_gc_tests();
            session.with_active(|| {
                // SAFETY: this active fixture publishes a valid weak header
                // under one arena lend, before any deferred collection runs.
                let (key, key_token, value, weak, weak_token) = unsafe {
                    with_arena(|arena| {
                        let key = arena.alloc_node(SEXPTYPE::INTSXP);
                        let key_token = arena.node_token(key).unwrap();
                        let value = arena.alloc_node(SEXPTYPE::REALSXP);
                        let nil = crate::sexp::globals::R_NilValue();
                        let mut header = crate::sexp::ffi::SexprecCore::new(SEXPTYPE::WEAKREFSXP);
                        header.attrib = arena.link_from_projection(nil).unwrap();
                        header.data = crate::sexp::ffi::NodeBody::List(crate::sexp::ffi::Listsxp {
                            carval: arena.link_from_projection(key).unwrap(),
                            cdrval: arena.link_from_projection(value).unwrap(),
                            tagval: arena.link_from_projection(nil).unwrap(),
                        });
                        instance::with_required_current_instance(|owner| {
                            let weak = (*owner).persistent_nodes.allocate_header(header).unwrap();
                            let token = (*owner).persistent_nodes.token(weak).unwrap();
                            assert!(!(*owner).raw_cons.contains(&weak));
                            assert!(!(*owner).symbol_nodes.contains(&weak));
                            assert!(!(*owner).env_nodes.contains(&weak));
                            (key, key_token, value, weak, token)
                        })
                    })
                };
                collect();
                assert!(!key_token.is_live(), "weak key must be reclaimed");
                assert!(weak_token.is_live());
                // SAFETY: membership is inspected without dereferencing the
                // dead key; only the live permanent header is read afterward.
                unsafe {
                    with_arena(|arena| {
                        assert!(!arena.contains(key));
                        assert!(arena.contains(value));
                    });
                    assert_eq!(
                        super::super::accessors::CAR(weak),
                        crate::sexp::globals::R_NilValue()
                    );
                    assert_eq!(super::super::accessors::CDR(weak), value);
                }
                full_gc();
                // SAFETY: the next cycle must retain the strong value and
                // leave the already-cleared weak key as immutable nil.
                unsafe {
                    assert_eq!(
                        super::super::accessors::CAR(weak),
                        crate::sexp::globals::R_NilValue()
                    );
                    assert!(with_arena(|arena| arena.contains(value)));
                }
            });
        }
    }

    #[test]
    fn checked_gc_rejects_stale_child_before_dereference_and_restores_scope() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            // The saved generation is invalidated before its physical slot is
            // reused. The graph must not silently capture the replacement.
            let (node, _root) = unsafe {
                with_arena(|arena| {
                    let old = arena.alloc_node(SEXPTYPE::LISTSXP);
                    let old_link = arena.link_from_projection(old).unwrap();
                    arena.free_node(old);
                    let replacement = arena.alloc_node(SEXPTYPE::LISTSXP);
                    assert_eq!(replacement, old);
                    let node = arena.alloc_node(SEXPTYPE::LISTSXP);
                    (*node).data.list_mut().carval = old_link;
                    let root = crate::sexp::protect::protect(node);
                    (node, root)
                })
            };
            let failure = std::panic::catch_unwind(full_gc)
                .expect_err("stale graph generation must fail before header access");
            let message = failure
                .downcast_ref::<String>()
                .expect("GC failure message");
            assert!(message.contains("InvalidLink"), "{message}");
            // The failed mark cycle performs no sweeping. Restore the actual
            // live root before retrying the collection scope.
            unsafe {
                super::super::accessors::SETCAR(node, crate::sexp::globals::R_NilValue());
            }
            full_gc();
            assert!(unsafe { with_arena(|arena| arena.contains(node)) });
        });
    }

    #[test]
    fn deeply_nested_cyclic_graph_uses_bounded_call_stack() {
        let _session = RSession::new_without_default_packages();
        let (head, tail) = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                let mut head = unsafe { crate::sexp::globals::R_NilValue() };
                let mut tail = head;
                let depth = if cfg!(miri) { 256 } else { 30_000 };
                for i in 0..depth {
                    let node = arena.alloc_node(SEXPTYPE::LISTSXP);
                    unsafe {
                        (*node).data.list_mut().cdrval = arena.link_from_projection(head).unwrap();
                    }
                    if i == 0 {
                        tail = node;
                    }
                    head = node;
                }
                // A back-edge also verifies that marking terminates on cycles.
                unsafe {
                    super::super::accessors::SETCAR(tail, head);
                }
                (head, tail)
            })
        };
        let _root = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            crate::sexp::protect::protect(head)
        };
        full_gc();
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                assert!(arena.active_nodes().any(|node| node == tail));
            })
        });
        unsafe {
            assert_eq!(super::super::accessors::CAR(tail), head);
        }
    }

    #[test]
    fn test_dotsxp_chain_is_traced_from_protected_head() {
        let _session = RSession::new_without_default_packages();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);

                let sym_a = arena.alloc_node(SEXPTYPE::SYMSXP);
                let sym_b = arena.alloc_node(SEXPTYPE::SYMSXP);
                let one = arena.alloc_node(SEXPTYPE::INTSXP);
                let two = arena.alloc_node(SEXPTYPE::INTSXP);
                let tail = arena.alloc_node(SEXPTYPE::DOTSXP);
                let head = arena.alloc_node(SEXPTYPE::DOTSXP);
                let nil = unsafe { crate::sexp::globals::R_NilValue() };
                unsafe {
                    (*tail).data.list_mut().tagval = arena.link_from_projection(sym_b).unwrap();
                    (*tail).data.list_mut().carval = arena.link_from_projection(two).unwrap();
                    (*tail).data.list_mut().cdrval = arena.link_from_projection(nil).unwrap();
                    (*head).data.list_mut().tagval = arena.link_from_projection(sym_a).unwrap();
                    (*head).data.list_mut().carval = arena.link_from_projection(one).unwrap();
                    (*head).data.list_mut().cdrval = arena.link_from_projection(tail).unwrap();
                }

                // Only the chain head is rooted; the cells beyond it are reachable
                // exclusively through the DOTSXP tracing arm.
                instance::with_required_current_instance(|inst| {
                    push_protect_in(inst, head);
                });

                minor_gc();

                let active: Vec<SEXP> = arena.active_nodes().collect();
                assert!(active.contains(&tail), "DOTSXP tail cell was swept");
                assert!(active.contains(&one), "DOTSXP car value was swept");
                assert!(active.contains(&two), "DOTSXP tail car value was swept");
                unsafe {
                    assert_eq!(super::super::accessors::CDR(head), tail);
                    assert_eq!(super::super::accessors::CAR(tail), two);
                }
            })
        });
    }

    #[test]
    fn test_remembered_old_list_keeps_young_car_across_minor_gc() {
        let _session = RSession::new_without_default_packages();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);

                let parent = arena.alloc_node(SEXPTYPE::LISTSXP);
                let child = arena.alloc_node(SEXPTYPE::INTSXP);
                let nil = unsafe { crate::sexp::globals::R_NilValue() };
                unsafe {
                    (*parent).sxpinfo.set_gcgen(Generation::Old as u8);
                    (*parent).data.list_mut().carval = arena.link_from_projection(child).unwrap();
                    (*parent).data.list_mut().cdrval = arena.link_from_projection(nil).unwrap();
                    (*parent).data.list_mut().tagval = arena.link_from_projection(nil).unwrap();
                    (*child).sxpinfo.set_gcgen(Generation::Young as u8);
                }

                // The parent is intentionally not rooted anywhere else: the
                // remembered set is the only path that must keep the young child
                // alive through a minor collection.
                write_barrier(parent, child);
                unsafe {
                    assert!(
                        !(*parent).sxpinfo.mark(),
                        "write_barrier must not borrow the mark bit for membership"
                    );
                }
                assert_eq!(with_gc_state(|state| state.remembered_set.len()), 1);

                minor_gc();

                let active: Vec<SEXP> = arena.active_nodes().collect();
                assert!(
                    active.contains(&child),
                    "young child of a remembered old parent was swept"
                );
                unsafe {
                    assert_eq!(super::super::accessors::CAR(parent), child);
                }
            })
        });
    }

    #[test]
    fn test_remembered_vector_element_survives_and_dedupes() {
        let _session = RSession::new_without_default_packages();

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                reset_gc_test_arena(arena);

                use crate::sexp::accessors::{SET_VECTOR_ELT, VECTOR_ELT};
                let parent = arena.alloc_vector(SEXPTYPE::VECSXP, 1);
                let child = arena.alloc_node(SEXPTYPE::STRSXP);
                unsafe {
                    (*parent).sxpinfo.set_gcgen(Generation::Old as u8);
                    (*child).sxpinfo.set_gcgen(Generation::Young as u8);
                    SET_VECTOR_ELT(parent, 0, child);
                }

                assert_eq!(with_gc_state(|state| state.remembered_set.len()), 1);
                unsafe {
                    SET_VECTOR_ELT(parent, 0, child);
                }
                assert_eq!(
                    with_gc_state(|state| state.remembered_set.len()),
                    1,
                    "duplicate barrier calls must deduplicate"
                );
                unsafe {
                    assert!(!(*parent).sxpinfo.mark());
                }

                minor_gc();

                let active: Vec<SEXP> = arena.active_nodes().collect();
                assert!(
                    active.contains(&child),
                    "young element of a remembered old vector was swept"
                );
                unsafe {
                    assert_eq!(VECTOR_ELT(parent, 0), child);
                }
            })
        });
    }

    #[test]
    fn test_remembered_set_remap_updates_membership() {
        let mut inst = instance::RInstance::new();
        let previous = inst.gc_state.remembered_set.len();
        let old = inst.arena.alloc_node(SEXPTYPE::INTSXP);
        let new = inst.arena.alloc_node(SEXPTYPE::REALSXP);
        unsafe {
            (*old).sxpinfo.set_gcgen(Generation::Old as u8);
            (*new).sxpinfo.set_gcgen(Generation::Old as u8);
        }

        inst.gc_state.remembered_set.add(old);
        let mut map = HashMap::new();
        map.insert(old as usize, new);
        update_remembered_set_in(&mut inst, &map);

        assert!(inst.gc_state.remembered_set.iter().any(|obj| obj == new));
        // Membership follows the remapped address: re-adding the new pointer
        // deduplicates, while the stale old address is no longer a member.
        inst.gc_state.remembered_set.add(new);
        assert_eq!(inst.gc_state.remembered_set.len(), previous + 1);
        inst.gc_state.remembered_set.add(old);
        assert_eq!(inst.gc_state.remembered_set.len(), previous + 2);
    }

    fn make_detached_env() -> SEXP {
        unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                let env = arena.alloc_node(SEXPTYPE::ENVSXP);
                unsafe {
                    super::super::accessors::SET_FRAME(env, crate::sexp::globals::R_NilValue());
                    super::super::accessors::SET_ENCLOS(env, crate::sexp::globals::R_NilValue());
                }
                env
            })
        }
    }

    /// The package namespace cache is the only root for a pure-R package
    /// namespace once attach-time references die. Untraced, a collection
    /// swept the namespace env and left a dangling raw SEXP in the cache.
    #[test]
    fn test_package_namespace_cache_value_is_traced() {
        let _session = RSession::new_without_default_packages();

        let payload = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
        };
        unsafe {
            *crate::sexp::accessors::INTEGER(payload) = 4242;
        }
        let namespace = make_detached_env();
        unsafe {
            super::super::accessors::SET_FRAME(namespace, payload);
        }
        instance::with_required_current_instance(|inst| unsafe {
            (*inst).package_namespace_cache.insert(
                "gcProbePkg".to_string(),
                (std::path::PathBuf::from("/gc-probe"), namespace),
            );
        });

        // The namespace env and its frame are reachable only through the
        // cache; both cycles must keep them, not just the first.
        for _ in 0..2 {
            full_gc();
            assert!(
                (unsafe {
                    /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                    with_arena(|arena| arena.contains(namespace))
                }),
                "cached namespace env swept"
            );
            assert!(
                (unsafe {
                    /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                    with_arena(|arena| arena.contains(payload))
                }),
                "cached namespace frame swept"
            );
            assert_eq!(unsafe { *crate::sexp::accessors::INTEGER(payload) }, 4242);
        }
    }

    /// Exercise the public namespace-lookup path, not just the cache data
    /// structure: after the first lookup returns, the namespace is retained
    /// only by `package_namespace_cache`. A full collection must preserve the
    /// environment and its exported binding for the next `::` lookup.
    #[test]
    fn test_cache_only_namespace_survives_full_gc_and_colon_lookup() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after Unix epoch")
            .as_nanos();
        let library = std::env::temp_dir().join(format!(
            "rport-namespace-gc-{}-{unique}",
            std::process::id()
        ));
        let package = library.join("gcProbePkg");
        std::fs::create_dir_all(package.join("R")).expect("create package fixture");
        std::fs::write(
            package.join("DESCRIPTION"),
            "Package: gcProbePkg\nVersion: 0.0.1\n",
        )
        .expect("write DESCRIPTION");
        std::fs::write(package.join("NAMESPACE"), "export(answer)\n").expect("write NAMESPACE");
        std::fs::write(package.join("R").join("answer.R"), "answer <- 4242\n")
            .expect("write package source");

        let mut session = RSession::new_without_default_packages();
        let library_literal = library
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'");
        let first_lookup = format!(".libPaths('{library_literal}'); gcProbePkg::answer");
        {
            let (result, _, _) = session.eval_script_with_output_capture(&first_lookup);
            let value = result.expect("initial namespace lookup");
            assert_eq!(
                unsafe { *crate::sexp::accessors::REAL(value.as_raw()) },
                4242.0
            );
        }

        let cached_namespace = instance::with_required_current_instance(|inst| unsafe {
            (*inst)
                .package_namespace_cache
                .get("gcProbePkg")
                .expect("namespace cached")
                .1
        });
        full_gc();
        assert!(
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena(|arena| arena.contains(cached_namespace))
            }),
            "cache-only namespace was swept"
        );

        {
            let (result, _, _) = session.eval_script_with_output_capture("gcProbePkg::answer");
            let value = result.expect("cached namespace lookup after full GC");
            assert_eq!(
                unsafe { *crate::sexp::accessors::REAL(value.as_raw()) },
                4242.0
            );
        }

        std::fs::remove_dir_all(library).expect("remove package fixture");
    }

    /// Root-bearing runtime caches outside the arena must share the same
    /// mark/remap contract as the namespace cache.
    #[test]
    fn test_auxiliary_instance_sexp_roots_are_traced_and_remapped() {
        let _session = RSession::new_without_default_packages();
        const NAMESPACE_ROOT: usize = if cfg!(target_arch = "wasm32") { 3 } else { 6 };
        let roots = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                (0..=NAMESPACE_ROOT)
                    .map(|_| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
                    .collect::<Vec<_>>()
            })
        };

        instance::with_required_current_instance(|inst| unsafe {
            (*inst).error_state.warning_call = instance::RuntimeValue::from_raw_in(inst, roots[0]);
            (*inst).objects_state.deferred_default_object =
                instance::RuntimeValue::from_raw_in(inst, roots[1]);
            unsafe { (*inst).eval_state.bc_stack.push(roots[2]) };
            #[cfg(not(target_arch = "wasm32"))]
            {
                let mut http_roots = roots[3..6].iter().copied();
                (*inst)
                    .httpd_state
                    .visit_roots(|slot| *slot = http_roots.next().expect("HTTP root slot"));
            }
            (*inst).package_namespace_cache.insert(
                "rootProbePkg".to_string(),
                (
                    std::path::PathBuf::from("/root-probe"),
                    roots[NAMESPACE_ROOT],
                ),
            );
        });

        full_gc();
        for &root in &roots {
            assert!(
                (unsafe {
                    /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                    with_arena(|arena| arena.contains(root))
                }),
                "instance-owned root was swept"
            );
        }

        let replacements = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                (0..roots.len())
                    .map(|_| arena.alloc_vector(SEXPTYPE::REALSXP, 1))
                    .collect::<Vec<_>>()
            })
        };
        let remap = roots
            .iter()
            .copied()
            .zip(replacements.iter().copied())
            .map(|(old, new)| (old as usize, new))
            .collect::<HashMap<_, _>>();
        instance::with_required_current_instance(|inst| update_instance_roots_in(inst, &remap));

        instance::with_required_current_instance(|inst| unsafe {
            assert_eq!(
                (*inst).error_state.warning_call.as_raw(),
                roots[0],
                "owning error fields preserve original allocation identity"
            );
            assert_eq!(
                (*inst).objects_state.deferred_default_object.as_raw(),
                roots[1],
                "owning S4 fields preserve original allocation identity"
            );
            let bytecode_root = Some((*inst).eval_state.bc_stack.at_owned(0).as_raw());
            assert_eq!(
                bytecode_root,
                Some(roots[2]),
                "owning bytecode entries preserve original allocation identity"
            );
            #[cfg(not(target_arch = "wasm32"))]
            {
                let mut http_roots = Vec::new();
                (*inst)
                    .httpd_state
                    .visit_roots(|root| http_roots.push(*root));
                assert_eq!(http_roots, replacements[3..6]);
            }
            let cached_root = {
                let cache = unsafe { &(*inst).package_namespace_cache };
                cache["rootProbePkg"].1
            };
            assert_eq!(cached_root, replacements[NAMESPACE_ROOT]);
        });
    }

    /// Active-binding values must be traced while their entry exists, and an
    /// entry keyed by a collected env/symbol must be swept before the LIFO
    /// free list recycles the address onto a new node.
    #[test]
    fn test_active_bindings_traced_and_dead_entries_swept() {
        let _session = RSession::new_without_default_packages();

        let sym =
            unsafe { crate::sexp::symbol::Rf_install(b"active_probe\0".as_ptr() as *const _) };
        let env = make_detached_env();
        // The binding function is reachable only through the side table.
        let fun = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
        };
        unsafe {
            *crate::sexp::accessors::INTEGER(fun) = 7;
        }
        instance::with_required_current_instance(|inst| unsafe {
            (*inst)
                .active_bindings
                .insert((env as usize, sym as usize), fun);
        });

        full_gc();
        assert!(
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena(|arena| arena.contains(fun))
            }),
            "active binding value swept while its entry was still live"
        );
        assert_eq!(unsafe { *crate::sexp::accessors::INTEGER(fun) }, 7);

        // The env is held only by a raw local the collector does not scan, so
        // this cycle reclaims it and the side-table entry must go with it.
        let env_addr = env as usize;
        full_gc();
        assert!(
            !(unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena(|arena| arena.contains(env))
            })
        );
        instance::with_required_current_instance(|inst| unsafe {
            assert!(
                !(*inst)
                    .active_bindings
                    .contains_key(&(env_addr, sym as usize)),
                "stale active binding entry survived the keyed env sweep"
            );
        });

        // Recycle the reclaimed address: the stale entry must not alias the
        // new node (a fresh env reporting an active binding it never had).
        let mut recycled = std::ptr::null_mut();
        for _ in 0..1024 {
            recycled = unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            };
            if recycled as usize == env_addr {
                break;
            }
        }
        assert_eq!(recycled as usize, env_addr, "address never recycled");
        assert!(!crate::sexp::envir::binding_is_active_raw(recycled, sym));
    }

    /// Locked-environment and locked-binding entries die with their keyed
    /// nodes; entries whose keys stay live must survive collections.
    #[test]
    fn test_locked_tables_swept_with_dead_keys_live_keys_kept() {
        let _session = RSession::new_without_default_packages();

        let sym = unsafe { crate::sexp::symbol::Rf_install(b"lock_probe\0".as_ptr() as *const _) };
        let live_env = make_detached_env();
        let dead_env = make_detached_env();
        crate::sexp::envir::lock_environment_raw(dead_env);
        crate::sexp::envir::lock_environment_raw(live_env);
        crate::sexp::envir::lock_binding_raw(dead_env, sym);
        crate::sexp::envir::lock_binding_raw(live_env, sym);

        // Root only live_env across the collection.
        instance::with_required_current_instance(|inst| push_protect_in(inst, live_env));

        full_gc();

        assert!(
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena(|arena| arena.contains(live_env))
            })
        );
        assert!(
            !(unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena(|arena| arena.contains(dead_env))
            })
        );
        assert!(crate::sexp::envir::environment_is_locked_raw(live_env));
        assert!(crate::sexp::envir::binding_is_locked_raw(live_env, sym));
        instance::with_required_current_instance(|inst| unsafe {
            assert!((*inst).locked_environments.contains(&(live_env as usize)));
            assert!(
                !(*inst).locked_environments.contains(&(dead_env as usize)),
                "locked-environment entry survived the keyed env sweep"
            );
            assert!(
                (*inst)
                    .locked_bindings
                    .contains(&(live_env as usize, sym as usize))
            );
            assert!(
                !(*inst)
                    .locked_bindings
                    .contains(&(dead_env as usize, sym as usize)),
                "locked-binding entry survived the keyed env sweep"
            );
        });
    }

    /// Session-locality style churn: many envs with active and locked
    /// bindings, interleaved allocation churn with both collector flavours;
    /// rooted envs keep resolving and swept envs leave no stale entries.
    #[test]
    fn test_binding_tables_resolve_across_gc_churn() {
        let _session = RSession::new_without_default_packages();

        let sym_active =
            unsafe { crate::sexp::symbol::Rf_install(b"churn_active\0".as_ptr() as *const _) };
        let sym_locked =
            unsafe { crate::sexp::symbol::Rf_install(b"churn_locked\0".as_ptr() as *const _) };

        let mut rooted = Vec::new();
        let mut transient = Vec::new();
        for i in 0..32usize {
            let env = make_detached_env();
            let fun = unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
            };
            unsafe {
                *crate::sexp::accessors::INTEGER(fun) = i as i32;
            }
            crate::sexp::envir::make_active_binding_raw(env, sym_active, fun);
            crate::sexp::envir::lock_binding_raw(env, sym_locked);
            if i % 2 == 0 {
                rooted.push((env, fun));
            } else {
                transient.push(env);
            }
        }
        instance::with_required_current_instance(|inst| {
            for &(env, _) in &rooted {
                push_protect_in(inst, env);
            }
        });

        for round in 0..8usize {
            for _ in 0..64 {
                (unsafe {
                    /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                    with_arena(|arena| {
                        let scratch = arena.alloc_vector(SEXPTYPE::INTSXP, 8);
                        unsafe {
                            *crate::sexp::accessors::INTEGER(scratch) = round as i32;
                        }
                    })
                });
            }
            if round % 2 == 0 {
                minor_gc();
            } else {
                full_gc();
            }
        }

        // Rooted envs still resolve: entries intact, values readable.
        for (i, &(env, fun)) in rooted.iter().enumerate() {
            assert!(
                (unsafe {
                    /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                    with_arena(|arena| arena.contains(env))
                }),
                "rooted env {i} swept"
            );
            assert!(crate::sexp::envir::binding_is_active_raw(env, sym_active));
            assert!(crate::sexp::envir::binding_is_locked_raw(env, sym_locked));
            assert!(
                (unsafe {
                    /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                    with_arena(|arena| arena.contains(fun))
                })
            );
            // rooted holds only even i, so enumerate index i maps to
            // original loop value 2*i.
            assert_eq!(
                unsafe { *crate::sexp::accessors::INTEGER(fun) },
                (i * 2) as i32
            );
        }

        // Transient envs were reclaimed without leaving stale entries that a
        // recycled address could alias.
        for &env in &transient {
            assert!(
                !(unsafe {
                    /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                    with_arena(|arena| arena.contains(env))
                })
            );
            instance::with_required_current_instance(|inst| unsafe {
                assert!(
                    !(*inst)
                        .active_bindings
                        .contains_key(&(env as usize, sym_active as usize))
                );
                assert!(
                    !(*inst)
                        .locked_bindings
                        .contains(&(env as usize, sym_locked as usize))
                );
                assert!(!(*inst).locked_environments.contains(&(env as usize)));
            });
        }
    }

    /// Upstream executes finalizers only at quiescent points after a
    /// collection (R_gc/R_gc_lite, eval.c interrupt checks); sweeps just
    /// mark them ready. The quiescent flush must therefore both sweep the
    /// dead key and run its finalizer once the deferred collection fires.
    #[test]
    fn test_quiescent_flush_runs_dead_key_finalizer() {
        use std::os::raw::c_void;
        use std::sync::atomic::{AtomicI32, Ordering};

        static RUNS: AtomicI32 = AtomicI32::new(0);
        unsafe extern "C" fn count_finalizer(_ptr: *mut c_void) {
            RUNS.fetch_add(1, Ordering::SeqCst);
        }

        let _session = RSession::new_without_default_packages();
        let session = RSession::new_without_default_packages();
        session.with_active_in(|inst| unsafe {
            instance::with_required_current_instance(|inst| {
                (*inst).memory_state.pending_finalizers.clear();
                (*inst).gc_state.gc_pending = false;
                (*inst).eval_state.eval_depth = 0;
            });
            RUNS.store(0, Ordering::SeqCst);

            // Young and unrooted: nothing marks it, so the sweep marks its
            // finalizer ready.
            let key = with_arena(|arena| arena.alloc_node(SEXPTYPE::EXTPTRSXP));
            crate::mainutils::memory_main::R_RegisterCFinalizerEx(key, count_finalizer, 0);

            // Nothing pending: no collection, so the finalizer stays put.
            run_pending_gc_if_quiescent_in(inst);
            assert_eq!(RUNS.load(Ordering::SeqCst), 0);

            instance::with_required_current_instance(|inst| {
                (*inst).gc_state.gc_pending = true;
            });
            run_pending_gc_if_quiescent_in(inst);
            assert_eq!(RUNS.load(Ordering::SeqCst), 1);
        });
    }

    /// Same contract at the eval-loop safe point (upstream eval.c:1096 runs
    /// R_RunPendingFinalizers right after the interrupt check): a collection
    /// that marks a finalizer ready is followed by running it; a skipped
    /// safe point runs nothing.
    #[test]
    fn test_eval_safe_point_runs_dead_key_finalizer_only_after_collection() {
        use std::os::raw::c_void;
        use std::sync::atomic::{AtomicI32, Ordering};

        static RUNS: AtomicI32 = AtomicI32::new(0);
        unsafe extern "C" fn count_finalizer(_ptr: *mut c_void) {
            RUNS.fetch_add(1, Ordering::SeqCst);
        }

        let _session = RSession::new_without_default_packages();
        let session = RSession::new_without_default_packages();
        session.with_protected(|| unsafe {
            instance::with_required_current_instance(|inst| {
                (*inst).memory_state.pending_finalizers.clear();
                (*inst).gc_state.gc_pending = false;
                (*inst).eval_state.eval_depth = 0;
            });
            RUNS.store(0, Ordering::SeqCst);

            let key = with_arena(|arena| arena.alloc_node(SEXPTYPE::EXTPTRSXP));
            crate::mainutils::memory_main::R_RegisterCFinalizerEx(key, count_finalizer, 0);

            // Under the trigger thresholds: no collection, no finalizer.
            maybe_collect_at_eval_safe_point();
            assert_eq!(RUNS.load(Ordering::SeqCst), 0);

            instance::with_required_current_instance(|inst| {
                (*inst).gc_state.gc_pending = true;
            });
            maybe_collect_at_eval_safe_point();
            assert_eq!(RUNS.load(Ordering::SeqCst), 1);
        });
    }
}

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
