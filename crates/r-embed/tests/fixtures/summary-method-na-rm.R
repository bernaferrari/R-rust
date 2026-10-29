Summary.summary_probe <- function(..., na.rm=FALSE) {
  invisible(gc())
  dots <- list(...)
  call <- paste(deparse(sys.call(), width.cutoff=500L), collapse="")
  substituted <- paste(deparse(substitute(na.rm), width.cutoff=500L), collapse="")
  cat("CALL|", call, "\n", sep="")
  cat("SUB|", substituted, "|MISS|", missing(na.rm), "|VAL|", as.character(na.rm), "\n", sep="")
  cat("DOT|", paste(names(dots), collapse=","), "|", paste(unlist(dots), collapse=","), "\n", sep="")
}
x <- structure(1:3, class="summary_probe")
sym <- TRUE
left_value <- 4
invisible(sum(x))
invisible(sum(x, na.rm=TRUE))
invisible(sum(x, left=4, na.rm=TRUE, right=5))
invisible(sum(x, na.rm=TRUE, left=4, right=5))
invisible(sum(x, na.r=TRUE, left=4, right=5))
invisible(sum(x, na.rm=sym))
invisible(sum(x, FALSE))
invisible(sum(x, left=4))
invisible(sum(x, left=left_value, right=6))
