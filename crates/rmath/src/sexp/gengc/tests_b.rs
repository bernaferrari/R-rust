//! gengc tests — extracted verbatim from gengc.rs.

use crate::sexp::protect::push_protect_in;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::super::memory::{ArenaBudget, with_arena};
use super::super::protect::with_protected_objects;
use crate::sexp::session::RSession;

use super::test_support::reset_gc_test_arena;
use super::*;

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
            // The heap domain retains the retired arena's backing
            // store (canonical values pin physical pages), so the old
            // handle may still validate; the reset above guarantees it
            // is no longer reachable from any root or persistent node.
            assert!(arena_token.same_heap(&sentinel_token));
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

    let sym = unsafe { crate::sexp::symbol::Rf_install(b"active_probe\0".as_ptr() as *const _) };
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
        // Boot-time bindings grew with the base-source work; the
        // registration allocation can cross the deferred-collection
        // threshold and request a collection. Re-anchor the no-pending
        // state so the flush contract below is what is under test.
        instance::with_required_current_instance(|inst| {
            (*inst).gc_state.gc_pending = false;
        });

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
        // See the sibling quiescent test: re-anchor the no-pending
        // state after the registration allocation. The safe-point
        // predicate also fires on arena growth, so anchor the growth
        // baseline too — the boot-time bindings from the base-source
        // work grew the session past the delta threshold.
        instance::with_required_current_instance(|inst| {
            (*inst).gc_state.gc_pending = false;
            (*inst).arena.nodes_at_last_gc = (*inst).arena.node_count();
        });

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
