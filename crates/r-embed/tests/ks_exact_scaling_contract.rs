use r_embed::{RSession, RuntimePathPolicy};

// GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
for (n in c(200L,400L,500L)) {
    x <- ((seq_len(n)-.5)+.6)/n
    z <- ks.test(x, "punif", exact=TRUE)
    stopifnot(abs(unname(z$statistic)-1.1/n) < 1e-14,
              identical(z$p.value, 1))
}
native <- get("C_pkolmogorov_two_exact", asNamespace("stats"))
q <- c(.00275,.003,.00325)
expected <- c(1.3177625442620577e-143,5.5495104732665456e-122,5.2466886749515492e-105)
p <- .Call(native, q, 400L)
stopifnot(all(abs(p/expected-1) < 1e-11))
cat("ks-exact-scaling-contract-ok\n")
"#;

#[test]
fn public_ks_exact_scaling_matches_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(output.contains("ks-exact-scaling-contract-ok"), "{output}");
    }
}
