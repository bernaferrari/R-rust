use r_embed::RSession;

const DIST_EMPTY_SCRIPT: &str = r##"
    empty_rows <- dist(matrix(numeric(), nrow=0, ncol=1))
    stopifnot(length(empty_rows) == 0L)
    stopifnot(identical(attr(empty_rows, "Size"), 0L))
    stopifnot(inherits(empty_rows, "dist"))
    stopifnot(identical(attr(empty_rows, "method"), "euclidean"))

    empty_columns <- dist(matrix(numeric(), nrow=2, ncol=0))
    stopifnot(length(empty_columns) == 1L)
    stopifnot(is.na(as.vector(empty_columns)))
    stopifnot(identical(attr(empty_columns, "Size"), 2L))
    stopifnot(inherits(empty_columns, "dist"))

    empty_both <- dist(matrix(numeric(), nrow=0, ncol=0))
    stopifnot(length(empty_both) == 0L)
    stopifnot(identical(attr(empty_both, "Size"), 0L))
    stopifnot(inherits(empty_both, "dist"))

    ordinary <- dist(matrix(c(1, 2, 4, 8), nrow=2, ncol=2))
    stopifnot(isTRUE(all.equal(as.numeric(ordinary), sqrt(17))))
    stopifnot(identical(attr(ordinary, "Size"), 2L))
    stopifnot(identical(attr(ordinary, "method"), "euclidean"))
    cat("dist-empty-dimensions: PASS\n")
    "##;

fn assert_dist_empty_dimensions(session: &mut RSession) {
    assert_eq!(
        session.eval(DIST_EMPTY_SCRIPT).unwrap(),
        "dist-empty-dimensions: PASS\n"
    );
}

#[test]
fn native_dist_handles_empty_dimensions_and_ordinary_data() {
    let mut session = RSession::new().unwrap();
    assert_dist_empty_dimensions(&mut session);
}

#[test]
fn portable_dist_handles_empty_dimensions_and_ordinary_data() {
    let mut session = RSession::new_with_path_policy(r_embed::RuntimePathPolicy::new(
        Vec::new(),
        std::env::temp_dir(),
    ))
    .unwrap();
    assert_dist_empty_dimensions(&mut session);
}
