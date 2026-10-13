use super::*;
use crate::sexp::RSession;
#[test]
fn test_FixupProb_basic() {
    let mut p = vec![0.5, 0.3, 0.2];
    assert!(FixupProb(&mut p, 3, true).is_none());
    assert!((p[0] - 0.5).abs() < 1e-10);
    assert!((p[1] - 0.3).abs() < 1e-10);
    assert!((p[2] - 0.2).abs() < 1e-10);
}
#[test]
fn test_FixupProb_normalize() {
    let mut p = vec![1.0, 2.0, 3.0];
    assert!(FixupProb(&mut p, 3, true).is_none());
    assert!((p[0] - 1.0 / 6.0).abs() < 1e-10);
    assert!((p[1] - 2.0 / 6.0).abs() < 1e-10);
    assert!((p[2] - 3.0 / 6.0).abs() < 1e-10);
}
#[test]
fn test_FixupProb_negative() {
    let mut p = vec![0.5, -0.1, 0.6];
    assert!(FixupProb(&mut p, 3, true).is_some());
}
#[test]
fn test_FixupProb_nan() {
    let mut p = vec![0.5, f64::NAN, 0.5];
    assert!(FixupProb(&mut p, 3, true).is_some());
}
#[test]
fn test_FixupProb_zero() {
    let mut p = vec![0.0, 0.0, 0.0];
    assert!(FixupProb(&mut p, 1, true).is_some());
}
#[test]
fn test_revsort_with_perm() {
    let mut p = vec![0.3, 0.1, 0.4, 0.2];
    let mut perm = vec![1, 2, 3, 4];
    revsort_with_perm(&mut p, &mut perm, 4);
    assert_eq!(p, vec![0.4, 0.3, 0.2, 0.1]);
    assert_eq!(perm[0], 3);
}
#[test]
fn test_rng_state_creation() {
    let state = RNGState::new();
    assert_eq!(state.rng_kind.get(), RNG_DEFAULT);
    assert_eq!(state.n01_kind.get(), N01_DEFAULT);
    assert_eq!(state.sample_kind.get(), Sample_DEFAULT);
}
#[test]
fn test_rng_init_mt() {
    let _session = RSession::new();
    with_rng_state(|rng| {
        RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
        assert_eq!(rng.rng_table[3].i_seed[0], MT_N as u32);
    });
}
#[test]
fn test_unif_rand_range() {
    let _session = RSession::new();
    with_rng_state(|rng| {
        rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
        RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 12345);
    });
    for _ in 0..100 {
        let u = r_unif_rand();
        assert!(u > 0.0 && u < 1.0, "unif_rand returned {}", u);
    }
}
#[test]
fn test_norm_rand_basic() {
    let _session = RSession::new();
    with_rng_state(|rng| {
        rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
        rng.n01_kind.set(N01type::INVERSION);
        RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 12345);
    });
    let mut sum = 0.0;
    let n = 1000;
    for _ in 0..n {
        let z = r_norm_rand();
        assert!(z.is_finite(), "norm_rand returned non-finite");
        sum += z;
    }
    let mean = sum / n as f64;
    assert!(mean.abs() < 0.2, "mean of norm_rand is {}", mean);
}
#[test]
fn test_unif_rand_reproducibility() {
    let _session = RSession::new();
    with_rng_state(|rng| {
        rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
        RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
    });
    let v1: Vec<f64> = (0..10).map(|_| r_unif_rand()).collect();
    with_rng_state(|rng| {
        rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
        RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
    });
    let v2: Vec<f64> = (0..10).map(|_| r_unif_rand()).collect();
    for (a, b) in v1.iter().zip(v2.iter()) {
        assert!(
            (a - b).abs() < 1e-15,
            "RNG not reproducible: {} vs {}",
            a,
            b
        );
    }
}
#[test]
fn test_marsaglia_multicarry() {
    let _session = RSession::new();
    with_rng_state(|rng| {
        rng.rng_kind.set(RNGtype::MARSAGLIA_MULTICARRY);
        RNG_Init(rng, RNGtype::MARSAGLIA_MULTICARRY, 42);
    });
    for _ in 0..100 {
        let u = r_unif_rand();
        assert!(u > 0.0 && u < 1.0, "Marsaglia unif_rand returned {}", u);
    }
}
#[test]
fn test_wichmann_hill() {
    let _session = RSession::new();
    with_rng_state(|rng| {
        rng.rng_kind.set(RNGtype::WICHMANN_HILL);
        RNG_Init(rng, RNGtype::WICHMANN_HILL, 42);
    });
    for _ in 0..100 {
        let u = r_unif_rand();
        assert!(u > 0.0 && u < 1.0, "WH unif_rand returned {}", u);
    }
}
#[test]
fn test_super_duper() {
    let _session = RSession::new();
    with_rng_state(|rng| {
        rng.rng_kind.set(RNGtype::SUPER_DUPER);
        RNG_Init(rng, RNGtype::SUPER_DUPER, 42);
    });
    for _ in 0..100 {
        let u = r_unif_rand();
        assert!(u > 0.0 && u < 1.0, "SD unif_rand returned {}", u);
    }
}
#[test]
fn test_lecuyer_cmrg() {
    let _session = RSession::new();
    with_rng_state(|rng| {
        rng.rng_kind.set(RNGtype::LECUYER_CMRG);
        RNG_Init(rng, RNGtype::LECUYER_CMRG, 42);
    });
    for _ in 0..100 {
        let u = r_unif_rand();
        // CMRG does not use fixup — can return exactly 0.0 when p1 == p2
        assert!(u >= 0.0 && u < 1.0, "CMRG unif_rand returned {}", u);
    }
}
#[test]
fn test_box_muller() {
    let _session = RSession::new();
    with_rng_state(|rng| {
        rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
        rng.n01_kind.set(N01type::BOX_MULLER);
        RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 12345);
    });
    let mut sum = 0.0;
    let n = 1000;
    for _ in 0..n {
        let z = r_norm_rand();
        assert!(z.is_finite(), "Box-Muller norm_rand returned non-finite");
        sum += z;
    }
    let mean = sum / n as f64;
    assert!(mean.abs() < 0.2, "Box-Muller mean is {}", mean);
}
#[test]
fn test_fixup() {
    assert!(fixup(0.0) > 0.0);
    assert!(fixup(1.0) < 1.0);
    assert_eq!(fixup(0.5), 0.5);
}
#[test]
fn test_R_unif_index() {
    let _session = RSession::new();
    with_rng_state(|rng| {
        rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
        rng.sample_kind.set(Sampletype::REJECTION);
        RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
    });
    for _ in 0..100 {
        let idx = r_R_unif_index(10.0);
        assert!(idx >= 0.0 && idx < 10.0, "R_unif_index returned {}", idx);
    }
}
#[test]
fn test_SampleReplace_r_range() {
    let _session = RSession::new();
    with_rng_state(|rng| {
        rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
        RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
    });
    let ans = SampleReplace_r(5, 100);
    for &a in &ans {
        assert!(a >= 1 && a <= 5);
    }
}
#[test]
fn test_ProbSampleReplace_r_range() {
    let _session = RSession::new();
    with_rng_state(|rng| {
        rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
        RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
    });
    let mut p = vec![0.1, 0.2, 0.3, 0.4];
    let ans = ProbSampleReplace_r(4, &mut p, 10);
    for &a in &ans {
        assert!(a >= 1 && a <= 4);
    }
}
#[test]
fn test_ProbSampleNoReplace_r_range() {
    let _session = RSession::new();
    with_rng_state(|rng| {
        rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
        RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
    });
    let mut p = vec![0.1, 0.2, 0.3, 0.4];
    let ans = ProbSampleNoReplace_r(4, &mut p, 2);
    assert_eq!(ans.len(), 2);
    for &a in &ans {
        assert!(a >= 1 && a <= 4);
    }
}
#[test]
fn test_knuth_taocp2() {
    let _session = RSession::new();
    with_rng_state(|rng| {
        rng.rng_kind.set(RNGtype::KNUTH_TAOCP2);
        RNG_Init(rng, RNGtype::KNUTH_TAOCP2, 42);
    });
    for _ in 0..100 {
        let u = r_unif_rand();
        assert!(u > 0.0 && u < 1.0, "Knuth TAOCP2 unif_rand returned {}", u);
    }
}
#[test]
fn test_rng_state_is_session_local_on_same_thread() {
    let left = RSession::new();
    let right = RSession::new();
    let left_first = left.with_protected(|| {
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
            rng.n01_kind.set(N01type::INVERSION);
            rng.sample_kind.set(Sampletype::REJECTION);
            RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
        });
        r_unif_rand()
    });
    let right_first = right.with_protected(|| {
        with_rng_state(|rng| {
            assert_eq!(rng.rng_kind.get(), RNG_DEFAULT);
            rng.rng_kind.set(RNGtype::WICHMANN_HILL);
            RNG_Init(rng, RNGtype::WICHMANN_HILL, 99);
        });
        r_unif_rand()
    });
    let left_second = left.with_protected(|| {
        with_rng_state(|rng| {
            assert_eq!(rng.rng_kind.get(), RNGtype::MERSENNE_TWISTER);
            assert_eq!(rng.n01_kind.get(), N01type::INVERSION);
            assert_eq!(rng.sample_kind.get(), Sampletype::REJECTION);
        });
        r_unif_rand()
    });
    let left_replayed = left.with_protected(|| {
        with_rng_state(|rng| {
            rng.rng_kind.set(RNGtype::MERSENNE_TWISTER);
            RNG_Init(rng, RNGtype::MERSENNE_TWISTER, 42);
        });
        r_unif_rand()
    });
    assert_eq!(left_first, left_replayed);
    assert_ne!(left_first, right_first);
    assert_ne!(left_first, left_second);
}
