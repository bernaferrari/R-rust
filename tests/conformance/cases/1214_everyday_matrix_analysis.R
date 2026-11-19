measurements <- data.frame(a = 1:6, duplicate = 2 * (1:6),
                           group = factor(rep(c("north", "south"), 3)),
                           y = c(4, 5, 9, 10, 13, 15))
design <- model.matrix(~ a + duplicate + group, data = measurements)
fit <- lm(y ~ a + duplicate + group, data = measurements)
diagnosis <- summary(fit)
stopifnot(identical(dim(design), c(6L, 4L)), qr(design)$rank == 3L, fit$rank == 3L,
          identical(diagnosis$aliased,
                    c("(Intercept)" = FALSE, a = FALSE, duplicate = TRUE, groupsouth = FALSE)),
          identical(names(coef(fit))[is.na(coef(fit))], "duplicate"),
          isTRUE(all.equal(unname(fitted(fit)), c(47/12, 21/4, 26/3, 10, 161/12, 59/4),
                           tolerance = 1e-12)),
          max(abs(crossprod(design, residuals(fit)))) < 1e-10)
cat("Rank-deficient matrix and model diagnostics agree\n")
