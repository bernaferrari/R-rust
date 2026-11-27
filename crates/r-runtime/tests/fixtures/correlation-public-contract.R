probe <- function(label, expr) {
    warnings <- character()
    result <- tryCatch(withCallingHandlers(force(expr), warning=function(e) {
        warnings <<- c(warnings, conditionMessage(e))
        invokeRestart("muffleWarning")
    }), error=function(e) list(error=conditionMessage(e)))
    if (is.numeric(result)) result <- list(type=typeof(result), dim=dim(result),
        dimnames=dimnames(result), names=names(result), values=as.vector(result),
        is.NA=as.vector(is.na(result)), is.NaN=as.vector(is.nan(result)))
    print(list(case=label, result=result, warnings=warnings))
    invisible(NULL)
}
print(c(numeric_integer=is.numeric(1L), numeric_double=is.numeric(1),
    numeric_factor=is.numeric(factor("a")), numeric_ordered=is.numeric(ordered("a")),
    integer_factor=is.integer(factor("a")), integer_ordered=is.integer(ordered("a")),
    numeric_complex=is.numeric(1+1i), numeric_logical=is.numeric(TRUE)))
x <- cbind(a=c(1,2,NA,4,5), b=c(2,NA,6,8,10), c=c(5,4,3,2,1))
y <- cbind(u=c(5,NA,3,2,1), v=c(2,4,6,8,10))
for (method in c("pearson", "spearman", "kendall")) {
    for (use in c("everything", "all.obs", "complete.obs", "na.or.complete", "pairwise.complete.obs")) {
        probe(paste("self",method,use), cor(x, use=use, method=method))
        probe(paste("cross",method,use), cor(x, y, use=use, method=method))
    }
}
probe("single-column self", cor(matrix(1:4,4,1)))
probe("single-column cross", cor(matrix(1:4,4,1), matrix(4:1,4,1)))
probe("vector matrix", cor(1:4, cbind(a=4:1)))
probe("logical vectors", cor(c(TRUE,FALSE,TRUE), c(FALSE,TRUE,TRUE)))
probe("unequal vectors", cor(1:3,1:2))
probe("unequal matrix rows", cor(matrix(1:6,3,2),matrix(1:4,2,2)))
probe("nonnumeric x", cor(c("1","2"),1:2))
probe("nonnumeric y", cor(1:2,c("1","2")))
probe("factor", cor(factor(c("a","b")),1:2))
probe("missing y", cor(1:3))
probe("invalid mode", cor(1:3,1:3,use="never"))
probe("invalid method", cor(1:3,1:3,method="never"))
for (use in c("everything", "all.obs", "complete.obs", "na.or.complete", "pairwise.complete.obs")) {
    probe(paste("empty",use), cor(numeric(),numeric(),use=use))
    probe(paste("no complete",use), cor(c(NA,NaN),1:2,use=use))
    probe(paste("one complete",use), cor(c(1,NA),c(2,NA),use=use))
    probe(paste("constant self",use), cor(cbind(a=c(1,1),b=c(2,3)),use=use))
}
for (method in c("pearson", "spearman", "kendall")) {
    for (use in c("everything", "complete.obs", "pairwise.complete.obs")) {
        probe(paste("constant vector",method,use), cor(c(1,1),c(2,3),method=method,use=use))
    }
}
probe("native invalid method", .Call(stats:::C_cor,1:3,1:3,0L,FALSE))
probe("native null x", .Call(stats:::C_cor,NULL,1:3,4L,FALSE))
probe("infinite Pearson", cor(c(1,Inf,3),1:3))
probe("infinite Spearman", cor(c(1,Inf,3),1:3,method="spearman"))
print(identical(cor(matrix(1:4,nrow=2),method="spearman"),
    matrix(c(1,1-.Machine$double.eps,1-.Machine$double.eps,1),2,2)))
probe("tied Spearman", cor(c(1,1,3),c(2,4,4),method="spearman"))
probe("tied Kendall", cor(c(1,1,3),c(2,4,4),method="kendall"))
probe("large offset", cor(1e12+c(1,2,4,8),1e12+c(2,4,1,3)))
probe("dataframe wrapper", cor(data.frame(a=1:4,b=4:1)))
probe("no columns everything", cor(matrix(numeric(),3,0)))
probe("no columns pairwise", cor(matrix(numeric(),3,0),use="pairwise.complete.obs"))
rm(probe,x,y)
invisible(NULL)
