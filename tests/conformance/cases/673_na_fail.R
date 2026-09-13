cat(paste(na.pass(c(1, NA, 3)), collapse = ","), "\n", sep = "")
cat(paste(napredict(NULL, 1:3), collapse = ","), "\n", sep = "")
cat(paste(na.fail(c(1, 2, 3)), collapse = ","), "\n", sep = "")
cat(tryCatch(na.fail(c(1, NA)), error = function(e) conditionMessage(e)), "\n", sep = "")
