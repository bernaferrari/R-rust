use super::tricube_sqrt_weight;

#[test]
fn exact_boundary_and_degenerate_radius_follow_gnu_arithmetic() {
    for weight in [0., 0.7, 1., 4.] {
        assert_eq!(tricube_sqrt_weight(1., 1., weight), 0.);
    }
    assert_eq!(tricube_sqrt_weight(0., 1., 4.), 2.);
    assert_eq!(tricube_sqrt_weight(0.25, 1., 1.), (343_f64 / 512.).sqrt());
    assert!(tricube_sqrt_weight(0., 0., 1.).is_nan());
}
