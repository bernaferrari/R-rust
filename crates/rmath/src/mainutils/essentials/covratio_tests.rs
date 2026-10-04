//! Independent pinned GNU covratio contracts; model values are original R outputs.
use crate::sexp::{
    RSession, SEXPTYPE,
    object::{SessionNodeFactory, Sexp, SexpMut},
};

fn real<'s>(f: &SessionNodeFactory<'s>, values: &[f64]) -> Sexp<'s> {
    let value = f
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::REALSXP, values.len() as i64)))
        .unwrap();
    let mut value = SexpMut::try_from_checked(value).unwrap();
    for (i, v) in values.iter().copied().enumerate() {
        value.try_set_real_elt(i as i64, v).unwrap();
    }
    value.freeze()
}
fn list<'s>(f: &SessionNodeFactory<'s>, names: &[&str], values: &[Sexp<'s>]) -> Sexp<'s> {
    let value = f
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::VECSXP, values.len() as i64)))
        .unwrap();
    let mut value = SexpMut::try_from_checked(value).unwrap();
    for (i, v) in values.iter().enumerate() {
        value.try_set_vector_elt(i as i64, v.clone()).unwrap();
    }
    let value = value.freeze();
    let names = f.strings(names).unwrap();
    unsafe {
        crate::sexp::attrib_core::setAttrib(
            value.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
            names.as_raw(),
        );
    }
    value
}
fn model<'s>(f: &SessionNodeFactory<'s>) -> Sexp<'s> {
    let residuals = real(
        f,
        &[
            -0.083333333333334952,
            0.90476190476190566,
            -1.1071428571428565,
            0.88095238095238138,
            -1.1309523809523807,
            0.85714285714285687,
            -1.1547619047619051,
            0.83333333333333304,
        ],
    );
    let names = f
        .strings(&[
            "row1", "row2", "row3", "row4", "row5", "row6", "row7", "row8",
        ])
        .unwrap();
    unsafe {
        crate::sexp::attrib_core::setAttrib(
            residuals.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
            names.as_raw(),
        );
    }
    let hat = real(
        f,
        &[
            0.41666666666666669,
            0.27380952380952378,
            0.17857142857142855,
            0.13095238095238093,
            0.13095238095238093,
            0.17857142857142858,
            0.27380952380952378,
            0.41666666666666657,
        ],
    );
    list(
        f,
        &["residuals", "hat", "sigma", "rank"],
        &[
            residuals,
            hat,
            real(f, &[1.0699725556486344]),
            real(f, &[2.]),
        ],
    )
}
fn call<'s>(f: &SessionNodeFactory<'s>, values: &[(&str, Sexp<'s>)]) -> Sexp<'s> {
    let mut args = f.nil();
    for (tag, value) in values.iter().rev() {
        let tag = if tag.is_empty() {
            f.nil()
        } else {
            f.wrap(unsafe {
                crate::sexp::symbol::Rf_install(std::ffi::CString::new(*tag).unwrap().as_ptr())
            })
            .unwrap()
        };
        args = f.pairlist_cell(value, &args, &tag).unwrap();
    }
    f.wrap(unsafe {
        super::mathstats::do_covratio(
            f.nil().as_raw(),
            f.nil().as_raw(),
            args.as_raw(),
            f.nil().as_raw(),
        )
    })
    .unwrap()
}
fn assert_values(value: &Sexp<'_>, expected: &[f64]) {
    assert_eq!(value.len(), expected.len() as i64);
    for (i, expected) in expected.iter().copied().enumerate() {
        let got = value.try_real_elt(i as i64).unwrap();
        if expected.is_nan() {
            assert!(got.is_nan());
        } else if expected.is_infinite() {
            assert_eq!(got, expected);
        } else {
            assert!(
                (got - expected).abs() < 1e-11,
                "index {i}: {got} != {expected}"
            );
        }
    }
}
#[test]
fn portable_covratio_is_registered_without_host_stats() {
    assert!(crate::eval::builtin::evaluated_builtin_handler("covratio").is_some());
}
#[test]
fn portable_covratio_preserves_gnu_optional_influence_residuals_and_names() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let fit = model(&f);
        let infl = list(
            &f,
            &["hat", "sigma"],
            &[real(&f, &[0.2; 8]), real(&f, &[2.; 8])],
        );
        let residuals = real(&f, &[1., 2., 3., 4., 5., 6., 7., 8.]);
        let names = f
            .strings(&[
                "res1", "res2", "res3", "res4", "res5", "res6", "res7", "res8",
            ])
            .unwrap();
        unsafe {
            crate::sexp::attrib_core::setAttrib(
                residuals.as_raw(),
                crate::sexp::attrib_core::R_NamesSymbol(),
                names.as_raw(),
            );
        }
        let result = call(&f, &[("", fit), ("infl", infl), ("res", residuals)]);
        assert_values(
            &result,
            &[
                1.5944636678200694,
                1.1519999999999997,
                0.73728000000000016,
                0.4499999999999999,
                0.27412254610350989,
                0.1704142011834319,
                0.10906508875739646,
                0.071999999999999981,
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
        assert_eq!(actual, names);
    });
}
#[test]
fn portable_covratio_default_matches_original_residual_sum_and_names() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let result = call(&f, &[("", model(&f))]);
        assert_values(
            &result,
            &[
                2.4600222698704957,
                1.3855286381584659,
                1.0741112640414454,
                1.2541544502434274,
                1.022992823969531,
                1.3262420096904088,
                1.0644762346691063,
                1.6870611709146055,
            ],
        );
        let names = f
            .wrap(unsafe {
                crate::sexp::attrib_core::getAttrib(
                    result.as_raw(),
                    crate::sexp::attrib_core::R_NamesSymbol(),
                )
            })
            .unwrap();
        assert_eq!(names.len(), 8);
    });
}

#[test]
fn portable_covratio_recycles_supplied_inputs_and_preserves_gnu_infinities() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let fit = model(&f);
        let infl = list(
            &f,
            &["hat", "sigma"],
            &[real(&f, &[0.1, 0.2]), real(&f, &[2., 3.])],
        );
        let result = call(
            &f,
            &[
                ("", fit),
                ("infl", infl),
                ("res", real(&f, &[1., 2., 3., 4., 5., 6., 7., 8.])),
            ],
        );
        assert_values(
            &result,
            &[
                1.4360110803324098,
                1.4579999999999997,
                0.71111111111111114,
                0.86272189349112416,
                0.28036776636019478,
                0.4499999999999999,
                0.1154822900423257,
                0.23327999999999999,
            ],
        );
        let fit = list(
            &f,
            &["residuals", "rank"],
            &[real(&f, &[-0.5, 0.5]), real(&f, &[1.])],
        );
        let infl = list(
            &f,
            &["hat", "sigma"],
            &[
                real(&f, &[0.5, 0.5]),
                real(&f, &[f64::INFINITY, f64::INFINITY]),
            ],
        );
        assert_values(
            &call(&f, &[("", fit), ("infl", infl)]),
            &[f64::INFINITY, f64::INFINITY],
        );
    });
}

fn collecting_case(action: u8) {
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let (fit, weak, old_residuals) = {
        let facade = facade.borrow();
        let session = facade.as_ref().unwrap();
        session.with_active(|| {
            let f = session.owner_token().unwrap().node_factory();
            let fit = model(&f);
            let old_residuals = fit.try_vector_elt(0).unwrap().allocation().unwrap().clone();
            (
                fit.into_owned().unwrap(),
                session.owner_token().unwrap().weak_owner().unwrap(),
                old_residuals,
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
                for index in 0..model.len() {
                    model.try_set_vector_elt(index, f.nil()).unwrap();
                }
                crate::sexp::gengc::full_gc();
                assert!(
                    old_residuals.is_live(),
                    "actual captured input alone keeps detached residuals alive"
                );
                match action {
                    0 => {}
                    1 => std::panic::panic_any(947_u32),
                    _ => drop(callback_facade.upgrade().unwrap().borrow_mut().take()),
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let raw = super::mathstats::do_covratio(
                f.nil().as_raw(),
                f.nil().as_raw(),
                args.as_raw(),
                f.nil().as_raw(),
            );
            f.wrap(raw).unwrap().into_owned().unwrap()
        })
    }));
    assert_eq!(observed.get(), 1, "real allocation callback must execute");
    assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
    match action {
        0 => assert_values(
            &outcome.unwrap(),
            &[
                2.4600222698704957,
                1.3855286381584659,
                1.0741112640414454,
                1.2541544502434274,
                1.022992823969531,
                1.3262420096904088,
                1.0644762346691063,
                1.6870611709146055,
            ],
        ),
        1 => assert_eq!(*outcome.unwrap_err().downcast::<u32>().unwrap(), 947),
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
fn portable_covratio_owns_detached_inputs_through_collecting_callback() {
    collecting_case(0);
}
#[test]
fn portable_covratio_preserves_live_callback_panic() {
    collecting_case(1);
}
#[test]
fn portable_covratio_refuses_revoked_original_publication() {
    collecting_case(2);
}

#[test]
fn portable_covratio_lm_preserves_generic_data_and_response_row_labels() {
    let mut session = RSession::new_with_path_policy(
        crate::mainutils::paths::RuntimePathPolicy::new(Vec::new(), "/tmp"),
    );
    let (output, _, _) = session.eval_script_with_output_capture("d<-data.frame(y=c(1,3,2,5,4,7,6,9),x=1:8,row.names=paste0('case',1:8));fit<-lm(y~x,data=d);identical(names(fit$residuals),row.names(d))&&identical(names(fit$fitted.values),row.names(d))&&identical(names(covratio(fit)),row.names(d))");
    assert_eq!(output.unwrap().try_logical_elt(0).unwrap(), 1);
}

#[path = "covratio/extension_tests.rs"]
mod extension_tests;
