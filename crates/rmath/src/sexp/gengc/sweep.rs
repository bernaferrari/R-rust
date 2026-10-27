//! gengc — extracted verbatim from gengc.rs.

use super::*;

// ---------------------------------------------------------------------------
// Reference Updating
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn update_field(field: &mut SEXP, old_to_new: &HashMap<usize, SEXP>) {
    if field.is_null() {
        return;
    }
    let addr = *field as usize;
    if let Some(&new_ptr) = old_to_new.get(&addr) {
        *field = new_ptr;
    }
}

#[inline]
pub(crate) fn vector_payload_has_sexp_refs(t: SEXPTYPE) -> bool {
    // BCODESXP (21) is included: its payload holds the instruction stream,
    // constant pool, and stack-depth vector as SEXP references. Without
    // tracing them, a collection frees a compiled closure's code while the
    // BCODESXP itself survives (e.g. `f <- function(x) x+1; gc(); f(1)`).
    matches!(t.0, 16 | 19 | 20 | 21) // STRSXP, VECSXP, EXPRSXP, BCODESXP
}

pub(crate) fn update_protect_stack_in(
    instance: *mut instance::RInstance,
    old_to_new: &HashMap<usize, SEXP>,
) {
    unsafe {
        update_protect_stack_refs_in(instance, |ptr| {
            let addr = ptr as usize;
            old_to_new.get(&addr).copied().unwrap_or(ptr)
        });
    }
}

pub(crate) fn update_preserve_stack_in(
    instance: *mut instance::RInstance,
    old_to_new: &HashMap<usize, SEXP>,
) {
    unsafe {
        update_preserve_stack_refs_in(instance, |ptr| {
            let addr = ptr as usize;
            old_to_new.get(&addr).copied().unwrap_or(ptr)
        });
    }
}

pub(crate) fn update_remembered_set(old_to_new: &HashMap<usize, SEXP>) {
    instance::with_required_current_instance(|instance| {
        update_remembered_set_in(instance, old_to_new)
    });
}

pub(crate) fn update_remembered_set_in(
    instance: *mut instance::RInstance,
    old_to_new: &HashMap<usize, SEXP>,
) {
    unsafe {
        with_gc_state_in(instance, |state| {
            state.remembered_set.remap(old_to_new);
        });
    }
}

pub(crate) fn update_references_in_object(obj: SEXP, old_to_new: &HashMap<NodeLink, NodeLink>) {
    if obj.is_null() || super::super::globals::immutable_singleton_projection(obj).is_some() {
        return;
    }
    let (_, parent) = super::super::memory::checked_projection(obj)
        .expect("GC parent belongs to checked storage");
    gc_trace_bridge::remap_children(&parent, old_to_new);
}

#[cfg(test)]
pub(crate) fn update_object_references(old_to_new: &HashMap<NodeLink, NodeLink>) {
    instance::with_required_current_instance(|owner| {
        update_object_references_in(owner, old_to_new);
    });
}

#[cfg(test)]
pub(crate) fn update_object_references_in(
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
pub(crate) fn update_swept_object_references_in(
    instance: *mut instance::RInstance,
    old_to_nil: &HashMap<NodeLink, NodeLink>,
) {
    let nodes: Vec<SEXP> = unsafe { (*instance).arena.active_nodes().collect() };
    let permanent: Vec<SEXP> = unsafe { (*instance).persistent_nodes.projections().collect() };
    for obj in nodes.into_iter().chain(permanent) {
        if super::super::memory::arena_node_marked(obj) {
            let (_, parent) = super::super::memory::checked_projection(obj)
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

pub(crate) fn sweep_link_remap(
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

pub(crate) fn remap_addr(addr: usize, old_to_new: &HashMap<usize, SEXP>) -> usize {
    old_to_new
        .get(&addr)
        .copied()
        .map(|ptr| ptr as usize)
        .unwrap_or(addr)
}

pub(crate) fn update_instance_roots_in(
    instance: *mut instance::RInstance,
    old_to_new: &HashMap<usize, SEXP>,
) {
    unsafe {
        update_field(&mut (*instance).empty_env, old_to_new);
        update_field(&mut (*instance).base_env, old_to_new);
        update_field(&mut (*instance).global_env, old_to_new);

        // Nonmoving sweep normally remaps only dead headers. Strong checked
        // roots therefore need no rewrite. Avoid allocating callback snapshots
        // when the map cannot change any root, including under a lowered limit.
        // Explicit remaps of a live root still use the exact-lease update path.
        let mapped = |value: RootValue| {
            value
                .projection()
                .is_some_and(|ptr| old_to_new.contains_key(&ptr.addr()))
        };
        if (*instance)
            .legacy_protect
            .checked_roots_for_marking()
            .chain((*instance).root_table.checked_roots_for_marking())
            .any(mapped)
        {
            update_protect_stack_in(instance, old_to_new);
        }
        if (*instance)
            .preserve_stack
            .checked_roots_for_marking()
            .any(mapped)
        {
            update_preserve_stack_in(instance, old_to_new);
        }

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
        update_field(&mut (*instance).arena.blank_string, old_to_new);

        let old_cache = std::mem::take(&mut (*instance).package_namespace_cache);
        (*instance).package_namespace_cache = old_cache
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

pub(crate) fn clear_swept_references_in(
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
pub(crate) fn normalize_free_list(arena: &mut RArena) {
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
