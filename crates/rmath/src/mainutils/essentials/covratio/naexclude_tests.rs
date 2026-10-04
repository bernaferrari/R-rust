//! Pinned original GNU na.exclude row and supplied-input contracts.
use super::{assert_values, call, list, real};
use crate::sexp::{
    RSession, SEXPTYPE,
    object::{SessionNodeFactory, Sexp, SexpMut},
};
fn integer<'s>(f: &SessionNodeFactory<'s>, values: &[i32]) -> Sexp<'s> {
    let x = f
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, values.len() as i64)))
        .unwrap();
    let mut x = SexpMut::try_from_checked(x).unwrap();
    for (i, v) in values.iter().copied().enumerate() {
        x.try_set_integer_elt(i as i64, v).unwrap();
    }
    x.freeze()
}
fn named<'s>(f: &SessionNodeFactory<'s>, value: &Sexp<'s>, labels: &[&str]) {
    let labels = f.strings(labels).unwrap();
    unsafe {
        crate::sexp::attrib_core::setAttrib(
            value.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
            labels.as_raw(),
        );
    }
}
fn excluded_model<'s>(f: &SessionNodeFactory<'s>) -> Sexp<'s> {
    let residuals = real(
        f,
        &[
            -0.42028985507246336,
            0.61594202898550743,
            0.68840579710144945,
            -1.2753623188405798,
            0.76086956521739102,
            -1.2028985507246377,
            0.83333333333333326,
        ],
    );
    named(
        f,
        &residuals,
        &[
            "case1", "case2", "case4", "case5", "case6", "case7", "case8",
        ],
    );
    let matrix = real(
        f,
        &[
            -2.6457513110645907,
            0.3779644730092272,
            0.3779644730092272,
            0.3779644730092272,
            0.3779644730092272,
            0.3779644730092272,
            0.3779644730092272,
            -12.472827609304495,
            6.2792174216674042,
            -0.048495329262298076,
            -0.20775084357994961,
            -0.36700635789760111,
            -0.52626187221525256,
            -0.68551738653290406,
        ],
    );
    let dims = integer(f, &[7, 2]);
    unsafe {
        crate::sexp::attrib_core::setAttrib(
            matrix.as_raw(),
            crate::sexp::attrib_core::R_DimSymbol(),
            dims.as_raw(),
        );
    }
    let qr = list(
        f,
        &["qr", "qraux", "rank"],
        &[
            matrix,
            real(f, &[1.3779644730092273, 1.270015699373005]),
            integer(f, &[2]),
        ],
    );
    let action = integer(f, &[3]);
    named(f, &action, &["case3"]);
    let class = f.strings(&["exclude"]).unwrap();
    unsafe {
        crate::sexp::attrib_core::setAttrib(
            action.as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
            class.as_raw(),
        );
    }
    list(
        f,
        &["residuals", "qr", "rank", "df.residual", "na.action"],
        &[residuals, qr, integer(f, &[2]), integer(f, &[5]), action],
    )
}
fn assert_names(f: &SessionNodeFactory<'_>, value: &Sexp<'_>, expected: &[&str]) {
    let names = f
        .wrap(unsafe {
            crate::sexp::attrib_core::getAttrib(
                value.as_raw(),
                crate::sexp::attrib_core::R_NamesSymbol(),
            )
        })
        .unwrap();
    assert_eq!(names.len(), expected.len() as i64);
    for (i, name) in expected.iter().enumerate() {
        assert_eq!(
            names.try_string_value_elt(i as i64).unwrap().as_deref(),
            Some(*name)
        );
    }
}
const ROWS: [&str; 8] = [
    "case1", "case2", "case3", "case4", "case5", "case6", "case7", "case8",
];
#[test]
fn covratio_naexclude_restores_original_default_rows() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let result = call(&f, &[("", excluded_model(&f))]);
        assert_values(
            &result,
            &[
                2.694267308775951,
                1.8661403286954854,
                crate::sexp::ffi::NA_REAL,
                1.4845588594715602,
                0.76308192424585231,
                1.4438115580871018,
                0.8520849119266789,
                1.623759239478423,
            ],
        );
        assert!(crate::sexp::ffi::is_na_real(
            result.try_real_elt(2).unwrap()
        ));
        assert_names(&f, &result, &ROWS);
    });
}
#[test]
fn covratio_naexclude_preserves_supplied_and_default_input_positions() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let fit = excluded_model(&f);
        let infl = list(
            &f,
            &["hat", "sigma"],
            &[real(&f, &[0.2; 7]), real(&f, &[2.; 7])],
        );
        let result = call(&f, &[("", fit.clone()), ("infl", infl.clone())]);
        assert_values(
            &result,
            &[
                1.9003133826365208,
                1.8422973015630559,
                crate::sexp::ffi::NA_REAL,
                1.8161542400526627,
                1.5375351932580921,
                1.7877537702341824,
                1.5765409948993363,
                1.7572759240147648,
            ],
        );
        assert!(crate::sexp::ffi::is_na_real(
            result.try_real_elt(2).unwrap()
        ));
        assert_names(&f, &result, &ROWS);
        let res = real(&f, &[1., 2., 3., 4., 5., 6., 7.]);
        let result = call(&f, &[("", fit.clone()), ("res", res.clone())]);
        assert_values(
            &result,
            &[
                1.5896233712143752,
                0.46439451864226661,
                0.16339987219668087,
                0.075971901777041148,
                0.020616275502334049,
                0.017508381403549912,
                0.0048867015473120414,
                1.3482667194301545,
            ],
        );
        assert_names(&f, &result, &ROWS);
        let labels = [
            "supplied1",
            "supplied2",
            "supplied3",
            "supplied4",
            "supplied5",
            "supplied6",
            "supplied7",
        ];
        named(&f, &res, &labels);
        let result = call(&f, &[("", fit.clone()), ("infl", infl), ("res", res)]);
        assert_values(
            &result,
            &[
                1.680319260659525,
                1.1337868480725621,
                0.67334399461324801,
                0.38580246913580241,
                0.22395789591556789,
                0.13437248051599029,
                0.083786303034111498,
            ],
        );
        assert_names(&f, &result, &labels);
        let res = real(&f, &[1., 2., 3., 4., 5., 6., 7., 8.]);
        named(
            &f,
            &res,
            &[
                "custom1", "custom2", "custom3", "custom4", "custom5", "custom6", "custom7",
                "custom8",
            ],
        );
        let result = call(&f, &[("", fit), ("res", res)]);
        assert_values(
            &result,
            &[
                1.5896233712143752,
                0.46439451864226661,
                0.16339987219668087,
                0.075971901777041148,
                0.020616275502334049,
                0.017508381403549912,
                0.0048867015473120414,
                0.0036184209425155229,
            ],
        );
        assert_names(&f, &result, &ROWS);
    });
}

const RECYCLING: &str = "longer object length is not a multiple of shorter object length";
#[test]
fn covratio_naexclude_preserves_exact_recycling_warnings() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let old = unsafe { crate::mainutils::options::R_SetOptionWarn(0) };
        let before = crate::mainutils::errors::collect_warnings();
        let fit = excluded_model(&f);
        let infl = list(
            &f,
            &["hat", "sigma"],
            &[real(&f, &[0.2; 7]), real(&f, &[2.; 7])],
        );
        let result = call(&f, &[("", fit.clone()), ("infl", infl)]);
        assert_eq!(result.len(), 8);
        assert_eq!(crate::mainutils::errors::collect_warnings() - before, 2);
        assert_eq!(
            crate::mainutils::errors::last_collected_warning_message(),
            RECYCLING
        );
        crate::mainutils::errors::restore_collect_warnings(before);
        let result = call(
            &f,
            &[("", fit), ("res", real(&f, &[1., 2., 3., 4., 5., 6., 7.]))],
        );
        assert_eq!(result.len(), 8);
        assert_eq!(crate::mainutils::errors::collect_warnings() - before, 1);
        assert_eq!(
            crate::mainutils::errors::last_collected_warning_message(),
            RECYCLING
        );
        crate::mainutils::errors::restore_collect_warnings(before);
        unsafe {
            crate::mainutils::options::R_SetOptionWarn(old);
        }
    });
}
#[test]
fn covratio_naexclude_warn_two_aborts_and_recovers() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let fit = excluded_model(&f);
        let infl = list(
            &f,
            &["hat", "sigma"],
            &[real(&f, &[0.2; 7]), real(&f, &[2.; 7])],
        );
        let before = crate::mainutils::errors::collect_warnings();
        let old = unsafe { crate::mainutils::options::R_SetOptionWarn(2) };
        let failed = catch_unwind(AssertUnwindSafe(|| {
            call(&f, &[("", fit.clone()), ("infl", infl)])
        }));
        unsafe {
            crate::mainutils::options::R_SetOptionWarn(old);
        }
        let error = failed.unwrap_err();
        let error = error
            .downcast_ref::<crate::sexp::context::RError>()
            .unwrap();
        assert!(error.message.contains(RECYCLING), "{}", error.message);
        assert_eq!(crate::mainutils::errors::collect_warnings(), before);
        let result = call(&f, &[("", fit)]);
        assert_values(
            &result,
            &[
                2.694267308775951,
                1.8661403286954854,
                crate::sexp::ffi::NA_REAL,
                1.4845588594715602,
                0.76308192424585231,
                1.4438115580871018,
                0.8520849119266789,
                1.623759239478423,
            ],
        );
        assert_names(&f, &result, &ROWS);
    });
}
fn replace_action<'s>(
    f: &SessionNodeFactory<'s>,
    model: &Sexp<'s>,
    values: &[f64],
    labels: &[&str],
) {
    let action = real(f, values);
    named(f, &action, labels);
    let class = f.strings(&["exclude"]).unwrap();
    unsafe {
        crate::sexp::attrib_core::setAttrib(
            action.as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
            class.as_raw(),
        );
    }
    let mut model = SexpMut::try_from_checked(model.clone()).unwrap();
    model.try_set_vector_elt(4, action).unwrap();
}
#[test]
fn covratio_naexclude_numeric_actions_truncate_and_report_invalid_names() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let fit = excluded_model(&f);
        replace_action(&f, &fit, &[3.9], &["case3"]);
        let result = call(&f, &[("", fit.clone())]);
        assert_values(
            &result,
            &[
                2.694267308775951,
                1.8661403286954854,
                crate::sexp::ffi::NA_REAL,
                1.4845588594715602,
                0.76308192424585231,
                1.4438115580871018,
                0.8520849119266789,
                1.623759239478423,
            ],
        );
        assert_names(&f, &result, &ROWS);
        replace_action(&f, &fit, &[10.], &["missing"]);
        let before = crate::mainutils::errors::collect_warnings();
        let failed = catch_unwind(AssertUnwindSafe(|| call(&f, &[("", fit.clone())])));
        assert_eq!(
            failed
                .unwrap_err()
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message,
            "'names' attribute [10] must be the same length as the vector [8]"
        );
        assert_eq!(crate::mainutils::errors::collect_warnings() - before, 1);
        crate::mainutils::errors::restore_collect_warnings(before);
        replace_action(&f, &fit, &[crate::sexp::ffi::NA_REAL], &["missing"]);
        let failed = catch_unwind(AssertUnwindSafe(|| call(&f, &[("", fit.clone())])));
        assert_eq!(
            failed
                .unwrap_err()
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message,
            "NAs are not allowed in subscripted assignments"
        );
        // Both supplied inputs leave the malformed, unused na.action alone.
        let infl = list(
            &f,
            &["hat", "sigma"],
            &[real(&f, &[0.2; 7]), real(&f, &[2.; 7])],
        );
        let result = call(
            &f,
            &[
                ("", fit),
                ("infl", infl),
                ("res", real(&f, &[1., 2., 3., 4., 5., 6., 7.])),
            ],
        );
        assert_values(
            &result,
            &[
                1.680319260659525,
                1.1337868480725621,
                0.67334399461324801,
                0.38580246913580241,
                0.22395789591556789,
                0.13437248051599029,
                0.083786303034111498,
            ],
        );
    });
}

fn collecting_excluded_case(action: u8) {
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let (fit, weak, old_action, old_labels) = {
        let facade = facade.borrow();
        let session = facade.as_ref().unwrap();
        session.with_active(|| {
            let f = session.owner_token().unwrap().node_factory();
            let fit = excluded_model(&f);
            let old_action = fit.try_vector_elt(4).unwrap();
            let labels = f
                .wrap(unsafe {
                    crate::sexp::attrib_core::getAttrib(
                        old_action.as_raw(),
                        crate::sexp::attrib_core::R_NamesSymbol(),
                    )
                })
                .unwrap();
            (
                fit.into_owned().unwrap(),
                session.owner_token().unwrap().weak_owner().unwrap(),
                old_action.allocation().unwrap().clone(),
                labels.allocation().unwrap().clone(),
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
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            let f = weak.node_factory().unwrap();
            let args = f.pairlist_cell(&fit, &f.nil(), &f.nil()).unwrap();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if counted.replace(1) != 0 {
                    return;
                }
                (*instance).memory_state.gc_force_gap = 0;
                let f = callback_weak.node_factory().unwrap();
                let mut source = SexpMut::try_from_checked(callback_fit.clone()).unwrap();
                for i in 0..source.len() {
                    source.try_set_vector_elt(i, f.nil()).unwrap();
                }
                crate::sexp::gengc::full_gc();
                assert!(
                    old_action.is_live(),
                    "only the canonical row snapshot owns the detached action"
                );
                assert!(
                    old_labels.is_live(),
                    "omitted labels survive collection after model detachment"
                );
                match action {
                    0 => {}
                    1 => std::panic::panic_any(642_u32),
                    _ => drop(callback_facade.upgrade().unwrap().borrow_mut().take()),
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let raw = super::super::mathstats::do_covratio(
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
            let result = result.unwrap();
            unsafe {
                crate::sexp::session::with_instance_active(instance, || {
                    crate::sexp::gengc::full_gc();
                    let f = weak.node_factory().unwrap();
                    assert_values(
                        &result,
                        &[
                            2.694267308775951,
                            1.8661403286954854,
                            crate::sexp::ffi::NA_REAL,
                            1.4845588594715602,
                            0.76308192424585231,
                            1.4438115580871018,
                            0.8520849119266789,
                            1.623759239478423,
                        ],
                    );
                    assert!(crate::sexp::ffi::is_na_real(
                        result.try_real_elt(2).unwrap()
                    ));
                    assert_names(&f, &result, &ROWS);
                });
            }
        }
        1 => assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 642),
        _ => assert_eq!(
            result
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
fn covratio_naexclude_owns_detached_action_and_labels_through_collection() {
    collecting_excluded_case(0);
}
#[test]
fn covratio_naexclude_preserves_exact_live_callback_unwind() {
    collecting_excluded_case(1);
}
#[test]
fn covratio_naexclude_refuses_original_revocation_after_collection() {
    collecting_excluded_case(2);
}

#[test]
fn covratio_naexclude_warning_refuses_callback_revocation() {
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let (fit, infl, res, weak, old_hat, old_sigma) = {
        let facade = facade.borrow();
        let session = facade.as_ref().unwrap();
        session.with_active(|| {
            let f = session.owner_token().unwrap().node_factory();
            unsafe {
                crate::mainutils::options::R_SetOptionWarn(0);
            }
            let fit = excluded_model(&f);
            let hat = real(&f, &[0.2; 7]);
            let sigma = real(&f, &[2.; 7]);
            let old_hat = hat.allocation().unwrap().clone();
            let old_sigma = sigma.allocation().unwrap().clone();
            let infl = list(&f, &["hat", "sigma"], &[hat, sigma]);
            (
                fit.into_owned().unwrap(),
                infl.into_owned().unwrap(),
                real(&f, &[1., 2., 3., 4., 5., 6., 7., 8.])
                    .into_owned()
                    .unwrap(),
                session.owner_token().unwrap().weak_owner().unwrap(),
                old_hat,
                old_sigma,
            )
        })
    };
    let pin = weak.pin().unwrap();
    let instance = pin.as_ptr();
    let observed = Rc::new(Cell::new(0));
    let counted = observed.clone();
    let callback_infl = infl.clone();
    let callback_weak = weak.clone();
    let callback_facade = Rc::downgrade(&facade);
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            let f = weak.node_factory().unwrap();
            let res_tag = f
                .wrap(crate::sexp::symbol::Rf_install(c"res".as_ptr()))
                .unwrap();
            let infl_tag = f
                .wrap(crate::sexp::symbol::Rf_install(c"infl".as_ptr()))
                .unwrap();
            let tail = f.pairlist_cell(&res, &f.nil(), &res_tag).unwrap();
            let tail = f.pairlist_cell(&infl, &tail, &infl_tag).unwrap();
            let args = f.pairlist_cell(&fit, &tail, &f.nil()).unwrap();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if counted.replace(1) != 0 {
                    return;
                }
                (*instance).memory_state.gc_force_gap = 0;
                let f = callback_weak.node_factory().unwrap();
                let mut infl = SexpMut::try_from_checked(callback_infl.clone()).unwrap();
                for i in 0..infl.len() {
                    infl.try_set_vector_elt(i, f.nil()).unwrap();
                }
                crate::sexp::gengc::full_gc();
                assert!(old_hat.is_live());
                assert!(old_sigma.is_live());
                drop(callback_facade.upgrade().unwrap().borrow_mut().take());
            }));
            // Every argument is dense, all syntax/options are initialized,
            // and both defaults are supplied: the first arena allocation in
            // the kernel is the genuine recycling-warning bridge.
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            super::super::mathstats::do_covratio(
                f.nil().as_raw(),
                f.nil().as_raw(),
                args.as_raw(),
                f.nil().as_raw(),
            );
        })
    }));
    assert_eq!(observed.get(), 1);
    assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
    assert_eq!(
        result
            .unwrap_err()
            .downcast_ref::<crate::sexp::context::RError>()
            .unwrap()
            .message,
        crate::sexp::object::SexpError::RootUnavailable.to_string()
    );
}
