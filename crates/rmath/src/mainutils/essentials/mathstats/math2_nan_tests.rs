use crate::sexp::{
    RSession,
    ffi::{R_IsNA, R_NA_BIT_PATTERN, SEXPTYPE},
    object::{SessionNodeFactory, Sexp, SexpMut},
};
fn reals(f: &SessionNodeFactory<'_>, values: &[f64]) -> Sexp<'static> {
    let value = f
        .allocate(|a| {
            a.alloc_vector_sexp(SEXPTYPE::REALSXP, values.len() as i64)
                .map(|v| v.as_raw())
        })
        .unwrap()
        .into_owned()
        .unwrap();
    let mut value = SexpMut::try_from_checked(value).unwrap();
    for (i, &v) in values.iter().enumerate() {
        value.try_set_real_elt(i as i64, v).unwrap();
    }
    value.freeze()
}
#[test]
fn math2_missing_operands_skip_scalar_calls_and_distinguish_quiet_na_neighbors() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let quiet = f64::from_bits(R_NA_BIT_PATTERN | (1 << 51));
        let neighbor = f64::from_bits((R_NA_BIT_PATTERN | (1 << 51)) ^ 1);
        assert!(R_IsNA(quiet));
        assert!(!R_IsNA(neighbor));
        let a = reals(&f, &[quiet, 1., 2., neighbor, 1., 1., 1.]);
        let b = reals(&f, &[f64::NAN, quiet, f64::NAN, 0., neighbor, 0., 1.]);
        let calls = std::cell::Cell::new(0);
        let raw = unsafe {
            super::apply_binary_scalar_fn(f.nil().as_raw(), a.as_raw(), b.as_raw(), |x, y| {
                calls.set(calls.get() + 1);
                x + y
            })
        };
        let output = f.wrap(raw).unwrap().into_owned().unwrap();
        assert_eq!(calls.get(), 2);
        assert!(R_IsNA(output.try_real_elt(0).unwrap()));
        assert!(R_IsNA(output.try_real_elt(1).unwrap()));
        for i in 2..5 {
            let value = output.try_real_elt(i).unwrap();
            assert!(value.is_nan());
            assert!(!R_IsNA(value));
        }
        assert_eq!(output.try_real_elt(5), Ok(1.));
        assert_eq!(output.try_real_elt(6), Ok(2.));
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert_eq!(output.try_real_elt(6), Ok(2.));
    });
}
#[test]
fn math2_public_native_psigamma_nan_derivative_is_not_order_zero() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let x = reals(&f, &[1.]);
        let deriv = reals(&f, &[f64::NAN, 0., 1., 2.]);
        let tail = f.pairlist_cell(&deriv, &f.nil(), &f.nil()).unwrap();
        let args = f.pairlist_cell(&x, &tail, &f.nil()).unwrap();
        let raw = unsafe {
            super::do_psigamma(
                f.nil().as_raw(),
                f.nil().as_raw(),
                args.as_raw(),
                f.nil().as_raw(),
            )
        };
        let output = f.wrap(raw).unwrap().into_owned().unwrap();
        assert!(output.try_real_elt(0).unwrap().is_nan());
        assert!((output.try_real_elt(1).unwrap() + 0.5772156649015329).abs() < 1e-14);
        assert!((output.try_real_elt(2).unwrap() - 1.6449340668482264).abs() < 1e-14);
        assert!((output.try_real_elt(3).unwrap() + 2.4041138063191885).abs() < 1e-13);
    });
}
