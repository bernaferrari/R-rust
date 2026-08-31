//! Generational garbage collector with remembered-set write barriers.
//!
//! The collector is intentionally defensive: it scopes state to the active
//! `RInstance`, catches panics at public GC entry points, and uses a
//! non-moving mark/sweep collector with free-list recycling.
//! Raw `SEXP` internals still require careful auditing; do not document new
//! invariants here unless they are enforced by code and regression tests.

use std::collections::{HashMap, HashSet};
#[cfg(test)]
use std::ptr;

use super::ffi::{SEXP, SEXPTYPE};
use super::instance;
use super::memory::{RArena, with_arena_for_gc};
use super::protect::{
    push_protect_in, update_preserve_stack_refs_in, update_protect_stack_refs_in,
};

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

/// Callback type for GC event notifications.
pub type GcCallback = Box<dyn Fn(&GcStats) + Send + Sync>;

/// Register a callback to be invoked after each GC cycle.
pub fn register_gc_callback(cb: GcCallback) {
    with_gc_state(|state| state.callbacks.push(cb));
}

fn notify_gc_callbacks_in(state: &GcState) {
    let stats = state.stats.clone();
    notify_gc_callbacks_with_stats(state, &stats);
}

fn notify_gc_callbacks_with_stats(state: &GcState, stats: &GcStats) {
    for cb in &state.callbacks {
        cb(stats);
    }
}

// ---------------------------------------------------------------------------
// GC Invariants Checking
// ---------------------------------------------------------------------------

fn verify_gc_invariants_in(instance: &mut instance::RInstance) {
    let stack = instance.protect_stack.borrow();
    for &obj in stack.iter() {
        if !obj.is_null() {
            // Debug-only: verify object is within arena bounds.
            // Full validation would require classifying singleton roots too.
        }
    }
}

// ---------------------------------------------------------------------------
// Root tracing
// ---------------------------------------------------------------------------

// Simplified mark: no per-GC HashSet<usize> "traceable" snapshot of all actives, no hash lookup
// on every edge. We rely on the mark bit in sxpinfo for "visited this collection" (as review
// suggested) + null guards + reachability from roots. This eliminates two HashSet allocations
// per GC and hashing cost on every pointer edge during marking. Sweep still walks actives
// (necessary) to find unmarked for free.
// Protected "force" marking uses the traced path (now default behavior).
#[inline(always)]
fn mark_reachable(obj: SEXP) {
    if obj.is_null() {
        return;
    }
    mark_reachable_traced(obj);
}

#[inline(always)]
fn mark_reachable_traced(obj: SEXP) {
    if obj.is_null() {
        return;
    }
    // A reachable SEXP is always pointer-aligned and lives far above the
    // null page. This used to be a silent skip that masked real heap
    // corruption: slots holding small integer sentinels or recycled native
    // pointers after the collector wrongly swept live bindings. With
    // persistent roots re-traced every cycle and raw stack references
    // protected across allocating calls, every traced slot is a real SEXP,
    // so keep only a debug tripwire that surfaces regressions loudly
    // instead of dereferencing (or silently skipping) garbage.
    debug_assert!(
        (obj as usize) >= 0x1_0000 && (obj as usize).trailing_zeros() >= 3,
        "mark_reachable_traced on implausible SEXP pointer {:#x}",
        obj as usize
    );

    unsafe {
        if (*obj).sxpinfo.mark() {
            return;
        }
        (*obj).sxpinfo.set_mark(true);

        let t = (*obj).sxpinfo.type_of();
        match t {
            SEXPTYPE::SYMSXP => {
                mark_reachable((*obj).data.symsxp.pname);
                mark_reachable((*obj).data.symsxp.value);
                mark_reachable((*obj).data.symsxp.internal);
            }
            // DOTSXP (...) chains are cons cells with the same listsxp
            // layout; skipping them left spliced `...` arguments untraced.
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP | SEXPTYPE::DOTSXP => {
                mark_reachable((*obj).data.listsxp.carval);
                mark_reachable((*obj).data.listsxp.cdrval);
                mark_reachable((*obj).data.listsxp.tagval);
            }
            SEXPTYPE::CLOSXP => {
                mark_reachable((*obj).data.closxp.formals);
                mark_reachable((*obj).data.closxp.body);
                mark_reachable((*obj).data.closxp.env);
            }
            SEXPTYPE::ENVSXP => {
                mark_reachable((*obj).data.envsxp.frame);
                mark_reachable((*obj).data.envsxp.enclos);
                mark_reachable((*obj).data.envsxp.hashtab);
            }
            SEXPTYPE::PROMSXP => {
                mark_reachable((*obj).data.promsxp.value);
                mark_reachable((*obj).data.promsxp.expr);
                mark_reachable((*obj).data.promsxp.env);
            }
            SEXPTYPE::EXTPTRSXP => {
                let extptr = (*obj).data.extptr;
                mark_reachable(extptr[1] as SEXP);
                mark_reachable(extptr[2] as SEXP);
            }
            SEXPTYPE::WEAKREFSXP => {
                mark_reachable((*obj).data.listsxp.cdrval);
                mark_reachable((*obj).data.listsxp.tagval);
            }
            _ => {}
        }

        if vector_payload_has_sexp_refs(t) {
            let len = (*obj).vecsxp_length();
            let data = (*obj).gengc_next_node as *mut SEXP;
            if !data.is_null() && len > 0 {
                for i in 0..len as usize {
                    mark_reachable(*data.add(i));
                }
            }
        }

        mark_reachable((*obj).attrib);
    }
}

#[inline(always)]
fn mark_context_roots(ctxt: &super::context::RCNTXT) {
    mark_reachable(ctxt.call);
    mark_reachable(ctxt.cloenv);
    mark_reachable(ctxt.sysparent);
    mark_reachable(ctxt.callfun);
    mark_reachable(ctxt.closure);
    mark_reachable(ctxt.promiseargs);
    mark_reachable(ctxt.savelist);
    mark_reachable(ctxt.handlerstack);
    mark_reachable(ctxt.restartstack);
    mark_reachable(ctxt.rpvec);
    mark_reachable(ctxt.returnValue);
    mark_reachable(ctxt.conexit);
    mark_reachable(ctxt.srcref);
}

#[inline(always)]
fn mark_instance_roots(instance: &mut instance::RInstance) {
    mark_reachable(instance.empty_env);
    mark_reachable(instance.base_env);
    mark_reachable(instance.global_env);

    {
        let stack = instance.protect_stack.borrow();
        for &obj in stack.iter() {
            // Protected roots must be marked even when the collector has already
            // removed a reused address from the active-node set. (Now unconditional
            // since no traceable set.)
            mark_reachable_traced(obj);
        }
    }
    {
        let stack = instance.preserve_stack.borrow();
        for &obj in stack.iter() {
            mark_reachable_traced(obj);
        }
    }
    for ctxt in &instance.context_stack {
        mark_context_roots(ctxt);
    }

    mark_reachable(instance.error_state.warnings);
    mark_reachable(instance.error_state.handler_stack);
    mark_reachable(instance.error_state.restart_stack);
    mark_reachable(instance.error_state.warning_call);

    mark_reachable(instance.eval_state.current_expr);
    mark_reachable(instance.eval_state.parse_error_file);
    mark_reachable(instance.eval_state.exec_token);
    mark_reachable(instance.eval_state.profiling.sref);
    mark_reachable(instance.eval_state.profiling.srcfiles_buffer);
    mark_reachable(instance.eval_state.printvector.na_string);
    mark_reachable(instance.eval_state.printvector.na_string_noquote);
    mark_reachable(instance.eval_state.print.data.na_string);
    mark_reachable(instance.eval_state.print.data.na_string_noquote);
    mark_reachable(instance.eval_state.print.data.env);
    mark_reachable(instance.eval_state.print.data.callArgs);
    instance
        .eval_state
        .bc_stack
        .visit_roots(|obj| mark_reachable(*obj));

    for &obj in instance.symbols.values() {
        mark_reachable(obj);
    }
    for &node in &instance.symbol_nodes {
        mark_reachable(node);
    }
    for &node in &instance.env_nodes {
        mark_reachable(node);
    }
    for &obj in &instance.names_state.ddval_symbols {
        mark_reachable(obj);
    }
    mark_reachable(instance.bind_state.blank_string);

    for &obj in instance.options.values() {
        mark_reachable(obj);
    }
    for callback in &instance.main_state.task_callbacks {
        mark_reachable(callback.fun);
        mark_reachable(callback.data);
    }
    for &generic in &instance.objects_state.prim_generics {
        mark_reachable(generic);
    }
    for &methods in &instance.objects_state.prim_mlist {
        mark_reachable(methods);
    }
    mark_reachable(instance.objects_state.deferred_default_object);
    for (&env, table) in &instance.env_hash_tables {
        mark_reachable(env as SEXP);
        for (&symbol, &value) in table {
            mark_reachable(symbol as SEXP);
            mark_reachable(value);
        }
    }
    // Namespace cache values may be reachable only through the cache: a
    // pure-R package namespace has no other root once attach-time references
    // die. Untraced, a collection swept the namespace env and left a dangling
    // raw pointer in the cache.
    for &(_, namespace) in instance.package_namespace_cache.values() {
        mark_reachable(namespace);
    }
    // Active-binding functions must live as long as their entry. The (env,
    // symbol) key addresses are deliberately not marked: bindings belong to
    // their environment, so entries whose keyed node is reclaimed this cycle
    // are swept in update_instance_roots_in instead of pinning the env.
    for value in instance.active_bindings.values() {
        mark_reachable(*value);
    }

    for finalizer in &instance.memory_state.pending_finalizers {
        if finalizer.is_ready() {
            mark_reachable(finalizer.obj());
        }
        if let crate::mainutils::memory_main::PendingFinalizer::R { fun, .. } = finalizer {
            mark_reachable(*fun);
        }
    }
    mark_reachable(instance.dynload_state.dll_info_eptrs);
    mark_reachable(instance.dynload_state.symbol_eptrs);
    mark_reachable(instance.dynload_state.c_entry_table);

    instance.httpd_state.visit_roots(|obj| mark_reachable(*obj));

    mark_reachable(instance.grid_runtime_state.current_grid_state);
    mark_reachable(instance.grid_runtime_state.eval_env);

    for &obj in &instance.raw_cons {
        mark_reachable(obj);
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
    entries: Vec<SEXP>,
    members: HashSet<usize>,
}

impl RememberedSet {
    #[inline]
    pub fn add(&mut self, obj: SEXP) {
        if obj.is_null() {
            return;
        }
        unsafe {
            if (*obj).sxpinfo.gcgen() == 0 {
                return;
            }
        }
        let addr = obj as usize;
        if self.members.contains(&addr) {
            return;
        }
        // Reserve both collections before mutating either, so an allocation
        // failure cannot leave entries and membership out of sync.
        if self.entries.try_reserve(1).is_err() || self.members.try_reserve(1).is_err() {
            return;
        }
        self.entries.push(obj);
        self.members.insert(addr);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.members.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = SEXP> + '_ {
        self.entries.iter().copied()
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Remap entries through a relocation map (non-moving sweep redirects
    /// freed addresses to `R_NilValue`) and rebuild membership so a later
    /// barrier cannot deduplicate against a stale address.
    pub fn remap(&mut self, old_to_new: &HashMap<usize, SEXP>) {
        for entry in &mut self.entries {
            let addr = *entry as usize;
            if let Some(&new_ptr) = old_to_new.get(&addr) {
                *entry = new_ptr;
            }
        }
        self.members = self
            .entries
            .iter()
            .filter(|entry| !entry.is_null())
            .map(|entry| *entry as usize)
            .collect();
    }
}

// ---------------------------------------------------------------------------
// GC State
// ---------------------------------------------------------------------------

pub struct GcState {
    pub(crate) stats: GcStats,
    pub(crate) callbacks: Vec<GcCallback>,
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

fn with_gc_state_in<F, R>(instance: &mut instance::RInstance, f: F) -> R
where
    F: FnOnce(&mut GcState) -> R,
{
    f(&mut instance.gc_state)
}

// ---------------------------------------------------------------------------
// Write Barriers
// ---------------------------------------------------------------------------

#[inline(always)]
pub fn write_barrier(parent: SEXP, child: SEXP) {
    if parent.is_null() || child.is_null() {
        return;
    }

    unsafe {
        let parent_gen = (*parent).sxpinfo.gcgen();
        let child_gen = (*child).sxpinfo.gcgen();

        if parent_gen == Generation::Old as u8 && child_gen == Generation::Young as u8 {
            with_gc_state(|state| {
                state.remembered_set.add(parent);
            });
        }
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
    unsafe {
        if obj.is_null() {
            return;
        }
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

fn update_protect_stack_in(instance: &mut instance::RInstance, old_to_new: &HashMap<usize, SEXP>) {
    update_protect_stack_refs_in(instance, |ptr| {
        let addr = ptr as usize;
        old_to_new.get(&addr).copied().unwrap_or(ptr)
    });
}

fn update_preserve_stack_in(instance: &mut instance::RInstance, old_to_new: &HashMap<usize, SEXP>) {
    update_preserve_stack_refs_in(instance, |ptr| {
        let addr = ptr as usize;
        old_to_new.get(&addr).copied().unwrap_or(ptr)
    });
}

fn update_remembered_set(old_to_new: &HashMap<usize, SEXP>) {
    instance::with_required_current_instance(|instance| {
        update_remembered_set_in(instance, old_to_new)
    });
}

fn update_remembered_set_in(instance: &mut instance::RInstance, old_to_new: &HashMap<usize, SEXP>) {
    with_gc_state_in(instance, |state| {
        state.remembered_set.remap(old_to_new);
    });
}

fn update_references_in_object(obj: SEXP, old_to_new: &HashMap<usize, SEXP>) {
    if obj.is_null() {
        return;
    }
    unsafe {
        let t = (*obj).sxpinfo.type_of();
        match t {
            SEXPTYPE::SYMSXP => {
                update_field(&mut (*obj).data.symsxp.pname, old_to_new);
                update_field(&mut (*obj).data.symsxp.value, old_to_new);
                update_field(&mut (*obj).data.symsxp.internal, old_to_new);
            }
            // DOTSXP (...) chains share the listsxp layout; keep sweep-time
            // redirection consistent with mark_reachable_traced above.
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP | SEXPTYPE::DOTSXP => {
                update_field(&mut (*obj).data.listsxp.carval, old_to_new);
                update_field(&mut (*obj).data.listsxp.cdrval, old_to_new);
                update_field(&mut (*obj).data.listsxp.tagval, old_to_new);
            }
            SEXPTYPE::CLOSXP => {
                update_field(&mut (*obj).data.closxp.formals, old_to_new);
                update_field(&mut (*obj).data.closxp.body, old_to_new);
                update_field(&mut (*obj).data.closxp.env, old_to_new);
            }
            SEXPTYPE::ENVSXP => {
                update_field(&mut (*obj).data.envsxp.frame, old_to_new);
                update_field(&mut (*obj).data.envsxp.enclos, old_to_new);
                update_field(&mut (*obj).data.envsxp.hashtab, old_to_new);
            }
            SEXPTYPE::PROMSXP => {
                update_field(&mut (*obj).data.promsxp.value, old_to_new);
                update_field(&mut (*obj).data.promsxp.expr, old_to_new);
                update_field(&mut (*obj).data.promsxp.env, old_to_new);
            }
            SEXPTYPE::EXTPTRSXP => {
                let tag = (*obj).data.extptr[1] as SEXP;
                let prot = (*obj).data.extptr[2] as SEXP;
                (*obj).data.extptr[1] = old_to_new.get(&(tag as usize)).copied().unwrap_or(tag)
                    as *mut std::ffi::c_void;
                (*obj).data.extptr[2] = old_to_new.get(&(prot as usize)).copied().unwrap_or(prot)
                    as *mut std::ffi::c_void;
            }
            SEXPTYPE::WEAKREFSXP => {
                update_field(&mut (*obj).data.listsxp.carval, old_to_new);
                update_field(&mut (*obj).data.listsxp.cdrval, old_to_new);
                update_field(&mut (*obj).data.listsxp.tagval, old_to_new);
            }
            _ => {}
        }

        if vector_payload_has_sexp_refs(t) {
            let len = (*obj).vecsxp_length();
            let data = (*obj).gengc_next_node as *mut SEXP;
            if !data.is_null() && len > 0 {
                for i in 0..len as usize {
                    update_field(&mut *data.add(i), old_to_new);
                }
            }
        }

        update_field(&mut (*obj).attrib, old_to_new);
    }
}

fn update_object_references(old_to_new: &HashMap<usize, SEXP>) {
    with_arena_for_gc(|arena| {
        let nodes: Vec<SEXP> = arena.active_nodes().collect();
        for &obj in &nodes {
            update_references_in_object(obj, old_to_new);
        }
    });
}

fn update_object_references_in(
    instance: &mut instance::RInstance,
    old_to_new: &HashMap<usize, SEXP>,
) {
    let nodes: Vec<SEXP> = instance.arena.active_nodes().collect();
    for &obj in &nodes {
        update_references_in_object(obj, old_to_new);
    }
}

fn remap_addr(addr: usize, old_to_new: &HashMap<usize, SEXP>) -> usize {
    old_to_new
        .get(&addr)
        .copied()
        .map(|ptr| ptr as usize)
        .unwrap_or(addr)
}

fn update_context_roots(ctxt: &mut super::context::RCNTXT, old_to_new: &HashMap<usize, SEXP>) {
    update_field(&mut ctxt.call, old_to_new);
    update_field(&mut ctxt.cloenv, old_to_new);
    update_field(&mut ctxt.sysparent, old_to_new);
    update_field(&mut ctxt.callfun, old_to_new);
    update_field(&mut ctxt.closure, old_to_new);
    update_field(&mut ctxt.promiseargs, old_to_new);
    update_field(&mut ctxt.savelist, old_to_new);
    update_field(&mut ctxt.handlerstack, old_to_new);
    update_field(&mut ctxt.restartstack, old_to_new);
    update_field(&mut ctxt.rpvec, old_to_new);
    update_field(&mut ctxt.returnValue, old_to_new);
    update_field(&mut ctxt.conexit, old_to_new);
    update_field(&mut ctxt.srcref, old_to_new);
}

fn update_instance_roots_in(instance: &mut instance::RInstance, old_to_new: &HashMap<usize, SEXP>) {
    update_field(&mut instance.empty_env, old_to_new);
    update_field(&mut instance.base_env, old_to_new);
    update_field(&mut instance.global_env, old_to_new);

    update_protect_stack_in(instance, old_to_new);
    update_preserve_stack_in(instance, old_to_new);
    for ctxt in &mut instance.context_stack {
        update_context_roots(ctxt, old_to_new);
    }

    update_field(&mut instance.error_state.warnings, old_to_new);
    update_field(&mut instance.error_state.handler_stack, old_to_new);
    update_field(&mut instance.error_state.restart_stack, old_to_new);
    update_field(&mut instance.error_state.warning_call, old_to_new);

    update_field(&mut instance.eval_state.current_expr, old_to_new);
    update_field(&mut instance.eval_state.parse_error_file, old_to_new);
    update_field(&mut instance.eval_state.exec_token, old_to_new);
    update_field(&mut instance.eval_state.profiling.sref, old_to_new);
    update_field(
        &mut instance.eval_state.profiling.srcfiles_buffer,
        old_to_new,
    );
    update_field(&mut instance.eval_state.printvector.na_string, old_to_new);
    update_field(
        &mut instance.eval_state.printvector.na_string_noquote,
        old_to_new,
    );
    update_field(&mut instance.eval_state.print.data.na_string, old_to_new);
    update_field(
        &mut instance.eval_state.print.data.na_string_noquote,
        old_to_new,
    );
    update_field(&mut instance.eval_state.print.data.env, old_to_new);
    update_field(&mut instance.eval_state.print.data.callArgs, old_to_new);
    instance
        .eval_state
        .bc_stack
        .visit_roots(|obj| update_field(obj, old_to_new));

    for obj in instance.symbols.values_mut() {
        update_field(obj, old_to_new);
    }
    for &node in &instance.symbol_nodes {
        update_references_in_object(node, old_to_new);
    }
    for &node in &instance.env_nodes {
        update_references_in_object(node, old_to_new);
    }
    for obj in &mut instance.names_state.ddval_symbols {
        update_field(obj, old_to_new);
    }
    update_field(&mut instance.bind_state.blank_string, old_to_new);

    for obj in instance.options.values_mut() {
        update_field(obj, old_to_new);
    }
    for callback in &mut instance.main_state.task_callbacks {
        update_field(&mut callback.fun, old_to_new);
        update_field(&mut callback.data, old_to_new);
    }
    for generic in &mut instance.objects_state.prim_generics {
        update_field(generic, old_to_new);
    }
    for methods in &mut instance.objects_state.prim_mlist {
        update_field(methods, old_to_new);
    }
    update_field(
        &mut instance.objects_state.deferred_default_object,
        old_to_new,
    );
    let old_hash_tables = std::mem::take(&mut instance.env_hash_tables);
    instance.env_hash_tables = old_hash_tables
        .into_iter()
        .map(|(env, table)| {
            let table = table
                .into_iter()
                .map(|(symbol, mut value)| {
                    update_field(&mut value, old_to_new);
                    (remap_addr(symbol, old_to_new), value)
                })
                .collect();
            (remap_addr(env, old_to_new), table)
        })
        .collect();
    let old_cache = std::mem::take(&mut instance.package_namespace_cache);
    instance.package_namespace_cache = old_cache
        .into_iter()
        .map(|(package, (dir, mut namespace))| {
            update_field(&mut namespace, old_to_new);
            (package, (dir, namespace))
        })
        .collect();

    // The binding tables are keyed by raw node addresses. Entries whose keyed
    // node was reclaimed this cycle must be dropped before `free_node` puts
    // the address back on the LIFO free list — a recycled address would
    // otherwise alias the stale entry (a fresh environment reporting locks or
    // active bindings it never had). `old_to_new` maps exactly the reclaimed
    // addresses to R_NilValue, so key membership identifies them; live keys
    // keep their address (the collector never moves nodes).
    instance.active_bindings.retain(|key, value| {
        update_field(value, old_to_new);
        !old_to_new.contains_key(&key.0) && !old_to_new.contains_key(&key.1)
    });
    instance
        .locked_environments
        .retain(|env| !old_to_new.contains_key(env));
    instance
        .locked_bindings
        .retain(|(env, symbol)| !old_to_new.contains_key(env) && !old_to_new.contains_key(symbol));

    for finalizer in &mut instance.memory_state.pending_finalizers {
        update_field(finalizer.obj_mut(), old_to_new);
        if let Some(fun) = finalizer.fun_mut() {
            update_field(fun, old_to_new);
        }
    }
    update_field(&mut instance.dynload_state.dll_info_eptrs, old_to_new);
    update_field(&mut instance.dynload_state.symbol_eptrs, old_to_new);
    update_field(&mut instance.dynload_state.c_entry_table, old_to_new);

    instance
        .httpd_state
        .visit_roots(|obj| update_field(obj, old_to_new));

    update_field(
        &mut instance.grid_runtime_state.current_grid_state,
        old_to_new,
    );
    update_field(&mut instance.grid_runtime_state.eval_env, old_to_new);

    for obj in &mut instance.raw_cons {
        let mut sexp = *obj as SEXP;
        update_field(&mut sexp, old_to_new);
        *obj = sexp;
        update_references_in_object(*obj, old_to_new);
    }
}

fn update_all_references_in(instance: &mut instance::RInstance, old_to_new: &HashMap<usize, SEXP>) {
    update_instance_roots_in(instance, old_to_new);
    update_remembered_set_in(instance, old_to_new);
    update_object_references_in(instance, old_to_new);
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
    instance::with_required_current_instance(minor_gc_in)
}

fn run_gc_cycle_in<F>(instance: &mut instance::RInstance, collect: F) -> (usize, usize)
where
    F: FnOnce(&mut instance::RInstance) -> (usize, usize),
{
    if instance.gc_state.in_progress {
        return (0, 0);
    }

    instance.gc_state.in_progress = true;
    verify_gc_invariants_in(instance);
    // Sweep only visits arena nodes, so persistent nodes keep whatever mark
    // the previous cycle left on them. Clear those marks before marking so
    // every cycle re-traces the persistent roots (environment frames,
    // interned symbol pnames, raw cons cells); a stale mark would make
    // `mark_reachable_traced` short-circuit and sweep bindings that are
    // still reachable, leaving dangling frame chains behind.
    clear_persistent_node_marks_in(instance);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| collect(instance)));
    instance.gc_state.in_progress = false;

    match result {
        Ok((promoted, freed)) => {
            record_collection_in(&mut instance.gc_state, promoted, freed);
            notify_gc_callbacks_in(&instance.gc_state);
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

/// Clear trace marks on nodes this instance owns outside the arena.
///
/// The mark bit doubles as the per-cycle "visited" flag, but sweep only
/// resets it for arena nodes. Persistent nodes (the empty/base/global
/// environment sentinels in `env_nodes`, interned symbols in
/// `symbol_nodes`, and out-of-arena cons cells in `raw_cons`) therefore
/// survived earlier cycles with the bit still set, and every later cycle
/// skipped tracing them. The process-global sentinels (`R_NilValue`,
/// `R_UnboundValue`, `R_MissingArg`, `R_RestartToken`) are deliberately not
/// touched: they are pre-marked to pin, and none of them traces children.
fn clear_persistent_node_marks_in(instance: &mut instance::RInstance) {
    for &node in &instance.env_nodes {
        unsafe {
            (*node).sxpinfo.set_mark(false);
        }
    }
    for &node in &instance.symbol_nodes {
        unsafe {
            (*node).sxpinfo.set_mark(false);
        }
    }
    for &node in &instance.raw_cons {
        unsafe {
            if !node.is_null() {
                (*node).sxpinfo.set_mark(false);
            }
        }
    }
}

pub(crate) fn minor_gc_in(instance: &mut instance::RInstance) -> (usize, usize) {
    run_gc_cycle_in(instance, do_minor_gc_in)
}

const GC_TRIGGER_THRESHOLD: usize = 10_000;
const GC_BYTE_THRESHOLD: usize = 64 * 1024 * 1024;

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
    instance: &mut instance::RInstance,
    torture_ticks: u32,
    collect_requested: bool,
) {
    if collect_requested {
        instance.gc_state.gc_pending = true;
    }
    if torture_ticks > 0 {
        maybe_torture_gc_in(instance, torture_ticks);
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
fn maybe_torture_gc_in(instance: &mut instance::RInstance, ticks: u32) {
    if instance.memory_state.gc_force_gap <= 0 {
        // Not armed: default behavior is identical (single branch).
        return;
    }
    if instance.gc_state.in_progress || instance.memory_state.in_gc != 0 {
        // Mirrors upstream's R_in_gc deferral: don't recurse into a
        // collection from inside one; run it at the next safe point.
        instance.gc_state.gc_pending = true;
        return;
    }
    // FORCE_GC countdown: `--gc_force_wait` fires when it reaches zero,
    // then re-arms to `gc_force_gap`. Deferred ticks are consumed in one
    // step, firing at most one collection per processing point.
    let ticks: std::os::raw::c_int = ticks.try_into().unwrap_or(std::os::raw::c_int::MAX);
    if instance.memory_state.gc_force_wait > ticks {
        instance.memory_state.gc_force_wait -= ticks;
        return;
    }
    instance.memory_state.gc_force_wait = instance.memory_state.gc_force_gap;
    let start = instance.protect_stack.borrow().len();
    push_environment_binding_protects(instance);
    let added = instance.protect_stack.borrow().len().saturating_sub(start);
    instance.gc_state.gc_pending = false;
    instance.memory_state.in_gc = 1;
    instance.memory_state.gc_count = instance.memory_state.gc_count.wrapping_add(1);
    run_gc_cycle_in(instance, do_torture_mark_sweep_in);
    instance.memory_state.in_gc = 0;
    super::protect::unprotect_count_in(instance, added);
}

fn eval_safe_point_gc_due_in(instance: &instance::RInstance) -> bool {
    instance.gc_state.gc_pending
        || instance.arena.node_count() > GC_TRIGGER_THRESHOLD
        || instance.arena.total_bytes_allocated() > GC_BYTE_THRESHOLD
}

fn sync_env_hash_tables_from_frames(instance: &mut instance::RInstance) {
    let envs: Vec<usize> = instance.env_hash_tables.keys().copied().collect();
    for env_addr in envs {
        let env = env_addr as SEXP;
        unsafe {
            if (*env).sxpinfo.type_of() != SEXPTYPE::ENVSXP {
                continue;
            }
            let mut frame = (*env).data.envsxp.frame;
            while !frame.is_null() {
                let tag = (*frame).data.listsxp.tagval;
                let val = (*frame).data.listsxp.carval;
                if !tag.is_null() {
                    super::env_hash::hash_insert_in(instance, env, tag, val);
                }
                frame = (*frame).data.listsxp.cdrval;
            }
        }
    }
}

fn collect_environment_binding_values(instance: &instance::RInstance) -> Vec<SEXP> {
    let mut values = Vec::new();
    let mut seen_envs = std::collections::HashSet::new();
    unsafe {
        let mut walk_env = |mut env: SEXP| {
            while !env.is_null() && seen_envs.insert(env as usize) {
                if (*env).sxpinfo.type_of() != SEXPTYPE::ENVSXP {
                    break;
                }
                let mut frame = (*env).data.envsxp.frame;
                while !frame.is_null() {
                    values.push(frame);
                    let val = (*frame).data.listsxp.carval;
                    if !val.is_null() {
                        values.push(val);
                    }
                    frame = (*frame).data.listsxp.cdrval;
                }
                env = (*env).data.envsxp.enclos;
            }
        };
        for ctxt in &instance.context_stack {
            walk_env(ctxt.cloenv);
        }
        walk_env(instance.global_env);
        walk_env(instance.base_env);
    }
    values
}

fn push_environment_binding_protects(instance: &mut instance::RInstance) {
    sync_env_hash_tables_from_frames(instance);
    let values = collect_environment_binding_values(instance);
    for value in values {
        push_protect_in(instance, value);
    }
}

/// Run an explicit user-requested collection (`gc()` / `gcinfo`-style entry
/// points) with the same environment force-protect preamble the eval safe
/// points use.
///
/// `gc()` can fire mid-evaluation — inside a loop body or a closure call —
/// unlike safe points, which only run between statements. Without the
/// preamble, a full collection there sweeps in-flight frame bindings (loop
/// variables, closure-call environments), surfacing later as
/// `object 'i' not found`.
pub fn collect_with_environment_protects(full: bool) -> (usize, usize) {
    instance::with_required_current_instance(|instance| {
        let start = instance.protect_stack.borrow().len();
        push_environment_binding_protects(instance);
        let added = instance.protect_stack.borrow().len().saturating_sub(start);
        instance.gc_state.gc_pending = false;
        let result = if full {
            full_gc_in(instance)
        } else {
            minor_gc_in(instance)
        };
        super::protect::unprotect_count_in(instance, added);
        result
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
    instance::with_required_current_instance(|instance| {
        if !eval_safe_point_gc_due_in(instance) {
            return;
        }
        let start = instance.protect_stack.borrow().len();
        push_environment_binding_protects(instance);
        let added = instance.protect_stack.borrow().len().saturating_sub(start);
        instance.gc_state.gc_pending = false;
        // Safe points normally collect the young generation only; without a
        // periodic full pass, old-generation garbage from promoted-then-dead
        // objects would accumulate unbounded between explicit gc() calls.
        instance.gc_state.safe_point_collections =
            instance.gc_state.safe_point_collections.wrapping_add(1);
        if instance.gc_state.safe_point_collections % SAFE_POINT_FULL_COLLECTION_INTERVAL == 0 {
            full_gc_in(instance);
        } else {
            minor_gc_in(instance);
        }
        super::protect::unprotect_count_in(instance, added);
    });
}

/// Flush a deferred collection after a top-level expression completes.
pub fn run_pending_gc_if_quiescent() {
    instance::with_current_instance(|inst| {
        if inst.eval_state.eval_depth == 0 && inst.gc_state.gc_pending {
            inst.gc_state.gc_pending = false;
            minor_gc_in(inst);
        }
    });
}

fn do_minor_gc_in(instance: &mut instance::RInstance) -> (usize, usize) {
    // No traceable HashSet: use mark bit visited. (Perf + addresses review complaint
    // about allocating HashSets and hashing every edge.)
    mark_instance_roots(instance);
    for &obj in &instance.gc_state.remembered_set.entries {
        mark_reachable(obj);
    }

    let mut freed_count = 0;
    let mut promoted_count = 0;
    let mut to_free = Vec::new();

    {
        let arena = &mut instance.arena;
        // Iterate directly; only allocate to_free vec (not full snapshot of actives).
        // Reduces temp memory/alloc pressure during GC (perf + memory win, especially on constrained Android/WASM).
        for obj in arena.active_nodes() {
            if obj.is_null() {
                continue;
            }
            unsafe {
                let obj_gen = (*obj).sxpinfo.gcgen();
                let marked = (*obj).sxpinfo.mark();

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

    if !to_free.is_empty() {
        let unreachable: HashSet<usize> = to_free.iter().map(|&obj| obj as usize).collect();
        let keep_alive = crate::mainutils::memory_main::mark_finalizers_ready_for_unreachable_in(
            &mut instance.memory_state,
            &unreachable,
        );
        if !keep_alive.is_empty() {
            to_free.retain(|obj| !keep_alive.contains(&(*obj as usize)));
        }
    }

    if !to_free.is_empty() {
        let nil = unsafe { crate::sexp::globals::R_NilValue() };
        let old_to_nil: HashMap<usize, SEXP> =
            to_free.iter().map(|&obj| (obj as usize, nil)).collect();
        update_all_references_in(instance, &old_to_nil);

        for obj in to_free {
            instance.arena.free_node(obj);
            freed_count += 1;
        }
    }

    instance.gc_state.remembered_set.clear();

    (promoted_count, freed_count)
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
    instance::with_required_current_instance(full_gc_in)
}

pub(crate) fn full_gc_in(instance: &mut instance::RInstance) -> (usize, usize) {
    run_gc_cycle_in(instance, do_full_mark_sweep_in)
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
fn do_torture_mark_sweep_in(instance: &mut instance::RInstance) -> (usize, usize) {
    mark_from_all_roots_in(instance);

    let mut freed_count = 0;
    let mut to_free = Vec::new();

    {
        let arena = &mut instance.arena;
        for obj in arena.active_nodes() {
            if obj.is_null() {
                continue;
            }
            unsafe {
                if (*obj).sxpinfo.mark() {
                    // Marked nodes stay in their generation: promotion to
                    // the old generation here would let the very next forced
                    // collection (only `gc_force_gap` allocations later)
                    // sweep an in-flight value that is momentarily
                    // reachable only from a Rust local. Promotion stays
                    // the safe-point collectors' job.
                    (*obj).sxpinfo.set_mark(false);
                } else if (*obj).sxpinfo.gcgen() != Generation::Young as u8 {
                    // Old-generation garbage: reclaim now.
                    to_free.push(obj);
                }
                // Unmarked young nodes survive alloc-time collections; the
                // next safe-point collection sweeps whichever stay dead.
            }
        }
    }

    let mut freed_set: HashSet<usize> = HashSet::new();
    if !to_free.is_empty() {
        let unreachable: HashSet<usize> = to_free.iter().map(|&obj| obj as usize).collect();
        let keep_alive = crate::mainutils::memory_main::mark_finalizers_ready_for_unreachable_in(
            &mut instance.memory_state,
            &unreachable,
        );
        if !keep_alive.is_empty() {
            to_free.retain(|obj| !keep_alive.contains(&(*obj as usize)));
        }
    }

    if !to_free.is_empty() {
        freed_set = to_free.iter().map(|&obj| obj as usize).collect();
        let nil = unsafe { crate::sexp::globals::R_NilValue() };
        let old_to_nil: HashMap<usize, SEXP> =
            to_free.iter().map(|&obj| (obj as usize, nil)).collect();
        update_all_references_in(instance, &old_to_nil);

        for obj in to_free {
            instance.arena.free_node(obj);
            freed_count += 1;
        }
    }

    // The remembered set cannot be cleared wholesale (unlike a true full
    // collection, reachable young nodes were not promoted, so live
    // old-to-young edges still exist). Drop only entries whose old parent
    // was reclaimed this cycle.
    instance
        .gc_state
        .remembered_set
        .entries
        .retain(|parent| !parent.is_null() && !freed_set.contains(&(*parent as usize)));

    (0, freed_count)
}

fn mark_from_all_roots_in(instance: &mut instance::RInstance) {
    // No traceable HashSet: use mark bit visited. (Perf + addresses review complaint
    // about allocating HashSets and hashing every edge.)
    mark_instance_roots(instance);
    for &obj in &instance.gc_state.remembered_set.entries {
        mark_reachable(obj);
    }
}

fn do_full_mark_sweep_in(instance: &mut instance::RInstance) -> (usize, usize) {
    mark_from_all_roots_in(instance);

    let mut freed_count = 0;
    let mut promoted_count = 0;
    let mut to_free = Vec::new();

    {
        let arena = &mut instance.arena;
        // Direct iter, only to_free alloc (less GC-time memory pressure).
        for obj in arena.active_nodes() {
            if obj.is_null() {
                continue;
            }
            unsafe {
                if (*obj).sxpinfo.mark() {
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

    if !to_free.is_empty() {
        let unreachable: HashSet<usize> = to_free.iter().map(|&obj| obj as usize).collect();
        let keep_alive = crate::mainutils::memory_main::mark_finalizers_ready_for_unreachable_in(
            &mut instance.memory_state,
            &unreachable,
        );
        if !keep_alive.is_empty() {
            to_free.retain(|obj| !keep_alive.contains(&(*obj as usize)));
        }
    }

    if !to_free.is_empty() {
        let nil = unsafe { crate::sexp::globals::R_NilValue() };
        let old_to_nil: HashMap<usize, SEXP> =
            to_free.iter().map(|&obj| (obj as usize, nil)).collect();
        update_all_references_in(instance, &old_to_nil);

        for obj in to_free {
            instance.arena.free_node(obj);
            freed_count += 1;
        }
    }

    instance.gc_state.remembered_set.clear();

    (promoted_count, freed_count)
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
    with_arena_for_gc(|arena| arena.fragmentation_ratio())
}

/// Force non-moving free-list cleanup.
///
/// This does not move live objects. It only normalizes the arena free list.
pub fn force_compact() {
    with_arena_for_gc(|arena| {
        normalize_free_list(arena);
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
mod tests {
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::super::memory::{ArenaBudget, with_arena};
    use super::super::protect::with_protected_objects;
    use crate::sexp::session::RSession;

    use super::*;

    /// Persistent environment sentinels live outside the arena, so sweep
    /// never resets their mark bits. Before `clear_persistent_node_marks_in`
    /// ran at cycle start, the mark left by cycle one made
    /// `mark_reachable_traced` short-circuit on every later cycle: global
    /// frame bindings were not re-traced and got swept while still
    /// reachable, leaving dangling frame chains that later collections (or
    /// frame walks) dereferenced as garbage.
    #[test]
    fn test_persistent_env_roots_retraced_every_cycle() {
        let _session = RSession::new();

        let sym =
            unsafe { crate::sexp::symbol::Rf_install(b"retrace_probe\0".as_ptr() as *const _) };
        let value = with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1));
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
        assert!(with_arena(|arena| arena.contains(found)));

        full_gc();
        let found = unsafe {
            crate::sexp::envir::R_findVarInFrame(crate::sexp::globals::R_GlobalEnv(), sym)
        };
        assert!(
            found != unsafe { crate::sexp::globals::R_UnboundValue() },
            "global binding swept after second cycle: persistent env mark went stale"
        );
        assert!(with_arena(|arena| arena.contains(found)));
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
        *arena = RArena::new();
        let nil = unsafe { crate::sexp::globals::R_NilValue() };
        instance::with_required_current_instance(|instance| {
            instance.protect_stack.borrow_mut().clear();
            instance.preserve_stack.borrow_mut().clear();
            instance.context_stack.clear();
            instance.gc_state.remembered_set.clear();
            instance.error_state.warnings = nil;
            instance.error_state.handler_stack = nil;
            instance.error_state.restart_stack = nil;
            instance.eval_state.current_expr = nil;
            instance.eval_state.parse_error_file = nil;
            instance.eval_state.exec_token = nil;
            instance.eval_state.profiling.sref = nil;
            instance.eval_state.profiling.srcfiles_buffer = nil;
            instance.eval_state.printvector.na_string = nil;
            instance.eval_state.printvector.na_string_noquote = nil;
            instance.eval_state.print.data.na_string = nil;
            instance.eval_state.print.data.na_string_noquote = nil;
            instance.eval_state.print.data.env = nil;
            instance.eval_state.print.data.callArgs = nil;
            instance.symbols.clear();
            for node in instance.symbol_nodes.drain(..) {
                if !node.is_null() {
                    drop(unsafe { Box::from_raw(node) });
                }
            }
            instance.names_state.ddval_symbols.clear();
            instance.bind_state.blank_string = nil;
            instance.options.clear();
            instance.main_state.task_callbacks.clear();
            instance.objects_state.prim_generics.clear();
            instance.objects_state.prim_mlist.clear();
            instance.env_hash_tables.clear();
            instance.memory_state.pending_finalizers.clear();
            instance.dynload_state.dll_info_eptrs = nil;
            instance.dynload_state.symbol_eptrs = nil;
            instance.dynload_state.c_entry_table = nil;
            instance.grid_runtime_state.current_grid_state = nil;
            instance.grid_runtime_state.eval_env = nil;
            instance.raw_cons.clear();
        });
        for env in [
            unsafe { crate::sexp::globals::R_EmptyEnv() },
            unsafe { crate::sexp::globals::R_BaseEnv() },
            unsafe { crate::sexp::globals::R_GlobalEnv() },
        ] {
            if !env.is_null() {
                unsafe {
                    (*env).data.envsxp.frame = nil;
                    (*env).data.envsxp.hashtab = nil;
                }
            }
        }
    }

    #[test]
    fn test_write_barrier_detects_old_to_young() {
        let _session = RSession::new();

        with_arena(|arena| {
            let old_obj = arena.alloc_node(SEXPTYPE::LISTSXP);
            let young_obj = arena.alloc_node(SEXPTYPE::INTSXP);

            unsafe {
                (*old_obj).sxpinfo.set_gcgen(Generation::Old as u8);
                (*young_obj).sxpinfo.set_gcgen(Generation::Young as u8);
            }

            write_barrier(old_obj, young_obj);

            assert_eq!(with_gc_state(|state| state.remembered_set.len()), 1);
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

        left.protect_stack.borrow_mut().push(old);
        left.preserve_stack.borrow_mut().push(old);
        left.gc_state.remembered_set.add(old);
        right.protect_stack.borrow_mut().push(right_obj);
        right.preserve_stack.borrow_mut().push(right_obj);
        right.gc_state.remembered_set.add(right_obj);

        update_protect_stack_in(&mut left, &old_to_new);
        update_preserve_stack_in(&mut left, &old_to_new);
        update_remembered_set_in(&mut left, &old_to_new);

        assert_eq!(left.protect_stack.borrow()[0], new);
        assert_eq!(left.preserve_stack.borrow()[0], new);
        assert!(left.gc_state.remembered_set.iter().any(|obj| obj == new));
        assert!(!left.gc_state.remembered_set.iter().any(|obj| obj == old));

        assert_eq!(right.protect_stack.borrow()[0], right_obj);
        assert_eq!(right.preserve_stack.borrow()[0], right_obj);
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
        let _session = RSession::new();

        with_arena(|arena| {
            reset_gc_test_arena(arena);
        });
        let (promoted, freed) = minor_gc();
        assert_eq!(promoted, 0);
        assert_eq!(freed, 0);
    }

    #[test]
    fn test_gc_with_only_young_objects() {
        let _session = RSession::new();

        with_arena(|arena| {
            reset_gc_test_arena(arena);
            arena.alloc_node(SEXPTYPE::INTSXP);
            arena.alloc_node(SEXPTYPE::REALSXP);
        });
        let (promoted, freed) = minor_gc();
        assert_eq!(promoted, 0);
        assert_eq!(freed, 2);
    }

    #[test]
    fn test_gc_with_only_old_objects() {
        let _session = RSession::new();

        with_arena(|arena| {
            reset_gc_test_arena(arena);
            let obj1 = arena.alloc_node(SEXPTYPE::INTSXP);
            let obj2 = arena.alloc_node(SEXPTYPE::REALSXP);
            unsafe {
                (*obj1).sxpinfo.set_gcgen(Generation::Old as u8);
                (*obj2).sxpinfo.set_gcgen(Generation::Old as u8);
            }
        });
        let (promoted, freed) = minor_gc();
        assert_eq!(promoted, 0);
        assert_eq!(freed, 0);
    }

    #[test]
    fn test_gc_with_mixed_objects() {
        let _session = RSession::new();

        with_arena(|arena| {
            reset_gc_test_arena(arena);
            let old_obj = arena.alloc_node(SEXPTYPE::INTSXP);
            let young_obj = arena.alloc_node(SEXPTYPE::REALSXP);
            unsafe {
                (*old_obj).sxpinfo.set_gcgen(Generation::Old as u8);
                (*young_obj).sxpinfo.set_gcgen(Generation::Young as u8);
            }
        });
        let (promoted, freed) = minor_gc();
        assert_eq!(promoted, 0);
        assert_eq!(freed, 1);
    }

    #[test]
    fn test_minor_gc_traces_global_environment_bindings() {
        let session = RSession::new();

        let value_raw = with_arena(|arena| {
            let value = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
            unsafe {
                *(crate::sexp::accessors::INTEGER(value)) = 123;
            }
            value
        });
        let value = session.sexp(value_raw).expect("value belongs to session");
        assert!(session.define_var("kept_by_global_env", value));

        with_arena(|arena| {
            let garbage = arena.alloc_node(SEXPTYPE::REALSXP);
            assert!(!garbage.is_null());
        });
        let (_, freed) = minor_gc();
        assert!(freed >= 1);
        with_arena(|arena| {
            for _ in 0..256 {
                assert!(!arena.alloc_node(SEXPTYPE::REALSXP).is_null());
            }
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
        let _session = RSession::new();

        instance::with_required_current_instance(|instance| {
            instance.gc_state.in_progress = true;
            assert_eq!(minor_gc_in(instance), (0, 0));
            assert!(instance.gc_state.in_progress);
            instance.gc_state.in_progress = false;
        });
    }

    #[test]
    fn test_gc_stats_tracking() {
        let _session = RSession::new();

        reset_gc_stats();
        with_arena(|arena| {
            reset_gc_test_arena(arena);
            arena.alloc_node(SEXPTYPE::INTSXP);
        });
        minor_gc();
        let stats = get_gc_stats();
        assert_eq!(stats.collections, 1);
        assert_eq!(stats.freed, 1);
    }

    #[test]
    fn test_gc_callback_invocation() {
        let _session = RSession::new();

        reset_gc_stats();
        let (tx, rx) = std::sync::mpsc::channel();

        register_gc_callback(Box::new(move |_| {
            let _ = tx.send(());
        }));

        with_arena(|arena| {
            reset_gc_test_arena(arena);
            arena.alloc_node(SEXPTYPE::INTSXP);
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
    fn test_full_gc_empty_arena() {
        let _session = RSession::new();

        with_arena(|arena| {
            reset_gc_test_arena(arena);
        });
        let (promoted, freed) = full_gc();
        assert_eq!(promoted, 0);
        assert_eq!(freed, 0);
    }

    #[test]
    fn test_full_gc_collects_unreachable_old_objects() {
        let _session = RSession::new();

        with_arena(|arena| {
            reset_gc_test_arena(arena);
            let obj = arena.alloc_node(SEXPTYPE::INTSXP);
            unsafe {
                (*obj).sxpinfo.set_gcgen(Generation::Old as u8);
            }
        });

        let (promoted, freed) = full_gc();
        assert_eq!(promoted, 0);
        assert_eq!(freed, 1);
        with_arena(|arena| {
            assert_eq!(arena.node_count(), 0);
            assert_eq!(arena.free_count(), 1);
        });
    }

    #[test]
    fn test_full_gc_preserves_protected_old_objects() {
        let _session = RSession::new();

        use super::super::protect::protect;

        let mut obj = ptr::null_mut();
        with_arena(|arena| {
            reset_gc_test_arena(arena);
            obj = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
            unsafe {
                (*obj).sxpinfo.set_gcgen(Generation::Old as u8);
                *(crate::sexp::accessors::INTEGER(obj)) = 42;
            }
            std::mem::forget(protect(obj));
        });

        let (promoted, freed) = full_gc();
        assert_eq!(promoted, 0);
        assert_eq!(freed, 0);

        let protected_obj = with_protected_objects(|objects| objects[0]);
        assert_eq!(protected_obj, obj);
        unsafe {
            assert_eq!((*protected_obj).sxpinfo.type_of(), SEXPTYPE::INTSXP);
            assert_eq!((*protected_obj).vecsxp_length(), 1);
            assert_eq!(*(crate::sexp::accessors::INTEGER(protected_obj)), 42);
        }

        drop(super::super::protect::protect_n(1));
    }

    #[test]
    fn test_full_gc_never_moves_protected_objects() {
        let _session = RSession::new();

        use super::super::protect::protect;

        let mut obj = ptr::null_mut();
        with_arena(|arena| {
            reset_gc_test_arena(arena);
            obj = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
            unsafe {
                (*obj).sxpinfo.set_gcgen(Generation::Old as u8);
                *(crate::sexp::accessors::INTEGER(obj)) = 99;
            }
            std::mem::forget(protect(obj));
            arena.set_budget(ArenaBudget::new(1, 0));
        });

        let (promoted, freed) = full_gc();
        assert_eq!((promoted, freed), (0, 0));

        let protected_obj = with_protected_objects(|objects| objects[0]);
        assert_eq!(protected_obj, obj);
        unsafe {
            assert_eq!((*protected_obj).sxpinfo.type_of(), SEXPTYPE::INTSXP);
            assert_eq!(*(crate::sexp::accessors::INTEGER(protected_obj)), 99);
        }
        with_arena(|arena| {
            assert!(arena.contains(obj));
        });

        drop(super::super::protect::protect_n(1));
    }

    /// GC soundness stress: many protected real vectors must retain their
    /// exact sentinel data across repeated full collections while unprotected
    /// garbage is churned underneath. This exercises the invariant the
    /// conformance suite does not cover: that GC never collects or corrupts a
    /// live (protected) object. A failure here would indicate premature
    /// collection or a mark/sweep bug.
    #[test]
    fn gc_stress_protected_vectors_retain_data_across_collections() {
        let _session = RSession::new();
        use super::super::protect::protect;

        const N: usize = 64;
        const LEN: usize = 8;
        let mut keepers: Vec<SEXP> = Vec::with_capacity(N);
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
        });

        // Churn unprotected garbage and collect repeatedly; prove real work
        // happens (freed > 0) and no protected vector is touched.
        let mut any_freed = 0usize;
        for _ in 0..20 {
            with_arena(|arena| {
                for _ in 0..200 {
                    let _ = arena.alloc_vector(SEXPTYPE::REALSXP, 4);
                }
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

        drop(super::super::protect::protect_n(N));
    }

    #[test]
    fn test_full_gc_preserves_external_pointer_edges_without_moving() {
        let _session = RSession::new();

        use super::super::protect::protect;

        let payload = 0x1234usize as *mut std::ffi::c_void;
        let mut ext = ptr::null_mut();
        let mut tag = ptr::null_mut();
        let mut prot = ptr::null_mut();

        with_arena(|arena| {
            reset_gc_test_arena(arena);
            tag = arena.alloc_node(SEXPTYPE::INTSXP);
            prot = arena.alloc_node(SEXPTYPE::REALSXP);
            ext = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
            unsafe {
                (*ext).sxpinfo.set_gcgen(Generation::Old as u8);
                (*ext).data.extptr = [payload, prot as *mut _, tag as *mut _];
            }
            std::mem::forget(protect(ext));
        });

        let (_, freed) = full_gc();
        assert_eq!(freed, 0);

        let protected_ext = with_protected_objects(|objects| objects[0]);
        assert_eq!(protected_ext, ext);
        unsafe {
            assert_eq!((*protected_ext).sxpinfo.type_of(), SEXPTYPE::EXTPTRSXP);
            assert_eq!((*protected_ext).data.extptr[0], payload);
            let linked_prot = (*protected_ext).data.extptr[1] as SEXP;
            let linked_tag = (*protected_ext).data.extptr[2] as SEXP;
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

        drop(super::super::protect::protect_n(1));
    }

    #[test]
    fn test_full_gc_weakref_traces_value_and_finalizer_but_not_key() {
        let _session = RSession::new();

        use super::super::protect::protect;

        let mut weak = ptr::null_mut();
        let mut key = ptr::null_mut();
        let mut value = ptr::null_mut();
        let mut finalizer = ptr::null_mut();

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
                (*weak).data.listsxp.carval = key;
                (*weak).data.listsxp.cdrval = value;
                (*weak).data.listsxp.tagval = finalizer;
            }
            std::mem::forget(protect(weak));
        });

        let (_, freed) = full_gc();
        assert_eq!(freed, 1);

        let protected_weak = with_protected_objects(|objects| objects[0]);
        assert_eq!(protected_weak, weak);
        unsafe {
            assert_eq!((*protected_weak).sxpinfo.type_of(), SEXPTYPE::WEAKREFSXP);
            assert_eq!(
                (*protected_weak).data.listsxp.carval,
                crate::sexp::globals::R_NilValue()
            );
            let linked_value = (*protected_weak).data.listsxp.cdrval;
            let linked_finalizer = (*protected_weak).data.listsxp.tagval;
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

        drop(super::super::protect::protect_n(1));
    }

    #[test]
    fn test_full_gc_weakref_forwards_live_key_without_marking_it() {
        let _session = RSession::new();

        use super::super::protect::protect;

        let mut weak = ptr::null_mut();
        let mut key = ptr::null_mut();
        let mut value = ptr::null_mut();

        with_arena(|arena| {
            reset_gc_test_arena(arena);
            key = arena.alloc_node(SEXPTYPE::INTSXP);
            value = arena.alloc_node(SEXPTYPE::REALSXP);
            weak = arena.alloc_node(SEXPTYPE::WEAKREFSXP);
            unsafe {
                for obj in [key, value, weak] {
                    (*obj).sxpinfo.set_gcgen(Generation::Old as u8);
                }
                (*weak).data.listsxp.carval = key;
                (*weak).data.listsxp.cdrval = value;
                (*weak).data.listsxp.tagval = crate::sexp::globals::R_NilValue();
            }
            std::mem::forget(protect(weak));
            std::mem::forget(protect(key));
        });

        let (_, freed) = full_gc();
        assert_eq!(freed, 0);

        let (protected_weak, protected_key) =
            with_protected_objects(|objects| (objects[0], objects[1]));
        assert_eq!(protected_weak, weak);
        assert_eq!(protected_key, key);
        unsafe {
            assert_eq!((*protected_weak).sxpinfo.type_of(), SEXPTYPE::WEAKREFSXP);
            let linked_key = (*protected_weak).data.listsxp.carval;
            let linked_value = (*protected_weak).data.listsxp.cdrval;
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

        drop(super::super::protect::protect_n(2));
    }

    #[test]
    fn test_gc_stats_reset() {
        let _session = RSession::new();

        reset_gc_stats();
        with_arena(|arena| {
            reset_gc_test_arena(arena);
            arena.alloc_node(SEXPTYPE::INTSXP);
            arena.alloc_node(SEXPTYPE::REALSXP);
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
        let mut left = RSession::new();
        let mut right = RSession::new();

        left.with_arena(|arena| {
            reset_gc_stats();
            reset_gc_test_arena(arena);
            arena.alloc_node(SEXPTYPE::INTSXP);
            let (_, freed) = minor_gc();
            assert_eq!(freed, 1);
            assert_eq!(get_gc_stats().collections, 1);
        })
        .unwrap();

        right
            .with_arena(|arena| {
                assert_eq!(get_gc_stats().collections, 0);
                reset_gc_stats();
                reset_gc_test_arena(arena);
                arena.alloc_node(SEXPTYPE::INTSXP);
                arena.alloc_node(SEXPTYPE::REALSXP);
                let (_, freed) = minor_gc();
                assert_eq!(freed, 2);
                assert_eq!(get_gc_stats().collections, 1);
            })
            .unwrap();

        left.with_arena(|_| {
            let stats = get_gc_stats();
            assert_eq!(stats.collections, 1);
            assert_eq!(stats.freed, 1);
        })
        .unwrap();
    }

    #[test]
    fn test_session_remembered_sets_are_local_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();

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
                minor_gc();
                assert_eq!(with_gc_state(|state| state.remembered_set.len()), 0);
            })
            .unwrap();

        left.with_arena(|_| {
            assert_eq!(with_gc_state(|state| state.remembered_set.len()), 1);
            minor_gc();
            assert_eq!(with_gc_state(|state| state.remembered_set.len()), 0);
        })
        .unwrap();
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
        let _session = RSession::new();

        for _ in 0..5 {
            with_arena(|arena| {
                reset_gc_test_arena(arena);
                let obj1 = arena.alloc_node(SEXPTYPE::INTSXP);
                let obj2 = arena.alloc_node(SEXPTYPE::REALSXP);
                unsafe {
                    (*obj1).sxpinfo.set_gcgen(Generation::Young as u8);
                    (*obj2).sxpinfo.set_gcgen(Generation::Young as u8);
                }
            });
            let (promoted, freed) = minor_gc();
            assert_eq!(promoted, 0);
            assert_eq!(freed, 2);
        }
    }

    #[test]
    fn test_gc_with_protected_objects() {
        let _session = RSession::new();

        use super::super::protect::protect;

        with_arena(|arena| {
            reset_gc_test_arena(arena);
            let obj = arena.alloc_node(SEXPTYPE::INTSXP);
            unsafe {
                (*obj).sxpinfo.set_gcgen(Generation::Young as u8);
            }
            std::mem::forget(protect(obj));
        });
        let (promoted, freed) = minor_gc();
        assert_eq!(promoted, 1);
        assert_eq!(freed, 0);

        drop(super::super::protect::protect_n(1));
    }

    #[test]
    fn test_compact_if_needed_runs_gc_but_never_moves() {
        let _session = RSession::new();

        with_arena(|arena| {
            reset_gc_test_arena(arena);
        });
        let result = compact_if_needed(0.0);
        assert!(!result);
    }

    #[test]
    fn test_get_fragmentation_ratio_empty() {
        let _session = RSession::new();

        with_arena(|arena| {
            reset_gc_test_arena(arena);
        });
        let ratio = get_fragmentation_ratio();
        assert_eq!(ratio, 0.0);
    }

    #[test]
    fn test_force_compact_only_normalizes_free_list() {
        let _session = RSession::new();

        with_arena(|arena| {
            reset_gc_test_arena(arena);
        });
        force_compact();
    }

    #[test]
    fn test_minor_gc_does_not_refree_nodes_on_free_list() {
        let _session = RSession::new();

        with_arena(|arena| {
            reset_gc_test_arena(arena);
            arena.alloc_vector(SEXPTYPE::REALSXP, 2);
        });

        let (_, freed1) = minor_gc();
        assert_eq!(freed1, 1);
        let free_after_first = with_arena(|arena| arena.free_count());

        let (_, freed2) = minor_gc();
        let free_after_second = with_arena(|arena| arena.free_count());
        assert_eq!(freed2, 0);
        assert_eq!(free_after_second, free_after_first);
    }

    #[test]
    fn test_update_object_references_skips_atomic_vector_payloads() {
        let _session = RSession::new();

        let mut marker: SEXP = ptr::null_mut();
        let mut vec: SEXP = ptr::null_mut();
        with_arena(|arena| {
            reset_gc_test_arena(arena);
            marker = arena.alloc_node(SEXPTYPE::LISTSXP);
            vec = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
        });

        let original_bits = marker as usize as u64;
        unsafe {
            *((*vec).gengc_next_node as *mut f64) = f64::from_bits(original_bits);
        }

        let replacement = 0x1234usize as SEXP;
        let mut map = HashMap::new();
        map.insert(marker as usize, replacement);
        update_object_references(&map);

        let after_bits = unsafe { *((*vec).gengc_next_node as *const f64) }.to_bits();
        assert_eq!(after_bits, original_bits);
    }

    #[test]
    fn test_update_object_references_updates_pointer_vector_payloads() {
        let _session = RSession::new();

        let mut marker: SEXP = ptr::null_mut();
        let mut vec: SEXP = ptr::null_mut();
        with_arena(|arena| {
            reset_gc_test_arena(arena);
            marker = arena.alloc_node(SEXPTYPE::LISTSXP);
            vec = arena.alloc_vector(SEXPTYPE::VECSXP, 1);
        });

        unsafe {
            *((*vec).gengc_next_node as *mut SEXP) = marker;
        }

        let replacement = 0x5678usize as SEXP;
        let mut map = HashMap::new();
        map.insert(marker as usize, replacement);
        update_object_references(&map);

        let after_ptr = unsafe { *((*vec).gengc_next_node as *mut SEXP) };
        assert_eq!(after_ptr, replacement);
    }

    #[test]
    fn test_dotsxp_chain_is_traced_from_protected_head() {
        let _session = RSession::new();

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
                (*tail).data.listsxp.tagval = sym_b;
                (*tail).data.listsxp.carval = two;
                (*tail).data.listsxp.cdrval = nil;
                (*head).data.listsxp.tagval = sym_a;
                (*head).data.listsxp.carval = one;
                (*head).data.listsxp.cdrval = tail;
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
                assert_eq!((*head).data.listsxp.cdrval, tail);
                assert_eq!((*tail).data.listsxp.carval, two);
            }
        });
    }

    #[test]
    fn test_remembered_old_list_keeps_young_car_across_minor_gc() {
        let _session = RSession::new();

        with_arena(|arena| {
            reset_gc_test_arena(arena);

            let parent = arena.alloc_node(SEXPTYPE::LISTSXP);
            let child = arena.alloc_node(SEXPTYPE::INTSXP);
            let nil = unsafe { crate::sexp::globals::R_NilValue() };
            unsafe {
                (*parent).sxpinfo.set_gcgen(Generation::Old as u8);
                (*parent).data.listsxp.carval = child;
                (*parent).data.listsxp.cdrval = nil;
                (*parent).data.listsxp.tagval = nil;
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
                assert_eq!((*parent).data.listsxp.carval, child);
            }
        });
    }

    #[test]
    fn test_remembered_vector_element_survives_and_dedupes() {
        let _session = RSession::new();

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
        });
    }

    #[test]
    fn test_remembered_set_remap_updates_membership() {
        let mut inst = instance::RInstance::new();
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
        assert_eq!(inst.gc_state.remembered_set.len(), 1);
        inst.gc_state.remembered_set.add(old);
        assert_eq!(inst.gc_state.remembered_set.len(), 2);
    }

    fn make_detached_env() -> SEXP {
        with_arena(|arena| {
            let env = arena.alloc_node(SEXPTYPE::ENVSXP);
            unsafe {
                (*env).data.envsxp.frame = crate::sexp::globals::R_NilValue();
                (*env).data.envsxp.enclos = crate::sexp::globals::R_NilValue();
            }
            env
        })
    }

    /// The package namespace cache is the only root for a pure-R package
    /// namespace once attach-time references die. Untraced, a collection
    /// swept the namespace env and left a dangling raw SEXP in the cache.
    #[test]
    fn test_package_namespace_cache_value_is_traced() {
        let _session = RSession::new();

        let payload = with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1));
        unsafe {
            *crate::sexp::accessors::INTEGER(payload) = 4242;
        }
        let namespace = make_detached_env();
        unsafe {
            (*namespace).data.envsxp.frame = payload;
        }
        instance::with_required_current_instance(|inst| {
            inst.package_namespace_cache.insert(
                "gcProbePkg".to_string(),
                (std::path::PathBuf::from("/gc-probe"), namespace),
            );
        });

        // The namespace env and its frame are reachable only through the
        // cache; both cycles must keep them, not just the first.
        for _ in 0..2 {
            full_gc();
            assert!(
                with_arena(|arena| arena.contains(namespace)),
                "cached namespace env swept"
            );
            assert!(
                with_arena(|arena| arena.contains(payload)),
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

        let mut session = RSession::new();
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

        let cached_namespace = instance::with_required_current_instance(|inst| {
            inst.package_namespace_cache
                .get("gcProbePkg")
                .expect("namespace cached")
                .1
        });
        full_gc();
        assert!(
            with_arena(|arena| arena.contains(cached_namespace)),
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
        let _session = RSession::new();
        let roots = with_arena(|arena| {
            (0..7)
                .map(|_| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
                .collect::<Vec<_>>()
        });

        instance::with_required_current_instance(|inst| {
            inst.error_state.warning_call = roots[0];
            inst.objects_state.deferred_default_object = roots[1];
            unsafe { inst.eval_state.bc_stack.push(roots[2]) };
            let mut http_roots = roots[3..6].iter().copied();
            inst.httpd_state
                .visit_roots(|slot| *slot = http_roots.next().expect("HTTP root slot"));
            inst.package_namespace_cache.insert(
                "rootProbePkg".to_string(),
                (std::path::PathBuf::from("/root-probe"), roots[6]),
            );
        });

        full_gc();
        for &root in &roots {
            assert!(
                with_arena(|arena| arena.contains(root)),
                "instance-owned root was swept"
            );
        }

        let replacements = with_arena(|arena| {
            (0..roots.len())
                .map(|_| arena.alloc_vector(SEXPTYPE::REALSXP, 1))
                .collect::<Vec<_>>()
        });
        let remap = roots
            .iter()
            .copied()
            .zip(replacements.iter().copied())
            .map(|(old, new)| (old as usize, new))
            .collect::<HashMap<_, _>>();
        instance::with_required_current_instance(|inst| update_instance_roots_in(inst, &remap));

        instance::with_required_current_instance(|inst| {
            assert_eq!(inst.error_state.warning_call, replacements[0]);
            assert_eq!(inst.objects_state.deferred_default_object, replacements[1]);
            let mut bytecode_root = None;
            inst.eval_state
                .bc_stack
                .visit_roots(|root| bytecode_root = Some(*root));
            assert_eq!(bytecode_root, Some(replacements[2]));
            let mut http_roots = Vec::new();
            inst.httpd_state.visit_roots(|root| http_roots.push(*root));
            assert_eq!(http_roots, replacements[3..6]);
            assert_eq!(
                inst.package_namespace_cache["rootProbePkg"].1,
                replacements[6]
            );
        });
    }

    /// Active-binding values must be traced while their entry exists, and an
    /// entry keyed by a collected env/symbol must be swept before the LIFO
    /// free list recycles the address onto a new node.
    #[test]
    fn test_active_bindings_traced_and_dead_entries_swept() {
        let _session = RSession::new();

        let sym =
            unsafe { crate::sexp::symbol::Rf_install(b"active_probe\0".as_ptr() as *const _) };
        let env = make_detached_env();
        // The binding function is reachable only through the side table.
        let fun = with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1));
        unsafe {
            *crate::sexp::accessors::INTEGER(fun) = 7;
        }
        instance::with_required_current_instance(|inst| {
            inst.active_bindings
                .insert((env as usize, sym as usize), fun);
        });

        full_gc();
        assert!(
            with_arena(|arena| arena.contains(fun)),
            "active binding value swept while its entry was still live"
        );
        assert_eq!(unsafe { *crate::sexp::accessors::INTEGER(fun) }, 7);

        // The env is held only by a raw local the collector does not scan, so
        // this cycle reclaims it and the side-table entry must go with it.
        let env_addr = env as usize;
        full_gc();
        assert!(!with_arena(|arena| arena.contains(env)));
        instance::with_required_current_instance(|inst| {
            assert!(
                !inst.active_bindings.contains_key(&(env_addr, sym as usize)),
                "stale active binding entry survived the keyed env sweep"
            );
        });

        // Recycle the reclaimed address: the stale entry must not alias the
        // new node (a fresh env reporting an active binding it never had).
        let mut recycled = std::ptr::null_mut();
        for _ in 0..1024 {
            recycled = with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP));
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
        let _session = RSession::new();

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

        assert!(with_arena(|arena| arena.contains(live_env)));
        assert!(!with_arena(|arena| arena.contains(dead_env)));
        assert!(crate::sexp::envir::environment_is_locked_raw(live_env));
        assert!(crate::sexp::envir::binding_is_locked_raw(live_env, sym));
        instance::with_required_current_instance(|inst| {
            assert!(inst.locked_environments.contains(&(live_env as usize)));
            assert!(
                !inst.locked_environments.contains(&(dead_env as usize)),
                "locked-environment entry survived the keyed env sweep"
            );
            assert!(
                inst.locked_bindings
                    .contains(&(live_env as usize, sym as usize))
            );
            assert!(
                !inst
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
        let _session = RSession::new();

        let sym_active =
            unsafe { crate::sexp::symbol::Rf_install(b"churn_active\0".as_ptr() as *const _) };
        let sym_locked =
            unsafe { crate::sexp::symbol::Rf_install(b"churn_locked\0".as_ptr() as *const _) };

        let mut rooted = Vec::new();
        let mut transient = Vec::new();
        for i in 0..32usize {
            let env = make_detached_env();
            let fun = with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1));
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
                with_arena(|arena| {
                    let scratch = arena.alloc_vector(SEXPTYPE::INTSXP, 8);
                    unsafe {
                        *crate::sexp::accessors::INTEGER(scratch) = round as i32;
                    }
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
                with_arena(|arena| arena.contains(env)),
                "rooted env {i} swept"
            );
            assert!(crate::sexp::envir::binding_is_active_raw(env, sym_active));
            assert!(crate::sexp::envir::binding_is_locked_raw(env, sym_locked));
            assert!(with_arena(|arena| arena.contains(fun)));
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
            assert!(!with_arena(|arena| arena.contains(env)));
            instance::with_required_current_instance(|inst| {
                assert!(
                    !inst
                        .active_bindings
                        .contains_key(&(env as usize, sym_active as usize))
                );
                assert!(
                    !inst
                        .locked_bindings
                        .contains(&(env as usize, sym_locked as usize))
                );
                assert!(!inst.locked_environments.contains(&(env as usize)));
            });
        }
    }
}
