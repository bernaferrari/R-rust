use super::random::{
    FixupProb, GetRNGstate, PutRNGstate, set_session_seed64, walker_ProbSampleReplace_r,
};
#[test]
fn walker_stream_contract() -> Result<(), String> {
    // --- API level: seeded walker stream matches trunk exactly ---
    let _session = crate::sexp::RSession::new();
    unsafe {
        set_session_seed64(7);
        GetRNGstate();
        let n = 500usize;
        let mut p: Vec<f64> = (1..=n as i64).map(|i| (i * i) as f64).collect();
        if FixupProb(&mut p, 10_000, true).is_some() {
            return Err("FixupProb((1:500)^2) failed".to_string());
        }
        let ans = walker_ProbSampleReplace_r(n, &mut p, 10_000);
        PutRNGstate();

        let sum: i64 = ans.iter().map(|&v| v as i64).sum();
        if sum != 3_767_164 {
            return Err(format!(
                "walker sample(1:500, 1e4, prob=(1:500)^2), seed 7: sum {sum}, trunk 3767164"
            ));
        }
        let want_head = [
            298, 415, 449, 375, 442, 323, 417, 431, 471, 268, 336, 374, 397, 335, 407, 388, 358,
            215, 451, 381,
        ];
        if ans[..20] != want_head {
            return Err(format!(
                "walker head mismatch: {:?} vs trunk {want_head:?}",
                &ans[..20]
            ));
        }
    }

    Ok(())
}
