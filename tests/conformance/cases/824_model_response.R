mf <- model.frame(y ~ x, list(y = 1:3, x = 4:6))
cat(paste(model.response(mf), collapse = ","), "\n", sep = "")
