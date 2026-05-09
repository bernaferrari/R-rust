f <- tempfile()
x <- 1:4
tsp(x) <- c(2000, 2003, 1)
class(x) <- "ts"
save(x, file = f, ascii = TRUE)
rm(x)
cat(paste(load(f, envir = .GlobalEnv), collapse = "|"), "\n", sep = "")
cat(paste(tsp(x), collapse = "|"), "\n", sep = "")
cat(paste(class(x), collapse = "|"), "\n", sep = "")
cat(paste(as.vector(x), collapse = "|"), "\n", sep = "")
