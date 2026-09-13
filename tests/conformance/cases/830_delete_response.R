t <- terms(y ~ x)
cat(deparse(delete.response(t)), "\n", sep = "")
cat(paste(class(t), collapse = ","), "\n", sep = "")
cat(attr(t, "response"), "\n", sep = "")
