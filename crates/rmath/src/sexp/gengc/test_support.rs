//! gengc shared test helper — extracted verbatim from gengc.rs.

use super::super::memory::RArena;
use super::*;

pub(crate) fn reset_gc_test_arena(arena: &mut RArena) {
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
                (*env).data.environment_mut().hashtab = arena.link_from_projection(nil).unwrap();
                (*env).data.environment_mut().enclos = arena.link_from_projection(enclos).unwrap();
            }
        }
    }
}
