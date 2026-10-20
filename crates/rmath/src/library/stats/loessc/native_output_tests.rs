use super::*;

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
        let mut x = values(&oracle, "x");
        let mut y = values(&oracle, "y");
        let mut weights = values(&oracle, "weights");
        let mut robust = values(&oracle, "robust");
        let (mut d, mut n, mut span, mut degree, mut np, mut drop_square, mut sum, mut cell) =
            (1, 8, 0.9, 1, 1, 2, 0, 0.18);
        let mut surface_name = surface.as_ptr().cast_mut();
        let mut fitted = vec![-11.; 8];
        let mut parameter = [0; 7];
        let mut a = vec![0; 200];
        let mut xi = vec![0.; 200];
        let mut vert = [0.; 2];
        let mut vval = vec![0.; 400];
        let mut diagonal = vec![-31.; 8];
        let (mut trace, mut delta1, mut delta2, mut lf) = (-37., -41., -43., 0);
        unsafe {
            loess_raw(
                y.as_mut_ptr(),
                x.as_mut_ptr(),
                weights.as_mut_ptr(),
                robust.as_mut_ptr(),
                &mut d,
                &mut n,
                &mut span,
                &mut degree,
                &mut np,
                &mut drop_square,
                &mut sum,
                &mut cell,
                &mut surface_name,
                fitted.as_mut_ptr(),
                parameter.as_mut_ptr(),
                a.as_mut_ptr(),
                xi.as_mut_ptr(),
                vert.as_mut_ptr(),
                vval.as_mut_ptr(),
                diagonal.as_mut_ptr(),
                &mut trace,
                &mut delta1,
                &mut delta2,
                &mut lf,
            );
        }
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
    for (mut family, prefix) in [(1, "direct"), (0, "symmetric")] {
        let mut x = values(&oracle, "x");
        let mut y = values(&oracle, "y");
        let mut robust = values(&oracle, "robust");
        let mut weights: Vec<f64> = values(&oracle, "weights")
            .iter()
            .zip(&robust)
            .map(|(w, r)| w * r)
            .collect();
        let mut queries = values(&oracle, "queries");
        let (mut span, mut degree, mut np, mut drop_square, mut sum, mut d, mut n, mut m) =
            (0.9, 1, 1, 2, 0, 1, 8, 2);
        let mut fit = vec![-47.; 2];
        let mut influence = vec![-53.; 16];
        unsafe {
            c_loess_dfitse(
                y.as_mut_ptr(),
                x.as_mut_ptr(),
                queries.as_mut_ptr(),
                weights.as_mut_ptr(),
                robust.as_mut_ptr(),
                &mut family,
                &mut span,
                &mut degree,
                &mut np,
                &mut drop_square,
                &mut sum,
                &mut d,
                &mut n,
                &mut m,
                fit.as_mut_ptr(),
                influence.as_mut_ptr(),
            );
        }
        matches(&fit, &oracle, &format!("{prefix}_fit"));
        matches(&influence, &oracle, &format!("{prefix}_L"));
    }
    let mut x = values(&oracle, "x");
    let mut y = values(&oracle, "y");
    let mut weights = values(&oracle, "weights");
    let mut queries = values(&oracle, "queries");
    let (mut span, mut degree, mut np, mut drop_square, mut sum, mut cell, mut d, mut n, mut m) =
        (0.9, 1, 1, 2, 0, 0.18, 1, 8, 2);
    let mut fit = vec![-59.; 2];
    let mut influence = vec![-61.; 16];
    unsafe {
        c_loess_ise(
            y.as_mut_ptr(),
            x.as_mut_ptr(),
            queries.as_mut_ptr(),
            weights.as_mut_ptr(),
            &mut span,
            &mut degree,
            &mut np,
            &mut drop_square,
            &mut sum,
            &mut cell,
            &mut d,
            &mut n,
            &mut m,
            fit.as_mut_ptr(),
            influence.as_mut_ptr(),
        );
    }
    matches(&fit, &oracle, "interpolate_fit");
    matches(&influence, &oracle, "interpolate_L");
}
