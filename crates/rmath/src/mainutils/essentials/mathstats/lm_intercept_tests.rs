//! Independent original GNU intercept-only model and empty-row contracts.
use crate::sexp::{
    RSession, SEXPTYPE,
    object::{SessionNodeFactory, Sexp, SexpMut},
};

fn real<'s>(factory: &SessionNodeFactory<'s>, values: &[f64]) -> Sexp<'s> {
    let value = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::REALSXP, values.len() as i64)))
        .unwrap();
    let mut value = SexpMut::try_from_checked(value).unwrap();
    for (index, number) in values.iter().copied().enumerate() {
        value.try_set_real_elt(index as i64, number).unwrap();
    }
    value.freeze()
}

fn formula_arguments<'s>(
    factory: &SessionNodeFactory<'s>,
    response: &Sexp<'s>,
) -> (Sexp<'s>, Sexp<'s>) {
    let one = real(factory, &[1.]);
    let tail = factory
        .pairlist_cell(&one, &factory.nil(), &factory.nil())
        .unwrap();
    let arguments = factory
        .pairlist_cell(response, &tail, &factory.nil())
        .unwrap();
    let head = factory
        .wrap(unsafe { crate::sexp::symbol::Rf_install(c"~".as_ptr()) })
        .unwrap();
    let weak = unsafe { crate::sexp::owner::OwnerToken::current() }
        .unwrap()
        .weak_owner()
        .unwrap();
    let formula = crate::sexp::owner::with_runtime(&weak, |access| {
        access
            .allocator(&factory.domain())
            .unwrap()
            .call(&head, &arguments)
            .unwrap()
    })
    .unwrap();
    let args = factory
        .pairlist_cell(&formula, &factory.nil(), &factory.nil())
        .unwrap();
    (args, arguments)
}

fn fit<'s>(factory: &SessionNodeFactory<'s>, response: &Sexp<'s>) -> Sexp<'s> {
    let (args, _) = formula_arguments(factory, response);
    factory
        .wrap(unsafe {
            super::do_lm(
                factory.nil().as_raw(),
                factory.nil().as_raw(),
                args.as_raw(),
                factory.nil().as_raw(),
            )
        })
        .unwrap()
}

#[test]
fn owned_lm_intercept_produces_rank_one_and_infinite_covratio() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let model = fit(&factory, &real(&factory, &[1., 2.]));
        assert_eq!(
            model.typeof_(),
            SEXPTYPE::VECSXP,
            "GNU returns a rank-one model"
        );
        assert_eq!(
            model.try_vector_elt(3).unwrap().try_integer_elt(0).unwrap(),
            1
        );
        assert_eq!(
            model.try_vector_elt(4).unwrap().try_integer_elt(0).unwrap(),
            1
        );
        let args = factory
            .pairlist_cell(&model, &factory.nil(), &factory.nil())
            .unwrap();
        let result = factory
            .wrap(unsafe {
                super::do_covratio(
                    factory.nil().as_raw(),
                    factory.nil().as_raw(),
                    args.as_raw(),
                    factory.nil().as_raw(),
                )
            })
            .unwrap();
        assert_eq!(result.len(), 2);
        for index in 0..2 {
            assert_eq!(result.try_real_elt(index).unwrap(), f64::INFINITY);
        }
    });
}

#[test]
fn owned_lm_intercept_rejects_zero_nonmissing_cases() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            fit(&factory, &real(&factory, &[]))
        }));
        assert_eq!(
            result
                .unwrap_err()
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message,
            "0 (non-NA) cases"
        );
    });
}

fn covariance<'s>(f: &SessionNodeFactory<'s>, model: &Sexp<'s>) -> Sexp<'s> {
    let args = f.pairlist_cell(model, &f.nil(), &f.nil()).unwrap();
    f.wrap(unsafe {
        super::do_covratio(
            f.nil().as_raw(),
            f.nil().as_raw(),
            args.as_raw(),
            f.nil().as_raw(),
        )
    })
    .unwrap()
}
fn numbers(value: &Sexp<'_>, expected: &[f64]) {
    assert_eq!(value.len() as usize, expected.len());
    for (i, expected) in expected.iter().copied().enumerate() {
        let actual = value.try_real_elt(i as i64).unwrap();
        if expected.is_nan() {
            assert!(actual.is_nan());
        } else {
            assert!(
                (actual - expected).abs() <= 1e-12 * expected.abs().max(1.),
                "{i}: {actual} != {expected}"
            );
        }
    }
}
fn attr<'s>(f: &SessionNodeFactory<'s>, value: &Sexp<'s>, name: &str) -> Sexp<'s> {
    let name = std::ffi::CString::new(name).unwrap();
    let symbol = f
        .wrap(unsafe { crate::sexp::symbol::Rf_install(name.as_ptr()) })
        .unwrap();
    f.wrap(unsafe { crate::sexp::attrib_core::getAttrib(value.as_raw(), symbol.as_raw()) })
        .unwrap()
}
fn names(f: &SessionNodeFactory<'_>, value: &Sexp<'_>, expected: &[&str]) {
    let names = attr(f, value, "names");
    assert_eq!(names.len() as usize, expected.len());
    for (i, expected) in expected.iter().enumerate() {
        assert_eq!(
            names.try_string_value_elt(i as i64).unwrap().as_deref(),
            Some(*expected)
        );
    }
}
#[test]
fn owned_lm_intercept_one_constant_and_zero_keep_rank_one() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        for values in [&[7.][0..], &[3., 3., 3., 3.], &[0., 0., 0.]] {
            let model = fit(&f, &real(&f, values));
            assert_eq!(
                model.try_vector_elt(3).unwrap().try_integer_elt(0).unwrap(),
                1
            );
            assert_eq!(
                model.try_vector_elt(4).unwrap().try_integer_elt(0).unwrap(),
                values.len() as i32 - 1
            );
            numbers(&model.try_vector_elt(0).unwrap(), &[values[0]]);
            numbers(&model.try_vector_elt(1).unwrap(), &vec![0.; values.len()]);
            numbers(&covariance(&f, &model), &vec![f64::NAN; values.len()]);
        }
    });
}
#[test]
fn owned_lm_intercept_integer_response_uses_original_qr_arithmetic() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let y = f
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3)))
            .unwrap();
        let mut y = SexpMut::try_from_checked(y).unwrap();
        for (i, v) in [1, 2, 4].into_iter().enumerate() {
            y.try_set_integer_elt(i as i64, v).unwrap();
        }
        let model = fit(&f, &y.freeze());
        numbers(&model.try_vector_elt(0).unwrap(), &[7. / 3.]);
        numbers(
            &covariance(&f, &model),
            &[1.28571428571429, 2.89285714285714, 0.321428571428571],
        );
    });
}
#[test]
fn owned_lm_intercept_missing_rows_preserve_labels_and_omission_action() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let y = real(&f, &[1., crate::sexp::ffi::NA_REAL, 3., f64::NAN, 5.]);
        let labels = f.strings(&["a", "b", "c", "d", "e"]).unwrap();
        unsafe {
            crate::sexp::attrib_core::setAttrib(
                y.as_raw(),
                crate::sexp::attrib_core::R_NamesSymbol(),
                labels.as_raw(),
            );
        }
        let model = fit(&f, &y);
        let residual = model.try_vector_elt(1).unwrap();
        numbers(&residual, &[-2., 0., 2.]);
        names(&f, &residual, &["a", "c", "e"]);
        let action = model.try_vector_elt(11).unwrap();
        assert_eq!(action.len(), 2);
        assert_eq!(action.try_integer_elt(0).unwrap(), 2);
        assert_eq!(action.try_integer_elt(1).unwrap(), 4);
        names(&f, &action, &["b", "d"]);
        assert_eq!(
            attr(&f, &action, "class")
                .try_string_value_elt(0)
                .unwrap()
                .as_deref(),
            Some("omit")
        );
        numbers(&covariance(&f, &model), &[0.75, 3., 0.75]);
    });
}
#[test]
fn owned_lm_intercept_nonfinite_errors_leave_runtime_usable() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        for (values, message) in [
            (
                &[f64::NAN, crate::sexp::ffi::NA_REAL][0..],
                "0 (non-NA) cases",
            ),
            (&[1., f64::INFINITY, 2.], "NA/NaN/Inf in 'y'"),
        ] {
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                fit(&f, &real(&f, values))
            }));
            assert_eq!(
                failure
                    .unwrap_err()
                    .downcast_ref::<crate::sexp::context::RError>()
                    .unwrap()
                    .message,
                message
            );
        }
        numbers(
            &fit(&f, &real(&f, &[1., 2., 3.])).try_vector_elt(0).unwrap(),
            &[2.],
        );
    });
}
fn collecting_intercept(action: u8) {
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let (arguments, lhs, weak, source) = {
        let facade = facade.borrow();
        let session = facade.as_ref().unwrap();
        session.with_active(|| {
            let f = session.owner_token().unwrap().node_factory();
            let response = real(&f, &[1., 2., 4.]);
            let source = response.allocation().unwrap().clone();
            let (args, lhs) = formula_arguments(&f, &response);
            (
                args.into_owned().unwrap(),
                lhs.into_owned().unwrap(),
                session.owner_token().unwrap().weak_owner().unwrap(),
                source,
            )
        })
    };
    let pin = weak.pin().unwrap();
    let instance = pin.as_ptr();
    let observed = Rc::new(Cell::new(0));
    let counted = observed.clone();
    let callback_facade = Rc::downgrade(&facade);
    let original_source = source.clone();
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            let f = weak.node_factory().unwrap();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if counted.replace(1) != 0 {
                    return;
                }
                (*instance).memory_state.gc_force_gap = 0;
                crate::sexp::accessors::SETCAR(lhs.as_raw(), crate::sexp::globals::R_NilValue());
                crate::sexp::gengc::full_gc();
                assert!(
                    source.is_live(),
                    "the captured response is the detached value's root"
                );
                match action {
                    0 => {}
                    1 => std::panic::panic_any(764_u32),
                    _ => drop(callback_facade.upgrade().unwrap().borrow_mut().take()),
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            f.wrap(super::do_lm(
                f.nil().as_raw(),
                f.nil().as_raw(),
                arguments.as_raw(),
                f.nil().as_raw(),
            ))
            .unwrap()
            .into_owned()
            .unwrap()
        })
    }));
    assert_eq!(observed.get(), 1);
    assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
    match action {
        0 => {
            let model = result.unwrap();
            unsafe {
                crate::sexp::session::with_instance_active(instance, || {
                    crate::sexp::gengc::full_gc();
                    assert!(
                        !original_source.is_live(),
                        "the operation releases its detached response root"
                    );
                    numbers(&model.try_vector_elt(0).unwrap(), &[7. / 3.]);
                    let f = weak.node_factory().unwrap();
                    numbers(
                        &covariance(&f, &model),
                        &[1.28571428571429, 2.89285714285714, 0.321428571428571],
                    );
                });
            }
        }
        1 => assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 764),
        _ => assert_eq!(
            result
                .unwrap_err()
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message,
            crate::sexp::object::SexpError::RootUnavailable.to_string()
        ),
    }
}
#[test]
fn owned_lm_intercept_detached_response_and_result_survive_collecting_callback() {
    collecting_intercept(0);
}
#[test]
fn owned_lm_intercept_live_callback_panic_is_preserved() {
    collecting_intercept(1);
}
#[test]
fn owned_lm_intercept_revocation_refuses_publication_and_restores_gc() {
    collecting_intercept(2);
}

#[test]
fn owned_lm_intercept_data_column_and_row_labels_survive_environment_callback() {
    use std::{cell::Cell, rc::Rc};
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let f = owner.node_factory();
        let response = real(&f, &[1., 2., 4.]);
        let rows = f.strings(&["alpha", "beta", "gamma"]).unwrap();
        let data = f
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::VECSXP, 1)))
            .unwrap();
        let mut data = SexpMut::try_from_checked(data).unwrap();
        data.try_set_vector_elt(0, response.clone()).unwrap();
        let data = data.freeze();
        let labels = f.strings(&["value"]).unwrap();
        let row_symbol = f
            .wrap(unsafe { crate::sexp::symbol::Rf_install(c"row.names".as_ptr()) })
            .unwrap();
        unsafe {
            crate::sexp::attrib_core::setAttrib(
                data.as_raw(),
                crate::sexp::attrib_core::R_NamesSymbol(),
                labels.as_raw(),
            );
            crate::sexp::attrib_core::setAttrib(data.as_raw(), row_symbol.as_raw(), rows.as_raw());
        }
        let response_token = response.allocation().unwrap().clone();
        let rows_token = rows.allocation().unwrap().clone();
        drop(response);
        drop(rows);
        drop(labels);
        let expression = f
            .wrap(unsafe { crate::sexp::symbol::Rf_install(c"value".as_ptr()) })
            .unwrap();
        let (args, _) = formula_arguments(&f, &expression);
        let formula = args.try_car().unwrap();
        let data_tag = f
            .wrap(unsafe { crate::sexp::symbol::Rf_install(c"data".as_ptr()) })
            .unwrap();
        let tail = f.pairlist_cell(&data, &f.nil(), &data_tag).unwrap();
        let args = f.pairlist_cell(&formula, &tail, &f.nil()).unwrap();
        let callback_data = data.clone().into_owned().unwrap();
        let old_response = response_token.clone();
        let old_rows = rows_token.clone();
        let weak = owner.weak_owner().unwrap();
        let callback_weak = weak.clone();
        let pin = weak.pin().unwrap();
        let instance = pin.as_ptr();
        let observed = Rc::new(Cell::new(0));
        let counted = observed.clone();
        unsafe {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if counted.replace(1) != 0 {
                    return;
                }
                (*instance).memory_state.gc_force_gap = 0;
                let f = callback_weak.node_factory().unwrap();
                let mut data = SexpMut::try_from_checked(callback_data.clone()).unwrap();
                data.try_set_vector_elt(0, f.nil()).unwrap();
                crate::sexp::accessors::SET_ATTRIB(callback_data.as_raw(), f.nil().as_raw());
                crate::sexp::gengc::full_gc();
                assert!(old_response.is_live());
                assert!(old_rows.is_live());
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        }
        let model = f
            .wrap(unsafe {
                super::do_lm(
                    f.nil().as_raw(),
                    f.nil().as_raw(),
                    args.as_raw(),
                    f.nil().as_raw(),
                )
            })
            .unwrap();
        assert_eq!(observed.get(), 1);
        numbers(&model.try_vector_elt(0).unwrap(), &[7. / 3.]);
        names(
            &f,
            &model.try_vector_elt(1).unwrap(),
            &["alpha", "beta", "gamma"],
        );
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert!(!response_token.is_live());
        assert!(!rows_token.is_live());
        numbers(
            &covariance(&f, &model),
            &[1.28571428571429, 2.89285714285714, 0.321428571428571],
        );
    });
}
