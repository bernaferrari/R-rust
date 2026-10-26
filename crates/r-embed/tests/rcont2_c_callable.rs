use r_embed::RSession;

#[test]
fn r2dtable_uses_repeatable_session_rng_and_preserves_margins() {
    let mut session = RSession::new().unwrap();
    let output = session
        .eval(
            r#"
set.seed(2718)
first <- r2dtable(3L, c(3L, 5L), c(2L, 6L))
set.seed(2718)
second <- r2dtable(3L, c(3L, 5L), c(2L, 6L))
stopifnot(identical(first, second))
stopifnot(all(vapply(first, function(x) identical(as.integer(rowSums(x)), c(3L, 5L)), logical(1))))
stopifnot(all(vapply(first, function(x) identical(as.integer(colSums(x)), c(2L, 6L)), logical(1))))
TRUE
"#,
        )
        .unwrap();
    assert_eq!(output, "[1] TRUE\n");
    session.close();
}
