cm <- matrix(c(0.05, 0.99), 2, 1, dimnames = list(c("a", "b"), "Estimate"))
invisible(capture.output(x <- printCoefmat(cm, P.values = FALSE, has.Pvalue = FALSE, signif.stars = FALSE)))
cat(paste(as.vector(x), collapse = ","), "\n", sep = "")
