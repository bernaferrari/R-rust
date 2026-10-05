# Pinned original GNU source bac583951b728e97b9786804d3b4081f0fe18df5.
# Preserve the original helper body and namespace; only the model's supplied
# na.action is varied to exercise the documented row-replacement expression.
naresid_original <- getFromNamespace('naresid.exclude', 'stats')
stopifnot(identical(body(naresid_original), body(stats:::naresid.exclude)))
d <- data.frame(x=1:8, y=c(1,3,2,5,4,7,6,9), row.names=paste0('row',1:8))
m <- stats::lm(y~x, data=d)
m$na.action <- structure(10, class='exclude', names='missing')
options(warn=0)
withCallingHandlers(tryCatch(stats::covratio(m), error=function(e)cat('downstream_error=',conditionMessage(e),'\n',sep='')), warning=function(w) {
    cat('row_warning_call=',paste(deparse(conditionCall(w)),collapse=' '),'\n',sep='')
    cat('row_warning_message=',conditionMessage(w),'\n',sep='')
    invokeRestart('muffleWarning')
})
options(warn=2)
tryCatch(stats::covratio(m),error=function(e) {
    cat('warn2_call=',paste(deparse(conditionCall(e)),collapse=' '),'\n',sep='')
    cat('warn2_message=',conditionMessage(e),'\n',sep='')
})
options(warn=0)
unname_model <- m
names(unname_model$residuals) <- NULL
supplied <- list(hat=rep(.2,8),sigma=rep(2,8))
supplied_calls <- character()
values <- withCallingHandlers(stats::covratio(unname_model,infl=supplied),warning=function(w) {
    supplied_calls <<- c(supplied_calls,paste(deparse(conditionCall(w)),collapse=' '))
    invokeRestart('muffleWarning')
})
cat('supplied_calls=',paste(supplied_calls,collapse=' || '),'\n',sep='')
cat('supplied_values=',paste(sprintf('%.17g',unname(values)),collapse=','),'\n',sep='')
