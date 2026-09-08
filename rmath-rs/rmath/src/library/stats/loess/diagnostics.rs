// Rust adaptation of the upstream LOESS algorithms.
// Copyright (C) 1998--2020 The R Core Team
//
// The authors of this software are Cleveland, Grosse, and Shyu.
// Copyright (c) 1989, 1992 by AT&T.
// Permission to use, copy, modify, and distribute this software for any
// purpose without fee is hereby granted, provided that this entire notice
// is included in all copies of any software which is or includes a copy
// or modification of this software and in all copies of the supporting
// documentation for such software.
// THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR IMPLIED
// WARRANTY. IN PARTICULAR, NEITHER THE AUTHORS NOR AT&T MAKE ANY
// REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE MERCHANTABILITY
// OF THIS SOFTWARE OR ITS FITNESS FOR ANY PARTICULAR PURPOSE.

//! Hat-matrix diagnostics and upstream empirical trace corrections.
use super::surface::hermite;
pub(super) fn exact(l: &[Vec<f64>]) -> (f64, f64) {
    let n = l.len();
    let mut delta1 = 0.;
    let mut delta2 = 0.;
    for i in 0..n {
        for j in 0..=i {
            let ll = (0..n)
                .map(|k| (l[i][k] - f64::from(i == k)) * (l[j][k] - f64::from(j == k)))
                .sum::<f64>();
            if i == j {
                delta1 += ll;
            }
            delta2 += ll * ll * (if i == j { 1. } else { 2. });
        }
    }
    (delta1, delta2)
}
const COEFFICIENTS: [f64; 48] = [
    0.2971620, 0.3802660, 0.5886043, 0.4263766, 0.3346498, 0.6271053, 0.5241198, 0.3484836,
    0.6687687, 0.6338795, 0.4076457, 0.7207693, 0.1611761, 0.3091323, 0.4401023, 0.2939609,
    0.3580278, 0.5555741, 0.3972390, 0.4171278, 0.6293196, 0.4675173, 0.4699070, 0.6674802,
    0.2848308, 0.2254512, 0.2914126, 0.5393624, 0.2517230, 0.3898970, 0.7603231, 0.2969113,
    0.4740130, 0.9664956, 0.3629838, 0.5348889, 0.2075670, 0.2822574, 0.2369957, 0.3911566,
    0.2981154, 0.3623232, 0.5508869, 0.3501989, 0.4371032, 0.7002667, 0.4291632, 0.4930370,
];
const NODES: [(f64, f64, f64); 10] = [
    (-0.005, -0.090572, 4.4844),
    (0.1204, 0.095807, -0.7978),
    (0.2017, 0.026152, -0.7286),
    (0.2815, -0.031926, -0.4457),
    (0.3705, -0.053718, -0.3495),
    (0.4536, -0.06417, 0.032813),
    (0.5591, -0.058387, 0.1611),
    (0.7132, -0.020636, 0.335),
    (0.8751, 0.040172, -0.041032),
    (1.005, -0.010856, -0.7736),
];
pub(super) fn approximate(n: usize, d: usize, tau: usize, trace: f64) -> (f64, f64) {
    if n == tau || !trace.is_finite() || trace <= 0. {
        return (f64::NAN, f64::NAN);
    }
    let cor = (tau as f64 / n as f64).sqrt();
    let z = (((tau as f64 / trace).sqrt() - cor) / (1. - cor)).clamp(0., 1.);
    let pair = NODES
        .windows(2)
        .find(|p| z >= p[0].0 && z <= p[1].0)
        .unwrap();
    let (a, b) = (pair[0], pair[1]);
    let c4 = hermite((z - a.0) / (b.0 - a.0), b.0 - a.0, a.1, b.1, a.2, b.2).exp();
    let delta = |degree: usize, offset: usize| {
        let i = 3 * (d - 1 + 4 * (degree - 1)) + offset;
        n as f64
            - trace
                * (COEFFICIENTS[i]
                    * z.powf(COEFFICIENTS[i + 1])
                    * (1. - z).powf(COEFFICIENTS[i + 2])
                    * c4)
                    .exp()
    };
    let alpha = (tau as f64 - (d + 1) as f64) / ((d + 2) * (d + 1) / 2 - d - 1) as f64;
    (
        (1. - alpha) * delta(1, 0) + alpha * delta(2, 0),
        (1. - alpha) * delta(1, 24) + alpha * delta(2, 24),
    )
}
