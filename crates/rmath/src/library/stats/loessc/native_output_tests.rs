use super::*;
use NativeBuffer::{Integer as I, Real as R};

fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../r-embed/tests/fixtures/gnu-loess-native-kernel.json"
    )))
    .unwrap()
}

fn values(oracle: &serde_json::Value, name: &str) -> Vec<f64> {
    oracle[name]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

fn matches(actual: &[f64], oracle: &serde_json::Value, name: &str) {
    let expected = values(oracle, name);
    assert_eq!(actual.len(), expected.len(), "{name}");
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (actual - expected).abs() <= 1e-9 * expected.abs().max(1.),
            "{name}[{index}]: {actual} versus GNU {expected}"
        );
    }
}

#[test]
fn native_raw_diagonals_and_unused_outputs_follow_gnu() {
    let _session = crate::sexp::RSession::new_for_gc_tests();
    let oracle = oracle();
    for (surface, expected) in [
        (c"direct/approximate", Some("direct_diagonal")),
        (c"interpolate/1.approx", Some("interpolate_diagonal")),
        (c"direct/none", None),
    ] {
        let surface_name = surface.to_bytes_with_nul().to_vec();
        let mut b = vec![
            R(values(&oracle, "y")),
            R(values(&oracle, "x")),
            R(values(&oracle, "weights")),
            R(values(&oracle, "robust")),
            I(vec![1]),
            I(vec![8]),
            R(vec![0.9]),
            I(vec![1]),
            I(vec![1]),
            I(vec![2]),
            I(vec![0]),
            R(vec![0.18]),
            NativeBuffer::Character(vec![surface_name]),
            R(vec![-11.; 8]),
            I(vec![0; 7]),
            I(vec![0; 200]),
            R(vec![0.; 200]),
            R(vec![0.; 2]),
            R(vec![0.; 400]),
            R(vec![-31.; 8]),
            R(vec![-37.]),
            R(vec![-41.]),
            R(vec![-43.]),
            I(vec![0]),
        ];
        loess_raw(&mut b).unwrap();
        let diagonal = b[19].reals().unwrap().to_vec();
        let (trace, delta1, delta2) = (
            b[20].reals().unwrap()[0],
            b[21].reals().unwrap()[0],
            b[22].reals().unwrap()[0],
        );
        if let Some(name) = expected {
            matches(&diagonal, &oracle, name);
        } else {
            assert_eq!(diagonal, vec![-31.; 8]);
            assert_eq!((trace, delta1, delta2), (0., -41., -43.));
        }
    }
}

#[test]
fn native_standard_error_outputs_publish_genuine_full_influence() {
    let _session = crate::sexp::RSession::new_for_gc_tests();
    let oracle = oracle();
    for (family, prefix) in [(1, "direct"), (0, "symmetric")] {
        let robust = values(&oracle, "robust");
        let weights: Vec<f64> = values(&oracle, "weights")
            .iter()
            .zip(&robust)
            .map(|(w, r)| w * r)
            .collect();
        let mut b = vec![
            R(values(&oracle, "y")),
            R(values(&oracle, "x")),
            R(values(&oracle, "queries")),
            R(weights),
            R(robust),
            I(vec![family]),
            R(vec![0.9]),
            I(vec![1]),
            I(vec![1]),
            I(vec![2]),
            I(vec![0]),
            I(vec![1]),
            I(vec![8]),
            I(vec![2]),
            R(vec![-47.; 2]),
            R(vec![-53.; 16]),
        ];
        loess_dfitse(&mut b).unwrap();
        let fit = b[14].reals().unwrap().to_vec();
        let influence = b[15].reals().unwrap().to_vec();
        matches(&fit, &oracle, &format!("{prefix}_fit"));
        matches(&influence, &oracle, &format!("{prefix}_L"));
    }
    let mut b = vec![
        R(values(&oracle, "y")),
        R(values(&oracle, "x")),
        R(values(&oracle, "queries")),
        R(values(&oracle, "weights")),
        R(vec![0.9]),
        I(vec![1]),
        I(vec![1]),
        I(vec![2]),
        I(vec![0]),
        R(vec![0.18]),
        I(vec![1]),
        I(vec![8]),
        I(vec![2]),
        R(vec![-59.; 2]),
        R(vec![-61.; 16]),
    ];
    loess_ise(&mut b).unwrap();
    let fit = b[13].reals().unwrap().to_vec();
    let influence = b[14].reals().unwrap().to_vec();
    matches(&fit, &oracle, "interpolate_fit");
    matches(&influence, &oracle, "interpolate_L");
}
