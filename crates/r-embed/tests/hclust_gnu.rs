use r_embed::RSession;

/// GNU R 4.6.1 oracle for `hclust(dist(cbind(c(0, 0, 1, 3))))`.
/// `dput` from `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript --vanilla`:
///
/// complete and single share the merge and the leaf order:
/// `structure(c(-1L, -3L, -4L, -2L, 1L, 2L), dim = 3:2)`
/// `c(4L, 3L, 1L, 2L)`
/// complete height: `c(0, 1, 3)`
/// single height: `c(0, 1, 2)`
/// Those heights are exact integers in binary64; compare with `identical`.
#[test]
fn hclust_matches_gnu_oracle() {
    let mut session = RSession::new().expect("session");
    let out = session
        .eval(
            r#"
            run <- function(label, expr) {
              tryCatch(expr, error=function(e) paste(label, "ERR", conditionMessage(e)))
            }
            show_vec <- function(x) paste(as.vector(x), collapse=",")
            check <- function(method, height) {
              hc <- hclust(d, method=method)
              merge <- as.integer(hc$merge)
              exp_merge <- c(-1L, -3L, -4L, -2L, 1L, 2L)
              if (!identical(dim(hc$merge), 3:2) || !identical(merge, exp_merge)) {
                stop(paste(method, "merge", show_vec(merge), "dim", show_vec(dim(hc$merge))))
              }
              if (!identical(as.numeric(hc$height), height)) {
                stop(paste(method, "height", show_vec(hc$height), typeof(hc$height)))
              }
              if (!identical(as.integer(hc$order), c(4L, 3L, 1L, 2L))) {
                stop(paste(method, "order", show_vec(hc$order)))
              }
              method
            }
            d <- dist(cbind(c(0, 0, 1, 3)))
            complete <- run("complete", check("complete", c(0, 1, 3)))
            single <- run("single", check("single", c(0, 1, 2)))
            paste("OK", complete, single, sep=" || ")
            "#,
        )
        .expect("hclust gnu oracle");
    assert!(
        out.contains("OK || complete || single"),
        "hclust oracle script failed: {out}"
    );
}
