//! Native lookup metadata must survive control-provider mutation and reentry.
use super::{NativeOperands, invoke_native_handler};
use crate::{
    mainutils::native_routines::NativeInterface,
    sexp::{
        RSession,
        altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
        ffi::{R_xlen_t, SEXP, SEXPTYPE},
        heap::CheckedNode,
        object::{SessionNodeFactory, Sexp, SexpMut, SexpResult},
    },
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

fn arguments(
    factory: &SessionNodeFactory<'_>,
    name: &Sexp<'static>,
    values: &[(Sexp<'static>, Sexp<'static>)],
) -> Sexp<'static> {
    let nil = factory.nil().into_owned().unwrap();
    let mut list = nil.clone();
    for (value, tag) in values.iter().rev() {
        list = factory
            .pairlist_cell(value, &list, tag)
            .unwrap()
            .into_owned()
            .unwrap();
    }
    factory
        .pairlist_cell(name, &list, &nil)
        .unwrap()
        .into_owned()
        .unwrap()
}

fn operator(session: &RSession, interface: NativeInterface) -> Sexp<'static> {
    let name = match interface {
        NativeInterface::Call => ".Call",
        NativeInterface::External => ".External",
        NativeInterface::External2 => ".External2",
    };
    unsafe {
        let raw = crate::eval::primitive::make_primitive_binding(name, SEXPTYPE::BUILTINSXP);
        session
            .owner_token()
            .unwrap()
            .sexp(raw)
            .unwrap()
            .into_owned()
            .unwrap()
    }
}

struct DetachingPackage {
    package: &'static str,
    container: Rc<Cell<SEXP>>,
    child: Rc<RefCell<Option<CheckedNode>>>,
    calls: Rc<Cell<usize>>,
    nested_calls: Rc<Cell<usize>>,
    session: Weak<RefCell<Option<RSession>>>,
    close: bool,
}
impl AltrepClass for DetachingPackage {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::STRSXP
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
        let text = context.string(self.package)?;
        unsafe {
            crate::sexp::accessors::SET_VECTOR_ELT(
                self.container.get(),
                0,
                crate::sexp::globals::R_NilValue(),
            );
        }
        context.gc()?;
        assert!(
            self.child.borrow().as_ref().unwrap().is_live(),
            "original lookup child must be retained by the operation, not the detached container"
        );
        // Genuine nested .Call reentry, with no runtime facade or RefCell loan.
        let factory = unsafe { crate::sexp::owner::OwnerToken::current()? }.node_factory();
        let nil = factory.nil().into_owned()?;
        let name = factory.strings(&["C_R_identC"])?.into_owned()?;
        let value = factory.strings(&["nested"])?.into_owned()?;
        let list = arguments(
            &factory,
            &name,
            &[(value.clone(), nil.clone()), (value, nil.clone())],
        );
        let result = unsafe {
            invoke_native_handler(
                nil.as_raw(),
                nil.as_raw(),
                list.as_raw(),
                nil.as_raw(),
                NativeInterface::Call,
            )
        }?;
        assert_eq!(result.try_logical_elt(0)?, 1);
        self.nested_calls.set(self.nested_calls.get() + 1);
        if self.close {
            self.session
                .upgrade()
                .unwrap()
                .borrow_mut()
                .as_mut()
                .unwrap()
                .close();
        }
        Ok(AltrepElement::String(text))
    }
}

fn exercise(interface: NativeInterface, close: bool) {
    let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let calls = Rc::new(Cell::new(0));
    let nested_calls = Rc::new(Cell::new(0));
    let collections = Rc::new(Cell::new(0));
    let observed = collections.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| observed.set(observed.get() + 1)));
    let container = Rc::new(Cell::new(std::ptr::null_mut()));
    let child = Rc::new(RefCell::new(None));
    let (list, op, nil) = {
        let mut borrowed = sessions.borrow_mut();
        let session = borrowed.as_mut().unwrap();
        let function = if interface == NativeInterface::External2 {
            Some(
                session
                    .eval_code_with_output_capture("function(x) (x - 2)^2")
                    .0
                    .unwrap()
                    .into_owned()
                    .unwrap(),
            )
        } else {
            None
        };
        let factory = session.owner_token().unwrap().node_factory();
        let nil = factory.nil().into_owned().unwrap();
        let (routine, package) = match interface {
            NativeInterface::Call => ("C_R_identC", "methods"),
            NativeInterface::External => ("C_devcur", "grDevices"),
            NativeInterface::External2 => ("C_do_fmin", "stats"),
        };
        let class = session
            .register_altrep_class(
                "native_detaching_package",
                DetachingPackage {
                    package,
                    container: container.clone(),
                    child: child.clone(),
                    calls: calls.clone(),
                    nested_calls: nested_calls.clone(),
                    session: Rc::downgrade(&sessions),
                    close,
                },
            )
            .unwrap()
            .into_owned()
            .unwrap();
        let package = AltrepBuilder::new(class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let original = factory.strings(&[routine]).unwrap().into_owned().unwrap();
        *child.borrow_mut() = Some(original.allocation().unwrap().clone()); // identity only, no root
        let outer = factory
            .allocate(|arena| {
                arena
                    .alloc_vector_sexp(SEXPTYPE::VECSXP, 1)
                    .map(|x| x.as_raw())
            })
            .unwrap()
            .into_owned()
            .unwrap();
        let mut outer = SexpMut::try_from_checked(outer).unwrap();
        outer.try_set_vector_elt(0, original).unwrap(); // consumes fixture's only child lease
        let outer = outer.freeze();
        container.set(outer.as_raw());
        let tag = unsafe {
            session
                .owner_token()
                .unwrap()
                .sexp(crate::sexp::symbol::Rf_install(c"PACKAGE".as_ptr()))
                .unwrap()
                .into_owned()
                .unwrap()
        };
        let mut values = match interface {
            NativeInterface::Call => {
                let value = factory.strings(&["same"]).unwrap().into_owned().unwrap();
                vec![(value.clone(), nil.clone()), (value, nil.clone())]
            }
            NativeInterface::External => vec![],
            NativeInterface::External2 => {
                let real = |x| unsafe {
                    session
                        .owner_token()
                        .unwrap()
                        .sexp(crate::sexp::constructors::Rf_ScalarReal(x))
                        .unwrap()
                        .into_owned()
                        .unwrap()
                };
                vec![
                    (function.unwrap(), nil.clone()),
                    (real(0.0), nil.clone()),
                    (real(4.0), nil.clone()),
                    (real(0.01), nil.clone()),
                ]
            }
        };
        values.insert(values.len() / 2, (package, tag));
        (
            arguments(&factory, &outer, &values),
            operator(session, interface),
            nil,
        )
    }; // release every source child/root and runtime facade loan before invocation
    let result = unsafe {
        invoke_native_handler(
            nil.as_raw(),
            op.as_raw(),
            list.as_raw(),
            nil.as_raw(),
            interface,
        )
    };
    assert_eq!(calls.get(), 1);
    assert_eq!(nested_calls.get(), 1);
    assert!(collections.get() > 0, "provider must execute real full GC");
    if close {
        assert!(
            result.is_err(),
            "closed original runtime must deny publication"
        );
    } else {
        let result = result.unwrap();
        match interface {
            NativeInterface::Call => assert_eq!(result.try_logical_elt(0).unwrap(), 1),
            NativeInterface::External => assert_eq!(result.try_integer_elt(0).unwrap(), 1),
            NativeInterface::External2 => {
                assert!((result.try_real_elt(0).unwrap() - 2.0).abs() < 0.01)
            }
        }
        assert!(
            list.try_car().unwrap().try_vector_elt(0).unwrap().is_nil(),
            "original structured name metadata must remain distinct from captured lookup identity"
        );
    }
}

#[test]
fn owned_native_call_lookup_child_survives_package_detach_gc_and_reentry() {
    exercise(NativeInterface::Call, false);
}
#[test]
fn owned_native_external_lookup_child_survives_package_detach_gc_and_reentry() {
    exercise(NativeInterface::External, false);
}
#[test]
fn owned_native_external2_lookup_child_survives_package_detach_gc_and_reentry() {
    exercise(NativeInterface::External2, false);
}
#[test]
fn owned_native_lookup_child_retained_until_package_revocation_rejects_publication() {
    exercise(NativeInterface::Call, true);
}

#[test]
fn empty_structured_native_name_has_no_fallback_identity() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let name = factory
        .allocate(|arena| {
            arena
                .alloc_vector_sexp(SEXPTYPE::VECSXP, 0)
                .map(|x| x.as_raw())
        })
        .unwrap()
        .into_owned()
        .unwrap();
    let list = arguments(&factory, &name, &[]);
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let absent = crate::sexp::owner::with_runtime(&owner, |access| {
        NativeOperands::capture(list, NativeInterface::Call, access)?.lookup_name()
    })
    .unwrap()
    .unwrap();
    assert!(absent.is_none());
}

struct DetachingListName {
    arguments: Rc<Cell<SEXP>>,
    payload: CheckedNode,
    calls: Rc<Cell<usize>>,
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
        unsafe {
            crate::sexp::accessors::SETCDR(
                self.arguments.get(),
                crate::sexp::globals::R_NilValue(),
            );
        }
        context.gc()?;
        assert!(
            self.payload.is_live(),
            "all original payloads must be owned before the list-name element provider can detach them"
        );
        Ok(AltrepElement::List(name))
    }
}

#[test]
fn owned_native_list_name_provider_retains_original_payload_before_detach_and_gc() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let nil = factory.nil().into_owned().unwrap();
    let calls = Rc::new(Cell::new(0));
    let original_arguments = Rc::new(Cell::new(std::ptr::null_mut()));
    let list = {
        let value = factory.strings(&["same"]).unwrap().into_owned().unwrap();
        let class = session
            .register_altrep_class(
                "native_detaching_list_name",
                DetachingListName {
                    arguments: original_arguments.clone(),
                    payload: value.allocation().unwrap().clone(),
                    calls: calls.clone(),
                },
            )
            .unwrap()
            .into_owned()
            .unwrap();
        let name = factory.strings(&["C_R_identC"]).unwrap();
        let name = AltrepBuilder::new(class)
            .data1(name)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        arguments(
            &factory,
            &name,
            &[(value.clone(), nil.clone()), (value, nil.clone())],
        )
    }; // the two source cells are the only roots for the handler payload
    original_arguments.set(list.as_raw());
    let op = operator(&session, NativeInterface::Call);
    let result = unsafe {
        invoke_native_handler(
            nil.as_raw(),
            op.as_raw(),
            list.as_raw(),
            nil.as_raw(),
            NativeInterface::Call,
        )
    }
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(result.try_logical_elt(0).unwrap(), 1);
    assert!(list.try_cdr().unwrap().is_nil());
}
