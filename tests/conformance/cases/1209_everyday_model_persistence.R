fit_and_save <- function() {
    file <- tempfile("analysis-", fileext = ".rds")
    on.exit(unlink(file))
    cars <- datasets::mtcars
    fit <- lm(mpg ~ wt + factor(am), data = cars)
    estimate <- unname(predict(fit, data.frame(wt = c(2, 3), am = c(0, 1))))
    stopifnot(nobs(fit) == nrow(cars),
              isTRUE(all.equal(estimate, c(26.6159284166051, 21.2395017501425), tolerance = 1e-8)),
              identical(names(coef(fit)), c("(Intercept)", "wt", "factor(am)1")))
    bundle <- list(coefficients = coef(fit), estimate = estimate, formula = deparse(formula(fit)))
    saveRDS(bundle, file)
    stopifnot(identical(readRDS(file), bundle))
}

fit_and_save()
cat("Model fitting, prediction and RDS round trip agree\n")
