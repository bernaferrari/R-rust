use super::*;
use crate::sexp::{RSession, memory::ArenaBudget};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn real_eigenvectors_unpack_into_validates_before_writing() {
    let marker = crate::sexp::ffi::Rcomplex { r: 31., i: 41. };
    let packed = [1., 2., 3., 4., 5., 6., 7., 8., 9.];
    let mut output = [marker; 10];
    assert_eq!(
        unscramble_into(&[0., 1., -1.], 3, &packed, &mut output),
        Some(())
    );
    let expected = [
        (1., 0.),
        (2., 0.),
        (3., 0.),
        (4., 7.),
        (5., 8.),
        (6., 9.),
        (4., -7.),
        (5., -8.),
        (6., -9.),
        (31., 41.),
    ];
    for (actual, expected) in output.iter().zip(expected) {
        assert_eq!((actual.r, actual.i), expected);
    }
    for (imaginary, n, values, out_len) in [
        (&[0., 1.][..], 3, &packed[..], 9),
        (&[0., 1., -1.][..], 3, &packed[..8], 9),
        (&[0., 1., -1.][..], 3, &packed[..], 8),
        (&[0., 0., 1.][..], 3, &packed[..], 9),
        (&[][..], usize::MAX, &[][..], 0),
    ] {
        let mut output = [marker; 9];
        assert_eq!(
            unscramble_into(imaginary, n, values, &mut output[..out_len]),
            None
        );
        assert!(output.iter().all(|z| z.r == marker.r && z.i == marker.i));
    }
    assert_eq!(unscramble_into(&[], 0, &[], &mut []), Some(()));
}

#[test]
fn real_eigenvectors_managed_result_denial_retry_and_gc() {
    let mut session = RSession::new_for_gc_tests();
    let imaginary = [1., -1.];
    let packed = [1., 2., 3., 4.];
    let warm = session.with_active(|| unsafe {
        let result = complex_eigenvector_matrix(2, imaginary.as_ptr(), packed.as_ptr());
        session.sexp(result).unwrap().into_owned().unwrap()
    });
    let held = session.with_active(|| unsafe {
        with_current_instance(|instance| {
            with_arena_in(instance, |arena| {
                let held = arena.workspace_bytes();
                arena.set_budget(ArenaBudget::new(1, 0));
                held
            })
        })
        .unwrap()
    });
    let failure = session.with_active(|| {
        catch_unwind(AssertUnwindSafe(|| unsafe {
            complex_eigenvector_matrix(2, imaginary.as_ptr(), packed.as_ptr());
        }))
    });
    let error = failure
        .unwrap_err()
        .downcast::<crate::sexp::context::RError>()
        .unwrap();
    assert!(
        error.message.contains("allocation") || error.message.contains("budget"),
        "{}",
        error.message
    );
    session.set_arena_budget(ArenaBudget::unlimited());
    let result = session.with_active(|| unsafe {
        assert_eq!(
            with_current_instance(
                |instance| with_arena_in(instance, |arena| arena.workspace_bytes())
            )
            .unwrap(),
            held
        );
        let result = complex_eigenvector_matrix(2, imaginary.as_ptr(), packed.as_ptr());
        session.sexp(result).unwrap().into_owned().unwrap()
    });
    drop(warm);
    session.gc();
    assert_eq!(result.len(), 4);
    for (index, expected) in [(1., 3.), (2., 4.), (1., -3.), (2., -4.)]
        .into_iter()
        .enumerate()
    {
        let actual = result.try_complex_elt(index as _).unwrap();
        assert_eq!((actual.r, actual.i), expected);
    }
}

#[test]
#[cfg(not(miri))]
fn real_eigenvectors_la_rg_conjugate_numerical_parity() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let x = Rf_allocVector(REALSXP_C, 4);
        let _x = protect(x);
        for (i, value) in [0., -1., 1., 0.].into_iter().enumerate() {
            *REAL(x).add(i) = value;
        }
        let dim = Rf_allocVector(INTSXP_C, 2);
        let _dim = protect(dim);
        *INTEGER(dim) = 2;
        *INTEGER(dim).add(1) = 2;
        setAttrib(x, R_DimSymbol(), dim);
        let only_values = Rf_ScalarLogical(0);
        let _only_values = protect(only_values);
        let result = La_rg(x, only_values);
        let _result = protect(result);
        let values = VECTOR_ELT(result, 0);
        let vectors = VECTOR_ELT(result, 1);
        assert_eq!(TYPEOF(values), CPLXSXP_C);
        assert_eq!(XLENGTH(values), 2);
        assert_eq!(XLENGTH(vectors), 4);
        let a = [0., -1., 1., 0.];
        for j in 0..2 {
            let lambda = *COMPLEX(values).add(j);
            assert!(lambda.r.abs() < 1e-12 && (lambda.i.abs() - 1.).abs() < 1e-12);
            let mut norm = 0.;
            for i in 0..2 {
                let value = *COMPLEX(vectors).add(i + j * 2);
                norm += value.r * value.r + value.i * value.i;
                let mut ar = 0.;
                let mut ai = 0.;
                for k in 0..2 {
                    let v = *COMPLEX(vectors).add(k + j * 2);
                    ar += a[i + k * 2] * v.r;
                    ai += a[i + k * 2] * v.i;
                }
                let expected = (
                    lambda.r * value.r - lambda.i * value.i,
                    lambda.r * value.i + lambda.i * value.r,
                );
                assert!((ar - expected.0).abs() < 1e-12 && (ai - expected.1).abs() < 1e-12);
            }
            assert!(norm > 0.1);
        }
    });
}
