//! Actual .Fortran admission through a collecting/revoking name provider.
use super::*;
use crate::sexp::{
    R_xlen_t, RSession, SEXP, SEXPTYPE, Sexp, SexpMut,
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    heap::CheckedNode,
    object::SessionNodeFactory,
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

struct Name {
    arguments: Rc<Cell<SEXP>>,
    identities: Vec<CheckedNode>,
    calls: Rc<Cell<usize>>,
    sessions: Weak<RefCell<Option<RSession>>>,
    close: bool,
}
impl AltrepClass for Name {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::VECSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> crate::sexp::SexpResult<R_xlen_t> {
        Ok(1)
    }
    fn element<'s>(
        &self,
        c: &AltrepContext<'s>,
        _: R_xlen_t,
    ) -> crate::sexp::SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        let name = c.data1()?;
        unsafe {
            crate::sexp::accessors::SETCDR(
                self.arguments.get(),
                crate::sexp::globals::R_NilValue(),
            );
        }
        c.gc()?;
        assert!(self.identities.iter().all(CheckedNode::is_live));
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
fn value(factory: &SessionNodeFactory<'_>, buffer: NativeBuffer) -> Sexp<'static> {
    let kind = match buffer {
        NativeBuffer::Real(_) => SEXPTYPE::REALSXP,
        _ => SEXPTYPE::INTSXP,
    };
    let node = factory
        .allocate(|arena| {
            arena
                .alloc_vector_sexp(kind, buffer.len() as R_xlen_t)
                .map(|x| x.as_raw())
        })
        .unwrap()
        .into_owned()
        .unwrap();
    let mut node = SexpMut::try_from_checked(node).unwrap();
    match buffer {
        NativeBuffer::Real(data) => {
            for (i, v) in data.into_iter().enumerate() {
                node.try_set_real_elt(i as R_xlen_t, v).unwrap();
            }
        }
        NativeBuffer::Integer(data) => {
            for (i, v) in data.into_iter().enumerate() {
                node.try_set_integer_elt(i as R_xlen_t, v).unwrap();
            }
        }
        _ => unreachable!(),
    }
    node.freeze()
}
fn run(close: bool, wrong_package: bool) {
    let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let arguments = Rc::new(Cell::new(std::ptr::null_mut()));
    let calls = Rc::new(Cell::new(0));
    let notified = Rc::new(Cell::new(0));
    let observed = notified.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| observed.set(observed.get() + 1)));
    let (args, op, nil, owner) = {
        let borrow = sessions.borrow();
        let session = borrow.as_ref().unwrap();
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let factory = session.owner_token().unwrap().node_factory();
        let nil = factory.nil().into_owned().unwrap();
        let mut values: Vec<_> = additive().into_iter().map(|b| value(&factory, b)).collect();
        // True repeated source identity, independent output scratch buffers.
        values[15] = values[14].clone();
        values[16] = values[14].clone();
        let identities = values
            .iter()
            .map(|v| v.allocation().unwrap().clone())
            .collect();
        let class = session
            .register_altrep_class(
                "collecting_stl_name",
                Name {
                    arguments: arguments.clone(),
                    identities,
                    calls: calls.clone(),
                    sessions: Rc::downgrade(&sessions),
                    close,
                },
            )
            .unwrap()
            .into_owned()
            .unwrap();
        let name = AltrepBuilder::new(class)
            .data1(factory.strings(&["C_stl"]).unwrap())
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let mut tail = nil.clone();
        for v in values.iter().rev() {
            tail = factory
                .pairlist_cell(v, &tail, &nil)
                .unwrap()
                .into_owned()
                .unwrap();
        }
        let package = factory
            .strings(&[if wrong_package { "tools" } else { "stats" }])
            .unwrap();
        let tag = unsafe {
            session
                .owner_token()
                .unwrap()
                .sexp(crate::sexp::symbol::Rf_install(c"PACKAGE".as_ptr()))
                .unwrap()
        };
        tail = factory
            .pairlist_cell(&package, &tail, &tag)
            .unwrap()
            .into_owned()
            .unwrap();
        let args = factory
            .pairlist_cell(&name, &tail, &nil)
            .unwrap()
            .into_owned()
            .unwrap();
        arguments.set(args.as_raw());
        let op = unsafe {
            session
                .owner_token()
                .unwrap()
                .sexp(crate::eval::primitive::make_primitive_binding(
                    ".Fortran",
                    SEXPTYPE::BUILTINSXP,
                ))
                .unwrap()
                .into_owned()
                .unwrap()
        };
        (args, op, nil, owner)
    };
    let before = crate::mainutils::native_routines::buffers::invocation_count();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::mainutils::dotcode::do_dotCode(
            nil.as_raw(),
            op.as_raw(),
            args.as_raw(),
            nil.as_raw(),
        )
    }));
    assert_eq!(calls.get(), 1);
    assert!(notified.get() > 0);
    if close || wrong_package {
        assert!(result.is_err());
        assert_eq!(
            crate::mainutils::native_routines::buffers::invocation_count(),
            before
        );
    } else {
        let result = owner
            .node_factory()
            .unwrap()
            .wrap(result.unwrap())
            .unwrap()
            .into_owned()
            .unwrap();
        assert_eq!(result.len(), 17);
        assert_eq!(
            crate::mainutils::native_routines::buffers::invocation_count(),
            before + 1
        );

        let weights = result.try_vector_elt(14).unwrap().into_owned().unwrap();
        let season = result.try_vector_elt(15).unwrap().into_owned().unwrap();
        let trend = result.try_vector_elt(16).unwrap().into_owned().unwrap();
        assert_ne!(weights.as_raw(), season.as_raw());
        assert_ne!(season.as_raw(), trend.as_raw());
        for i in 0..24 {
            assert_eq!(weights.try_real_elt(i).unwrap(), 1.);
        }
        let expected = cases().remove("additive").unwrap();
        for index in [15, 16] {
            let actual = result.try_vector_elt(index as R_xlen_t).unwrap();
            let values = expected[index].1.reals().unwrap();
            for (i, value) in values.iter().take(24).enumerate() {
                assert!((actual.try_real_elt(i as R_xlen_t).unwrap() - value).abs() < 1e-10);
            }
        }
        drop(args);
        drop(result);
        sessions
            .borrow()
            .as_ref()
            .unwrap()
            .with_active(crate::sexp::gengc::full_gc);
        assert_eq!(weights.try_real_elt(0).unwrap(), 1.);
        assert!(trend.try_real_elt(23).unwrap().is_finite());
    }
}
#[test]
fn stl_managed_collecting_name_owns_aliased_payloads_and_results() {
    run(false, false);
}
#[test]
fn stl_managed_original_close_denies_kernel() {
    run(true, false);
}
#[test]
fn stl_managed_wrong_package_denies_kernel_after_collection() {
    run(false, true);
}
