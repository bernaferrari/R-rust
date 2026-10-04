//! Independent pinned GNU native outputs, including untouched buffer tails.
use super::{BufferInterface, NativeBuffer, invocation_count};

fn buffer(kind: &str, values: &str) -> NativeBuffer {
    let elements = || values.split(',').filter(|v| !v.is_empty());
    match kind {
        "integer" => NativeBuffer::Integer(elements().map(|v| v.parse().unwrap()).collect()),
        "real" => NativeBuffer::Real(elements().map(|v| v.parse().unwrap()).collect()),
        _ => panic!("unknown independent oracle element type"),
    }
}

fn additive() -> Vec<NativeBuffer> {
    include_str!("fixtures/holtwinters-gnu-r90451.tsv")
        .lines()
        .skip(1)
        .filter_map(|row| {
            let fields: Vec<_> = row.split('\t').collect();
            (fields[0] == "additive").then(|| buffer(fields[2], fields[3]))
        })
        .collect()
}

#[test]
fn holtwinters_admission_rejects_invalid_inputs_before_any_kernel_or_output_write() {
    let routine = crate::library::tools::native_calls::lookup_buffer("HoltWinters").unwrap();
    let before = invocation_count();
    let reject = |mut values: Vec<NativeBuffer>, interface| {
        let original = format!("{values:?}");
        assert!(routine.invoke(interface, &mut values).is_err());
        assert_eq!(
            format!("{values:?}"),
            original,
            "admission cannot partially initialize outputs"
        );
        assert_eq!(invocation_count(), before);
    };
    reject(additive(), BufferInterface::Fortran);
    let mut short = additive();
    short.pop();
    reject(short, BufferInterface::C);
    let mut long = additive();
    long.push(NativeBuffer::Real(vec![0.0]));
    reject(long, BufferInterface::C);
    for index in 0..17 {
        let mut values = additive();
        values[index] = match values[index] {
            NativeBuffer::Real(_) => NativeBuffer::Integer(vec![1]),
            _ => NativeBuffer::Real(vec![1.0]),
        };
        reject(values, BufferInterface::C);
    }
    for (index, replacement) in [
        (0, NativeBuffer::Real(vec![1.0])),
        (1, NativeBuffer::Integer(vec![-1])),
        (1, NativeBuffer::Integer(vec![8])),
        (5, NativeBuffer::Integer(vec![0])),
        (7, NativeBuffer::Integer(vec![-1])),
        (7, NativeBuffer::Integer(vec![i32::MAX])),
        (12, NativeBuffer::Real(vec![0.0; 2])),
        (13, NativeBuffer::Real(vec![])),
        (14, NativeBuffer::Real(vec![0.0; 6])),
        (15, NativeBuffer::Real(vec![0.0; 6])),
        (16, NativeBuffer::Real(vec![0.0; 8])),
    ] {
        let mut values = additive();
        values[index] = replacement;
        reject(values, BufferInterface::C);
    }
}

#[test]
fn holtwinters_minimal_workspace_and_original_input_prefix_are_admitted() {
    let routine = crate::library::tools::native_calls::lookup_buffer("HoltWinters").unwrap();
    let mut values = additive();
    values[1] = NativeBuffer::Integer(vec![4]);
    for (index, length) in [(14, 4), (15, 4), (16, 6)] {
        match &mut values[index] {
            NativeBuffer::Real(v) => v.truncate(length),
            _ => unreachable!(),
        }
    }
    routine.invoke(BufferInterface::C, &mut values).unwrap();
    assert_eq!(
        values[0].reals().unwrap(),
        &[20.0, 19.0, 21.0, 23.0, 20.0, 24.0, 26.0]
    );
    let mut full = additive();
    routine.invoke(BufferInterface::C, &mut full).unwrap();
    for index in [14, 15, 16] {
        let short = values[index].reals().unwrap();
        assert_eq!(short, &full[index].reals().unwrap()[..short.len()]);
    }
}

#[test]
fn holtwinters_owned_matches_all_independent_gnu_outputs_and_untouched_tails() {
    let descriptor = crate::library::tools::native_calls::lookup_buffer("HoltWinters")
        .expect("GNU's seventeen-argument native filter must resolve");
    assert_eq!(descriptor.package(), "stats");
    assert_eq!(descriptor.interface(), BufferInterface::C);
    assert_eq!(descriptor.arity(), 17);
    let mut cases = std::collections::BTreeMap::<_, Vec<_>>::new();
    for row in include_str!("fixtures/holtwinters-gnu-r90451.tsv")
        .lines()
        .skip(1)
    {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 5);
        let entries = cases.entry(fields[0]).or_default();
        assert_eq!(fields[1].parse::<usize>().unwrap(), entries.len());
        entries.push((buffer(fields[2], fields[3]), buffer(fields[2], fields[4])));
    }
    assert_eq!(cases.len(), 9);
    let mut records = 0;
    for (name, entries) in cases {
        assert_eq!(entries.len(), 17);
        let (mut inputs, expected): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
        let original: Vec<_> = inputs
            .iter()
            .map(|input| match input {
                NativeBuffer::Real(values) => {
                    values.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
                }
                _ => vec![],
            })
            .collect();
        descriptor.invoke(BufferInterface::C, &mut inputs).unwrap();
        for (index, (actual, expected)) in inputs.iter().zip(&expected).enumerate() {
            assert_eq!(actual.len(), expected.len(), "{name}, argument {index}");
            match (actual, expected) {
                (NativeBuffer::Integer(a), NativeBuffer::Integer(b)) => assert_eq!(a, b),
                (NativeBuffer::Real(a), NativeBuffer::Real(b)) => {
                    for (position, (a, b)) in a.iter().zip(b).enumerate() {
                        if index < 13 || b.to_bits() == original[index][position] {
                            assert_eq!(
                                a.to_bits(),
                                b.to_bits(),
                                "{name}: unchanged argument {index}, element {position}"
                            );
                        }
                        // Portable C/Rust builds can round each recurrence
                        // differently (including fused multiply-add in C).
                        assert!(
                            (a - b).abs() <= 64.0 * f64::EPSILON * b.abs().max(1.0),
                            "{name}, argument {index}, element {position}: {a} != GNU {b}"
                        );
                    }
                }
                _ => panic!("{name}, argument {index}: type differs"),
            }
            records += 1;
        }
    }
    assert_eq!(records, 153);
}

mod unread_buffers;
