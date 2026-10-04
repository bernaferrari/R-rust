//! Independent GNU r90451 .C output records, preserving every input tail.
use super::{BufferInterface, NativeBuffer};

fn buffer(kind: &str, values: &str) -> NativeBuffer {
    let elements = || values.split(',');
    match kind {
        "integer" => NativeBuffer::Integer(elements().map(|v| v.parse().unwrap()).collect()),
        "double" => NativeBuffer::Real(elements().map(|v| v.parse().unwrap()).collect()),
        _ => panic!("unexpected independent GNU element type"),
    }
}

#[test]
fn multi_burg_registered_matches_independent_gnu_all_eleven_buffers() {
    let routine = crate::library::tools::native_calls::lookup_buffer("multi_burg")
        .expect("GNU's eleven-argument multivariate Burg routine must resolve");
    assert_eq!(routine.package(), "stats");
    assert_eq!(routine.interface(), BufferInterface::C);
    assert_eq!(routine.arity(), 11);
    let mut cases = std::collections::BTreeMap::<_, Vec<_>>::new();
    for row in include_str!("fixtures/multi-burg-gnu-r90451.tsv")
        .lines()
        .skip(1)
    {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 5);
        let entries = cases.entry(fields[0]).or_default();
        assert_eq!(fields[1].parse::<usize>().unwrap(), entries.len());
        entries.push((buffer(fields[2], fields[3]), buffer(fields[2], fields[4])));
    }
    assert_eq!(cases.len(), 8);
    for (case, entries) in cases {
        assert_eq!(entries.len(), 11);
        let (mut input, expected): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
        routine.invoke(BufferInterface::C, &mut input).unwrap();
        for (index, (actual, expected)) in input.iter().zip(&expected).enumerate() {
            match (actual, expected) {
                (NativeBuffer::Integer(actual), NativeBuffer::Integer(expected)) => {
                    assert_eq!(actual, expected, "{case}, argument {index}")
                }
                (NativeBuffer::Real(actual), NativeBuffer::Real(expected)) => {
                    assert_eq!(actual.len(), expected.len());
                    for (element, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                        let tolerance = 2e-10 * expected.abs().max(1.0);
                        assert!(
                            (actual - expected).abs() <= tolerance,
                            "{case}, argument {index}, element {element}: actual {actual}, GNU {expected}"
                        );
                    }
                }
                _ => panic!("{case}, argument {index}: element type changed"),
            }
        }
    }
}

fn scalar_case() -> Vec<NativeBuffer> {
    include_str!("fixtures/multi-burg-gnu-r90451.tsv")
        .lines()
        .skip(1)
        .filter_map(|row| {
            let f: Vec<_> = row.split('\t').collect();
            (f[0] == "scalar-v1").then(|| buffer(f[2], f[3]))
        })
        .collect()
}
#[test]
fn multi_burg_admission_rejects_every_wrong_type_and_workspace_before_invocation() {
    let routine = super::multi_burg::ROUTINE;
    let before = super::invocation_count();
    let reject = |mut values: Vec<NativeBuffer>, interface| {
        let original = format!("{values:?}");
        assert!(routine.invoke(interface, &mut values).is_err());
        assert_eq!(format!("{values:?}"), original);
        assert_eq!(super::invocation_count(), before);
    };
    reject(scalar_case(), BufferInterface::Fortran);
    let mut short = scalar_case();
    short.pop();
    reject(short, BufferInterface::C);
    let mut long = scalar_case();
    long.push(NativeBuffer::Integer(vec![1]));
    reject(long, BufferInterface::C);
    for index in 0..11 {
        let mut values = scalar_case();
        values[index] = match values[index] {
            NativeBuffer::Integer(_) => NativeBuffer::Real(vec![1.]),
            _ => NativeBuffer::Integer(vec![1]),
        };
        reject(values, BufferInterface::C);
    }
    for (index, value) in [
        (0, NativeBuffer::Integer(vec![0])),
        (0, NativeBuffer::Integer(vec![-1])),
        (1, NativeBuffer::Real(vec![0.; 11])),
        (2, NativeBuffer::Integer(vec![12])),
        (3, NativeBuffer::Integer(vec![0])),
        (3, NativeBuffer::Integer(vec![i32::MAX])),
        (4, NativeBuffer::Real(vec![0.; 2])),
        (5, NativeBuffer::Real(vec![0.; 2])),
        (6, NativeBuffer::Real(vec![0.; 2])),
        (7, NativeBuffer::Real(vec![0.; 2])),
        (8, NativeBuffer::Integer(vec![])),
        (9, NativeBuffer::Integer(vec![])),
        (10, NativeBuffer::Integer(vec![3])),
    ] {
        let mut values = scalar_case();
        values[index] = value;
        reject(values, BufferInterface::C);
    }
}
#[test]
fn multi_burg_singular_errors_match_gnu_and_do_not_partially_write_buffers() {
    for (max_order, expected) in [
        (1, "Singular matrix in qr_solve"),
        (0, "Singular matrix in ldet"),
    ] {
        let mut values = scalar_case();
        values[1] = NativeBuffer::Real(vec![0.; 14]);
        values[2] = NativeBuffer::Integer(vec![max_order]);
        let original = format!("{values:?}");
        assert_eq!(
            super::multi_burg::ROUTINE
                .invoke(BufferInterface::C, &mut values)
                .unwrap_err()
                .to_string(),
            expected
        );
        assert_eq!(format!("{values:?}"), original);
    }
}

mod managed;

#[test]
fn multi_burg_order_zero_accepts_gnu_unread_empty_variance_method() {
    // Independently captured pinned GNU r90451 .C eleven-buffer result:
    // /tmp/rport-multi-burg-empty-method-oracle.R and its completed log.
    let mut buffers = vec![
        NativeBuffer::Integer(vec![4]),
        NativeBuffer::Real(vec![1., 2., 3., 4.]),
        NativeBuffer::Integer(vec![0]),
        NativeBuffer::Integer(vec![1]),
        NativeBuffer::Real(vec![0.]),
        NativeBuffer::Real(vec![0.]),
        NativeBuffer::Real(vec![0.]),
        NativeBuffer::Real(vec![0.]),
        NativeBuffer::Integer(vec![0]),
        NativeBuffer::Integer(vec![0]),
        NativeBuffer::Integer(vec![]),
    ];
    super::multi_burg::ROUTINE
        .invoke(BufferInterface::C, &mut buffers)
        .unwrap();
    let expected = vec![
        NativeBuffer::Integer(vec![4]),
        NativeBuffer::Real(vec![1., 2., 3., 4.]),
        NativeBuffer::Integer(vec![0]),
        NativeBuffer::Integer(vec![1]),
        NativeBuffer::Real(vec![1.]),
        NativeBuffer::Real(vec![0.]),
        NativeBuffer::Real(vec![7.5]),
        NativeBuffer::Real(vec![8.059612082169059]),
        NativeBuffer::Integer(vec![0]),
        NativeBuffer::Integer(vec![0]),
        NativeBuffer::Integer(vec![]),
    ];
    for (actual, expected) in buffers.iter().zip(expected) {
        match (actual, expected) {
            (NativeBuffer::Integer(a), NativeBuffer::Integer(b)) => assert_eq!(a, &b),
            (NativeBuffer::Real(a), NativeBuffer::Real(b)) => {
                assert_eq!(a.len(), b.len());
                for (a, b) in a.iter().zip(b) {
                    assert!((a - b).abs() < 1e-12);
                }
            }
            _ => panic!("GNU buffer types changed"),
        }
    }
}
