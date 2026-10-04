use super::*;
use crate::sexp::session::RSession;

#[test]
fn methods_implicit_base_namespace_does_not_publish_stats_toeplitz() {
    let session = RSession::new_without_default_packages();
    session.with_active(|| unsafe {
        assert_eq!(
            crate::sexp::envir::R_findVarInFrame(
                crate::sexp::globals::R_BaseEnv(),
                Rf_install(c"toeplitz".as_ptr())
            ),
            crate::sexp::globals::R_UnboundValue(),
            "GNU base namespace has no stats::toeplitz binding"
        );
    });
}

// Faithful prefix of the namespace loader, ending at the original registration
// seam. This keeps installed methods bodies and lazy promises unchanged, and
// deliberately makes no claim about the later complete cacheMetaData startup.
fn populated_methods_namespace(session: &RSession) -> crate::sexp::Sexp<'static> {
    session.with_active(|| unsafe {
        let path = find_package_path("methods");
        assert!(
            !path.is_empty(),
            "actual installed methods namespace required"
        );
        let path = Path::new(&path);
        let guard = NamespaceLoadGuard::enter("methods");
        assert!(!guard.reentered);
        let factory = session.owner_token().unwrap().node_factory();
        let ns = factory
            .wrap(crate::sexp::memory_ext::NewEnvironment(
                R_NilValue(),
                crate::sexp::envir::R_BaseNamespace(),
                R_NilValue(),
            ))
            .unwrap()
            .into_owned()
            .unwrap();
        define_package_metadata("methods", ns.as_raw());
        cache_package_namespace("methods", path, ns.as_raw());
        crate::library::methods::native_calls::install_methods_call_symbols(ns.as_raw());
        populate_package_namespace("methods", path, ns.as_raw(), &mut vec!["methods".into()])
            .expect("original methods lazy population");
        bind_methods_base_primitives(ns.as_raw());
        purge_missing_arg_placeholders(ns.as_raw());
        retarget_methods_generics(ns.as_raw());
        ns
    })
}

#[test]
fn methods_implicit_registration_preserves_stats_unloaded_with_real_table() {
    let mut session = RSession::new_without_default_packages();
    session.set_library_paths(
        crate::mainutils::paths::RuntimePathPolicy::from_env()
            .library_paths()
            .to_vec(),
    );
    let ns = populated_methods_namespace(&session);
    session.with_active(|| unsafe {
        assert!(cached_namespace_by_name("stats").is_none());
        register_implicit_generics_table(ns.as_raw());
        assert!(
            cached_namespace_by_name("stats").is_none(),
            "GNU methods registration must not load stats"
        );
    });
    let (value, _, _) = session.eval_script_with_output_capture(
        r#"
      ns <- asNamespace("methods")
      tab <- get(".__IG__table", ns)
      identical(slot(get("toeplitz", tab), "package"), "stats") &&
        is.null(get("implicitGeneric", ns)("toeplitz", where=baseenv())) &&
        !isNamespaceLoaded("stats")
    "#,
    );
    assert_eq!(value.unwrap().logical_elt(0), Some(TRUE));
    session.owner_token().unwrap().full_gc().unwrap();
    session.with_active(|| unsafe {
        assert_eq!(cached_namespace_by_name("methods").unwrap(), ns.as_raw());
        assert!(cached_namespace_by_name("stats").is_none());
    });
}

#[test]
fn methods_implicit_registration_preserves_explicit_stats_closure_identity() {
    let mut session = RSession::new_without_default_packages();
    session.set_library_paths(
        crate::mainutils::paths::RuntimePathPolicy::from_env()
            .library_paths()
            .to_vec(),
    );
    let methods = populated_methods_namespace(&session);
    session.with_active(|| unsafe {
        let factory = session.owner_token().unwrap().node_factory();
        let stats = factory.wrap(load_package_namespace_by_name("stats").unwrap()).unwrap().into_owned().unwrap();
        let symbol = factory.wrap(Rf_install(c"toeplitz".as_ptr())).unwrap();
        let binding = crate::sexp::envir::find_var_in_frame_result(stats.clone(), symbol.clone()).unwrap().unwrap();
        let function = if binding.typeof_() == SEXPTYPE::PROMSXP {
            crate::sexp::envir::force_promise_result(binding.clone()).unwrap().unwrap()
        } else { binding.clone() };
        assert_eq!(TYPEOF(function.as_raw()), SEXPTYPE::CLOSXP);
        assert_eq!(crate::sexp::accessors::CLOENV(function.as_raw()), stats.as_raw());
        let body = function.try_body().unwrap();
        register_implicit_generics_table(methods.as_raw());
        session.owner_token().unwrap().full_gc().unwrap();
        let current_binding = crate::sexp::envir::find_var_in_frame_result(stats.clone(), symbol).unwrap().unwrap();
        assert_eq!(current_binding, binding, "registration must preserve the original lazy binding");
        let current = if current_binding.typeof_() == SEXPTYPE::PROMSXP {
            crate::sexp::envir::force_promise_result(current_binding).unwrap().unwrap()
        } else { current_binding };
        assert_eq!(current, function, "registration must preserve the original stats closure, not replace it with a base wrapper");
        assert_eq!(current.try_body().unwrap(), body);
        assert_eq!(crate::sexp::accessors::CLOENV(current.as_raw()), stats.as_raw());
        assert_eq!(cached_namespace_by_name("stats").unwrap(), stats.as_raw());
        assert_eq!(crate::sexp::envir::R_findVarInFrame(crate::sexp::globals::R_BaseEnv(), Rf_install(c"toeplitz".as_ptr())), crate::sexp::globals::R_UnboundValue());
    });
    // Supplemental exact GNU numerical behavior through the original namespace
    // closure. The older default-constructor S4 tests remain unchanged.
    let (result, _, _) = session.eval_script_with_output_capture(
        r"
        identical(as.vector(stats::toeplitz(c(-1, 0, 0), c(-1, 11, 0))),
                  c(-1, 0, 0, 11, -1, 0, 0, 11, -1)) &&
        identical(as.vector(stats::toeplitz(1:3)),
                  c(1L, 2L, 3L, 2L, 1L, 2L, 3L, 2L, 1L))
    ",
    );
    assert_eq!(result.unwrap().logical_elt(0), Some(TRUE));
}

fn registration_parser_collecting_callback(revoke: bool) {
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let (owner, instance, namespace) = facade.borrow().as_ref().unwrap().with_active(|| unsafe {
        let token = crate::sexp::owner::OwnerToken::current().unwrap();
        let owner = token.weak_owner().unwrap();
        let namespace = token
            .sexp(crate::sexp::memory_ext::NewEnvironment(
                R_NilValue(),
                crate::sexp::globals::R_EmptyEnv(),
                R_NilValue(),
            ))
            .unwrap()
            .into_owned()
            .unwrap();
        (owner, token.as_ptr(), namespace)
    });
    let original_pin = owner.pin().unwrap();
    let node = namespace.allocation().unwrap().clone();
    let projection = namespace.as_raw();
    drop(namespace);
    let observed = Rc::new(Cell::new(0));
    let callback_observed = observed.clone();
    let callback_facade = Rc::downgrade(&facade);
    let outcome = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                callback_observed.set(callback_observed.get() + 1);
                (*instance).memory_state.gc_force_gap = 0;
                assert!(
                    node.is_live(),
                    "only the production helper's input owner keeps the detached namespace alive through parser GC"
                );
                assert_eq!(
                    node.heap_identity()
                        .node_snapshot(&node)
                        .unwrap()
                        .sxpinfo
                        .type_of(),
                    SEXPTYPE::ENVSXP
                );
                assert_eq!((*instance).legacy_protect.len(), 0);
                if revoke {
                    drop(callback_facade.upgrade().unwrap().borrow_mut().take());
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            register_implicit_generics_table(projection);
        })
    }));
    assert_eq!(
        observed.get(),
        1,
        "actual allocation collection callback must run"
    );
    assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
    if revoke {
        assert!(facade.borrow().is_none());
        assert!(original_pin.require_live().is_err());
        assert!(
            outcome.unwrap_err().is::<crate::sexp::context::RError>(),
            "revoked original runtime must deny publication through a typed error"
        );
    } else {
        assert!(outcome.is_ok());
        assert!(original_pin.require_live().is_ok());
    }
}

#[test]
fn methods_implicit_collecting_parser_retains_detached_original_namespace() {
    registration_parser_collecting_callback(false);
}

#[test]
fn methods_implicit_collecting_parser_denies_revoked_original_publication() {
    registration_parser_collecting_callback(true);
}
