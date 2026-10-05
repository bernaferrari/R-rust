use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn logical_recycling_matches_pinned_gnu_values_and_warning_admission() {
    // Independently executed with pinned GNU R 4.7.0 r90451, with warn=2.
    let code = r#"options(warn=2)
stopifnot(identical((1:5)[c(TRUE,FALSE)],c(1L,3L,5L)))
stopifnot(identical((1:3)[c(TRUE,NA)],c(1L,NA_integer_,3L)))
m<-matrix(1:10,5,2)
stopifnot(identical(m[c(TRUE,FALSE),],m[c(1L,3L,5L),]))
stopifnot(identical(m[,c(TRUE,FALSE)],1:5))
x<-1:5; x[c(TRUE,FALSE)]<-9L
stopifnot(identical(x,c(9L,2L,9L,4L,9L)))
stopifnot(identical((1:3)[c(TRUE,FALSE,TRUE,TRUE)],c(1L,3L,NA_integer_)))
stopifnot(inherits(tryCatch(m[c(TRUE,FALSE,TRUE,FALSE,TRUE,TRUE),],error=identity),'error'))
cat('logical recycling: complete public family passed\n')
"#;
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap(),
    ] {
        assert_eq!(
            session.eval(code).unwrap(),
            "logical recycling: complete public family passed\n"
        );
    }
}
