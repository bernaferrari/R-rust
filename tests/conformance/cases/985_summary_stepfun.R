sf <- stepfun(1:3, c(0, 1, 2, 3))
invisible(capture.output(summary.stepfun(sf)))
cat(paste(knots(sf), collapse = ","), "\n", sep = "")
