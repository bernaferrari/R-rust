//! gengc tests — extracted verbatim from gengc.rs.

use super::super::memory::with_arena;
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

    let sym = unsafe { crate::sexp::symbol::Rf_install(b"retrace_probe\0".as_ptr() as *const _) };
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
    let found =
        unsafe { crate::sexp::envir::R_findVarInFrame(crate::sexp::globals::R_GlobalEnv(), sym) };
    assert!(found != unsafe { crate::sexp::globals::R_UnboundValue() });
    assert!(
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| arena.contains(found))
        })
    );

    full_gc();
    let found =
        unsafe { crate::sexp::envir::R_findVarInFrame(crate::sexp::globals::R_GlobalEnv(), sym) };
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
    let found =
        unsafe { crate::sexp::envir::R_findVarInFrame(crate::sexp::globals::R_GlobalEnv(), sym) };
    assert!(
        found != unsafe { crate::sexp::globals::R_UnboundValue() },
        "global binding swept by preamble-less minor gc"
    );
    assert_eq!(unsafe { *crate::sexp::accessors::INTEGER(found) }, 42);
}
