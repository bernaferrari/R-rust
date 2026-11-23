use r_embed::RSession;

#[test]
fn native_cutree_matches_gnu_and_rejects_invalid_merge_references() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"merge <- matrix(c(-1L,-3L,1L,-2L,-4L,2L),3L,2L)
want <- matrix(c(1L,2L,3L,4L,1L,1L,2L,2L,1L,1L,2L,2L,1L,1L,1L,1L,1L,1L,2L,3L),4L,5L)
f <- function(merge, k) .Call(stats:::C_cutree, merge, k)
stopifnot(identical(f(merge,c(4L,2L,2L,1L,3L)),want),
          identical(f(matrix(integer(),0L,2L),c(1L,1L)),matrix(c(1L,1L),1L,2L)),
          identical(f(merge,integer()),matrix(integer(),4L,0L)))
for (bad in c(NA_integer_, -3L, 0L, 1L)) {
    err <- tryCatch(f(matrix(c(bad,-2L),1L,2L),1L), error=function(e) conditionMessage(e))
    stopifnot(identical(err, "invalid 'tree' ('merge' component)"))
}
cat('cutree-ok\n')
"#
            )
            .unwrap(),
        "cutree-ok\n"
    );
}
