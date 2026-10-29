use r_embed::{RSession, RuntimePathPolicy};

// GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
x <- c(1,2,9,10); y <- 3:8
for(a in c("two.sided","less","greater")) {
    expected <- switch(a,"two.sided"=1/105,"less"=1,"greater"=1/210)
    z <- ansari.test(x,y,alternative=a,exact=TRUE)
    stopifnot(identical(unname(z$statistic),6), abs(z$p.value-expected)<1e-14)
    z <- ansari.test(-x,-y,alternative=a,exact=TRUE)
    stopifnot(abs(z$p.value-expected)<1e-14)
}
native <- get("C_pAnsari",asNamespace("stats"))
q <- c(5:19,6-5e-8,6-2e-7)
expected <- c(0,1,5,14,30,56,88,122,154,180,196,205,209,210,210,1,0)/210
p <- .Call(native,q,4L,6L)
stopifnot(all(abs(p-expected)<1e-14), all(diff(p[1:15])>=0))
cat("ansari-exact-contract-ok\n")
"#;

#[test]
fn public_ansari_exact_p_values_match_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(output.contains("ansari-exact-contract-ok"), "{output}");
    }
}
