use r_embed::RSession;

const ORACLE: &str = include_str!("fixtures/array_attributes_gnu.R");

#[test]
fn tsp_assignment_matches_pinned_gnu_attribute_contract() {
    let mut session = RSession::new().expect("session");
    session.eval(ORACLE).expect("oracle definitions");
    assert_eq!(
        session
            .eval("verify_tsp(); cat('tsp-ok')")
            .expect("tsp contract"),
        "tsp-ok"
    );
}

#[test]
fn singleton_array_drop_keeps_only_unambiguous_names() {
    let mut session = RSession::new().expect("session");
    session.eval(ORACLE).expect("oracle definitions");
    assert_eq!(
        session
            .eval("verify_drop(); cat('drop-ok')")
            .expect("drop contract"),
        "drop-ok"
    );
}

#[test]
fn asplit_preserves_slices_and_dimnames_under_gctorture() {
    let mut session = RSession::new().expect("session");
    assert_eq!(
        session
            .eval(
                r#"x <- matrix(1:6, 2L, 3L, dimnames=list(c('a','b'), letters[1:3]))
split <- function(x) {
    gctorture(TRUE)
    on.exit(gctorture(FALSE))
    asplit(x, 1L)
}
z <- split(x)
stopifnot(identical(dim(z), 2L), identical(dimnames(z), list(c('a','b'))),
          identical(as.integer(z[[1L]]), c(1L,3L,5L)),
          identical(as.integer(z[[2L]]), c(2L,4L,6L)),
          identical(dim(z[[1L]]), 3L), identical(dimnames(z[[1L]]), list(letters[1:3])))
cat('asplit-ok\n')
"#
            )
            .expect("asplit contract"),
        "asplit-ok\n"
    );
}
