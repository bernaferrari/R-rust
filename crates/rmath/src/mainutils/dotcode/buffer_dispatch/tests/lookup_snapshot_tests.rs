//! Real list-name providers can mutate the source graph before admission.
use super::*;
use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    heap::CheckedNode,
};
use std::rc::Weak;

struct DetachingListName {
    arguments: Rc<Cell<SEXP>>,
    attribute_source: SEXP,
    payloads: Vec<CheckedNode>,
    attribute: CheckedNode,
    calls: Rc<Cell<usize>>,
    sessions: Weak<RefCell<Option<RSession>>>,
    close: bool,
}

impl AltrepClass for DetachingListName {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::VECSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<R_xlen_t> {
        Ok(1)
    }
    fn element<'s>(
        &self,
        context: &AltrepContext<'s>,
        _: R_xlen_t,
    ) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        let name = context.data1()?;
        // The original source edges are live at callback entry. Remove the
        // attribute too, so retaining just each payload vector is insufficient.
        unsafe {
            crate::sexp::accessors::SET_ATTRIB(
                self.attribute_source,
                crate::sexp::globals::R_NilValue(),
            );
            crate::sexp::accessors::SETCDR(
                self.arguments.get(),
                crate::sexp::globals::R_NilValue(),
            );
        }
        context.gc()?;
        assert!(
            self.payloads.iter().all(CheckedNode::is_live),
            "every original payload must be owned before the list-name provider"
        );
        assert!(
            self.attribute.is_live(),
            "detached original attributes need their own admission snapshot"
        );
        if self.close {
            self.sessions
                .upgrade()
                .unwrap()
                .borrow_mut()
                .as_mut()
                .unwrap()
                .close();
        }
        Ok(AltrepElement::List(name))
    }
}

fn exercise(interface: BufferInterface, close: bool) {
    let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let source = Rc::new(Cell::new(std::ptr::null_mut()));
    let calls = Rc::new(Cell::new(0));
    let notifications = Rc::new(Cell::new(0));
    let observed = notifications.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| observed.set(observed.get() + 1)));
    let (args, op, nil, output_index, attribute_identity) = {
        let borrow = sessions.borrow();
        let session = borrow.as_ref().unwrap();
        let factory = session.owner_token().unwrap().node_factory();
        let nil = factory.nil().into_owned().unwrap();
        let (routine, values, output_index) = match interface {
            BufferInterface::C => ("C_kmeans_Lloyd", kmeans_arguments(&factory), 3),
            BufferInterface::Fortran => (
                "C_bvalus",
                vec![
                    integer(&factory, &[3]),
                    real(&factory, &[0., 0., 0., 0., 1., 1., 1., 1.]),
                    real(&factory, &[0., 1., 2., 3.]),
                    integer(&factory, &[4]),
                    real(&factory, &[0., 0.5, 1.]),
                    real(&factory, &[0.; 3]),
                    integer(&factory, &[0]),
                ],
                5,
            ),
        };
        let marker = factory
            .pairlist_cell(&integer(&factory, &[777]), &nil, &symbol(session, "marker"))
            .unwrap()
            .into_owned()
            .unwrap();
        let attribute_identity = marker.allocation().unwrap().clone(); // identity only
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        with_runtime(&owner, |access| {
            install_attributes(&values[output_index], &marker, &access.domain(), access)
        })
        .unwrap()
        .unwrap();
        let class = session
            .register_altrep_class(
                "buffer_detaching_list_name",
                DetachingListName {
                    arguments: source.clone(),
                    attribute_source: values[output_index].as_raw(),
                    payloads: values
                        .iter()
                        .map(|value| value.allocation().unwrap().clone())
                        .collect(),
                    attribute: attribute_identity.clone(),
                    calls: calls.clone(),
                    sessions: Rc::downgrade(&sessions),
                    close,
                },
            )
            .unwrap()
            .into_owned()
            .unwrap();
        let original = factory.strings(&[routine]).unwrap();
        let name = AltrepBuilder::new(class)
            .data1(original)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let mut payload: Vec<_> = values
            .into_iter()
            .map(|value| (value, nil.clone()))
            .collect();
        payload[output_index].1 = symbol(session, "answer");
        payload.insert(
            2,
            (
                factory.strings(&["stats"]).unwrap().into_owned().unwrap(),
                symbol(session, "PACKAGE"),
            ),
        );
        let args = request(&factory, &name, &payload);
        source.set(args.as_raw());
        let primitive = if interface == BufferInterface::C {
            ".C"
        } else {
            ".Fortran"
        };
        let op = unsafe {
            session
                .owner_token()
                .unwrap()
                .sexp(crate::eval::primitive::make_primitive_binding(
                    primitive,
                    SEXPTYPE::BUILTINSXP,
                ))
                .unwrap()
                .into_owned()
                .unwrap()
        };
        (args, op, nil, output_index, attribute_identity)
    }; // no source payload, attribute, or session facade loan survives this scope
    let before = buffers::invocation_count();
    let result = unsafe {
        invoke(
            nil.as_raw(),
            op.as_raw(),
            args.as_raw(),
            nil.as_raw(),
            interface,
        )
    };
    assert_eq!(calls.get(), 1);
    assert!(notifications.get() > 0, "actual full GC must notify");
    if close {
        assert!(
            !sessions.borrow().as_ref().unwrap().is_active(),
            "the actual original facade must have closed inside the provider"
        );
        assert!(
            result.is_err(),
            "closed original authority must deny invocation and publication"
        );
        assert_eq!(buffers::invocation_count(), before);
    } else {
        assert!(args.try_cdr().unwrap().is_nil());
        let result = result.unwrap();
        assert_eq!(buffers::invocation_count(), before + 1);
        let output = result.try_vector_elt(output_index as R_xlen_t).unwrap();
        let expected: &[f64] = if interface == BufferInterface::C {
            &[1.5, 8.5]
        } else {
            &[0., 1.5, 3.]
        };
        for (index, expected) in expected.iter().enumerate() {
            assert_eq!(output.try_real_elt(index as R_xlen_t).unwrap(), *expected);
        }
        assert_eq!(
            output.try_attrib().unwrap().allocation().unwrap(),
            &attribute_identity
        );
        let names = result.try_attrib().unwrap().try_car().unwrap();
        assert_eq!(
            names.string_value_elt(output_index as R_xlen_t),
            Some(Some("answer".into()))
        );
    }
}

#[test]
fn c_list_name_provider_owns_original_payloads_and_attributes_before_collection() {
    exercise(BufferInterface::C, false);
}
#[test]
fn fortran_list_name_provider_owns_original_payloads_and_attributes_before_collection() {
    exercise(BufferInterface::Fortran, false);
}
#[test]
fn c_list_name_provider_close_denies_kernel_and_publication() {
    exercise(BufferInterface::C, true);
}
#[test]
fn fortran_list_name_provider_close_denies_kernel_and_publication() {
    exercise(BufferInterface::Fortran, true);
}
