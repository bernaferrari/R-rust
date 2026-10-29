use r_embed::{RSession, RuntimePathPolicy};

// GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
ns <- c(3:12,20)
expected.w <- c(.97959183673469408,.96212403759749954,.94982453465555883,.94092402896269101,.93421051167723057,.92898257364456982,.9248146093286026,.92142792746397628,.91863204475925186,.91629244147428457,.90613062847654635)
expected.p <- c(.72622630994205595,.79226858783220777,.7359347808849005,.66665303819572808,.5872501520049821,.50686119152337505,.43358377802836257,.36901854014624469,.30741084749323166,.25667346766162591,.053809588828204018)
for (i in seq_along(ns)) {
    x <- seq_len(ns[i])^2
    z <- shapiro.test(x)
    cat(ns[i], z$p.value, "\n")
    stopifnot(abs(unname(z$statistic)-expected.w[i]) < 1e-12,
              abs(z$p.value-expected.p[i]) < 1e-12)
}
cat("shapiro-small-sample-contract-ok\n")
"#;

#[test]
fn public_shapiro_small_sample_p_values_match_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(
            output.contains("shapiro-small-sample-contract-ok"),
            "{output}"
        );
    }
}
