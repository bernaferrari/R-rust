use r_embed::{RSession, RuntimePathPolicy};

// GNU R bac583951b728e97b9786804d3b4081f0fe18df5, revision 90451.
const CONTROLS: &str = r#"
stopifnot(identical(seq.int(1, 3, length.out="3"), 1:3),
          identical(seq.int("1", "3", by="1"), 1:3))
stopifnot(identical(rep_len(1:3, "5"), c(1L, 2L, 3L, 1L, 2L)),
          identical(rep(1:2, length.out="5"), c(1L, 2L, 1L, 2L, 1L)),
          identical(rep(1:2, each="2"), c(1L, 1L, 2L, 2L)),
          identical(rep.int(1:2, "2"), c(1L, 2L, 1L, 2L)))
events <- character()
z <- withCallingHandlers(
    tryCatch(seq.int(1, 3, length.out="bad"), error=function(e) {
        events <<- c(events, paste("error", conditionMessage(e)))
        NULL
    }), warning=function(w) {
        events <<- c(events, paste("warning", conditionMessage(w)))
        invokeRestart("muffleWarning")
    })
stopifnot(is.null(z), identical(events, c("warning NAs introduced by coercion",
    "error 'length.out' must be a finite number")))
warning_text <- ""
z <- withCallingHandlers(seq.int(1, 3, length.out=3+2i), warning=function(w) {
    warning_text <<- conditionMessage(w)
    invokeRestart("muffleWarning")
})
stopifnot(identical(z, 1:3),
          identical(warning_text, "imaginary parts discarded in coercion"))
z <- ksmooth(1:3, c(2, 4, 8), n.points=2.1)
if (!identical(z$x, c(1, 2, 3))) stop("fractional grid differs")
z <- ksmooth(1:3, c(2, 4, 8), n.points="3")
if (!identical(z$x, c(1, 2, 3))) stop("string grid differs")
z <- ksmooth(1:3, c(2, 4, 8), n.points=NULL)
if (!identical(z$x, c(1, 2, 3))) stop("NULL grid differs")
z <- ksmooth(1:3, c(2, 4, 8), n.points=TRUE)
stopifnot(identical(z$x, 1), identical(z$y, 2))
for (n in list(NA_real_, Inf, numeric(0))) {
    msg <- tryCatch(ksmooth(1:3, 1:3, n.points=n), error=conditionMessage)
    stopifnot(identical(msg, "'length.out' must be a finite number"))
}
msg <- tryCatch(ksmooth(1:3, 1:3, n.points=-1), error=conditionMessage)
stopifnot(identical(msg, "'length.out' must be a non-negative number"))
# n.points is unused when x.points is supplied.
z <- ksmooth(1:3, 1:3, n.points=NA_real_, x.points=c(1, 3))
stopifnot(identical(z$x, c(1, 3)), identical(z$y, c(1, 3)))
cat("ksmooth-grid-length-ok\n")
"#;

#[test]
fn ksmooth_prediction_grid_matches_pinned_gnu_length_semantics() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(output.contains("ksmooth-grid-length-ok"), "{output}");
    }
}
