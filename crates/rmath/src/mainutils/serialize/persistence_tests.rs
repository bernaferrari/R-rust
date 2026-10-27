use super::*;
use crate::sexp::{
    object::{SessionNodeFactory, Sexp, SexpMut},
    session::RSession,
};

fn env<'s>(factory: &SessionNodeFactory<'s>) -> Sexp<'s> {
    factory
        .wrap(unsafe {
            crate::sexp::memory_ext::NewEnvironment(R_NilValue(), R_EmptyEnv(), R_NilValue())
        })
        .unwrap()
}

fn define(factory: &SessionNodeFactory<'_>, environment: &Sexp<'_>, name: &CStr, value: Sexp<'_>) {
    let symbol = factory.wrap(unsafe { Rf_install(name.as_ptr()) }).unwrap();
    unsafe { crate::sexp::envir::define_var_safe(symbol, value, environment.clone()) };
}

#[test]
fn serialization_workspace_lazy_cache_keeps_keys_and_entries_charged_until_reader_drop() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let data = env(&factory);
    let cache = env(&factory);
    let replacement = env(&factory);
    define(&factory, &cache, c"env::broken", replacement.clone());
    define(&factory, &data, c"cache", cache);
    let fixture = include_bytes!("fixtures/gnu-persistent-environment-v2.rds");
    let mut reader = BinaryReader::new(&fixture[14..]);
    reader.set_xdr_body(true);
    reader.lazy_restore = Some(LazyLoadRestore {
        data: data.into_owned().unwrap(),
    });
    let mut refs = ReadRefTable::for_owner(&owner);
    let restored = unsafe { ReadItemInternal(&mut reader, &mut refs).unwrap() };
    assert_eq!(restored, replacement.as_raw());
    let combined = owner.with_arena(|a| a.workspace_bytes()).unwrap();
    drop(reader);
    let references = owner.with_arena(|a| a.workspace_bytes()).unwrap();
    assert!(
        combined > references,
        "private cache storage must retain its own admission"
    );
    assert!(references > 0);
    drop(refs);
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
}

fn fixture<'s>(
    factory: &SessionNodeFactory<'s>,
) -> (Sexp<'s>, Sexp<'s>, Sexp<'s>, std::path::PathBuf) {
    let data = env(factory);
    let cache = env(factory);
    let refs = env(factory);
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "rport-persistence-error-{}-{}.rdb",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::write(&path, b"badRDS").unwrap();
    let file = factory.strings(&[path.to_str().unwrap()]).unwrap();
    let compressed = factory.wrap(unsafe { Rf_ScalarInteger(0) }).unwrap();
    let key = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 2)))
        .unwrap();
    let mut key = SexpMut::try_from_checked(key).unwrap();
    key.try_set_integer_elt(0, 0).unwrap();
    key.try_set_integer_elt(1, 6).unwrap();
    define(factory, &refs, c"env::broken", key.freeze());
    define(factory, &data, c"cache", cache.clone());
    define(factory, &data, c"refs", refs);
    define(factory, &data, c"datafile", file);
    define(factory, &data, c"compressed", compressed);
    let bytes = include_bytes!("fixtures/gnu-persistent-environment-v2.rds");
    let input = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t)))
        .unwrap();
    let mut input = SexpMut::try_from_checked(input).unwrap();
    for (i, byte) in bytes.iter().copied().enumerate() {
        input.try_set_raw_elt(i as R_xlen_t, byte).unwrap();
    }
    let input = input.freeze();
    (data, cache, input, path)
}

#[test]
fn lazy_load_persistence_failure_preserves_error_and_never_returns_empty_success() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let (data, cache, input, path) = fixture(&factory);
        for _ in 0..2 {
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                R_unserialize(input.as_raw(), data.as_raw())
            }))
            .expect_err("corrupt persistent reference must propagate its actual error");
            assert_eq!(
                failure
                    .downcast_ref::<crate::sexp::context::RError>()
                    .unwrap()
                    .message,
                "unknown input format"
            );
            let symbol = factory
                .wrap(unsafe { Rf_install(c"env::broken".as_ptr()) })
                .unwrap();
            assert!(
                unsafe { crate::sexp::envir::find_var_in_frame_result(cache.clone(), symbol) }
                    .unwrap()
                    .is_none(),
                "failed placeholder must not authenticate retry"
            );
            session.owner_token().unwrap().full_gc().unwrap();
        }
        std::fs::remove_file(path).unwrap();
    });
}

#[test]
fn lazy_persistence_rollback_keeps_original_and_callback_bindings_after_revocation() {
    use crate::sexp::{
        ffi::{EdgeField, NodeBody},
        owner::WeakOwner,
    };
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let (data, cache, input, path, weak, before) = {
        let facade = facade.borrow();
        let session = facade.as_ref().unwrap();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let (data, cache, input, path) = fixture(&factory);
            let keep = factory.wrap(unsafe { Rf_ScalarInteger(61) }).unwrap();
            define(&factory, &cache, c"caller.keep", keep);
            let before = cache.try_frame().unwrap().into_owned().unwrap();
            (
                data.into_owned().unwrap(),
                cache.into_owned().unwrap(),
                input.into_owned().unwrap(),
                path,
                session.owner_token().unwrap().weak_owner().unwrap(),
                before,
            )
        })
    };
    let weak: WeakOwner = weak;
    let pin = weak.pin().unwrap();
    let instance = pin.as_ptr();
    let observed = Rc::new(Cell::new(0));
    let callback_observed = observed.clone();
    let callback_head = Rc::new(RefCell::new(None));
    let saved_head = callback_head.clone();
    let callback_cache = cache.clone();
    let callback_facade = Rc::downgrade(&facade);
    let callback_weak = weak.clone();
    let failure = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if callback_observed.get() != 0 {
                    return;
                }
                let factory = callback_weak.node_factory().unwrap();
                let symbol = factory.wrap(Rf_install(c"env::broken".as_ptr())).unwrap();
                if crate::sexp::envir::find_var_in_frame_result(callback_cache.clone(), symbol)
                    .unwrap()
                    .is_none()
                {
                    return;
                }
                callback_observed.set(1);
                (*instance).memory_state.gc_force_gap = 0;
                let keep = factory.wrap(Rf_ScalarInteger(87)).unwrap();
                define(&factory, &callback_cache, c"callback.keep", keep);
                *saved_head.borrow_mut() =
                    Some(callback_cache.try_frame().unwrap().into_owned().unwrap());
                crate::sexp::gengc::full_gc();
                drop(callback_facade.upgrade().unwrap().borrow_mut().take());
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            R_unserialize(input.as_raw(), data.as_raw())
        })
    }))
    .expect_err("revoked original cannot publish a partially restored environment");
    assert_eq!(
        observed.get(),
        1,
        "callback must occur after provisional cache publication"
    );
    assert_eq!(
        failure
            .downcast_ref::<crate::sexp::context::RError>()
            .unwrap()
            .message,
        crate::sexp::object::SexpError::RootUnavailable.to_string()
    );
    assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
    let head = callback_head.borrow_mut().take().unwrap();
    let cache_node = cache.allocation().unwrap();
    let heap = cache_node.heap_identity();
    assert_eq!(
        heap.edge(cache_node, EdgeField::EnvironmentFrame),
        head.allocation().unwrap().link()
    );
    let NodeBody::List(body) = heap.node_snapshot(head.allocation().unwrap()).unwrap().data else {
        panic!("callback binding remains a list cell")
    };
    assert_eq!(
        Some(body.cdrval),
        before.allocation().unwrap().link(),
        "exact failed middle entry removed without revoking other bindings"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn bounded_small_decodes_do_not_retrace_an_unchanged_high_live_heap() {
    use crate::sexp::{gengc::get_gc_stats, memory::ArenaBudget, session::RSession};
    let mut session = RSession::new_for_gc_tests();
    session.set_arena_budget(ArenaBudget::new(usize::MAX, 222));
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let retained: Vec<_> = (0..200)
        .map(|_| {
            factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
                .unwrap()
        })
        .collect();
    owner.full_gc().unwrap();
    // A complete XDR version-2 RDS integer scalar. Repeating this small read
    // near the node cap must leave room for useful work between full traces.
    let mut bytes = b"X\n".to_vec();
    for value in [2_i32, 0x40700, 0x20300, 13, 1, 7] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    let before = get_gc_stats().collections;
    for _ in 0..3 {
        let mut reader = BinaryReader::new(&bytes);
        let value = unsafe { decode_stream_object(&owner, &mut reader) };
        assert_eq!(owner.sexp(value).unwrap().integer_elt(0), Some(7));
    }
    assert_eq!(
        get_gc_stats().collections,
        before,
        "small decodes retraced the same retained graph"
    );
    // Continued garbage growth must still trigger collection before the
    // unchanged cap is reached; amortization must not disable reclamation.
    for _ in 0..30 {
        let mut reader = BinaryReader::new(&bytes);
        let value = unsafe { decode_stream_object(&owner, &mut reader) };
        assert_eq!(owner.sexp(value).unwrap().integer_elt(0), Some(7));
    }
    let collections = get_gc_stats().collections - before;
    assert!(
        (1..=4).contains(&collections),
        "unexpected traces: {collections}"
    );
    assert_eq!(retained.len(), 200);
    assert_eq!(session.arena_budget().max_nodes, 222);
}

#[test]
fn imported_bytecode_reuses_private_metadata_within_one_original_owner() {
    use crate::sexp::{object::SexpMut, session::RSession};
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../r-embed/tests/fixtures/gnu-bytecode-metadata-sharing.rds"
    ));
    let input = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::RAWSXP, bytes.len() as _)))
        .unwrap();
    let mut input = SexpMut::try_from_checked(input).unwrap();
    for (index, byte) in bytes.iter().enumerate() {
        input.try_set_raw_elt(index as _, *byte).unwrap();
    }
    let input = input.freeze();
    let result = unsafe { R_unserialize(input.as_raw(), R_NilValue()) };
    let result = owner.sexp(result).unwrap();
    unsafe {
        let left = BODY(VECTOR_ELT(result.as_raw(), 0));
        let right = BODY(VECTOR_ELT(result.as_raw(), 1));
        assert_eq!(TYPEOF(left), SEXPTYPE::BCODESXP);
        assert_eq!(TYPEOF(right), SEXPTYPE::BCODESXP);
        assert_eq!(
            VECTOR_ELT(left, 3),
            VECTOR_ELT(right, 3),
            "duplicate private bytecode dialect markers"
        );
        assert_eq!(
            VECTOR_ELT(left, 2),
            VECTOR_ELT(right, 2),
            "duplicate private imported stack hints"
        );
        owner.full_gc().unwrap();
        assert_eq!(
            owner.sexp(VECTOR_ELT(left, 2)).unwrap().integer_elt(0),
            Some(4)
        );
        assert_eq!(
            owner.sexp(VECTOR_ELT(right, 3)).unwrap().integer_elt(0),
            Some(crate::eval::bytecode::GNU_BC_DIALECT_MARKER)
        );
    }
}

// Independently accepted by pinned GNU R's unserialize(refhook = identity):
// list(persistent "token", as.raw(rep(42L, 65536L))).
fn owned_reader_payload() -> Vec<u8> {
    let mut bytes = b"X\n".to_vec();
    for word in [2_i32, 0x40700, 0x20300, 19, 2, 247, 0, 1, 262153, 5] {
        bytes.extend_from_slice(&word.to_be_bytes());
    }
    bytes.extend_from_slice(b"token");
    for word in [24_i32, 65536] {
        bytes.extend_from_slice(&word.to_be_bytes());
    }
    bytes.resize(bytes.len() + 65536, 42);
    bytes
}

#[test]
fn owned_reader_reuses_admitted_snapshot_under_original_cap() {
    let mut session = RSession::new_for_gc_tests();
    let hook = session
        .eval_code_with_output_capture("function(x) { .Internal(gc(FALSE, FALSE, TRUE)); x }")
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    session.with_active(|| unsafe {
        crate::mainutils::options::InitOptions();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let source = factory
            .wrap(raw_from_bytes(&owned_reader_payload()))
            .unwrap();
        let bytes = super::super::compress::raw_snapshot(&owner, source.as_raw(), 0, false);
        let workspace = owner.with_arena(|a| a.workspace_bytes()).unwrap();
        let limit = owner
            .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes() + 65536 + 32768)
            .unwrap();
        owner
            .with_arena(|a| a.set_budget(crate::sexp::memory::ArenaBudget::new(limit, 0)))
            .unwrap();
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            R_unserialize(source.as_raw(), hook.as_raw())
        }))
        .expect_err("the public raw API must admit its separate mutable-input snapshot");
        assert!(
            failure
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("memory")
        );
        owner.full_gc().unwrap();
        let value = unserialize_native_snapshot(&owner, &bytes, &hook).unwrap();
        let result = value.try_vector_elt(1).unwrap();
        assert_eq!(result.len(), 65536);
        assert_eq!(result.try_raw_elt(0).unwrap(), 42);
        assert_eq!(result.try_raw_elt(65535).unwrap(), 42);
        assert_eq!(
            owner.with_arena(|a| a.workspace_bytes()).unwrap(),
            workspace
        );
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
    });
}

fn owned_reader_hook_reentry(action: u8) {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let hook = facade
        .borrow_mut()
        .as_mut()
        .unwrap()
        .eval_code_with_output_capture("function(x) { .Internal(gc(FALSE, FALSE, TRUE)); x }")
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    let (weak, source) = {
        let facade = facade.borrow();
        let session = facade.as_ref().unwrap();
        let owner = session.owner_token().unwrap();
        let source = session.with_active(|| unsafe {
            owner
                .sexp(raw_from_bytes(&owned_reader_payload()))
                .unwrap()
                .into_owned()
                .unwrap()
        });
        (owner.weak_owner().unwrap(), source)
    };
    let pin = weak.pin().unwrap();
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let closed = Rc::downgrade(&facade);
    let callback_source = source.clone();
    let replacement = Rc::new(RefCell::new(None));
    let installed = replacement.clone();
    unsafe {
        crate::sexp::session::with_instance_active(pin.as_ptr(), || {
            let owner = crate::sexp::owner::OwnerToken::from_raw(pin.as_ptr());
            let bytes = super::super::compress::raw_snapshot(&owner, source.as_raw(), 0, false);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if observed.replace(observed.get() + 1) != 0 {
                    return;
                }
                match action {
                    0 => {
                        let mut source =
                            SexpMut::try_from_checked(callback_source.clone()).unwrap();
                        for index in 0..source.len() {
                            source.try_set_raw_elt(index, 0).unwrap();
                        }
                    }
                    1 => {
                        drop(closed.upgrade().unwrap().borrow_mut().take());
                        *installed.borrow_mut() = Some(RSession::new_for_gc_tests());
                    }
                    2 => std::panic::panic_any(819_u32),
                    _ => unreachable!(),
                }
            }));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                unserialize_native_snapshot(&owner, &bytes, &hook)
            }));
            assert!(calls.get() > 0, "actual restoration closure must collect");
            if action == 0 {
                let value = result.unwrap().unwrap();
                let raw = value.try_vector_elt(1).unwrap();
                assert_eq!(raw.try_raw_elt(0).unwrap(), 42);
                assert_eq!(raw.try_raw_elt(65535).unwrap(), 42);
                assert_eq!(source.try_raw_elt(0).unwrap(), 0);
            } else {
                let error =
                    result.expect_err("a revoked owner or hook unwind must not publish success");
                if action == 2 {
                    assert_eq!(error.downcast_ref::<u32>(), Some(&819));
                } else {
                    assert_eq!(
                        error
                            .downcast_ref::<crate::sexp::context::RError>()
                            .unwrap()
                            .message,
                        crate::sexp::object::SexpError::RootUnavailable.to_string()
                    );
                }
            }
        });
    }
}

#[test]
fn owned_reader_snapshot_survives_collecting_hook_input_mutation() {
    owned_reader_hook_reentry(0);
}
#[test]
fn owned_reader_refuses_revoked_owner_after_collecting_hook() {
    owned_reader_hook_reentry(1);
}
#[test]
fn owned_reader_preserves_collecting_hook_unwind() {
    owned_reader_hook_reentry(2);
}

#[test]
fn native_lazy_codec_retry_reclaims_garbage_but_preserves_retained_denial() {
    use std::{cell::Cell, rc::Rc};
    for keep in [false, true] {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let garbage = owner
                .node_factory()
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, 128 * 1024)))
                .unwrap();
            let retained = keep.then(|| garbage.clone());
            drop(garbage);
            let limit = owner
                .with_arena(|arena| arena.total_bytes_allocated() + 4096)
                .unwrap();
            owner
                .with_arena(|arena| {
                    arena.set_budget(crate::sexp::memory::ArenaBudget::new(limit, 0))
                })
                .unwrap();
            let calls = Rc::new(Cell::new(0));
            let observed = calls.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                observed.set(observed.get() + 1)
            }));
            let result = decode_lazy_record(
                &owner,
                include_bytes!("fixtures/gnu-lazy-codecs/mode1.bin"),
                1,
            );
            assert_eq!(
                calls.get(),
                1,
                "only an actual denied codec gets one collecting retry"
            );
            if keep {
                assert!(matches!(
                    result,
                    Err(super::super::raw_codecs::DecodeError::Resource(_))
                ));
                assert_eq!(retained.as_ref().unwrap().len(), 128 * 1024);
            } else {
                assert_eq!(
                    &*result.unwrap(),
                    include_bytes!("fixtures/gnu-lazy-codecs/expected.rds")
                );
            }
            assert_eq!(
                owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                0
            );
            assert_eq!(
                owner.with_arena(|arena| arena.budget().max_bytes).unwrap(),
                limit
            );
        });
    }
}

#[test]
fn native_lazy_codec_retry_refuses_owner_revoked_by_collection() {
    use std::{cell::RefCell, rc::Rc};
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let weak = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let pin = weak.pin().unwrap();
    let closed = Rc::downgrade(&facade);
    let replacement = Rc::new(RefCell::new(None));
    let installed = replacement.clone();
    unsafe {
        crate::sexp::session::with_instance_active(pin.as_ptr(), || {
            let owner = crate::sexp::owner::OwnerToken::from_raw(pin.as_ptr());
            owner
                .with_arena(|arena| {
                    arena.alloc_vector(SEXPTYPE::RAWSXP, 128 * 1024);
                })
                .unwrap();
            let limit = owner
                .with_arena(|arena| arena.total_bytes_allocated() + 4096)
                .unwrap();
            owner
                .with_arena(|arena| {
                    arena.set_budget(crate::sexp::memory::ArenaBudget::new(limit, 0))
                })
                .unwrap();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                drop(closed.upgrade().unwrap().borrow_mut().take());
                *installed.borrow_mut() = Some(RSession::new_for_gc_tests());
            }));
            let result = decode_lazy_record(
                &owner,
                include_bytes!("fixtures/gnu-lazy-codecs/mode1.bin"),
                1,
            );
            let Err(super::super::raw_codecs::DecodeError::Resource(message)) = result else {
                panic!("revocation must prevent a retry against the replacement runtime");
            };
            assert_eq!(
                message,
                crate::sexp::object::SexpError::RootUnavailable.to_string()
            );
        });
    }
}

#[test]
fn native_lazy_fetch_collecting_retry_retains_entire_call_argument_graph() {
    use std::{cell::Cell, rc::Rc};
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let path =
            std::env::temp_dir().join(format!("rport-owned-fetch-{}.rdb", std::process::id()));
        let fixture = include_bytes!("fixtures/gnu-lazy-codecs/mode1.bin");
        std::fs::write(&path, fixture).unwrap();
        let args = {
            let file = factory.strings(&[path.to_str().unwrap()]).unwrap();
            let key = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 2)))
                .unwrap();
            let mut key = SexpMut::try_from_checked(key).unwrap();
            key.try_set_integer_elt(0, 0).unwrap();
            key.try_set_integer_elt(1, fixture.len() as i32).unwrap();
            let compressed = factory.wrap(Rf_ScalarInteger(1)).unwrap();
            let mut args = factory.nil();
            for value in [factory.nil(), compressed, file, key.freeze()] {
                args = factory
                    .pairlist_cell(&value, &args, &factory.nil())
                    .unwrap();
            }
            args
        };
        owner
            .with_arena(|arena| {
                arena.alloc_vector(SEXPTYPE::RAWSXP, 128 * 1024);
            })
            .unwrap();
        let limit = owner
            .with_arena(|arena| arena.total_bytes_allocated() + 4096)
            .unwrap();
        owner
            .with_arena(|arena| arena.set_budget(crate::sexp::memory::ArenaBudget::new(limit, 0)))
            .unwrap();
        let raw_args = args.as_raw();
        drop(args);
        let calls = Rc::new(Cell::new(0));
        let observed = calls.clone();
        let weak = owner.weak_owner().unwrap();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            let graph = weak.node_factory().unwrap().wrap(raw_args).unwrap();
            assert_eq!(graph.typeof_(), SEXPTYPE::LISTSXP);
            assert_eq!(graph.try_car().unwrap().len(), 2);
            observed.set(observed.get() + 1);
        }));
        let value = do_lazyLoadDBfetch(R_NilValue(), R_NilValue(), raw_args, R_NilValue());
        let value = owner.sexp(value).unwrap();
        assert_eq!(value.integer_elt(0), Some(7));
        assert_eq!(value.integer_elt(127), Some(7));
        assert_eq!(calls.get(), 1);
        assert_eq!(
            owner.with_arena(|arena| arena.budget().max_bytes).unwrap(),
            limit
        );
        std::fs::remove_file(path).unwrap();
    });
}
