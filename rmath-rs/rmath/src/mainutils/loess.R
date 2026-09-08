function(formula, data=parent.frame(), weights=NULL, subset=NULL, na.action=na.omit,
         model=FALSE, span=0.75, enp.target=NULL, degree=2L, parametric=FALSE,
         drop.square=FALSE, normalize=TRUE, family=c("gaussian","symmetric"),
         method=c("loess","model.frame"), control=loess.control(...), ...) {
    if (match.arg(method,c("loess","model.frame")) != "loess") stop("LOESS method='model.frame' is not implemented")
    if (!missing(span) && !is.null(enp.target)) {
        warning("both 'span' and 'enp.target' specified: 'span' will be used")
        enp.target <- NULL
    }
    .rport_loess_fit(formula,data,substitute(weights),substitute(subset),span,enp.target,
                    degree,parametric,drop.square,normalize,match.arg(family,c("gaussian","symmetric")),control,
                    model,match.call(),substitute(na.action))
}
