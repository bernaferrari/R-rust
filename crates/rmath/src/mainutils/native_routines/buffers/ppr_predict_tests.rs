//! Genuine managed .Fortran entry versus independently pinned packed models.
mod managed;
use super::{BufferInterface, NativeBuffer, invocation_count};
use crate::sexp::{R_xlen_t, RSession, SEXPTYPE, Sexp, SexpMut, object::SessionNodeFactory};

fn value(f: &SessionNodeFactory<'_>, b: NativeBuffer) -> Sexp<'static> {
    let kind = match &b {
        NativeBuffer::Integer(_) => SEXPTYPE::INTSXP,
        _ => SEXPTYPE::REALSXP,
    };
    let n = f
        .allocate(|a| {
            a.alloc_vector_sexp(kind, b.len() as R_xlen_t)
                .map(|n| n.as_raw())
        })
        .unwrap()
        .into_owned()
        .unwrap();
    let mut n = SexpMut::try_from_checked(n).unwrap();
    match b {
        NativeBuffer::Integer(v) => {
            for (i, x) in v.into_iter().enumerate() {
                n.try_set_integer_elt(i as R_xlen_t, x).unwrap();
            }
        }
        NativeBuffer::Real(v) => {
            for (i, x) in v.into_iter().enumerate() {
                n.try_set_real_elt(i as R_xlen_t, x).unwrap();
            }
        }
        NativeBuffer::Character(_) => unreachable!(),
    }
    n.freeze()
}
fn data(case: &str) -> Vec<(NativeBuffer, NativeBuffer)> {
    let read = |kind: &str, values: &str| match kind {
        "integer" => NativeBuffer::Integer(
            values
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| {
                    let x = s.parse::<f64>().unwrap();
                    assert!(
                        x.is_finite()
                            && x.fract() == 0.
                            && x >= f64::from(i32::MIN)
                            && x <= f64::from(i32::MAX)
                    );
                    x as i32
                })
                .collect(),
        ),
        "double" => NativeBuffer::Real(
            values
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.parse().unwrap())
                .collect(),
        ),
        _ => panic!("unexpected independent GNU type {kind}"),
    };
    include_str!("fixtures/ppr-predict-gnu-r90451.tsv")
        .lines()
        .skip(1)
        .filter_map(|r| {
            let f: Vec<_> = r.split('\t').collect();
            (f[0] == case).then(|| (read(f[2], f[3]), read(f[2], f[4])))
        })
        .collect()
}
#[test]
fn ppr_prediction_managed_fortran_matches_gnu_zero_term_short_model() {
    let session = RSession::new_for_gc_tests();
    let f = session.owner_token().unwrap().node_factory();
    let nil = f.nil().into_owned().unwrap();
    let expected = data("zero-terms-short");
    assert_eq!(expected.len(), 5);
    let mut tail = nil.clone();
    let package = f.strings(&["stats"]).unwrap().into_owned().unwrap();
    let tag = f
        .wrap(unsafe { crate::sexp::symbol::Rf_install(c"PACKAGE".as_ptr()) })
        .unwrap();
    tail = f
        .pairlist_cell(&package, &tail, &tag)
        .unwrap()
        .into_owned()
        .unwrap();
    let mut wanted = Vec::new();
    for (input, output) in expected.into_iter().rev() {
        let input = value(&f, input);
        tail = f
            .pairlist_cell(&input, &tail, &nil)
            .unwrap()
            .into_owned()
            .unwrap();
        wanted.push(output);
    }
    wanted.reverse();
    let name = f.strings(&["pppred"]).unwrap();
    let args = f
        .pairlist_cell(&name, &tail, &nil)
        .unwrap()
        .into_owned()
        .unwrap();
    let op = f
        .wrap(unsafe {
            crate::eval::primitive::make_primitive_binding(".Fortran", SEXPTYPE::BUILTINSXP)
        })
        .unwrap()
        .into_owned()
        .unwrap();
    let before = invocation_count();
    let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::mainutils::dotcode::do_dotCode(
            nil.as_raw(),
            op.as_raw(),
            args.as_raw(),
            nil.as_raw(),
        )
    }))
    .unwrap_or_else(|e| {
        panic!(
            "GNU-valid packed prediction must succeed: {}",
            e.downcast_ref::<crate::sexp::context::RError>()
                .map_or("unexpected panic", |e| e.message.as_str())
        )
    });
    let output = f.wrap(output).unwrap().into_owned().unwrap();
    assert_eq!(invocation_count(), before + 1);
    assert_eq!(output.len(), 5);
    for (i, expected) in wanted.into_iter().enumerate() {
        let actual = output.try_vector_elt(i as R_xlen_t).unwrap();
        assert_eq!(actual.len(), expected.len() as R_xlen_t);
        match expected {
            NativeBuffer::Integer(v) => {
                for (j, x) in v.into_iter().enumerate() {
                    assert_eq!(actual.try_integer_elt(j as R_xlen_t).unwrap(), x);
                }
            }
            NativeBuffer::Real(v) => {
                for (j, x) in v.into_iter().enumerate() {
                    let a = actual.try_real_elt(j as R_xlen_t).unwrap();
                    assert!((a - x).abs() < 1e-10, "buffer{i}:{j}: {a} vs GNU{x}");
                }
            }
            NativeBuffer::Character(_) => unreachable!(),
        }
    }
}

fn copy_buffer(b: &NativeBuffer) -> NativeBuffer {
    match b {
        NativeBuffer::Integer(v) => NativeBuffer::Integer(v.clone()),
        NativeBuffer::Real(v) => NativeBuffer::Real(v.clone()),
        NativeBuffer::Character(_) => unreachable!(),
    }
}
fn check(actual: &[NativeBuffer], expected: &[NativeBuffer]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(a.len(), e.len());
        match (a, e) {
            (NativeBuffer::Integer(a), NativeBuffer::Integer(e)) => assert_eq!(a, e),
            (NativeBuffer::Real(a), NativeBuffer::Real(e)) => {
                for (j, (a, e)) in a.iter().zip(e).enumerate() {
                    assert!(
                        (a - e).abs() < 1e-10,
                        "PPR buffer{i}:{j}: {a:.17e} vs GNU{e:.17e}"
                    );
                }
            }
            _ => panic!("PPR output type changed"),
        }
    }
}
#[test]
fn ppr_prediction_registered_matches_gnu_all_five_buffers_and_sorting_ties() {
    let routine = super::ppr_predict::ROUTINE;
    for case in [
        "trained",
        "zero-observations",
        "shared-model-workspace",
        "multi-response",
        "zero-terms-full",
        "zero-terms-short",
        "singleton-curve",
        "tied-projections",
        "descending-long",
    ] {
        let (mut input, expected): (Vec<_>, Vec<_>) = data(case).into_iter().unzip();
        routine
            .invoke(BufferInterface::Fortran, &mut input)
            .unwrap_or_else(|e| panic!("{case}: {e}"));
        check(&input, &expected);
    }
}
#[test]
fn ppr_prediction_admission_rejects_bad_models_without_invocation_or_writes() {
    let routine = super::ppr_predict::ROUTINE;
    let before = invocation_count();
    let base: Vec<_> = data("trained").into_iter().map(|(x, _)| x).collect();
    let clone = || base.iter().map(copy_buffer).collect::<Vec<_>>();
    let reject = |mut b: Vec<NativeBuffer>, interface| {
        let original = format!("{b:?}");
        assert!(routine.invoke(interface, &mut b).is_err());
        assert_eq!(format!("{b:?}"), original);
        assert_eq!(invocation_count(), before);
    };
    reject(clone(), BufferInterface::C);
    let mut b = clone();
    b.pop();
    reject(b, BufferInterface::Fortran);
    let mut b = clone();
    b.push(NativeBuffer::Real(Vec::new()));
    reject(b, BufferInterface::Fortran);
    for i in 0..5 {
        let mut b = clone();
        b[i] = if i == 0 {
            NativeBuffer::Real(vec![5.])
        } else {
            NativeBuffer::Integer(vec![5])
        };
        reject(b, BufferInterface::Fortran);
        let mut b = clone();
        b[i] = if i == 0 {
            NativeBuffer::Integer(Vec::new())
        } else {
            NativeBuffer::Real(Vec::new())
        };
        reject(b, BufferInterface::Fortran);
    }
    for (index, value) in [
        (0, f64::NAN),
        (0, -2.),
        (1, f64::INFINITY),
        (2, 1e30),
        (3, 0.),
        (4, 2.),
    ] {
        let mut b = clone();
        let NativeBuffer::Real(model) = &mut b[2] else {
            unreachable!()
        };
        model[index] = value;
        reject(b, BufferInterface::Fortran);
    }
    let mut b = clone();
    b[0] = NativeBuffer::Integer(vec![-1]);
    reject(b, BufferInterface::Fortran);
    let mut b = clone();
    let NativeBuffer::Real(x) = &mut b[1] else {
        unreachable!()
    };
    x[0] = f64::NAN;
    reject(b, BufferInterface::Fortran);
    // Even finite inputs can create Inf-Inf; reject this unordered projection
    // before model sorting rather than allowing an out-of-range binary search.
    let mut b: Vec<_> = data("multi-response").into_iter().map(|(x, _)| x).collect();
    let NativeBuffer::Real(model) = &mut b[2] else {
        unreachable!()
    };
    let base = model[2] as usize + 6;
    model[base] = f64::MAX;
    model[base + 1] = -f64::MAX;
    let NativeBuffer::Real(x) = &mut b[1] else {
        unreachable!()
    };
    x[0] = f64::MAX;
    x[5] = f64::MAX;
    reject(b, BufferInterface::Fortran);
}

#[test]
fn ppr_prediction_zero_terms_accepts_gnu_unread_empty_inputs_and_workspace() {
    let mut b: Vec<_> = data("zero-terms-short")
        .into_iter()
        .map(|(x, _)| x)
        .collect();
    b[1] = NativeBuffer::Real(Vec::new());
    b[4] = NativeBuffer::Real(Vec::new());
    super::ppr_predict::ROUTINE
        .invoke(BufferInterface::Fortran, &mut b)
        .unwrap();
    let NativeBuffer::Real(y) = &b[3] else {
        unreachable!()
    };
    assert_eq!(&y[..5], &[3.; 5]);
    assert_eq!(y[5], 3.5);
    assert_eq!(b[1].len(), 0);
    assert_eq!(b[4].len(), 0);
}
