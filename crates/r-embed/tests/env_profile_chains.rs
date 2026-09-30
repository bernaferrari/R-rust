//! reg-tests-1e.R: tiny hashed environments must grow for `size <= 4`.
//!
//! GNU R 4.6.1 pins `env.profile` nchains and sizes for
//! `list2env(hash=TRUE, size=1..6)` of 123 named NULL elements.

use r_embed::RSession;

#[test]
fn small_hashed_list2env_profiles_match_gnu() {
    let mut session = RSession::new().unwrap();
    let report = session
        .eval(
            r#"
            n <- 123
            l <- setNames(vector("list", n), seq_len(n))
            ehLs <- lapply(1:6, function(sz) list2env(l, hash = TRUE, size = sz))
            nch <- vapply(ehLs, function(.) env.profile(.)$nchains, 0L)
            sizes <- vapply(ehLs, function(.) env.profile(.)$size, 0L)
            cat(paste0(
                "NCH=", paste(nch, collapse = ","), "\n",
                "SIZES=", paste(sizes, collapse = ","), "\n",
                "LS=", length(ls(ehLs[[1]])), "\n",
                "NULL1=", is.null(ehLs[[1]][["1"]]), "\n",
                "EXISTS=", exists("1", ehLs[[1]], inherits = FALSE), "\n"
            ))
            "#,
        )
        .unwrap_or_else(|err| panic!("env profile eval failed: {err}"));
    let report = report.trim();
    assert!(
        report.lines().any(|line| line == "NCH=106,106,106,106,106,111"),
        "{report}"
    );
    assert!(
        report.lines().any(|line| line == "SIZES=146,146,146,146,146,143"),
        "{report}"
    );
    assert!(report.lines().any(|line| line == "LS=123"), "{report}");
    assert!(
        report.lines().any(|line| line == "NULL1=TRUE"),
        "{report}"
    );
    assert!(
        report.lines().any(|line| line == "EXISTS=TRUE"),
        "{report}"
    );
}
