use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn cumulative_sum_and_product_match_gnu_missing_values_and_warnings() {
    let mut s = RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        s.eval(
            r#"
x <- c(a=1,b=NaN,c=2,d=NA_real_,e=3)
expected <- c(a=1,b=NaN,c=NaN,d=NA_real_,e=NA_real_)
stopifnot(identical(cumsum(x), expected), identical(cumprod(x), expected),
          identical(cumsum(c(Inf,-Inf,NA_real_)), c(Inf,NaN,NA_real_)),
          identical(cumprod(c(0,Inf,NA_real_)), c(0,NaN,NA_real_)),
          identical(cumprod(c(2L,3L)), c(2,6)),
          identical(cumsum(c(a=TRUE,b=FALSE,c=NA)), c(a=1L,b=1L,c=NA_integer_)),
          identical(cumsum(integer()), integer()), identical(cumprod(complex()), complex()))
z <- complex(real=c(1,NaN,3,NA,5), imaginary=c(2,1,2,3,4))
s <- cumsum(z)
p <- cumprod(z)
stopifnot(identical(Re(s),c(1,NaN,NaN,NA_real_,NA_real_)), identical(Im(s),c(2,3,5,8,12)),
          identical(Re(p),c(1,NaN,NaN,NA_real_,NA_real_)), identical(Im(p),c(2,NaN,NaN,NA_real_,NA_real_)))
events <- character()
v <- withCallingHandlers(cumsum(c(2147483647L,1L,3L)), warning=function(w) {
    events <<- c(events, conditionMessage(w))
    gc()
    invokeRestart("muffleWarning")
})
stopifnot(identical(v,c(2147483647L,NA_integer_,NA_integer_)),
          identical(events,"integer overflow in 'cumsum'; use 'cumsum(as.numeric(.))'"),
          identical(suppressWarnings(cumsum(c(-2147483647L,-1L,3L))),c(-2147483647L,NA_integer_,NA_integer_)))
cat("cumulative-contract-ok\n")
"#
        )
        .unwrap(),
        "cumulative-contract-ok\n"
    );
}
