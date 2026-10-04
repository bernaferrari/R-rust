//! Independent pinned GNU supersmoother buffer results.
use super::{BufferInterface, NativeBuffer};
fn buffer(kind: &str, values: &str) -> NativeBuffer {
    let values = || values.split(',').filter(|x| !x.is_empty());
    match kind {
        "integer" => NativeBuffer::Integer(values().map(|x| x.parse().unwrap()).collect()),
        "double" => NativeBuffer::Real(values().map(|x| x.parse().unwrap()).collect()),
        _ => panic!("unknown GNU type"),
    }
}
fn cases() -> std::collections::BTreeMap<&'static str, Vec<(NativeBuffer, NativeBuffer)>> {
    let mut result = std::collections::BTreeMap::<_, Vec<_>>::new();
    for row in include_str!("fixtures/supsmu-gnu-r90451.tsv")
        .lines()
        .skip(1)
    {
        let f: Vec<_> = row.split('\t').collect();
        assert_eq!(f.len(), 5);
        let v = result.entry(f[0]).or_default();
        assert_eq!(v.len(), f[1].parse::<usize>().unwrap());
        v.push((buffer(f[2], f[3]), buffer(f[2], f[4])));
    }
    result
}
#[test]
fn supsmu_registered_matches_independent_gnu_all_buffers_and_unread_branches() {
    let routine = crate::library::tools::native_calls::lookup_buffer("supsmu")
        .expect("GNU supersmoother10 must resolve");
    assert_eq!(routine.arity(), 10);
    assert_eq!(routine.interface(), BufferInterface::Fortran);
    assert_eq!(routine.package(), "stats");
    let cases = cases();
    assert_eq!(cases.len(), 11);
    for (case, v) in cases {
        let (mut input, expected): (Vec<_>, Vec<_>) = v.into_iter().unzip();
        let original: Vec<Vec<u64>> = input
            .iter()
            .map(|v| match v {
                NativeBuffer::Real(v) => v.iter().map(|x| x.to_bits()).collect(),
                _ => vec![],
            })
            .collect();
        routine
            .invoke(BufferInterface::Fortran, &mut input)
            .unwrap();
        for (index, (actual, expected)) in input.iter().zip(expected).enumerate() {
            match (actual, expected) {
                (NativeBuffer::Integer(a), NativeBuffer::Integer(b)) => assert_eq!(a, &b),
                (NativeBuffer::Real(a), NativeBuffer::Real(b)) => {
                    assert_eq!(a.len(), b.len());
                    for (i, (a, b)) in a.iter().zip(b).enumerate() {
                        if index <= 6 || b.to_bits() == original[index][i] {
                            assert_eq!(a.to_bits(), b.to_bits(), "{case} unchanged{index}:{i}");
                        }
                        assert!(
                            (a - b).abs() < 2e-10 * b.abs().max(1.),
                            "{case} buffer{index}:{i} {a} != GNU{b}"
                        );
                    }
                }
                _ => panic!("wrong output type"),
            }
        }
    }
}

fn automatic() -> Vec<NativeBuffer> {
    cases()
        .remove("cv")
        .unwrap()
        .into_iter()
        .map(|(a, _)| a)
        .collect()
}
/// Mirror the real adapter's repeated-source snapshot before testing callbacks.
#[test]
fn supsmu_same_cv_alias_snapshot_matches_gnu_directly() {
    let mut input = automatic();
    let scratch = input[8].reals().unwrap().to_vec();
    input[7] = NativeBuffer::Real(scratch.clone());
    input[9] = NativeBuffer::Real(scratch.clone());
    super::supsmu::ROUTINE
        .invoke(BufferInterface::Fortran, &mut input)
        .unwrap();
    let expected = cases().remove("cv").unwrap();
    for (index, count) in [(7, 24), (8, 168), (9, 1)] {
        let actual = input[index].reals().unwrap();
        let values = expected[index].1.reals().unwrap();
        for (i, v) in values.iter().take(count).enumerate() {
            assert!(
                (actual[i] - v).abs() < 1e-10,
                "CV buffer{index}:{i}: actual{:.17e} != GNU{v:.17e}, difference{:.17e}",
                actual[i],
                (actual[i] - v).abs()
            );
        }
        for (a, original) in actual[count..].iter().zip(&scratch[count..]) {
            assert_eq!(
                a.to_bits(),
                original.to_bits(),
                "unread CV buffer{index} tail"
            );
        }
    }
}

#[test]
fn supsmu_admission_rejects_types_shapes_and_invalid_math_before_invocation() {
    let routine = super::supsmu::ROUTINE;
    let before = super::invocation_count();
    let reject = |mut b: Vec<NativeBuffer>, interface| {
        let original = format!("{b:?}");
        assert!(routine.invoke(interface, &mut b).is_err());
        assert_eq!(format!("{b:?}"), original);
        assert_eq!(super::invocation_count(), before);
    };
    let baseline = automatic();
    let clone = || {
        baseline
            .iter()
            .map(|b| match b {
                NativeBuffer::Real(v) => NativeBuffer::Real(v.clone()),
                NativeBuffer::Integer(v) => NativeBuffer::Integer(v.clone()),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>()
    };
    reject(clone(), BufferInterface::C);
    let mut b = clone();
    b.pop();
    reject(b, BufferInterface::Fortran);
    let mut b = clone();
    b.push(NativeBuffer::Real(vec![]));
    reject(b, BufferInterface::Fortran);
    for i in 0..10 {
        let mut b = clone();
        b[i] = match b[i] {
            NativeBuffer::Real(_) => NativeBuffer::Integer(vec![1]),
            _ => NativeBuffer::Real(vec![1.]),
        };
        reject(b, BufferInterface::Fortran);
    }
    for (i, v) in [
        (0, NativeBuffer::Integer(vec![0])),
        (0, NativeBuffer::Integer(vec![-1])),
        (0, NativeBuffer::Integer(vec![3])),
        (1, NativeBuffer::Real(vec![])),
        (2, NativeBuffer::Real(vec![])),
        (3, NativeBuffer::Real(vec![])),
        (4, NativeBuffer::Integer(vec![])),
        (5, NativeBuffer::Real(vec![])),
        (6, NativeBuffer::Real(vec![])),
        (7, NativeBuffer::Real(vec![])),
        (8, NativeBuffer::Real(vec![0.; 167])),
        (9, NativeBuffer::Real(vec![])),
    ] {
        let mut b = clone();
        b[i] = v;
        reject(b, BufferInterface::Fortran);
    }
    let mut b = clone();
    b[4] = NativeBuffer::Integer(vec![2]);
    b[5] = NativeBuffer::Real(vec![3.]);
    reject(b, BufferInterface::Fortran);
    let mut b = clone();
    b[1] = NativeBuffer::Real(vec![f64::NAN; 24]);
    reject(b, BufferInterface::Fortran);
    let mut b = clone();
    if let NativeBuffer::Real(x) = &mut b[1] {
        x.swap(1, 2);
    }
    reject(b, BufferInterface::Fortran);
}
mod managed;

mod public;
