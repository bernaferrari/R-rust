existing <- "."
missing <- "rport-normalize-missing"

paths <- normalizePath(c(existing, missing), mustWork = FALSE)
print(length(paths))
print(basename(paths[1]))
print(paths[2])

named <- normalizePath(path = existing, mustWork = TRUE, winslash = "/")
print(basename(named))

print(is.na(normalizePath(NA_character_, mustWork = FALSE)))
err <- tryCatch(
  normalizePath(missing, mustWork = TRUE),
  error = function(e) paste("ERR", grepl("No such file or directory", conditionMessage(e)))
)
print(err)
