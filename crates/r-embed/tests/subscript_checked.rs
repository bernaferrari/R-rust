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

#[test]
fn matrix_subscripts_match_gnu_early_exit_and_name_rules() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"x <- matrix(1:6, 2L, dimnames=list(c('a','b'),c('c','d','e')))
stopifnot(
  identical(
    x[rbind(c(0,NA),c(NA,0),c(0,99),c(2,3))],
    c(NA_integer_,6L)
  ),
  identical(
    x[rbind(c(NA,99),c(1,1))],
    c(NA_integer_,1L)
  ),
  identical(
    x[rbind(c(NA,'c'),c('b','e'))],
    c(NA_integer_,6L)
  )
)
y <- x
dimnames(y) <- list(NULL,c('c','d','e'))
stopifnot(identical(y[rbind(c(NA,'c'))],NA_integer_))
z <- x
dimnames(z) <- list(c('','b'),c('c','d','e'))
stopifnot(
  grepl(
    'subscript out of bounds',
    tryCatch(z[rbind(c('','c'))],error=conditionMessage),
    fixed=TRUE
  )
)
warning_text <- ''
value <- withCallingHandlers(
  x[rbind(c(Inf,1),c(1.9,2.9))],
  warning=function(w) {
    warning_text <<- conditionMessage(w)
    invokeRestart('muffleWarning')
  }
)
stopifnot(
  identical(value,c(NA_integer_,3L)),
  identical(warning_text,'NAs introduced by coercion to integer range')
)
x[rbind(c('b','e'),c('a','c'))] <- c(9L,8L)
stopifnot(
  identical(unname(x),matrix(c(8L,2L,3L,4L,5L,9L),2L))
)
cat('matrix-subscript-checked: PASS\n')"#
            )
            .unwrap(),
        "matrix-subscript-checked: PASS\n"
    );
}
