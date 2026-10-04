//! Real public entry versus independently pinned supersmoother outputs.
use crate::sexp::{R_xlen_t, RSession, SEXPTYPE, Sexp, SexpMut, object::SessionNodeFactory};
fn real(f: &SessionNodeFactory<'_>, data: &[f64]) -> Sexp<'static> {
    let n = f
        .allocate(|a| {
            a.alloc_vector_sexp(SEXPTYPE::REALSXP, data.len() as R_xlen_t)
                .map(|v| v.as_raw())
        })
        .unwrap()
        .into_owned()
        .unwrap();
    let mut n = SexpMut::try_from_checked(n).unwrap();
    for (i, &v) in data.iter().enumerate() {
        n.try_set_real_elt(i as R_xlen_t, v).unwrap();
    }
    n.freeze()
}
fn fixture(case: &str) -> Vec<Vec<f64>> {
    include_str!("../fixtures/supsmu-gnu-r90451.tsv")
        .lines()
        .skip(1)
        .filter(|row| row.split('\t').next() == Some(case))
        .map(|row| {
            let f: Vec<_> = row.split('\t').collect();
            f[3].split(',')
                .filter(|x| !x.is_empty())
                .map(|x| x.parse().unwrap())
                .collect()
        })
        .collect()
}
#[test]
fn supsmu_public_default_and_fixed_span_match_gnu_instead_of_lowess() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let nil = factory.nil().into_owned().unwrap();
    for case in ["cv", "fixed", "weighted", "periodic"] {
        let data = fixture(case);
        let n = data[0][0] as usize;
        let mut values = vec![
            real(&factory, &data[1][..n]),
            real(&factory, &data[2][..n]),
            real(&factory, &data[3][..n]),
            real(&factory, &data[5][..1]),
            real(&factory, &[f64::from(data[4][0] == 2.)]),
            real(&factory, &[data[6].first().copied().unwrap_or(0.)]),
        ];
        let mut tail = nil.clone();
        for v in values.drain(..).rev() {
            tail = factory
                .pairlist_cell(&v, &tail, &nil)
                .unwrap()
                .into_owned()
                .unwrap();
        }
        let result = unsafe {
            crate::library::stats::lowess::do_supsmu(
                nil.as_raw(),
                nil.as_raw(),
                tail.as_raw(),
                nil.as_raw(),
            )
        };
        let result = factory.wrap(result).unwrap().into_owned().unwrap();
        let smooth = result.try_vector_elt(1).unwrap();
        let expected: Vec<f64> = include_str!("../fixtures/supsmu-gnu-r90451.tsv")
            .lines()
            .find(|row| row.starts_with(&format!("{case}\t7\t")))
            .unwrap()
            .split('\t')
            .nth(4)
            .unwrap()
            .split(',')
            .map(|x| x.parse().unwrap())
            .collect();
        assert_eq!(smooth.len(), n as R_xlen_t);
        for (i, e) in expected.iter().take(n).enumerate() {
            assert!(
                (smooth.try_real_elt(i as R_xlen_t).unwrap() - e).abs() < 1e-10,
                "public {case} index{i}"
            );
        }
    }
}

use crate::sexp::{
    SEXP,
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    heap::CheckedNode,
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};
struct CollectingX {
    data: Vec<f64>,
    arguments: Rc<Cell<SEXP>>,
    identities: Vec<CheckedNode>,
    calls: Rc<Cell<usize>>,
    sessions: Weak<RefCell<Option<RSession>>>,
    close: bool,
}
impl AltrepClass for CollectingX {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> crate::sexp::SexpResult<R_xlen_t> {
        Ok(self.data.len() as R_xlen_t)
    }
    fn element<'s>(
        &self,
        c: &AltrepContext<'s>,
        i: R_xlen_t,
    ) -> crate::sexp::SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        if i == 0 {
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
        }
        Ok(AltrepElement::Real(self.data[i as usize]))
    }
}
fn collecting_public(close: bool) {
    let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let arguments = Rc::new(Cell::new(std::ptr::null_mut()));
    let calls = Rc::new(Cell::new(0));
    let notifications = Rc::new(Cell::new(0));
    let observed = notifications.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| observed.set(observed.get() + 1)));
    let (args, nil, owner) = {
        let borrow = sessions.borrow();
        let session = borrow.as_ref().unwrap();
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let f = session.owner_token().unwrap().node_factory();
        let nil = f.nil().into_owned().unwrap();
        let data = fixture("cv");
        let n = data[0][0] as usize;
        let y = real(&f, &data[2][..n]);
        let weights = real(&f, &data[3][..n]);
        let identities = vec![
            y.allocation().unwrap().clone(),
            weights.allocation().unwrap().clone(),
        ];
        let class = session
            .register_altrep_class(
                "collecting_public_supsmu",
                CollectingX {
                    data: data[1][..n].to_vec(),
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
        let x = AltrepBuilder::new(class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let args = f
            .pairlist_cell(
                &x,
                &f.pairlist_cell(&y, &f.pairlist_cell(&weights, &nil, &nil).unwrap(), &nil)
                    .unwrap(),
                &nil,
            )
            .unwrap()
            .into_owned()
            .unwrap();
        arguments.set(args.as_raw());
        (args, nil, owner)
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::library::stats::lowess::do_supsmu(
            nil.as_raw(),
            nil.as_raw(),
            args.as_raw(),
            nil.as_raw(),
        )
    }));
    assert!(calls.get() > 0);
    assert!(notifications.get() > 0);
    if close {
        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
    } else {
        let result = owner
            .node_factory()
            .unwrap()
            .wrap(result.unwrap())
            .unwrap()
            .into_owned()
            .unwrap();
        let smoothed = result.try_vector_elt(1).unwrap().into_owned().unwrap();
        assert_eq!(smoothed.len(), 24);
        drop(args);
        drop(result);
        sessions
            .borrow()
            .as_ref()
            .unwrap()
            .with_active(crate::sexp::gengc::full_gc);
        assert!(smoothed.try_real_elt(23).unwrap().is_finite());
    }
}
#[test]
fn supsmu_public_detached_numeric_provider_keeps_original_argument_roots() {
    collecting_public(false);
}
#[test]
fn supsmu_public_original_close_during_numeric_provider_denies_publication() {
    collecting_public(true);
}

#[test]
fn supsmu_public_sorting_ties_and_finite_filter_match_independent_gnu() {
    let session = RSession::new_for_gc_tests();
    let f = session.owner_token().unwrap().node_factory();
    let nil = f.nil().into_owned().unwrap();
    for row in include_str!("../fixtures/supsmu-public-gnu-r90451.tsv")
        .lines()
        .skip(1)
    {
        let data: Vec<_> = row.split('\t').collect();
        assert_eq!(data.len(), 9);
        let parse = |text: &str| {
            text.split(',')
                .filter(|x| !x.is_empty())
                .map(|x| x.parse::<f64>().unwrap())
                .collect::<Vec<_>>()
        };
        let values: Vec<_> = data[1..7].iter().map(|s| real(&f, &parse(s))).collect();
        let mut args = nil.clone();
        for v in values.iter().rev() {
            args = f
                .pairlist_cell(v, &args, &nil)
                .unwrap()
                .into_owned()
                .unwrap();
        }
        let output = unsafe {
            crate::library::stats::lowess::do_supsmu(
                nil.as_raw(),
                nil.as_raw(),
                args.as_raw(),
                nil.as_raw(),
            )
        };
        let output = f.wrap(output).unwrap().into_owned().unwrap();
        for index in 0..2 {
            let actual = output.try_vector_elt(index as R_xlen_t).unwrap();
            let expected = parse(data[index + 7]);
            assert_eq!(actual.len(), expected.len() as R_xlen_t);
            for (i, value) in expected.iter().enumerate() {
                assert!(
                    (actual.try_real_elt(i as R_xlen_t).unwrap() - value).abs() < 1e-10,
                    "{} output{index}:{i}",
                    data[0]
                );
            }
        }
        let names = unsafe {
            crate::sexp::attrib_core::getAttrib(
                output.as_raw(),
                crate::sexp::attrib_core::R_NamesSymbol(),
            )
        };
        let names = f.wrap(names).unwrap();
        assert_eq!(names.try_string_value_elt(0).unwrap().as_deref(), Some("x"));
        assert_eq!(names.try_string_value_elt(1).unwrap().as_deref(), Some("y"));
    }
}
#[test]
fn supsmu_public_named_cv_and_missing_weight_use_real_defaults() {
    let session = RSession::new_for_gc_tests();
    let f = session.owner_token().unwrap().node_factory();
    let nil = f.nil().into_owned().unwrap();
    let data = fixture("cv");
    let x = real(&f, &data[1][..24]);
    let y = real(&f, &data[2][..24]);
    let span = f.strings(&["cv"]).unwrap();
    let tag = unsafe {
        session
            .owner_token()
            .unwrap()
            .sexp(crate::sexp::symbol::Rf_install(c"span".as_ptr()))
            .unwrap()
    };
    let mut args = f
        .pairlist_cell(&span, &nil, &tag)
        .unwrap()
        .into_owned()
        .unwrap();
    args = f
        .pairlist_cell(&f.missing(), &args, &nil)
        .unwrap()
        .into_owned()
        .unwrap();
    args = f
        .pairlist_cell(&y, &args, &nil)
        .unwrap()
        .into_owned()
        .unwrap();
    args = f
        .pairlist_cell(&x, &args, &nil)
        .unwrap()
        .into_owned()
        .unwrap();
    let result = unsafe {
        crate::library::stats::lowess::do_supsmu(
            nil.as_raw(),
            nil.as_raw(),
            args.as_raw(),
            nil.as_raw(),
        )
    };
    let result = f.wrap(result).unwrap().into_owned().unwrap();
    let output = result.try_vector_elt(1).unwrap();
    let expected: Vec<f64> = include_str!("../fixtures/supsmu-gnu-r90451.tsv")
        .lines()
        .find(|r| r.starts_with("cv\t7\t"))
        .unwrap()
        .split('\t')
        .nth(4)
        .unwrap()
        .split(',')
        .map(|x| x.parse().unwrap())
        .collect();
    for (i, v) in expected.iter().take(24).enumerate() {
        assert!((output.try_real_elt(i as R_xlen_t).unwrap() - v).abs() < 1e-10);
    }
}

#[test]
fn supsmu_public_matches_exact_names_before_positionals_and_partial_names() {
    let session = RSession::new_for_gc_tests();
    let f = session.owner_token().unwrap().node_factory();
    let nil = f.nil().into_owned().unwrap();
    let data = fixture("cv");
    let x = real(&f, &data[1][..24]);
    let y = real(&f, &data[2][..24]);
    let x_tag = unsafe {
        session
            .owner_token()
            .unwrap()
            .sexp(crate::sexp::symbol::Rf_install(c"x".as_ptr()))
            .unwrap()
    };
    // GNU supsmu(y_vector, x=x_vector) matches exact x first, then positional y.
    let args = f
        .pairlist_cell(&y, &f.pairlist_cell(&x, &nil, &x_tag).unwrap(), &nil)
        .unwrap()
        .into_owned()
        .unwrap();
    let result = unsafe {
        crate::library::stats::lowess::do_supsmu(
            nil.as_raw(),
            nil.as_raw(),
            args.as_raw(),
            nil.as_raw(),
        )
    };
    let result = f.wrap(result).unwrap().into_owned().unwrap();
    let output = result.try_vector_elt(0).unwrap();
    for i in 0..24 {
        assert_eq!(output.try_real_elt(i).unwrap(), data[1][i as usize]);
    }
    // Match the exact y and partial span before assigning positional x.
    let span = f.strings(&["cv"]).unwrap();
    let sp_tag = unsafe {
        session
            .owner_token()
            .unwrap()
            .sexp(crate::sexp::symbol::Rf_install(c"sp".as_ptr()))
            .unwrap()
    };
    let y_tag = unsafe {
        session
            .owner_token()
            .unwrap()
            .sexp(crate::sexp::symbol::Rf_install(c"y".as_ptr()))
            .unwrap()
    };
    let args = f
        .pairlist_cell(
            &span,
            &f.pairlist_cell(&x, &f.pairlist_cell(&y, &nil, &y_tag).unwrap(), &nil)
                .unwrap(),
            &sp_tag,
        )
        .unwrap()
        .into_owned()
        .unwrap();
    let result = unsafe {
        crate::library::stats::lowess::do_supsmu(
            nil.as_raw(),
            nil.as_raw(),
            args.as_raw(),
            nil.as_raw(),
        )
    };
    let result = f.wrap(result).unwrap().into_owned().unwrap();
    assert_eq!(result.try_vector_elt(0).unwrap().len(), 24);
}
