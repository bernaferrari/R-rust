//! Actual pinned GNU stats::covratio GLM/weighted contracts.
use super::{assert_values, call, list, real};
use crate::sexp::{
    RSession, SEXPTYPE,
    object::{SessionNodeFactory, Sexp, SexpMut},
};
fn dim<'s>(f: &SessionNodeFactory<'s>, value: &Sexp<'s>, rows: i32, cols: i32) {
    let d = f
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 2)))
        .unwrap();
    let mut d = SexpMut::try_from_checked(d).unwrap();
    d.try_set_integer_elt(0, rows).unwrap();
    d.try_set_integer_elt(1, cols).unwrap();
    let d = d.freeze();
    unsafe {
        crate::sexp::attrib_core::setAttrib(
            value.as_raw(),
            crate::sexp::attrib_core::R_DimSymbol(),
            d.as_raw(),
        );
    }
}
fn names<'s>(f: &SessionNodeFactory<'s>, value: &Sexp<'s>, labels: &[&str]) {
    let labels = f.strings(labels).unwrap();
    unsafe {
        crate::sexp::attrib_core::setAttrib(
            value.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
            labels.as_raw(),
        );
    }
}
#[test]
fn covratio_glm_fixed_and_estimated_dispersion_match_original_body() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let matrix = real(
            &f,
            &[
                -1.2720270310221018,
                0.3417347616112924,
                0.37320183571524407,
                0.39077945177726198,
                0.39077945177726187,
                0.37320183571524412,
                0.34173476161129224,
                0.30204165705065861,
                -5.7241216395994563,
                2.6676229927508177,
                0.12244989177265299,
                -0.058121715188318455,
                -0.24446064958027702,
                -0.41142182587287407,
                -0.53968468274156434,
                -0.62102462176449502,
            ],
        );
        dim(&f, &matrix, 8, 2);
        let qr = list(&f, &["qr"], &[matrix]);
        for (family, dispersion, expected) in [
            (
                "binomial",
                f64::NAN,
                [
                    1.0710743801652896,
                    0.4499999999999999,
                    0.1777777777777777,
                    0.071999999999999981,
                    0.037230680838839424,
                    0.017999999999999995,
                    0.011319765918420824,
                    0.006228373702422141,
                ],
            ),
            (
                "gaussian",
                f64::NAN,
                [
                    0.75732292318213756,
                    0.27412254610350989,
                    0.061960290963048933,
                    0.034244946492271104,
                    0.010510895293182132,
                    0.0079337476971815257,
                    0.0029669650420091095,
                    0.0026627218934911234,
                ],
            ),
            (
                "gaussian",
                4.,
                [
                    1.4360110803324098,
                    1.1519999999999997,
                    0.71111111111111114,
                    0.4499999999999999,
                    0.28036776636019478,
                    0.1704142011834319,
                    0.1154822900423257,
                    0.071999999999999981,
                ],
            ),
        ] {
            let fixed = dispersion.is_finite() || family == "binomial";
            let family = list(
                &f,
                &["family", "dispersion"],
                &[f.strings(&[family]).unwrap(), real(&f, &[dispersion])],
            );
            let fit = list(
                &f,
                &["residuals", "rank", "family", "qr"],
                &[
                    real(&f, &[0.; 8]),
                    real(&f, &[2.]),
                    family.clone(),
                    qr.clone(),
                ],
            );
            let class = f.strings(&["glm", "lm"]).unwrap();
            unsafe {
                crate::sexp::attrib_core::setAttrib(
                    fit.as_raw(),
                    crate::sexp::attrib_core::R_ClassSymbol(),
                    class.as_raw(),
                );
            }
            let infl = list(
                &f,
                &["hat", "sigma"],
                &[
                    real(&f, &[0.1, 0.2, 0.1, 0.2, 0.1, 0.2, 0.1, 0.2]),
                    real(&f, &[0.7, 0.8, 0.7, 0.8, 0.7, 0.8, 0.7, 0.8]),
                ],
            );
            if fixed {
                let unused_sigma = list(
                    &f,
                    &["hat", "sigma"],
                    &[
                        infl.try_vector_elt(0).unwrap(),
                        f.strings(&["unused"]).unwrap(),
                    ],
                );
                assert_values(
                    &call(
                        &f,
                        &[
                            ("", fit.clone()),
                            ("infl", unused_sigma),
                            ("res", real(&f, &[1., 2., 3., 4., 5., 6., 7., 8.])),
                        ],
                    ),
                    &expected,
                );
            }
            if family
                .try_vector_elt(0)
                .unwrap()
                .try_string_value_elt(0)
                .unwrap()
                .as_deref()
                == Some("binomial")
            {
                let weighted = list(
                    &f,
                    &[
                        "residuals",
                        "rank",
                        "family",
                        "qr",
                        "weights",
                        "prior.weights",
                    ],
                    &[
                        real(&f, &[1., 2., 3., 4., 5., 6., 7., 8.]),
                        real(&f, &[2.]),
                        family.clone(),
                        qr.clone(),
                        real(&f, &[2.; 8]),
                        real(&f, &[1., 0., 1., 1., 0., 1., 1., 1.]),
                    ],
                );
                unsafe {
                    crate::sexp::attrib_core::setAttrib(
                        weighted.as_raw(),
                        crate::sexp::attrib_core::R_ClassSymbol(),
                        class.as_raw(),
                    );
                }
                let supplied = list(
                    &f,
                    &["hat", "sigma"],
                    &[
                        real(&f, &[0.1, 0.2, 0.1, 0.2, 0.1, 0.2]),
                        f.strings(&["unused"]).unwrap(),
                    ],
                );
                assert_values(
                    &call(&f, &[("", weighted), ("infl", supplied)]),
                    &[
                        0.76686390532544346,
                        0.059504132231404917,
                        0.024319759804841422,
                        0.0049861495844875292,
                        0.0030838786436644845,
                        0.0016528925619834704,
                    ],
                );
            }
            assert_values(
                &call(
                    &f,
                    &[
                        ("", fit),
                        ("infl", infl),
                        ("res", real(&f, &[1., 2., 3., 4., 5., 6., 7., 8.])),
                    ],
                ),
                &expected,
            );
        }
    });
}
#[test]
fn covratio_weighted_residuals_filter_zero_cases_and_keep_original_names() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let fit = weighted_model(&f);
        let result = call(&f, &[("", fit)]);
        assert_values(
            &result,
            &[
                2.8779599271402554,
                1.4968223416181592,
                1.046565109233857,
                1.6048732045605292,
                0.40399460829980499,
                1.5408874707075904,
            ],
        );
        let actual = f
            .wrap(unsafe {
                crate::sexp::attrib_core::getAttrib(
                    result.as_raw(),
                    crate::sexp::attrib_core::R_NamesSymbol(),
                )
            })
            .unwrap();
        let expected = f
            .strings(&[
                "weighted1",
                "weighted3",
                "weighted4",
                "weighted6",
                "weighted7",
                "weighted8",
            ])
            .unwrap();
        assert_eq!(actual.len(), expected.len());
        for i in 0..actual.len() {
            assert_eq!(
                actual.try_string_value_elt(i).unwrap(),
                expected.try_string_value_elt(i).unwrap()
            );
        }
    });
}

fn weighted_model<'s>(f: &SessionNodeFactory<'s>) -> Sexp<'s> {
    let residuals = real(
        &f,
        &[
            -1.2847644853009169e-15,
            1.0000000000000018,
            -0.99999999999999911,
            1.0000000000000002,
            -0.99999999999999911,
            0.99999999999999967,
            -1.0000000000000002,
            1.0000000000000002,
        ],
    );
    names(
        &f,
        &residuals,
        &[
            "weighted1",
            "weighted2",
            "weighted3",
            "weighted4",
            "weighted5",
            "weighted6",
            "weighted7",
            "weighted8",
        ],
    );
    let matrix = real(
        &f,
        &[
            -3.6055512754639891,
            0.39223227027636809,
            0.48038446141526142,
            0.27735009811261457,
            0.55470019622522915,
            0.39223227027636809,
            -19.137156769770407,
            7.795462190866604,
            0.082734952320410268,
            -0.20879246814886554,
            -0.67414445144017632,
            -0.65810708604778967,
        ],
    );
    dim(&f, &matrix, 6, 2);
    let qr = list(
        &f,
        &["qr", "qraux", "rank"],
        &[
            matrix,
            real(&f, &[1.2773500981126147, 1.2489677786279891]),
            real(&f, &[2.]),
        ],
    );
    list(
        &f,
        &["residuals", "rank", "qr", "weights"],
        &[
            residuals,
            real(&f, &[2.]),
            qr,
            real(&f, &[1., 0., 2., 3., 0., 1., 4., 2.]),
        ],
    )
}

fn collecting_weighted_case(action: u8) {
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let (fit, weak, old_weights) = {
        let facade = facade.borrow();
        let session = facade.as_ref().unwrap();
        session.with_active(|| {
            let f = session.owner_token().unwrap().node_factory();
            let fit = weighted_model(&f);
            let old_weights = fit.try_vector_elt(3).unwrap().allocation().unwrap().clone();
            (
                fit.into_owned().unwrap(),
                session.owner_token().unwrap().weak_owner().unwrap(),
                old_weights,
            )
        })
    };
    let pin = weak.pin().unwrap();
    let instance = pin.as_ptr();
    let observed = Rc::new(Cell::new(0));
    let counted = observed.clone();
    let callback_fit = fit.clone();
    let callback_weak = weak.clone();
    let callback_facade = Rc::downgrade(&facade);
    let outcome = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            let f = weak.node_factory().unwrap();
            let args = f.pairlist_cell(&fit, &f.nil(), &f.nil()).unwrap();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if counted.replace(1) != 0 {
                    return;
                }
                (*instance).memory_state.gc_force_gap = 0;
                let f = callback_weak.node_factory().unwrap();
                let mut model = SexpMut::try_from_checked(callback_fit.clone()).unwrap();
                for i in 0..model.len() {
                    model.try_set_vector_elt(i, f.nil()).unwrap();
                }
                crate::sexp::gengc::full_gc();
                assert!(
                    old_weights.is_live(),
                    "the captured weight field alone survives source detachment"
                );
                match action {
                    0 => {}
                    1 => std::panic::panic_any(948_u32),
                    _ => drop(callback_facade.upgrade().unwrap().borrow_mut().take()),
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let raw = crate::mainutils::essentials::mathstats::do_covratio(
                f.nil().as_raw(),
                f.nil().as_raw(),
                args.as_raw(),
                f.nil().as_raw(),
            );
            f.wrap(raw).unwrap().into_owned().unwrap()
        })
    }));
    assert_eq!(observed.get(), 1);
    assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
    match action {
        0 => {
            let result = outcome.unwrap();
            assert_values(
                &result,
                &[
                    2.8779599271402554,
                    1.4968223416181592,
                    1.046565109233857,
                    1.6048732045605292,
                    0.40399460829980499,
                    1.5408874707075904,
                ],
            );
            unsafe {
                crate::sexp::session::with_instance_active(instance, || {
                    crate::sexp::gengc::full_gc();
                    let f = weak.node_factory().unwrap();
                    let labels = f
                        .wrap(crate::sexp::attrib_core::getAttrib(
                            result.as_raw(),
                            crate::sexp::attrib_core::R_NamesSymbol(),
                        ))
                        .unwrap();
                    assert_eq!(labels.len(), 6);
                    for (i, expected) in [
                        "weighted1",
                        "weighted3",
                        "weighted4",
                        "weighted6",
                        "weighted7",
                        "weighted8",
                    ]
                    .iter()
                    .enumerate()
                    {
                        assert_eq!(
                            labels.try_string_value_elt(i as i64).unwrap().as_deref(),
                            Some(*expected)
                        );
                    }
                });
            }
        }
        1 => assert_eq!(*outcome.unwrap_err().downcast::<u32>().unwrap(), 948),
        _ => assert_eq!(
            outcome
                .unwrap_err()
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message,
            crate::sexp::object::SexpError::RootUnavailable.to_string()
        ),
    }
    drop(facade.borrow_mut().take());
}
#[test]
fn covratio_weighted_owns_detached_fields_through_real_collection() {
    collecting_weighted_case(0);
}
#[test]
fn covratio_weighted_preserves_live_unwind_payload() {
    collecting_weighted_case(1);
}
#[test]
fn covratio_weighted_refuses_original_revocation() {
    collecting_weighted_case(2);
}
