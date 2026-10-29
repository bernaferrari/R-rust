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
for(n in c(1L,2L,5L,20L)) {
    p <- .Call(native,c(0,.2,.5,1),n)
    expected <- switch(as.character(n), "1"=c(0,0,0,1), "2"=c(0,0,.5,1),
                       "5"=c(0,.038400000000000004,.888000000000000123,1),
                       "20"=c(0,.64727982637658399,.99996212404759444,1.00000000000000044))
    stopifnot(all(abs(p-expected) < 1e-12))
}
x <- c(.05,.12,.23,.29,.41,.52,.67,.75,.84,.93)
z <- ks.test(x,"punif",exact=TRUE)
stopifnot(abs(unname(z$statistic)-.11)<1e-14, abs(z$p.value-.99834230728422091)<1e-12,
          identical(.Call(native,numeric(0),400L),numeric(0)))
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
