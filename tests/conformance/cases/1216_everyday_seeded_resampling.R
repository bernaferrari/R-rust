times <- c(11, 19, 25, 14, 32, 21, 16, 48, 24, 37, 30, 27)
boot_mean <- function(x, seed) {
    set.seed(seed)
    replicate(400L, mean(x[sample.int(length(x), replace = TRUE)]))
}

estimates <- boot_mean(times, 379L)
invisible(runif(12L))
stopifnot(identical(estimates, boot_mean(times, 379L)))
cat("first estimates: ", paste(sprintf("%.6f", estimates[1:6]), collapse = " "), "\n", sep = "")
cat("90% interval: ", paste(sprintf("%.6f", quantile(estimates, c(.05, .95))), collapse = " "), "\n", sep = "")
cat("bias: ", sprintf("%.6f", mean(estimates) - mean(times)), "\n", sep = "")
