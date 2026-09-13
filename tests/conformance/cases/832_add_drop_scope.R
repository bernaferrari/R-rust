t <- terms(y ~ x)
cat(paste(drop.scope(t), collapse = ","), "\n", sep = "")
cat(paste(add.scope(t, ~x + z), collapse = ","), "\n", sep = "")
