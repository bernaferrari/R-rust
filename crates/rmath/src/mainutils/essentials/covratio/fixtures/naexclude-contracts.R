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
cat('raw_model_residual_count=',length(m$residuals),' generic_residual_count=',length(stats::residuals(m)),' covratio_count=',length(covratio_original(m)),'\n',sep='')
print(covratio_original(m),digits=17)
cat('na.action class=',class(m$na.action),' positions=',m$na.action,' names=',names(m$na.action),'\n',sep='')
dump<-function(label,x) cat(label,'=',paste(sprintf('%.17g',as.vector(x)),collapse=','),' names=',paste(names(x),collapse=','),'\n',sep='')
dump('raw_residuals',m$residuals);dump('qr',m$qr$qr);dump('qraux',m$qr$qraux);dump('df.residual',m$df.residual)
infl<-list(hat=rep(.2,7),sigma=rep(2,7));res<-1:7;names(res)<-paste0('supplied',1:7)
dump('influence_only',covratio_original(m,infl=infl))
dump('residual_only',suppressWarnings(covratio_original(m,res=1:7)))
dump('both_supplied',covratio_original(m,infl=infl,res=res))
res<-1:8;names(res)<-paste0('custom',1:8)
dump('residual_named_eight',covratio_original(m,res=res))
# Record exact warning counts and expression attribution independently.
for (mode in c("influence_only", "residual_only")) {
    messages <- calls <- character()
    answer <- withCallingHandlers(
        if (mode == "influence_only") covratio_original(m, infl=infl) else covratio_original(m, res=1:7),
        warning=function(w) {
            messages <<- c(messages, conditionMessage(w))
            calls <<- c(calls, paste(deparse(conditionCall(w)), collapse=" "))
            invokeRestart("muffleWarning")
        })
    cat(mode, '_warnings=', length(messages), ' messages=', paste(messages,collapse=' | '),
        ' calls=', paste(calls,collapse=' | '), '\n', sep='')
}
fractional <- m
fractional$na.action <- structure(3.9, class='exclude', names='case3')
dump('fractional_exclude',covratio_original(fractional))
invalid <- m
invalid$na.action <- structure(10, class='exclude', names='missing')
cat('invalid_names=',tryCatch(suppressWarnings(covratio_original(invalid)),error=conditionMessage),'\n',sep='')
invalid$na.action <- structure(NA_real_,class='exclude',names='missing')
cat('invalid_missing_position=',tryCatch(covratio_original(invalid),error=conditionMessage),'\n',sep='')
