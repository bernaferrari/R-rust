//! Owned interpolation dispatch: no numerical pointer conversion or model cache.
#![forbid(unsafe_code)]
use super::*;
use crate::library::stats::loess::{
    Execution,
    pruned::{self, PrunedSurface},
};

pub(crate) const ROUTINE: BufferRoutine = BufferRoutine::owned(
    "stats",
    BufferInterface::C,
    &[
        BufferType::Integer,
        BufferType::Integer,
        BufferType::Real,
        BufferType::Real,
        BufferType::Real,
        BufferType::Integer,
        BufferType::Real,
        BufferType::Real,
    ],
    validate,
    invoke,
);

fn validate(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let m = dimension(b, 5, 0)?;
    let shape = pruned::shape(b[0].integers()?).map_err(BufferError::new)?;
    pruned::admit_predictions(&shape, m, &Execution::new(&|| Ok(()))).map_err(BufferError::new)?;
    minimum(b, 1, shape.nc)?;
    minimum(b, 2, shape.nc)?;
    minimum(b, 3, product(&[2, shape.d])?)?;
    minimum(b, 4, product(&[shape.d + 1, shape.nv])?)?;
    minimum(b, 6, product(&[m, shape.d])?)?;
    minimum(b, 7, m)
}

fn invoke(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    let m = dimension(b, 5, 0)?;
    let parameter = b[0].integers()?;
    let shape = pruned::shape(parameter).map_err(BufferError::new)?;
    let x = b[6].reals()?;
    let queries: Vec<_> = (0..m)
        .map(|i| (0..shape.d).map(|j| x[i + j * m]).collect())
        .collect();
    let check = || {
        if crate::sexp::instance::is_cancellation_requested() {
            Err("operation cancelled".into())
        } else {
            Ok(())
        }
    };
    let values = PrunedSurface::predict(
        parameter,
        b[1].integers()?,
        b[2].reals()?,
        b[3].reals()?,
        b[4].reals()?,
        &queries,
        &Execution::new(&check),
    )
    .map_err(BufferError::new)?;
    let NativeBuffer::Real(output) = &mut b[7] else {
        unreachable!("admitted real result")
    };
    output[..m].copy_from_slice(&values);
    Ok(())
}

#[cfg(test)]
mod tests {
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
    #[test]
    fn owned_loess_ifit_decodes_gnu_surfaces_and_preserves_output_tails() {
        for case in cases() {
            let mut b = buffers(&case);
            ROUTINE.invoke(BufferInterface::C, &mut b).unwrap();
            let output = b[7].reals().unwrap();
            for (actual, expected) in output[..3].iter().zip(case["fit"].as_array().unwrap()) {
                let expected = expected.as_f64().unwrap();
                assert!((actual - expected).abs() < 2e-10 * (1. + expected.abs()));
            }
            assert_eq!(&output[3..], &[99., 99.]);
        }
    }
    #[test]
    fn owned_loess_ifit_rejects_truncation_before_invocation() {
        let case = cases().remove(0);
        for index in 0..8 {
            let mut b = buffers(&case);
            match &mut b[index] {
                NativeBuffer::Integer(v) => v.clear(),
                NativeBuffer::Real(v) => v.clear(),
                NativeBuffer::Character(_) => unreachable!(),
            }
            let before = invocation_count();
            assert!(
                ROUTINE.invoke(BufferInterface::C, &mut b).is_err(),
                "argument {index}"
            );
            assert_eq!(invocation_count(), before);
        }
    }
    #[test]
    fn owned_loess_ifit_invalid_tree_does_not_publish_partial_output() {
        let case = cases().remove(0);
        let mut b = buffers(&case);
        let NativeBuffer::Integer(a) = &mut b[1] else {
            unreachable!()
        };
        a[0] = 9;
        assert!(ROUTINE.invoke(BufferInterface::C, &mut b).is_err());
        assert_eq!(b[7].reals().unwrap(), &[99.; 5]);
    }
}
