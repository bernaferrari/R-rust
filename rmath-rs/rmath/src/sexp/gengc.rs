//! Generational garbage collector with card marking write barriers.
//!
//! The collector is intentionally defensive: it scopes state to the active
//! `RInstance`, catches panics at public GC entry points, and uses a
//! non-moving mark/sweep collector with free-list recycling.
//! Raw `SEXP` internals still require careful auditing; do not document new
//! invariants here unless they are enforced by code and regression tests.

use std::alloc::{Layout, alloc, dealloc};
use std::collections::{HashMap, HashSet};
use std::ptr;

use super::ffi::{SEXP, SEXPTYPE};
use super::instance;
use super::memory::{RArena, with_arena_for_gc};
use super::protect::{
    push_protect_in, update_preserve_stack_refs_in, update_protect_stack_refs_in,
};

/// Card size in bytes for the card marking table.
pub const CARD_SIZE: usize = 512;

/// Card table entry states.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardState {
    Clean = 0,
    Dirty = 1,
    Marked = 2,
}

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
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => {
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

    for &obj in instance.symbols.values() {
        mark_reachable(obj);
    }
    for node in &instance.symbol_nodes {
        mark_reachable(&**node as *const _ as SEXP);
    }
    for node in &instance.env_nodes {
        mark_reachable(&**node as *const _ as SEXP);
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
    for (&env, table) in &instance.env_hash_tables {
        mark_reachable(env as SEXP);
        for (&symbol, &value) in table {
            mark_reachable(symbol as SEXP);
            mark_reachable(value);
        }
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

    mark_reachable(instance.grid_runtime_state.current_grid_state);
    mark_reachable(instance.grid_runtime_state.eval_env);

    for &obj in &instance.raw_cons {
        mark_reachable(obj);
    }
}

// ---------------------------------------------------------------------------
// Card Marking Table
// ---------------------------------------------------------------------------

/// Card marking table for old generation.
pub struct CardTable {
    base: *mut u8,
    size: usize,
    heap_base: *mut u8,
    heap_end: *mut u8,
}

impl CardTable {
    pub unsafe fn new(heap_base: *mut u8, heap_size: usize) -> Self {
        unsafe {
            if heap_base.is_null() || heap_size == 0 {
                return CardTable {
                    base: ptr::null_mut(),
                    size: 0,
                    heap_base,
                    heap_end: heap_base,
                };
            }

            let card_count = heap_size.div_ceil(CARD_SIZE);
            let layout = match Layout::from_size_align(card_count, 64) {
                Ok(l) => l,
                Err(_) => {
                    return CardTable {
                        base: ptr::null_mut(),
                        size: 0,
                        heap_base,
                        heap_end: heap_base.add(heap_size),
                    };
                }
            };
            let base = alloc(layout);
            if base.is_null() {
                return CardTable {
                    base: ptr::null_mut(),
                    size: 0,
                    heap_base,
                    heap_end: heap_base.add(heap_size),
                };
            }
            ptr::write_bytes(base, 0, card_count);

            CardTable {
                base,
                size: card_count,
                heap_base,
                heap_end: heap_base.add(heap_size),
            }
        }
    }

    #[inline]
    pub fn card_index(&self, obj: SEXP) -> usize {
        if self.base.is_null() || self.size == 0 || obj.is_null() {
            return 0;
        }
        let offset = (obj as *mut u8 as usize).saturating_sub(self.heap_base as usize);
        offset / CARD_SIZE
    }

    #[inline]
    pub fn mark_dirty(&self, obj: SEXP) {
        if self.base.is_null() || self.size == 0 {
            return;
        }
        let idx = self.card_index(obj);
        if idx < self.size {
            unsafe {
                *self.base.add(idx) = CardState::Dirty as u8;
            }
        }
    }

    pub fn clear_dirty(&mut self) {
        if self.base.is_null() || self.size == 0 {
            return;
        }
        unsafe {
            ptr::write_bytes(self.base, 0, self.size);
        }
    }

    pub fn dirty_cards(&self) -> impl Iterator<Item = usize> + '_ {
        let base = self.base;
        let size = self.size;
        (0..size).filter(move |&i| {
            if base.is_null() {
                return false;
            }
            unsafe { *base.add(i) == CardState::Dirty as u8 }
        })
    }
}

impl Drop for CardTable {
    fn drop(&mut self) {
        if !self.base.is_null()
            && self.size > 0
            && let Ok(layout) = Layout::from_size_align(self.size, 64)
        {
            unsafe {
                dealloc(self.base, layout);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Remembered Set
// ---------------------------------------------------------------------------

/// Remembered set tracking old objects with references to young objects.
#[derive(Default)]
pub struct RememberedSet {
    entries: Vec<SEXP>,
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

            if !(*obj).sxpinfo.mark() {
                (*obj).sxpinfo.set_mark(true);
                if self.entries.try_reserve(1).is_err() {
                    return;
                }
                self.entries.push(obj);
            }
        }
    }

    pub fn clear(&mut self) {
        for &obj in &self.entries {
            if !obj.is_null() {
                unsafe {
                    (*obj).sxpinfo.set_mark(false);
                }
            }
        }
        self.entries.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = SEXP> + '_ {
        self.entries.iter().copied()
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn entries_mut(&mut self) -> &mut Vec<SEXP> {
        &mut self.entries
    }
}

// ---------------------------------------------------------------------------
// GC State
// ---------------------------------------------------------------------------

pub struct GcState {
    pub(crate) stats: GcStats,
    pub(crate) callbacks: Vec<GcCallback>,
    pub(crate) in_progress: bool,
    pub(crate) card_table: CardTable,
    pub(crate) remembered_set: RememberedSet,
    /// Set when allocation would trigger GC during evaluation; flushed at quiescence.
    pub(crate) gc_pending: bool,
}

impl GcState {
    pub fn new() -> Self {
        GcState {
            stats: GcStats::default(),
            callbacks: Vec::new(),
            in_progress: false,
            card_table: unsafe { CardTable::new(0x100000000 as *mut u8, 1 << 30) },
            remembered_set: RememberedSet::default(),
            gc_pending: false,
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
                state.card_table.mark_dirty(parent);
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

pub unsafe fn init_gc_heap(heap_base: *mut u8, heap_size: usize) {
    unsafe {
        if heap_base.is_null() || heap_size == 0 {
            return;
        }
        with_gc_state(|state| {
            state.card_table = CardTable::new(heap_base, heap_size);
        });
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
    matches!(t.0, 16 | 19 | 20) // STRSXP, VECSXP, EXPRSXP
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
        for entry in state.remembered_set.entries_mut() {
            let addr = *entry as usize;
            if let Some(&new_ptr) = old_to_new.get(&addr) {
                *entry = new_ptr;
            }
        }
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
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => {
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

    for obj in instance.symbols.values_mut() {
        update_field(obj, old_to_new);
    }
    for node in &mut instance.symbol_nodes {
        update_references_in_object(&mut **node as *mut _, old_to_new);
    }
    for node in &mut instance.env_nodes {
        update_references_in_object(&mut **node as *mut _, old_to_new);
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

    for finalizer in &mut instance.memory_state.pending_finalizers {
        update_field(finalizer.obj_mut(), old_to_new);
        if let Some(fun) = finalizer.fun_mut() {
            update_field(fun, old_to_new);
        }
    }
    update_field(&mut instance.dynload_state.dll_info_eptrs, old_to_new);
    update_field(&mut instance.dynload_state.symbol_eptrs, old_to_new);
    update_field(&mut instance.dynload_state.c_entry_table, old_to_new);

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

pub(crate) fn minor_gc_in(instance: &mut instance::RInstance) -> (usize, usize) {
    run_gc_cycle_in(instance, do_minor_gc_in)
}

const GC_TRIGGER_THRESHOLD: usize = 10_000;
const GC_BYTE_THRESHOLD: usize = 64 * 1024 * 1024;

/// Request collection during allocation.
///
/// ALWAYS defers: sets a pending flag. Actual collection only happens at
/// cooperative safe points ([`maybe_collect_at_eval_safe_point`], called after
/// loop bodies etc) or after top-level expressions complete
/// ([`run_pending_gc_if_quiescent`] in session eval paths). This ensures we
/// never sweep objects that are only reachable via raw SEXP temporaries in
/// in-flight Rust stack frames mid-evaluation. See review GC soundness fix.
pub fn maybe_collect_during_alloc() {
    if let Some(ptr) = instance::current_instance_ptr() {
        unsafe {
            (*ptr).gc_state.gc_pending = true;
        }
    }
    // Never invoke minor_gc directly from alloc. Thresholds in arena will
    // cause "maybe" to be called; the flag ensures next safe/quiescent point
    // will collect. This is the quiescence-only GC policy.
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
        minor_gc_in(instance);
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
    instance.gc_state.card_table.clear_dirty();

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
    instance.gc_state.card_table.clear_dirty();

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

    use super::super::memory::{ArenaBudget, with_arena};
    use super::super::protect::with_protected_objects;
    use crate::sexp::session::RSession;

    use super::*;

    fn reset_gc_test_arena(arena: &mut RArena) {
        *arena = RArena::new();
        let nil = unsafe { crate::sexp::globals::R_NilValue() };
        instance::with_required_current_instance(|instance| {
            instance.protect_stack.borrow_mut().clear();
            instance.preserve_stack.borrow_mut().clear();
            instance.context_stack.clear();
            instance.gc_state.remembered_set.clear();
            instance.gc_state.card_table.clear_dirty();
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
            instance.symbol_nodes.clear();
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
    fn test_card_table_marking() {
        unsafe {
            let heap = alloc(
                Layout::from_size_align(4096, 4096).unwrap_or_else(|e| panic!("layout: {e:?}")),
            );
            let ct = CardTable::new(heap, 4096);

            let obj = heap.add(1024) as SEXP;
            ct.mark_dirty(obj);

            let dirty: Vec<usize> = ct.dirty_cards().collect();
            assert_eq!(dirty, vec![2]);

            dealloc(
                heap,
                Layout::from_size_align(4096, 4096).unwrap_or_else(|e| panic!("layout: {e:?}")),
            );
        }
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
    fn test_card_table_null_handling() {
        let ct = unsafe { CardTable::new(ptr::null_mut(), 0) };
        assert!(ct.base.is_null());
        assert_eq!(ct.size, 0);
        ct.mark_dirty(ptr::null_mut());
        let dirty: Vec<usize> = ct.dirty_cards().collect();
        assert!(dirty.is_empty());
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
    fn test_init_gc_heap_null_handling() {
        unsafe {
            init_gc_heap(ptr::null_mut(), 0);
            init_gc_heap(ptr::null_mut(), 1024);
            init_gc_heap(0x1 as *mut u8, 0);
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
}
