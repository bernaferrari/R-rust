setClass("S4SummaryProbe", contains = "numeric")
setMethod("Summary", "S4SummaryProbe", function(x, ..., na.rm = FALSE) {
  cat("METHOD|miss=", missing(na.rm), "|na=", as.character(na.rm), "|sub=", paste(deparse(substitute(na.rm)), collapse=""), "\n", sep="")
  cat("CALL|", paste(deparse(sys.call(), width.cutoff=500L), collapse=""), "\n", sep="")
  gc()
  99L
})
x <- new("S4SummaryProbe", 1:3)
invisible(sum(x))
invisible(sum(x, na.rm=TRUE))
invisible(sum(x, 4L, na.rm=TRUE))
invisible(sum(x, na.rm=TRUE, 4L))
