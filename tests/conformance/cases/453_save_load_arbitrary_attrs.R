f <- tempfile()
x <- 1:3
attr(x, "label") <- "score"
attr(x, "meta") <- list(source = "unit", version = 2L)
attr(x, "flag") <- TRUE
save(x, file = f, ascii = TRUE)
rm(x)
cat(paste(load(f, envir = .GlobalEnv), collapse = "|"), "\n", sep = "")
cat(paste(names(attributes(x)), collapse = "|"), "\n", sep = "")
cat(attr(x, "label"), "\n", sep = "")
cat(paste(names(attr(x, "meta")), collapse = "|"), "\n", sep = "")
cat(paste(vapply(attr(x, "meta"), typeof, ""), collapse = "|"), "\n", sep = "")
cat(attr(x, "flag"), "\n", sep = "")
invisible(unlink(f))
