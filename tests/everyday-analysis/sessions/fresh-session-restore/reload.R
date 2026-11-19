stopifnot(!exists("analysis", inherits = FALSE),
          !exists("observations", inherits = FALSE))
restored <- new.env()
stopifnot(identical(sort(load("analysis.RData", envir = restored)),
                    c("analysis", "observations")),
          inherits(restored$analysis, "lm"),
          identical(restored$observations$hours, 1:6),
          identical(restored$observations$score, c(3, 5, 8, 10, 12, 15)))
estimate <- unname(predict(restored$analysis, newdata = data.frame(hours = c(2, 8))))
stopifnot(isTRUE(all.equal(estimate, c(5.27619047619048, 19.5047619047619),
                           tolerance = 1e-12)))
cat("Fresh session reproduced the saved model predictions\n")
