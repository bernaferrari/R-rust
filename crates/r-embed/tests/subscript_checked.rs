use r_embed::RSession;

#[test]
fn numeric_and_named_subscripts_match_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"v <- c(10L,20L,30L,40L)
stopifnot(
  identical(v[c(4L,0L,1L,NA_integer_)], c(40L,10L,NA_integer_)),
  identical(v[c(-1L,0L,-4L)], c(20L,30L)),
  identical(v[c(-1.9,-3.1)], c(20L,40L)),
  identical(v[c(0.9,3.8,NA_real_)], c(30L,NA_integer_)))
named <- c(a=1L)
named[c('b','b','c')] <- c(9L,8L,7L)
stopifnot(identical(named, c(a=1L,b=8L,c=7L)))
cat('subscript-checked: PASS\n')"#
            )
            .unwrap(),
        "subscript-checked: PASS\n"
    );
}
