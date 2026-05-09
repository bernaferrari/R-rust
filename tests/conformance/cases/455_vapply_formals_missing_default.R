fun <- function(x, y = 2L) x + y
cat(paste(vapply(formals(fun), typeof, ""), collapse = "|"), "\n", sep = "")
cat(paste(names(vapply(formals(fun), typeof, "")), collapse = "|"), "\n", sep = "")
cat(paste(sapply(formals(fun), typeof), collapse = "|"), "\n", sep = "")
cat(paste(names(sapply(formals(fun), typeof)), collapse = "|"), "\n", sep = "")
