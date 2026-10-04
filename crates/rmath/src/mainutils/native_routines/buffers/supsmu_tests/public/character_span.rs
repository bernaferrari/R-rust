//! Plain character span contracts from an independently pinned GNU wrapper.
use super::*;

fn invoke(f: &SessionNodeFactory<'_>, span: Sexp<'static>) -> Result<Sexp<'static>, String> {
    let data = fixture("cv");
    let x = real(f, &data[1][..24]);
    let y = real(f, &data[2][..24]);
    let nil = f.nil().into_owned().unwrap();
    let tag = f
        .wrap(unsafe { crate::sexp::symbol::Rf_install(c"span".as_ptr()) })
        .unwrap();
    let tail = f.pairlist_cell(&span, &nil, &tag).unwrap();
    let tail = f.pairlist_cell(&y, &tail, &nil).unwrap();
    let args = f
        .pairlist_cell(&x, &tail, &nil)
        .unwrap()
        .into_owned()
        .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::library::stats::lowess::do_supsmu(
            nil.as_raw(),
            nil.as_raw(),
            args.as_raw(),
            nil.as_raw(),
        )
    }));
    result
        .map(|v| f.wrap(v).unwrap().into_owned().unwrap())
        .map_err(|e| {
            e.downcast_ref::<crate::sexp::context::RError>()
                .expect("R error")
                .message
                .clone()
        })
}

#[test]
fn supsmu_public_character_span_matches_pinned_gnu_lexical_and_scalar_contracts() {
    let session = RSession::new_for_gc_tests();
    let f = session.owner_token().unwrap().node_factory();
    for row in include_str!("../../fixtures/supsmu-span-gnu-r90451.tsv")
        .lines()
        .skip(1)
    {
        let fields: Vec<_> = row.split('\t').collect();
        let span = match fields[0] {
            "decimal" => f.strings(&["0.25"]),
            "trailing" => f.strings(&["0.25 "]),
            "zeros" => f.strings(&["00.25"]),
            "exponent" => f.strings(&["0e3"]),
            "hex" => f.strings(&["0x1p-2"]),
            "cv" => f.strings(&["cv"]),
            "dot" => f.strings(&[".25"]),
            "leading" => f.strings(&[" 0.25"]),
            "plus" => f.strings(&["+0.25"]),
            "too_high" => f.strings(&["1e-1"]),
            "junk" => f.strings(&["0.5foo"]),
            "empty_text" => f.strings(&[""]),
            "text_na" => f.strings(&["NA"]),
            "empty" => f.strings(&[]),
            "multiple" => f.strings(&["cv", "0.25"]),
            "missing" => {
                let n = f.strings(&[""]).unwrap().into_owned().unwrap();
                let mut n = SexpMut::try_from_checked(n).unwrap();
                let na = f
                    .wrap(unsafe { crate::sexp::globals::R_NaString() })
                    .unwrap();
                n.try_set_string_elt(0, na).unwrap();
                Ok(n.freeze())
            }
            "real_empty" => Ok(real(&f, &[])),
            "real_multiple" => Ok(real(&f, &[0.25, 0.5])),
            "real_missing" | "real_nan" => Ok(real(&f, &[f64::NAN])),
            "real_inf" => Ok(real(&f, &[f64::INFINITY])),
            name => panic!("unknown GNU case {name}"),
        }
        .unwrap()
        .into_owned()
        .unwrap();
        let result = invoke(&f, span);
        let warnings =
            unsafe { crate::mainutils::errors::take_warnings_block() }.unwrap_or_default();
        if fields[2].is_empty() {
            assert!(warnings.is_empty(), "{}: {warnings}", fields[0]);
        } else {
            assert!(warnings.contains(fields[2]), "{}: {warnings}", fields[0]);
        }
        if !fields[1].is_empty() {
            assert_eq!(result.unwrap_err(), fields[1], "{}", fields[0]);
            continue;
        }
        let result = result.unwrap_or_else(|e| panic!("{}: {e}", fields[0]));
        for (column, expected) in [fields[3], fields[4]].iter().enumerate() {
            let values = result.try_vector_elt(column as R_xlen_t).unwrap();
            let expected: Vec<f64> = expected.split(',').map(|s| s.parse().unwrap()).collect();
            assert_eq!(values.len(), expected.len() as R_xlen_t);
            for (i, e) in expected.into_iter().enumerate() {
                let actual = values.try_real_elt(i as R_xlen_t).unwrap();
                assert!(
                    (actual - e).abs() < 1e-10,
                    "{} column{column} index{i}: {actual} vs {e}",
                    fields[0]
                );
            }
        }
    }
}

struct CollectingSpan {
    arguments: Rc<Cell<SEXP>>,
    identities: Vec<CheckedNode>,
    calls: Rc<Cell<usize>>,
    sessions: Weak<RefCell<Option<RSession>>>,
    close_on: usize,
}
impl AltrepClass for CollectingSpan {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::STRSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> crate::sexp::SexpResult<R_xlen_t> {
        Ok(1)
    }
    fn element<'s>(
        &self,
        c: &AltrepContext<'s>,
        _: R_xlen_t,
    ) -> crate::sexp::SexpResult<AltrepElement<'s>> {
        let read = self.calls.get() + 1;
        self.calls.set(read);
        if read == 1 {
            unsafe {
                crate::sexp::accessors::SETCDR(
                    self.arguments.get(),
                    crate::sexp::globals::R_NilValue(),
                );
            }
            c.gc()?;
            assert!(self.identities.iter().all(CheckedNode::is_live));
        }
        let text = c.string("0.25")?;
        if read == self.close_on {
            self.sessions
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
fn collecting_span(close_on: usize) {
    let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let arguments = Rc::new(Cell::new(std::ptr::null_mut()));
    let calls = Rc::new(Cell::new(0));
    let notifications = Rc::new(Cell::new(0));
    let observed = notifications.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| observed.set(observed.get() + 1)));
    let (args, nil, owner) = {
        let borrow = sessions.borrow();
        let session = borrow.as_ref().unwrap();
        let f = session.owner_token().unwrap().node_factory();
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let nil = f.nil().into_owned().unwrap();
        let data = fixture("cv");
        let x = real(&f, &data[1][..24]);
        let y = real(&f, &data[2][..24]);
        let identities = vec![
            x.allocation().unwrap().clone(),
            y.allocation().unwrap().clone(),
        ];
        let class = session
            .register_altrep_class(
                "collecting-character-span",
                CollectingSpan {
                    arguments: arguments.clone(),
                    identities,
                    calls: calls.clone(),
                    sessions: Rc::downgrade(&sessions),
                    close_on,
                },
            )
            .unwrap()
            .into_owned()
            .unwrap();
        let span = AltrepBuilder::new(class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let tag = f
            .wrap(unsafe { crate::sexp::symbol::Rf_install(c"span".as_ptr()) })
            .unwrap();
        let args = f.pairlist_cell(&span, &nil, &tag).unwrap();
        let args = f.pairlist_cell(&y, &args, &nil).unwrap();
        let args = f
            .pairlist_cell(&x, &args, &nil)
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
    assert!(notifications.get() > 0, "actual collection callback");
    assert_eq!(
        calls.get(),
        4,
        "GNU comparison reads precede final numeric coercion read"
    );
    if close_on > 0 {
        assert!(result.is_err(), "revoked owner must deny publication");
    } else {
        let f = owner.node_factory().unwrap();
        let result = f.wrap(result.unwrap()).unwrap().into_owned().unwrap();
        let y = result.try_vector_elt(1).unwrap().into_owned().unwrap();
        drop(args);
        drop(result);
        sessions
            .borrow()
            .as_ref()
            .unwrap()
            .with_active(crate::sexp::gengc::full_gc);
        let row = include_str!("../../fixtures/supsmu-span-gnu-r90451.tsv")
            .lines()
            .nth(1)
            .unwrap();
        for (i, e) in row.split('\t').nth(4).unwrap().split(',').enumerate() {
            let expected: f64 = e.parse().unwrap();
            assert!((y.try_real_elt(i as R_xlen_t).unwrap() - expected).abs() < 1e-10);
        }
    }
}
#[test]
fn supsmu_public_character_span_detachment_and_collection_preserves_original_inputs() {
    collecting_span(0);
}
#[test]
fn supsmu_public_character_span_original_close_at_coercion_denies_publication() {
    collecting_span(4);
}
