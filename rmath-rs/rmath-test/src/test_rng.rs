use rmath::rng::{KnuthTaocp, LecuyerCmrg, MersenneTwister, SuperDuper, WichmannHill};

const TOL: f64 = 1e-14;

fn check(name: &str, expected: &[f64], actual: &[f64]) -> Result<(), String> {
    for (i, (e, a)) in expected.iter().zip(actual.iter()).enumerate() {
        let diff = (e - a).abs();
        if diff > TOL {
            return Err(format!(
                "{}: value[{}] expected={:.17} got={:.17} diff={:.2e}",
                name, i, e, a, diff
            ));
        }
    }
    Ok(())
}

pub fn run_tests() -> Result<(), String> {
    let mut mt = MersenneTwister::new();
    mt.set_seed(42);
    let mt_vals: Vec<f64> = (0..5).map(|_| mt.get_rand()).collect();
    check(
        "MersenneTwister",
        &[
            0.914_806_043_496_355_4,
            0.937_075_413_297_861_8,
            0.286_139_534_786_343_6,
            0.830_447_626_067_325_5,
            0.641_745_518_893_003_5,
        ],
        &mt_vals,
    )?;

    let mut wh = WichmannHill::new();
    wh.set_seed(42);
    let wh_vals: Vec<f64> = (0..5).map(|_| wh.get_rand()).collect();
    check(
        "WichmannHill",
        &[
            0.25080964353400526,
            0.761_803_344_363_039_8,
            0.20390793930585005,
            0.941_854_416_455_615_5,
            0.15301282704026198,
        ],
        &wh_vals,
    )?;

    let mut sd = SuperDuper::new();
    sd.set_seed(42);
    let sd_vals: Vec<f64> = (0..5).map(|_| sd.get_rand()).collect();
    check(
        "SuperDuper",
        &[
            0.772_879_730_158_690_2,
            0.838_451_734_007_906_8,
            0.261_147_795_771_515_8,
            0.392_996_063_081_779_4,
            0.39984154128465826,
        ],
        &sd_vals,
    )?;

    let mut kt = KnuthTaocp::new();
    kt.set_seed(42);
    let kt_vals: Vec<f64> = (0..5).map(|_| kt.get_rand()).collect();
    check(
        "KnuthTaocp",
        &[
            0.016140771098434932,
            0.092_272_087_931_633_04,
            0.550_551_142_543_554_5,
            0.137_899_955_734_610_6,
            0.256_486_915_983_259_8,
        ],
        &kt_vals,
    )?;

    let mut cmrg = LecuyerCmrg::new();
    cmrg.set_seed(42);
    let cmrg_vals: Vec<f64> = (0..5).map(|_| cmrg.get_rand()).collect();
    check(
        "LecuyerCmrg",
        &[
            0.17384558454153168,
            0.554_740_096_765_090_8,
            0.48337712221370116,
            0.737_483_073_816_746_4,
            0.796_564_767_762_43,
        ],
        &cmrg_vals,
    )?;

    Ok(())
}
