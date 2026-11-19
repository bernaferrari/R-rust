predict_and_export <- function() {
    observations <- data.frame(
        x = c(-3, -2, -1, 0, 1, 2, 3, 4, 5, 6),
        event = c(0L, 1L, 0L, 0L, 1L, 0L, 1L, 1L, NA_integer_, 1L))
    fit <- glm(event ~ x, observations, family = binomial(), na.action = na.exclude)
    stopifnot(isTRUE(fit$converged), nobs(fit) == 9L,
              length(residuals(fit)) == nrow(observations),
              identical(unname(which(is.na(residuals(fit)))), 9L))

    predictions <- data.frame(x = c(-1, 0, 1))
    predictions$probability <- unname(predict(fit, predictions, type = "response"))
    expected <- c(0.335054976676748, 0.449814658816348, 0.570176323883755)
    stopifnot(isTRUE(all.equal(predictions$probability, expected, tolerance = 1e-8)))

    path <- tempfile(fileext = ".csv")
    on.exit(unlink(path))
    write.csv(predictions, path, row.names = FALSE)
    reloaded <- read.csv(path)
    stopifnot(identical(names(reloaded), c("x", "probability")),
              identical(reloaded$x, c(-1L, 0L, 1L)),
              isTRUE(all.equal(reloaded$probability, expected, tolerance = 1e-8)))
}

predict_and_export()
cat("Binomial prediction and probability export agree\n")
