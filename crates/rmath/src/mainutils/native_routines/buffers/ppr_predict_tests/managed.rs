//! Original owning payloads survive a real collecting list-name provider.
use super::*;
use crate::sexp::{
    SEXP,
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    heap::CheckedNode,
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
        // A different runtime performs real work; returning restores the
        // original provider authority rather than changing payload affiliation.
        let nested = RSession::new_for_gc_tests();
        nested.with_active(crate::sexp::gengc::full_gc);
        drop(nested);
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
fn run(close: bool, wrong_package: bool) {
    let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let arguments = Rc::new(Cell::new(std::ptr::null_mut()));
    let calls = Rc::new(Cell::new(0));
    let notifications = Rc::new(Cell::new(0));
    let observed = notifications.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| observed.set(observed.get() + 1)));
    let (args, op, nil, owner) = {
        let borrow = sessions.borrow();
        let session = borrow.as_ref().unwrap();
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let f = session.owner_token().unwrap().node_factory();
        let nil = f.nil().into_owned().unwrap();
        let mut values: Vec<_> = data("shared-model-workspace")
            .into_iter()
            .map(|(b, _)| value(&f, b))
            .collect();
        values[4] = values[2].clone();
        let identities = values
            .iter()
            .map(|v| v.allocation().unwrap().clone())
            .collect();
        let class = session
            .register_altrep_class(
                "collecting-packed-PPR",
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
            .data1(f.strings(&["C_pppred"]).unwrap())
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let package = f
            .strings(&[if wrong_package { "tools" } else { "stats" }])
            .unwrap();
        let tag = f
            .wrap(unsafe { crate::sexp::symbol::Rf_install(c"PACKAGE".as_ptr()) })
            .unwrap();
        let mut tail = f
            .pairlist_cell(&package, &nil, &tag)
            .unwrap()
            .into_owned()
            .unwrap();
        for v in values.iter().rev() {
            tail = f
                .pairlist_cell(v, &tail, &nil)
                .unwrap()
                .into_owned()
                .unwrap();
        }
        let args = f
            .pairlist_cell(&name, &tail, &nil)
            .unwrap()
            .into_owned()
            .unwrap();
        arguments.set(args.as_raw());
        let op = f
            .wrap(unsafe {
                crate::eval::primitive::make_primitive_binding(".Fortran", SEXPTYPE::BUILTINSXP)
            })
            .unwrap()
            .into_owned()
            .unwrap();
        (args, op, nil, owner)
    };
    let before = invocation_count();
    let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::mainutils::dotcode::do_dotCode(
            nil.as_raw(),
            op.as_raw(),
            args.as_raw(),
            nil.as_raw(),
        )
    }));
    assert_eq!(calls.get(), 1);
    assert!(notifications.get() > 0, "actual original GC notification");
    if close || wrong_package {
        assert!(output.is_err());
        assert_eq!(invocation_count(), before);
        return;
    }
    assert_eq!(invocation_count(), before + 1);
    let f = owner.node_factory().unwrap();
    let output = f.wrap(output.unwrap()).unwrap().into_owned().unwrap();
    assert_eq!(output.len(), 5);
    let model = output.try_vector_elt(2).unwrap().into_owned().unwrap();
    let scratch = output.try_vector_elt(4).unwrap().into_owned().unwrap();
    assert_ne!(
        model.as_raw(),
        scratch.as_raw(),
        "shared source gets independent writable buffers"
    );
    for (i, (_, expected)) in data("shared-model-workspace").into_iter().enumerate() {
        let actual = output.try_vector_elt(i as R_xlen_t).unwrap();
        assert_eq!(actual.len(), expected.len() as R_xlen_t);
        match expected {
            NativeBuffer::Integer(v) => {
                for (j, e) in v.into_iter().enumerate() {
                    assert_eq!(actual.try_integer_elt(j as R_xlen_t).unwrap(), e);
                }
            }
            NativeBuffer::Real(v) => {
                for (j, e) in v.into_iter().enumerate() {
                    let a = actual.try_real_elt(j as R_xlen_t).unwrap();
                    assert!(
                        (a - e).abs() < 1e-10,
                        "managed PPR buffer{i}:{j}: {a} vs GNU{e}"
                    );
                }
            }
            NativeBuffer::Character(_) => unreachable!(),
        }
    }
    drop(args);
    drop(output);
    sessions
        .borrow()
        .as_ref()
        .unwrap()
        .with_active(crate::sexp::gengc::full_gc);
    assert!(model.try_real_elt(model.len() - 1).unwrap().is_finite());
    assert!(scratch.try_real_elt(scratch.len() - 1).unwrap().is_finite());
}
#[test]
fn ppr_prediction_detached_alias_payloads_survive_gc_and_runtime_reentry() {
    run(false, false);
}
#[test]
fn ppr_prediction_original_close_denies_kernel_invocation() {
    run(true, false);
}
#[test]
fn ppr_prediction_wrong_package_denies_kernel_invocation() {
    run(false, true);
}
