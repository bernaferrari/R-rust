//! gengc — extracted verbatim from gengc.rs.

use super::*;

// ---------------------------------------------------------------------------
// Root tracing
// ---------------------------------------------------------------------------

// Checked allocation tokens and collection epochs replace mutable header
// marks. Raw projection reads remain confined to the snapshot bridge.
pub(crate) const EDGE_PNAME: u32 = 1 << 0;
pub(crate) const EDGE_SYM_VALUE: u32 = 1 << 1;
pub(crate) const EDGE_INTERNAL: u32 = 1 << 2;
pub(crate) const EDGE_CAR: u32 = 1 << 3;
pub(crate) const EDGE_CDR: u32 = 1 << 4;
pub(crate) const EDGE_TAG: u32 = 1 << 5;
pub(crate) const EDGE_FORMALS: u32 = 1 << 6;
pub(crate) const EDGE_BODY: u32 = 1 << 7;
pub(crate) const EDGE_CLOENV: u32 = 1 << 8;
pub(crate) const EDGE_FRAME: u32 = 1 << 9;
pub(crate) const EDGE_ENCLOS: u32 = 1 << 10;
pub(crate) const EDGE_HASHTAB: u32 = 1 << 11;
pub(crate) const EDGE_PROM_VALUE: u32 = 1 << 12;
pub(crate) const EDGE_PROM_EXPR: u32 = 1 << 13;
pub(crate) const EDGE_PROM_ENV: u32 = 1 << 14;
pub(crate) const EDGE_EXT_TAG: u32 = 1 << 15;
pub(crate) const EDGE_EXT_PROT: u32 = 1 << 16;
pub(crate) const EDGE_ATTRIB: u32 = 1 << 17;
pub(crate) const EDGE_VECTOR: u32 = 1 << 18;
pub(crate) const EDGE_VECTOR_METADATA: u32 = 1 << 19;

thread_local! {
    static MARK_WHERE: std::cell::Cell<&'static str> = const { std::cell::Cell::new("unlabeled") };
}

/// Which pointer slots a node owns.
///
/// Mark and update both use this mask. Mark passes `follow_weak_key = false`
/// so a weak reference's key stays unmarked; update passes `true` so a live
/// key is still rewritten when its node is forwarded.
pub(crate) fn child_mask(type_code: i32, follow_weak_key: bool) -> u32 {
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
pub(crate) fn each_child(obj: SEXP, follow_weak_key: bool, visit: impl FnMut(&mut NodeLink)) {
    let (_, parent) = super::super::memory::checked_projection(obj)
        .expect("GC parent belongs to checked storage");
    gc_trace_bridge::rewrite_children(&parent, follow_weak_key, visit);
}

#[inline(always)]
pub(crate) fn mark_reachable(obj: SEXP) {
    if obj.is_null() {
        return;
    }
    mark_reachable_traced(obj);
}

#[inline(always)]
pub(crate) fn mark_reachable_traced(obj: SEXP) {
    let mut pending = gc_trace::TraceWorklist::new(gc_trace::TraceScope::active());
    trace_result(pending.enqueue(obj));
    drain_trace_worklist(pending);
}

pub(crate) fn trace_result<T>(result: Result<T, gc_trace::TraceError>) -> T {
    result.unwrap_or_else(|error| {
        let where_ = MARK_WHERE.with(|label| label.get());
        panic!("invalid GC graph while marking {where_}: {error:?}");
    })
}

pub(crate) fn drain_trace_worklist(mut pending: gc_trace::TraceWorklist) {
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

pub(crate) fn mark_checked_root_snapshot(roots: impl IntoIterator<Item = RootValue>) {
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

pub(crate) fn mark_instance_roots(instance: *mut instance::RInstance) {
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
        let fresh = super::super::memory::fresh_allocation_roots_for_marking(identity).map(
            |(projection, allocation)| RootValue::Checked {
                projection,
                allocation,
            },
        );
        MARK_WHERE.with(|w| w.set("fresh_allocations"));
        mark_checked_root_snapshot(fresh);

        let automatic = super::super::memory::automatic_roots_for_marking(identity).map(
            |(projection, allocation)| RootValue::Checked {
                projection,
                allocation,
            },
        );
        MARK_WHERE.with(|w| w.set("automatic_handles"));
        mark_checked_root_snapshot(automatic);

        // Every native and Rust protection channel carries original checked
        // allocation identities; raw address reuse never creates a new root.
        MARK_WHERE.with(|w| w.set("legacy_protect"));
        mark_checked_root_snapshot((*instance).legacy_protect.checked_roots_for_marking());
        MARK_WHERE.with(|w| w.set("root_table"));
        mark_checked_root_snapshot((*instance).root_table.checked_roots_for_marking());
        MARK_WHERE.with(|w| w.set("preserve_stack"));
        mark_checked_root_snapshot((*instance).preserve_stack.checked_roots_for_marking());
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

pub(crate) fn run_gc_cycle_in<F>(instance: *mut instance::RInstance, collect: F) -> (usize, usize)
where
    F: FnOnce(*mut instance::RInstance) -> (usize, usize),
{
    unsafe {
        // Defer direct requests before touching an arena with an active mutable lend.
        if super::super::memory::is_arena_lent(instance) {
            (*instance).gc_state.gc_pending = true;
            return (0, 0);
        }
        if (*instance).gc_state.in_progress {
            return (0, 0);
        }

        // Native Rust resources detached by sweep may have destructors that
        // reenter R. Release them after tracing, arena lends, and collection
        // bookkeeping have ended, including when collection unwinds.
        let _resource_drops = super::super::memory::resource_drop_guard_in(instance);
        (*instance).gc_state.in_progress = true;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Managed and permanent headers share epoch metadata; immutable
            // process singletons are recognized without marking their bytes.
            super::super::memory::begin_gc_epoch();
            let _trace_scope = gc_trace::TraceScope::enter(gc_trace::TraceContext::new(
                (*instance).arena.heap_identity(),
                super::super::memory::current_gc_epoch(),
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

pub(crate) fn do_minor_gc_in(instance: *mut instance::RInstance) -> (usize, usize) {
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
                    let marked = super::super::memory::arena_node_marked(obj);

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
pub(crate) fn do_torture_mark_sweep_in(instance: *mut instance::RInstance) -> (usize, usize) {
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
                    if super::super::memory::arena_node_marked(obj) {
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

pub(crate) fn mark_from_all_roots_in(instance: *mut instance::RInstance) {
    unsafe {
        // No traceable HashSet: use mark bit visited. (Perf + addresses review complaint
        // about allocating HashSets and hashing every edge.)
        mark_instance_roots(instance);
        mark_checked_root_snapshot((*instance).gc_state.remembered_set.checked_roots());
    }
}

pub(crate) fn do_full_mark_sweep_in(instance: *mut instance::RInstance) -> (usize, usize) {
    unsafe {
        // Remembered parents are conservative roots only while collecting
        // the young generation. Full collection establishes old reachability
        // from actual roots; retaining the hints here pins dead method graphs
        // until another full cycle and can exhaust bounded sessions.
        mark_instance_roots(instance);
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
                    if super::super::memory::arena_node_marked(obj) {
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
