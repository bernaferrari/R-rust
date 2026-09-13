mf <- list(`(weights)` = c(1, 2, 1), y = 1:3)
cat(paste(model.weights(mf), collapse = ","), "\n", sep = "")
cat(is.null(model.weights(list(y = 1:3))), "\n", sep = "")
