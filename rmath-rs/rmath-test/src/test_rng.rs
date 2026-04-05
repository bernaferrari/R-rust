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
            0.91480604349635541,
            0.93707541329786181,
            0.28613953478634357,
            0.83044762606732547,
            0.64174551889300346,
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
            0.76180334436303976,
            0.20390793930585005,
            0.94185441645561552,
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
            0.77287973015869016,
            0.83845173400790685,
            0.26114779577151581,
            0.39299606308177942,
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
            0.092272087931633037,
            0.550551142543554528,
            0.137899955734610613,
            0.256486915983259789,
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
            0.55474009676509084,
            0.48337712221370116,
            0.73748307381674638,
            0.79656476776243001,
        ],
        &cmrg_vals,
    )?;

    Ok(())
}
