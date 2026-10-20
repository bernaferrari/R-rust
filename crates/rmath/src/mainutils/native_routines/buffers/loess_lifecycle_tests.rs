//! Actual LOESS .C model admission across collection, revocation and foreign owners.
use super::*;
fn cases() -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(include_str!(
        "../../../library/stats/loess/fixtures/pruned-surfaces.json"
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
fn buffers(case: &serde_json::Value) -> Vec<NativeBuffer> {
    let integer = |name: &str| {
        NativeBuffer::Integer(
            case[name]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect(),
        )
    };
    let real = |name: &str| {
        NativeBuffer::Real(
            case[name]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect(),
        )
    };
    vec![
        integer("parameter"),
        integer("a"),
        real("xi"),
        real("vert"),
        real("vval"),
        NativeBuffer::Integer(vec![3]),
        real("queries"),
        NativeBuffer::Real(vec![99.; 5]),
    ]
}

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
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Collect,
    CloseDuringLookup,
    WrongPackage,
    ForeignLive,
    ForeignClosed,
}
fn run(mode: Mode) {
    let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let arguments = Rc::new(Cell::new(std::ptr::null_mut()));
    let calls = Rc::new(Cell::new(0));
    let notified = Rc::new(Cell::new(0));
    let observed = notified.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| observed.set(observed.get() + 1)));
    let case = cases().remove(0);
    let (args, op, nil, owner) = {
        let borrow = sessions.borrow();
        let session = borrow.as_ref().unwrap();
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let factory = session.owner_token().unwrap().node_factory();
        let nil = factory.nil().into_owned().unwrap();
        let values: Vec<_> = buffers(&case)
            .into_iter()
            .map(|b| value(&factory, b))
            .collect();
        let identities = values
            .iter()
            .map(|v| v.allocation().unwrap().clone())
            .collect();
        let class = session
            .register_altrep_class(
                "collecting_loess_model_name",
                Name {
                    arguments: arguments.clone(),
                    identities,
                    calls: calls.clone(),
                    sessions: Rc::downgrade(&sessions),
                    close: mode == Mode::CloseDuringLookup,
                },
            )
            .unwrap()
            .into_owned()
            .unwrap();
        let name = AltrepBuilder::new(class)
            .data1(factory.strings(&["loess_ifit"]).unwrap())
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
            .strings(&[if mode == Mode::WrongPackage {
                "tools"
            } else {
                "stats"
            }])
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
        let op = operator(session);
        (args, op, nil, owner)
    };
    let before = invocation_count();
    if mode == Mode::ForeignLive || mode == Mode::ForeignClosed {
        if mode == Mode::ForeignClosed {
            sessions.borrow_mut().as_mut().unwrap().close();
        }
        let other = RSession::new_for_gc_tests();
        other.with_active(|| {
            let other_op = operator(&other);
            let other_nil = other.owner_token().unwrap().node_factory().nil();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                crate::mainutils::dotcode::do_dotCode(
                    other_nil.as_raw(),
                    other_op.as_raw(),
                    args.as_raw(),
                    other_nil.as_raw(),
                )
            }));
            let failure = result.unwrap_err();
            let error = failure
                .downcast_ref::<crate::sexp::context::RError>()
                .expect("managed domain error");
            assert!(
                error.message.contains("does not belong to its owner"),
                "{}",
                error.message
            );
            assert_eq!(
                calls.get(),
                0,
                "foreign model must fail before provider lookup"
            );
            assert_eq!(invocation_count(), before);
        });
        return;
    }
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
    if mode != Mode::Collect {
        let failure = result.unwrap_err();
        let error = failure
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("managed admission error");
        let expected = if mode == Mode::CloseDuringLookup {
            "owner could not retain its root"
        } else {
            "not registered in the requested PACKAGE"
        };
        assert!(error.message.contains(expected), "{}", error.message);
        assert_eq!(invocation_count(), before);
        return;
    }
    let result = owner
        .node_factory()
        .unwrap()
        .wrap(result.unwrap())
        .unwrap()
        .into_owned()
        .unwrap();
    assert_eq!(result.len(), 8);
    assert_eq!(invocation_count(), before + 1);
    let predictions = result.try_vector_elt(7).unwrap().into_owned().unwrap();
    drop(args);
    drop(result);
    sessions
        .borrow()
        .as_ref()
        .unwrap()
        .with_active(crate::sexp::gengc::full_gc);
    sessions.borrow_mut().as_mut().unwrap().close();
    // Physical numerical output remains readable after the runtime closes.
    for (i, expected) in case["fit"].as_array().unwrap().iter().enumerate() {
        assert!(
            (predictions.try_real_elt(i as R_xlen_t).unwrap() - expected.as_f64().unwrap()).abs()
                < 2e-10
        );
    }
    for i in 3..5 {
        assert_eq!(predictions.try_real_elt(i).unwrap(), 99.);
    }
}
fn operator(session: &RSession) -> Sexp<'static> {
    unsafe {
        session
            .owner_token()
            .unwrap()
            .sexp(crate::eval::primitive::make_primitive_binding(
                ".C",
                SEXPTYPE::BUILTINSXP,
            ))
            .unwrap()
            .into_owned()
            .unwrap()
    }
}
#[test]
fn loess_managed_collecting_lookup_retains_model_and_detached_results() {
    run(Mode::Collect);
}
#[test]
fn loess_managed_callback_close_denies_kernel() {
    run(Mode::CloseDuringLookup);
}
#[test]
fn loess_managed_wrong_package_denies_kernel_after_collection() {
    run(Mode::WrongPackage);
}
#[test]
fn loess_managed_foreign_live_model_denied_before_lookup_or_kernel() {
    run(Mode::ForeignLive);
}
#[test]
fn loess_managed_foreign_closed_model_denied_before_lookup_or_kernel() {
    run(Mode::ForeignClosed);
}

#[test]
fn loess_registered_robust_workspaces_publish_gnu_permutations_and_preserve_tails() {
    let routine = |name| crate::library::tools::native_calls::lookup_buffer(name).unwrap();
    for (residuals, permutation, weights) in [
        (
            vec![0., 1., 2.],
            vec![1, 2, 3],
            vec![1., 0.945216049382716, 0.7901234567901235],
        ),
        (vec![0.; 4], vec![1, 4, 3, 2], vec![1.; 4]),
        (
            vec![2., 1.],
            vec![2, 1],
            vec![0.9036732205456486, 0.9754610577655847],
        ),
    ] {
        let n = residuals.len();
        let mut input = residuals.clone();
        input.push(77.);
        let mut b = vec![
            NativeBuffer::Real(input.clone()),
            NativeBuffer::Integer(vec![n as i32, 78]),
            NativeBuffer::Real(vec![79.; n + 1]),
            NativeBuffer::Integer(vec![80; n + 1]),
        ];
        routine("lowesw")
            .invoke(BufferInterface::Fortran, &mut b)
            .unwrap();
        assert_eq!(b[0].reals().unwrap(), input);
        assert_eq!(b[1].integers().unwrap(), &[n as i32, 78]);
        let actual = b[2].reals().unwrap();
        for (actual, expected) in actual[..n].iter().zip(weights) {
            assert!((actual - expected).abs() < 1e-14);
        }
        assert_eq!(actual[n], 79.);
        assert_eq!(&b[3].integers().unwrap()[..n], permutation);
        assert_eq!(b[3].integers().unwrap()[n], 80);
    }
    for zero in [false, true] {
        let y = vec![3., 1., 4., 1., 5., 9., 2., 6., 77.];
        let fitted = if zero {
            y.clone()
        } else {
            vec![2., 2., 3., 2., 4., 8., 1., 5., 78.]
        };
        let weights = vec![1., 4., 1., 4., 1., 4., 1., 4., 79.];
        let robust = vec![1., 0.5, 1., 0.5, 1., 0.5, 1., 0.5, 80.];
        let mut b = vec![
            NativeBuffer::Integer(vec![8, 81]),
            NativeBuffer::Real(y.clone()),
            NativeBuffer::Real(fitted.clone()),
            NativeBuffer::Real(weights.clone()),
            NativeBuffer::Real(robust.clone()),
            NativeBuffer::Integer(vec![82; 9]),
            NativeBuffer::Real(vec![83.; 9]),
        ];
        routine("lowesp")
            .invoke(BufferInterface::Fortran, &mut b)
            .unwrap();
        assert_eq!(b[0].integers().unwrap(), &[8, 81]);
        for (index, expected) in [(1, y), (2, fitted), (3, weights), (4, robust)] {
            assert_eq!(b[index].reals().unwrap(), expected);
        }
        let permutation = if zero {
            [6, 1, 7, 8, 2, 5, 3, 4]
        } else {
            [5, 1, 7, 3, 2, 8, 6, 4]
        };
        assert_eq!(&b[5].integers().unwrap()[..8], &permutation);
        assert_eq!(b[5].integers().unwrap()[8], 82);
        let actual = b[6].reals().unwrap();
        assert_eq!(actual[8], 83.);
        if zero {
            assert!(actual[..8].iter().all(|x| x.is_nan()));
        } else {
            let expected = [
                3.3598188707226502,
                1.3200905646386749,
                4.3598188707226502,
                1.3200905646386749,
                5.3598188707226502,
                8.679909435361326,
                2.3598188707226502,
                5.679909435361325,
            ];
            for (actual, expected) in actual[..8].iter().zip(expected) {
                assert!((actual - expected).abs() < 1e-14);
            }
        }
    }
}
