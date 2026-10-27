use crate::mainutils::native_symbols::{self, AttributeKeys};
use crate::sexp::{
    ffi::{SEXP, SEXPTYPE},
    object::{Sexp, SexpMut},
    owner::OwnerToken,
    session::RSession,
};

fn keys(owner: OwnerToken<'_>) -> AttributeKeys<'_> {
    unsafe {
        AttributeKeys {
            names: owner
                .sexp(crate::sexp::attrib_core::R_NamesSymbol())
                .unwrap(),
            class: owner
                .sexp(crate::sexp::attrib_core::R_ClassSymbol())
                .unwrap(),
        }
    }
}

fn info(owner: OwnerToken<'_>) -> Sexp<'static> {
    native_symbols::symbol_info(owner, "R_identC", "methods", true, &keys(owner))
        .unwrap()
        .unwrap()
}

#[test]
fn typed_native_symbol_handle_is_exactly_bound_and_roots_its_original_allocation() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let object = info(owner);
        let address = object.try_vector_elt(1).unwrap().into_owned().unwrap();
        let authority = native_symbols::symbol(owner, &address).unwrap();
        let attributes = address.try_attrib().unwrap().into_owned().unwrap();
        let token = address.allocation().unwrap().clone();
        drop(object);
        drop(address);
        owner.full_gc().unwrap();
        assert!(
            token.is_live(),
            "the operation authority must retain its source allocation"
        );
        assert!(authority.routine().is_ok());

        let factory = owner.node_factory();
        let fake = factory
            .allocate(|a| Some(a.alloc_node(SEXPTYPE::EXTPTRSXP)))
            .unwrap();
        let mut mutable = SexpMut::try_from_checked(fake.clone()).unwrap();
        mutable.try_set_attribute(&attributes).unwrap();
        drop(mutable);
        unsafe {
            crate::mainutils::memory_main::R_SetExternalPtrAddr(
                fake.as_raw(),
                std::ptr::without_provenance_mut(1),
            );
        }
        assert!(
            native_symbols::symbol(owner, &fake).is_err(),
            "pointer bits cannot authenticate a handle"
        );
        drop(authority);
        owner.full_gc().unwrap();
        assert!(!token.is_live());
    });
}

#[test]
fn typed_native_symbol_clear_and_address_replacement_revoke_retained_authority() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        for replace in [false, true] {
            let object = info(owner);
            let address = object.try_vector_elt(1).unwrap();
            let authority = native_symbols::symbol(owner, &address).unwrap();
            let before = owner.with_arena(|a| a.workspace_bytes()).unwrap();
            unsafe {
                if replace {
                    crate::mainutils::memory_main::R_SetExternalPtrAddr(
                        address.as_raw(),
                        std::ptr::without_provenance_mut(1),
                    );
                } else {
                    crate::mainutils::memory_main::R_ClearExternalPtr(address.as_raw());
                }
            }
            assert!(authority.routine().is_err());
            assert!(native_symbols::symbol(owner, &address).is_err());
            assert_eq!(
                owner.with_arena(|a| a.workspace_bytes()).unwrap(),
                before,
                "revocation must retain the actual resource storage charge"
            );
        }
    });
}

#[test]
fn typed_native_symbol_invalidation_does_not_remove_unrelated_resources() {
    struct Other(u32);
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let resource = std::rc::Rc::new(Other(713));
        let object = owner
            .node_factory()
            .allocate(|a| {
                let pointer = a.alloc_node(SEXPTYPE::EXTPTRSXP);
                let node = a.node_token(pointer)?;
                a.heap_identity().attach_resource(&node, resource.clone())?;
                Some(pointer)
            })
            .unwrap();
        unsafe {
            crate::mainutils::memory_main::R_SetExternalPtrAddr(
                object.as_raw(),
                std::ptr::without_provenance_mut(1),
            );
            crate::mainutils::memory_main::R_ClearExternalPtr(object.as_raw());
        }
        let node = object.allocation().unwrap();
        assert_eq!(node.heap_identity().resource::<Other>(node).unwrap().0, 713);
        assert!(native_symbols::symbol(owner, &object).is_err());
    });
}

#[test]
fn typed_native_symbol_authority_rejects_revoked_owner_and_replacement_domain() {
    let original = RSession::new_for_gc_tests();
    let (address, authority, ledger, weak) = original.with_active(|| {
        let owner = original.owner_token().unwrap();
        let address = info(owner).try_vector_elt(1).unwrap().into_owned().unwrap();
        let authority = native_symbols::symbol(owner, &address).unwrap();
        let ledger = address.allocation().unwrap().heap_identity().byte_ledger();
        let weak = owner.weak_owner().unwrap();
        (address, authority, ledger, weak)
    });
    drop(original);
    assert_eq!(weak.allocation_strong_count(), 0);
    drop(weak);
    let replacement = RSession::new_for_gc_tests();
    replacement.with_active(|| {
        let owner = replacement.owner_token().unwrap();
        let live = info(owner);
        let live_address = live.try_vector_elt(1).unwrap();
        let before = owner.with_arena(|a| a.workspace_bytes()).unwrap();
        assert!(authority.routine().is_err());
        assert!(native_symbols::symbol(owner, &address).is_err());
        drop(authority);
        drop(address);
        assert_eq!(
            ledger.workspace_bytes_for_test(),
            0,
            "the retired handle must not form a resource cycle"
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), before);
        assert!(
            native_symbols::symbol(owner, &live_address)
                .unwrap()
                .routine()
                .is_ok()
        );
    });
}

#[test]
fn typed_native_symbol_metadata_denial_precedes_allocation() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let keys = keys(owner);
        let before = owner
            .with_arena(|a| {
                let state = (
                    a.total_bytes_allocated(),
                    a.workspace_bytes(),
                    a.node_count(),
                );
                a.set_budget(crate::sexp::memory::ArenaBudget::new(state.0 + state.1, 0));
                state
            })
            .unwrap();
        assert!(native_symbols::symbol_info(owner, "R_identC", "methods", true, &keys).is_err());
        assert_eq!(
            owner
                .with_arena(|a| (
                    a.total_bytes_allocated(),
                    a.workspace_bytes(),
                    a.node_count()
                ))
                .unwrap(),
            before
        );
    });
}

thread_local! { static INVOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
unsafe extern "C-unwind" fn counted_call(_: SEXP) -> SEXP {
    INVOCATIONS.with(|n| n.set(n.get() + 1));
    unsafe { crate::sexp::globals::R_NilValue() }
}
unsafe extern "C-unwind" fn counted_external(_: SEXP) -> SEXP {
    INVOCATIONS.with(|n| n.set(n.get() + 1));
    unsafe { crate::sexp::globals::R_NilValue() }
}

#[test]
fn typed_native_symbol_descriptor_rejects_wrong_arity_before_callback() {
    use crate::mainutils::native_routines::{CallRoutine, NativeRoutine};
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        INVOCATIONS.with(|n| n.set(0));
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let address = native_symbols::test_symbol_pointer(
            owner,
            NativeRoutine::Call(CallRoutine::Args1(counted_call)),
        )
        .unwrap();
        let nil = factory.nil();
        let tail = factory.pairlist_cell(&nil, &nil, &nil).unwrap();
        let tail = factory.pairlist_cell(&nil, &tail, &nil).unwrap();
        let args = factory.pairlist_cell(&address, &tail, &nil).unwrap();
        let op = factory
            .wrap(crate::eval::primitive::make_primitive_binding(
                ".Call",
                SEXPTYPE::BUILTINSXP,
            ))
            .unwrap();
        let before = owner.with_arena(|a| a.node_count()).unwrap();
        let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::mainutils::dotcode::do_dotcall(
                nil.as_raw(),
                op.as_raw(),
                args.as_raw(),
                nil.as_raw(),
            )
        }))
        .unwrap_err();
        let error = denied.downcast::<crate::sexp::context::RError>().unwrap();
        assert!(
            error.message.contains("expected 1, received 2"),
            "{}",
            error.message
        );
        INVOCATIONS.with(|n| assert_eq!(n.get(), 0));
        assert_eq!(owner.with_arena(|a| a.node_count()).unwrap(), before);
    });
}

#[test]
fn typed_native_symbol_rechecks_revocation_after_collecting_argument_construction() {
    use crate::mainutils::native_routines::{NativeRoutine, PayloadArity};
    use std::{cell::Cell, rc::Rc};
    for clear in [false, true] {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            INVOCATIONS.with(|n| n.set(0));
            let owner = session.owner_token().unwrap();
            let factory = owner.node_factory();
            let address = native_symbols::test_symbol_pointer(
                owner,
                NativeRoutine::External1(counted_external, PayloadArity::Fixed(0)),
            )
            .unwrap();
            let nil = factory.nil();
            let args = factory.pairlist_cell(&address, &nil, &nil).unwrap();
            let op = factory
                .wrap(crate::eval::primitive::make_primitive_binding(
                    ".External",
                    SEXPTYPE::BUILTINSXP,
                ))
                .unwrap();
            let raw = address.as_raw();
            let observed = Rc::new(Cell::new(0));
            let notifications = observed.clone();
            let weak = owner.weak_owner().unwrap();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if notifications.replace(1) != 0 {
                    return;
                }
                let pin = weak.pin().unwrap();
                (*pin.as_ptr()).memory_state.gc_force_gap = 0;
                if clear {
                    crate::mainutils::memory_main::R_ClearExternalPtr(raw);
                }
            }));
            (*owner.as_ptr()).memory_state.gc_force_gap = 1;
            (*owner.as_ptr()).memory_state.gc_force_wait = 1;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::mainutils::dotcode::do_External(
                    nil.as_raw(),
                    op.as_raw(),
                    args.as_raw(),
                    nil.as_raw(),
                )
            }));
            assert_eq!(observed.get(), 1, "the construction must actually collect");
            if clear {
                let error = result
                    .unwrap_err()
                    .downcast::<crate::sexp::context::RError>()
                    .unwrap();
                assert!(error.message.contains("native symbol reference"));
                INVOCATIONS.with(|n| assert_eq!(n.get(), 0));
            } else {
                assert!(factory.wrap(result.unwrap()).unwrap().is_nil());
                INVOCATIONS.with(|n| assert_eq!(n.get(), 1));
            }
        });
    }
}

#[test]
fn typed_native_symbol_name_copy_is_bounded_and_denies_before_buffer_allocation() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let character = factory.character("R_identC").unwrap();
        let name = native_symbols::name(owner, &character).unwrap();
        assert_eq!(name.as_str(), "R_identC");
        drop(name);
        let long = factory.character(&"x".repeat(1024)).unwrap();
        let before = owner.with_arena(|a| a.workspace_bytes()).unwrap();
        assert!(native_symbols::name(owner, &long).is_err());
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), before);
        let state = owner
            .with_arena(|a| {
                let state = (
                    a.total_bytes_allocated(),
                    a.workspace_bytes(),
                    a.node_count(),
                );
                a.set_budget(crate::sexp::memory::ArenaBudget::new(state.0 + state.1, 0));
                state
            })
            .unwrap();
        assert!(native_symbols::name(owner, &character).is_err());
        assert_eq!(
            owner
                .with_arena(|a| (
                    a.total_bytes_allocated(),
                    a.workspace_bytes(),
                    a.node_count()
                ))
                .unwrap(),
            state
        );
    });
}

#[test]
fn typed_native_symbol_introspection_keeps_foreign_loading_disabled() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        assert!(!super::native_extensions_enabled());
        let _table = native_symbols::dll_table(owner, &keys(owner)).unwrap();
        let _info = info(owner);
        assert!(!super::native_extensions_enabled());
        let nil = owner.node_factory().nil();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::do_dynload(nil.as_raw(), nil.as_raw(), nil.as_raw(), nil.as_raw())
        }))
        .unwrap_err()
        .downcast::<crate::sexp::context::RError>()
        .unwrap();
        assert!(
            result.message.contains("disabled in this pure-R"),
            "{}",
            result.message
        );
    });
}

#[test]
fn typed_native_symbol_lookup_rejects_library_clear_or_owner_replacement_from_name_provider() {
    use crate::sexp::altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement};
    use crate::sexp::object::SexpResult;
    use std::{
        cell::{Cell, RefCell},
        rc::{Rc, Weak},
    };
    struct NameProvider {
        replace: bool,
        sessions: Weak<RefCell<Option<RSession>>>,
        replacement: Weak<RefCell<Option<RSession>>>,
        calls: Rc<Cell<usize>>,
    }
    impl AltrepClass for NameProvider {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::STRSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(1)
        }
        fn element<'s>(
            &self,
            context: &AltrepContext<'s>,
            _: i64,
        ) -> SexpResult<AltrepElement<'s>> {
            self.calls.set(self.calls.get() + 1);
            let name = context.string("R_identC")?;
            context.gc()?;
            if self.replace {
                drop(self.sessions.upgrade().unwrap().borrow_mut().take());
                *self.replacement.upgrade().unwrap().borrow_mut() =
                    Some(RSession::new_for_gc_tests());
            } else {
                let reference = context.data1()?;
                unsafe {
                    crate::mainutils::memory_main::R_ClearExternalPtr(reference.as_raw());
                }
            }
            Ok(AltrepElement::String(name))
        }
    }
    for replace in [false, true] {
        let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
        let replacement = Rc::new(RefCell::new(None::<RSession>));
        let calls = Rc::new(Cell::new(0));
        let (name, reference, registration, weak) = {
            let borrowed = sessions.borrow();
            let session = borrowed.as_ref().unwrap();
            let owner = session.owner_token().unwrap();
            let dll = native_symbols::dll_info(owner, "methods", &keys(owner)).unwrap();
            let reference = dll.try_vector_elt(4).unwrap().into_owned().unwrap();
            let class = session
                .register_altrep_class(
                    "native_symbol_lookup_name",
                    NameProvider {
                        replace,
                        sessions: Rc::downgrade(&sessions),
                        replacement: Rc::downgrade(&replacement),
                        calls: calls.clone(),
                    },
                )
                .unwrap()
                .into_owned()
                .unwrap();
            let name = AltrepBuilder::new(class)
                .data1(reference.clone())
                .build()
                .unwrap()
                .into_owned()
                .unwrap();
            (
                name,
                reference,
                owner
                    .node_factory()
                    .domain()
                    .logical(true)
                    .into_owned()
                    .unwrap(),
                owner.weak_owner().unwrap(),
            )
        };
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::R_getSymbolInfo(name.as_raw(), reference.as_raw(), registration.as_raw())
        }))
        .unwrap_err()
        .downcast::<crate::sexp::context::RError>()
        .unwrap();
        assert_eq!(calls.get(), 1);
        if replace {
            assert!(
                error.message.contains("owner could not retain its root"),
                "{}",
                error.message
            );
            assert_eq!(
                weak.allocation_strong_count(),
                0,
                "the entry pin must unwind without retaining the closed runtime"
            );
            // ALTREP's scoped activation restores no ambient pointer when its
            // previous owner was revoked; the replacement remains usable.
            assert_eq!(crate::sexp::instance::current_instance_ptr(), None);
            let replacement = replacement.borrow();
            let replacement = replacement.as_ref().unwrap();
            replacement.with_active(|| {
                let owner = replacement.owner_token().unwrap();
                let valid = info(owner).try_vector_elt(1).unwrap();
                assert!(
                    native_symbols::symbol(owner, &valid)
                        .unwrap()
                        .routine()
                        .is_ok()
                );
            });
        } else {
            assert!(
                error.message.contains("native symbol reference"),
                "{}",
                error.message
            );
        }
    }
}
