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
dump <- function(label,x) cat(label,"=",paste(sprintf("%.17g",x),collapse=",")," names=",paste(names(x),collapse=","),"\n",sep="")
model <- stats::glm(c(0,0,1,0,1,1,0,1)~seq_len(8),family=stats::binomial())
infl <- list(hat=rep(c(.1,.2),4),sigma=rep(c(.7,.8),4))
dump("fixed_glm",covratio_original(model,infl=infl,res=1:8))
model$family$family <- "gaussian"; model$family$dispersion <- NA_real_
dump("estimated_glm",covratio_original(model,infl=infl,res=1:8))
d <- data.frame(y=c(1,3,2,5,4,7,6,9),x=1:8,w=c(1,0,2,3,0,1,4,2),row.names=paste0("weighted",1:8))
fit <- stats::lm(y~x,data=d,weights=w)
infl <- stats::lm.influence(fit,do.coef=FALSE)
dump("weighted_model_residuals",fit$residuals);dump("weights",fit$weights);dump("weighted_residuals",stats::weighted.residuals(fit));dump("weighted_hat",infl$hat);dump("weighted_sigma",infl$sigma);dump("weighted_covratio",covratio_original(fit));dump("weighted_rank",fit$rank);dump("qr_rows",nrow(fit$qr$qr))

dump("weighted_qr", as.vector(fit$qr$qr)); dump("weighted_qraux",fit$qr$qraux); dump("weighted_qr_rank",fit$qr$rank)

dump("glm_qr",as.vector(model$qr$qr));dump("glm_qraux",model$qr$qraux)
model$family$dispersion <- 4
dump("explicit_glm",covratio_original(model,infl=list(hat=rep(c(.1,.2),4),sigma=rep(c(.7,.8),4)),res=1:8))

model$family <- stats::binomial()
model$residuals <- 1:8
model$weights <- rep(2,8)
model$prior.weights <- c(1,0,1,1,0,1,1,1)
dump("glm_prior_weight_filter",covratio_original(model,infl=list(hat=rep(c(.1,.2),3),sigma="unused")))
