use crate::sexp::{
    RSession, SEXPTYPE,
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    ffi::NodeBody,
    heap::CheckedNode,
    object::{Sexp, SexpError, SexpResult},
    owner::WeakOwner,
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

struct DetachedParameters {
    facade: Weak<RefCell<Option<RSession>>>,
    calls: Rc<Cell<usize>>,
    object: CheckedNode,
    action: u8,
}

impl AltrepClass for DetachedParameters {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }

    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(3)
    }

    fn element<'s>(
        &self,
        context: &AltrepContext<'s>,
        index: i64,
    ) -> SexpResult<AltrepElement<'s>> {
        if self.calls.replace(self.calls.get() + 1) == 0 {
            let head = context.data1()?;
            let nil = head.try_cdr()?.try_cdr()?;
            let node = head.allocation()?;
            let heap = node.heap_identity();
            let mut header = heap.node_snapshot(node).unwrap();
            let NodeBody::List(body) = &mut header.data else {
                panic!("expected source pairlist")
            };
            body.carval = nil.link_in(&heap)?;
            body.cdrval = nil.link_in(&heap)?;
            heap.replace_node(node, header).unwrap();
            context.gc()?;
            assert!(
                self.object.is_live(),
                "normalization must retain the detached target through full GC"
            );
            if self.action == 2 {
                self.facade
                    .upgrade()
                    .unwrap()
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .close();
            }
            if self.action != 0 {
                std::panic::panic_any(863_u32);
            }
        }
        Ok(AltrepElement::Real([1., 4., 1.][index as usize]))
    }
}

fn real_vector(owner: &WeakOwner, numbers: &[f64]) -> Sexp<'static> {
    owner
        .node_factory()
        .unwrap()
        .allocate(|arena| {
            let pointer = arena.alloc_vector(SEXPTYPE::REALSXP, numbers.len() as i64);
            let token = arena.node_token(pointer)?;
            let payload = arena.heap_identity().payload_lease(&token)?;
            for (index, number) in numbers.iter().copied().enumerate() {
                payload.set_real_elt(index, number)?;
            }
            Some(pointer)
        })
        .unwrap()
        .into_owned()
        .unwrap()
}

fn detaching_parameters(action: u8, publication: bool) {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let calls = Rc::new(Cell::new(0));
    let (owner, source, object_pointer, input_pointer, object_identity, input_identity) = {
        let borrow = facade.borrow();
        let session = borrow.as_ref().unwrap();
        session.with_active(|| {
            let owner = session.owner_token().unwrap().weak_owner().unwrap();
            let factory = owner.node_factory().unwrap();
            let object = real_vector(&owner, &[10., 20., 30., 40.]);
            let object_identity = object.allocation().unwrap().clone();
            let class = session
                .register_altrep_class(
                    "tsp.detaching.parameters",
                    DetachedParameters {
                        facade: Rc::downgrade(&facade),
                        calls: calls.clone(),
                        object: object_identity.clone(),
                        action,
                    },
                )
                .unwrap();
            let input = AltrepBuilder::new(class).build().unwrap();
            let tail = factory
                .pairlist_cell(&object, &factory.nil(), &factory.nil())
                .unwrap();
            let source = factory
                .pairlist_cell(&input, &tail, &factory.nil())
                .unwrap();
            crate::sexp::altrep::set_data1(&input, source.clone()).unwrap();
            // Only source edges retain these values on entry. CheckedNode is
            // allocation identity metadata, not an owning root lease.
            (
                owner,
                source.into_owned().unwrap(),
                object.as_raw(),
                input.as_raw(),
                object_identity,
                input.allocation().unwrap().clone(),
            )
        })
    };
    let pin = owner.pin().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(pin.as_ptr(), || {
            if publication {
                crate::mainutils::memory_main::R_gc_torture(1, 1, 0);
                super::setAttrib(object_pointer, super::R_TspSymbol(), input_pointer);
                crate::mainutils::memory_main::R_gc_torture(0, 0, 0);
                crate::sexp::owner::OwnerToken::current()
                    .unwrap()
                    .sexp(object_pointer)
                    .unwrap()
                    .into_owned()
                    .unwrap()
            } else {
                super::normalize_tsp(object_pointer, input_pointer)
            }
        })
    }));
    if action != 2 {
        assert!(source.try_car().unwrap().is_nil());
        assert!(source.try_cdr().unwrap().is_nil());
    }
    assert_eq!(unsafe { (*pin.as_ptr()).memory_state.in_gc }, 0);
    match action {
        0 => {
            let result = result.unwrap();
            assert_eq!(calls.get(), 3);
            let parameters = if publication {
                let attributes = result.try_attrib().unwrap();
                assert!(attributes.try_tag_name_eq(b"tsp").unwrap());
                assert!(attributes.try_cdr().unwrap().is_nil());
                attributes.try_car().unwrap().into_owned().unwrap()
            } else {
                result.clone()
            };
            assert_eq!(parameters.typeof_(), SEXPTYPE::REALSXP);
            for (index, number) in [1., 4., 1.].into_iter().enumerate() {
                assert_eq!(parameters.try_real_elt(index as i64).unwrap(), number);
            }
            drop(parameters);
            // Normalization alone does not retain the target. Publication
            // returns the target with its new parameters, retaining that graph.
            crate::sexp::owner::with_runtime(&owner, |access| {
                access.with_native(|token| token.full_gc())
            })
            .unwrap()
            .unwrap();
            assert_eq!(object_identity.is_live(), publication);
            assert!(!input_identity.is_live());
            if publication {
                assert_eq!(
                    result
                        .try_attrib()
                        .unwrap()
                        .try_car()
                        .unwrap()
                        .try_real_elt(1)
                        .unwrap(),
                    4.
                );
                drop(result);
                crate::sexp::owner::with_runtime(&owner, |access| {
                    access.with_native(|token| token.full_gc())
                })
                .unwrap()
                .unwrap();
                assert!(!object_identity.is_live());
            } else {
                assert_eq!(result.try_real_elt(1).unwrap(), 4.);
            }
        }
        1 => {
            assert_eq!(calls.get(), 1);
            assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 863);
            assert!(owner.pin().is_ok());
            unsafe {
                crate::sexp::session::with_instance_active(pin.as_ptr(), || {
                    let object = real_vector(&owner, &[10., 20., 30., 40.]);
                    let input = real_vector(&owner, &[1., 4., 1.]);
                    let result = super::normalize_tsp(object.as_raw(), input.as_raw());
                    assert_eq!(result.try_real_elt(1).unwrap(), 4.);
                });
            }
        }
        _ => {
            assert_eq!(calls.get(), 1);
            let payload = result.unwrap_err();
            let error = payload.downcast::<crate::sexp::context::RError>().unwrap();
            assert_eq!(error.message, SexpError::RootUnavailable.to_string());
            assert!(owner.pin().is_err());
        }
    }
}

#[test]
fn owned_tsp_target_and_parameters_survive_source_detachment_and_full_gc() {
    detaching_parameters(0, false);
}

#[test]
fn owned_tsp_live_provider_panic_preserves_payload_and_recovers() {
    detaching_parameters(1, false);
}

#[test]
fn owned_tsp_revoked_provider_panic_cannot_publish_success() {
    detaching_parameters(2, false);
}

#[test]
fn owned_tsp_publication_survives_detachment_and_collecting_allocations() {
    detaching_parameters(0, true);
}

#[test]
fn owned_tsp_publication_appends_to_chain_replaced_during_cell_allocation() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let token = session.owner_token().unwrap();
        let owner = token.weak_owner().unwrap();
        let factory = token.node_factory();
        let object = real_vector(&owner, &[10., 20., 30., 40.]);
        let input = real_vector(&owner, &[1., 4., 1.]);
        let old_tag = unsafe { token.sexp(super::Rf_install(c"old".as_ptr())) }
            .unwrap()
            .into_owned()
            .unwrap();
        let current_tag = unsafe { token.sexp(super::Rf_install(c"current".as_ptr())) }
            .unwrap()
            .into_owned()
            .unwrap();
        // Warm the epsilon symbol before arming allocation notifications.
        unsafe { super::Rf_install(c"ts.eps".as_ptr()) };
        let old = factory
            .pairlist_cell(&factory.nil(), &factory.nil(), &old_tag)
            .unwrap()
            .into_owned()
            .unwrap();
        let current = factory
            .pairlist_cell(&factory.nil(), &factory.nil(), &current_tag)
            .unwrap()
            .into_owned()
            .unwrap();
        crate::sexp::object::SexpMut::try_from_checked(object.clone())
            .unwrap()
            .try_set_attribute(&old)
            .unwrap();
        let identity = object.allocation().unwrap().clone();
        let tsp_name = unsafe { token.sexp(super::R_TspSymbol()) }
            .unwrap()
            .into_owned()
            .unwrap();
        let tsp_link = tsp_name.link_in(&identity.heap_identity()).unwrap();
        let domain = crate::sexp::owner::with_runtime(&owner, |access| access.domain()).unwrap();
        let replaced = Rc::new(Cell::new(false));
        let callback_replaced = replaced.clone();
        let callback_current = current.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if callback_replaced.get() {
                return;
            }
            // Constructors notify only after the new cell is initialized.
            // Finish this arena inspection before changing edges or collecting.
            let tsp_cell_exists =
                crate::sexp::instance::with_required_current_instance(|runtime| unsafe {
                    (*runtime).arena.active_nodes().any(|pointer| {
                        let Some(node) = (*runtime).arena.node_token(pointer) else {
                            return false;
                        };
                        let Some(header) = node.heap_identity().node_snapshot(&node) else {
                            return false;
                        };
                        header.sxpinfo.type_of() == SEXPTYPE::LISTSXP
                            && matches!(header.data, NodeBody::List(body) if body.tagval == tsp_link)
                    })
                });
            if !tsp_cell_exists {
                return;
            }
            callback_replaced.set(true);
            let pointer = identity
                .heap_identity()
                .projection_of_link(identity.link().unwrap())
                .unwrap();
            let target = domain.wrap(pointer).unwrap();
            crate::sexp::object::SexpMut::try_from_checked(target)
                .unwrap()
                .try_set_attribute(&callback_current)
                .unwrap();
            crate::sexp::gengc::full_gc();
        }));
        unsafe {
            crate::mainutils::memory_main::R_gc_torture(1, 1, 0);
            super::setAttrib(object.as_raw(), super::R_TspSymbol(), input.as_raw());
            crate::mainutils::memory_main::R_gc_torture(0, 0, 0);
        }
        assert!(
            replaced.get(),
            "replacement happened during initialized cell allocation"
        );
        assert_eq!(object.try_attrib().unwrap(), current);
        assert!(
            old.try_cdr().unwrap().is_nil(),
            "detached tail must remain unchanged"
        );
        let added = current.try_cdr().unwrap();
        assert!(added.try_tag_name_eq(b"tsp").unwrap());
        assert!(added.try_cdr().unwrap().is_nil());
        assert_eq!(added.try_car().unwrap().try_real_elt(1).unwrap(), 4.);
    });
}

struct CountedParameters(Rc<Cell<usize>>);

impl AltrepClass for CountedParameters {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }

    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(3)
    }

    fn element<'s>(&self, _: &AltrepContext<'s>, index: i64) -> SexpResult<AltrepElement<'s>> {
        self.0.set(self.0.get() + 1);
        Ok(AltrepElement::Real([1., 4., 1.][index as usize]))
    }
}

#[test]
fn owned_tsp_foreign_parameters_rejected_before_provider_execution() {
    let original = RSession::new_for_gc_tests();
    let calls = Rc::new(Cell::new(0));
    let input = original.with_active(|| {
        let class = original
            .register_altrep_class("tsp.foreign.parameters", CountedParameters(calls.clone()))
            .unwrap();
        AltrepBuilder::new(class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap().weak_owner().unwrap();
        let object = real_vector(&owner, &[10., 20., 30., 40.]);
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::normalize_tsp(object.as_raw(), input.as_raw())
        }))
        .unwrap_err();
        let error = payload.downcast::<crate::sexp::context::RError>().unwrap();
        assert_eq!(
            error.message,
            SexpError::UnownedPointer {
                address: input.as_raw() as usize,
            }
            .to_string()
        );
        assert_eq!(calls.get(), 0);
    });
}
