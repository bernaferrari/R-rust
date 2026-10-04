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
