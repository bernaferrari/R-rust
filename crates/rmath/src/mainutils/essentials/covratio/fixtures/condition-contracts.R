# Pinned GNU R source revision bac583951b728e97b9786804d3b4081f0fe18df5.
# Copied without rewriting from pinned stats::covratio; helpers remain in its original namespace.
covratio_original <- function(model, infl = lm.influence(model, do.coef = FALSE), res = weighted.residuals(model)) {
    n <- nrow(qr.lm(model)$qr)
    p <- model$rank
    omh <- 1 - infl$hat
    e.star <- if (inherits(model, "glm") && !estDisp(model$family))
        res/(sigma(model) * sqrt(omh))
    else res/(infl$sigma * sqrt(omh))
    e.star[is.infinite(e.star)] <- NaN
    1/(omh * (((n - p - 1) + e.star^2)/(n - p))^p)
}
environment(covratio_original) <- asNamespace("stats")
stopifnot(identical(body(covratio_original), body(stats::covratio)))
d<-data.frame(y=c(1,3,NA,5,4,7,6,9),x=1:8,row.names=paste0('case',1:8))
m<-stats::lm(y~x,data=d,na.action=stats::na.exclude)

probe <- function(label, model, infl, res) {
    messages <- calls <- character()
    invisible(withCallingHandlers(covratio_original(model, infl=infl, res=res), warning=function(w) {
        messages <<- c(messages,conditionMessage(w))
        calls <<- c(calls,paste(deparse(conditionCall(w)),collapse=" "))
        invokeRestart("muffleWarning")
    }))
    cat(label,":count=",length(calls),":calls=",paste(calls,collapse=" | "),":messages=",paste(messages,collapse=" | "),"\n",sep="")
}
probe("denominator_only",m,list(hat=c(.2,.4,.1),sigma=c(2,3)),1:6)
probe("denominator_and_final",m,list(hat=rep(.2,7),sigma=rep(2,8)),1:8)
probe("studentized_and_final",m,list(hat=rep(.2,7),sigma=rep(2,7)),1:8)
g <- stats::glm(c(0,1,0,1,1,0,1,0) ~ seq_len(8),family=stats::binomial())
probe("fixed_glm",g,list(hat=rep(.2,8),sigma=rep(2,3)),1:7)
options(warn=2)
tryCatch(covratio_original(m,infl=list(hat=rep(.2,7),sigma=rep(2,7))),error=function(e)
    cat("warn2:call=",paste(deparse(conditionCall(e)),collapse=" "),":message=",conditionMessage(e),"\n",sep=""))
