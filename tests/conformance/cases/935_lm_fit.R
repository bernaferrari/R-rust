cat(paste(round(.lm.fit(cbind(1, 1:5), c(1.1, 1.9, 3.2, 3.8, 5.1))$coefficients, 4), collapse = ","), "\n", sep = "")
