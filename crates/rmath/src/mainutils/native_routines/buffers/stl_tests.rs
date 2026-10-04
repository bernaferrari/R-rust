//! Pinned GNU STL outputs, including all scalar/vector sentinel tails.
use super::{BufferInterface, NativeBuffer};
fn buffer(kind: &str, values: &str) -> NativeBuffer {
    let v = || values.split(',').filter(|x| !x.is_empty());
    match kind {
        "double" => NativeBuffer::Real(v().map(|x| x.parse().unwrap()).collect()),
        "integer" => NativeBuffer::Integer(v().map(|x| x.parse().unwrap()).collect()),
        _ => panic!("unexpected GNU buffer type"),
    }
}
fn cases() -> std::collections::BTreeMap<&'static str, Vec<(NativeBuffer, NativeBuffer)>> {
    let mut cases = std::collections::BTreeMap::<_, Vec<_>>::new();
    for row in include_str!("fixtures/stl-gnu-r90451.tsv").lines().skip(1) {
        let f: Vec<_> = row.split('\t').collect();
        assert_eq!(f.len(), 5);
        let v = cases.entry(f[0]).or_default();
        assert_eq!(f[1].parse::<usize>().unwrap(), v.len());
        v.push((buffer(f[2], f[3]), buffer(f[2], f[4])));
    }
    cases
}
#[test]
fn stl_registered_matches_all_independent_gnu_buffers_and_unread_branches() {
    let routine =
        crate::library::tools::native_calls::lookup_buffer("stl").expect("GNU STL17 must resolve");
    assert_eq!(routine.arity(), 17);
    assert_eq!(routine.interface(), BufferInterface::Fortran);
    assert_eq!(routine.package(), "stats");
    let cases = cases();
    assert_eq!(cases.len(), 9);
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
                        if index == 0 || b.to_bits() == original[index][i] {
                            assert_eq!(
                                a.to_bits(),
                                b.to_bits(),
                                "{case} unchanged argument{index} element{i}"
                            );
                        }
                        assert!(
                            (a - b).abs() < 2e-10 * b.abs().max(1.),
                            "{case} argument{index} element{i}: {a} != GNU{b}"
                        );
                    }
                }
                _ => panic!("GNU buffer type mismatch"),
            }
        }
    }
}

fn additive() -> Vec<NativeBuffer> {
    cases()
        .remove("additive")
        .unwrap()
        .into_iter()
        .map(|(a, _)| a)
        .collect()
}
#[test]
fn stl_admission_rejects_every_wrong_type_dimension_and_workspace_before_invocation() {
    let routine = super::stl::ROUTINE;
    let before = super::invocation_count();
    let reject = |mut v: Vec<NativeBuffer>, interface| {
        let original = format!("{v:?}");
        assert!(routine.invoke(interface, &mut v).is_err());
        assert_eq!(format!("{v:?}"), original);
        assert_eq!(super::invocation_count(), before);
    };
    reject(additive(), BufferInterface::C);
    let mut short = additive();
    short.pop();
    reject(short, BufferInterface::Fortran);
    let mut long = additive();
    long.push(NativeBuffer::Integer(vec![1]));
    reject(long, BufferInterface::Fortran);
    for index in 0..17 {
        let mut v = additive();
        v[index] = match v[index] {
            NativeBuffer::Real(_) => NativeBuffer::Integer(vec![1]),
            _ => NativeBuffer::Real(vec![1.]),
        };
        reject(v, BufferInterface::Fortran);
    }
    for (index, value) in [
        (0, NativeBuffer::Real(vec![])),
        (1, NativeBuffer::Integer(vec![-1])),
        (1, NativeBuffer::Integer(vec![i32::MAX])),
        (2, NativeBuffer::Integer(vec![i32::MAX])),
        (9, NativeBuffer::Integer(vec![0])),
        (10, NativeBuffer::Integer(vec![-1])),
        (11, NativeBuffer::Integer(vec![0])),
        (14, NativeBuffer::Real(vec![0.; 23])),
        (15, NativeBuffer::Real(vec![0.; 23])),
        (16, NativeBuffer::Real(vec![0.; 23])),
    ] {
        let mut v = additive();
        v[index] = value;
        reject(v, BufferInterface::Fortran);
    }
    // Missing scalar buffers are not unread: GNU passes their dereferenced values to stlstp.
    for index in 1..14 {
        let mut v = additive();
        v[index] = NativeBuffer::Integer(vec![]);
        reject(v, BufferInterface::Fortran);
    }
    let mut v = additive();
    v[1] = NativeBuffer::Integer(vec![3]);
    reject(v, BufferInterface::Fortran);
    let mut v: Vec<_> = cases()
        .remove("zero-size")
        .unwrap()
        .into_iter()
        .map(|(a, _)| a)
        .collect();
    v[13] = NativeBuffer::Integer(vec![1]);
    reject(v, BufferInterface::Fortran);
}
mod managed;
