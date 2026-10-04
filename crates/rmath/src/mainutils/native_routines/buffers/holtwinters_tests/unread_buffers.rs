//! Independent GNU branch neighbors, including truly empty unread buffers.
use super::*;
fn cases() -> std::collections::BTreeMap<&'static str, Vec<(NativeBuffer, NativeBuffer)>> {
    let mut cases = std::collections::BTreeMap::<_, Vec<_>>::new();
    for row in include_str!("../fixtures/holtwinters-unread-gnu-r90451.tsv")
        .lines()
        .skip(1)
    {
        let f: Vec<_> = row.split('\t').collect();
        assert_eq!(f.len(), 5);
        let values = cases.entry(f[0]).or_default();
        assert_eq!(f[1].parse::<usize>().unwrap(), values.len());
        let kind = if f[2] == "double" { "real" } else { f[2] };
        values.push((buffer(kind, f[3]), buffer(kind, f[4])));
    }
    cases
}
#[test]
fn holtwinters_unread_buffers_match_all_independent_gnu_branch_outputs() {
    let cases = cases();
    assert_eq!(cases.len(), 3);
    for (case, values) in cases {
        let (mut values, expected): (Vec<_>, Vec<_>) = values.into_iter().unzip();
        super::super::holtwinters::ROUTINE
            .invoke(BufferInterface::C, &mut values)
            .unwrap();
        for (index, (actual, expected)) in values.iter().zip(expected).enumerate() {
            match (actual, expected) {
                (NativeBuffer::Integer(a), NativeBuffer::Integer(b)) => {
                    assert_eq!(a, &b, "{case} argument{index}")
                }
                (NativeBuffer::Real(a), NativeBuffer::Real(b)) => {
                    assert_eq!(a.len(), b.len());
                    for (a, b) in a.iter().zip(b) {
                        assert!((a - b).abs() < 1e-12, "{case} argument{index}");
                    }
                }
                _ => panic!("GNU buffer type mismatch"),
            }
        }
    }
}

#[test]
fn holtwinters_conditional_bounds_reject_required_reads_before_outputs_or_invocation() {
    let reject = |mut inputs: Vec<NativeBuffer>| {
        let before = super::super::invocation_count();
        let original = format!("{inputs:?}");
        assert!(
            super::super::holtwinters::ROUTINE
                .invoke(BufferInterface::C, &mut inputs)
                .is_err()
        );
        assert_eq!(super::super::invocation_count(), before);
        assert_eq!(format!("{inputs:?}"), original);
    };
    for (case, index) in [
        ("init-only", 10),
        ("init-only", 14),
        ("init-trend-season", 11),
        ("init-trend-season", 12),
        ("init-trend-season", 15),
        ("init-trend-season", 16),
        ("iter-disabled-components", 0),
        ("iter-disabled-components", 2),
        ("iter-disabled-components", 13),
        ("iter-disabled-components", 15),
    ] {
        let mut inputs: Vec<_> = cases()
            .remove(case)
            .unwrap()
            .into_iter()
            .map(|(a, _)| a)
            .collect();
        inputs[index] = NativeBuffer::Real(vec![]);
        reject(inputs);
    }
}
