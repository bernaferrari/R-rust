use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn public_structure_signals_classed_deprecations_before_remapping() {
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap(),
    ] {
        let result=session.eval(r#"
            options(useFancyQuotes=FALSE)
            x <- 1:4; warnings <- list()
            y <- withCallingHandlers(structure(x,.Dim=c(2L,2L),.Dimnames=list(c('A','B'),c('1','2'))),warning=function(w) {warnings[[length(warnings)+1L]] <<- w; invokeRestart('muffleWarning')})
            w <- warnings[[1L]]
            stopifnot(length(warnings)==1L,inherits(w,'deprecatedWarning'))
            stopifnot(identical(conditionMessage(w),"Replacing special names '.Dim', '.Dimnames' is deprecated; use 'dim', 'dimnames' instead."))
            stopifnot(identical(w$old,'structure'),is.null(w$new),is.null(w$package))
            stopifnot(identical(conditionCall(w),quote(structure(x,.Dim=c(2L,2L),.Dimnames=list(c('A','B'),c('1','2'))))))
            stopifnot(identical(x,1:4),identical(y,matrix(x,2L,2L,dimnames=list(c('A','B'),c('1','2')))))
            options(warn=2)
            msg <- tryCatch(structure(1,.Names='a'),error=conditionMessage)
            stopifnot(grepl('Replacing special names',msg,fixed=TRUE))
            stopifnot(identical(structure(1,names='a'),c(a=1)))
            TRUE
        "#).unwrap();
        assert_eq!(result.trim(), "[1] TRUE");
    }
}

#[test]
fn deparse_round_trips_missing_character_values_and_names() {
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap(),
    ] {
        let result = session
            .eval(
                r#"
            values <- list(NA_character_,rep(NA_character_,3),c(NA,'NA'),c('NA',NA),
                setNames(as.list(c(1,2,99)),c('A','NA',NA)),
                setNames(c(1,2,99),c('A','NA',NA)),
                structure(matrix(c('NA',NA,'',NA),2),dimnames=list(c(NA,'NA'),c('x','y'))))
            for(x in values) {
                txt <- deparse(x)
                stopifnot(identical(x,eval(parse(text=txt))))
                txt <- capture.output(dput(x))
                stopifnot(identical(x,eval(parse(text=txt))))
            }
            stopifnot(identical(deparse(NA_character_),'NA_character_'))
            stopifnot(identical(deparse(c('NA',NA)),'c("NA", NA)'))
            TRUE
        "#,
            )
            .unwrap();
        assert_eq!(result.trim(), "[1] TRUE");
    }
}
