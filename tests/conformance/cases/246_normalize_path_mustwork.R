## normalizePath parity, chdir-neutral: every fixture lives in a per-run
## tempfile() directory so the golden never embeds the checkout path.
base <- tempfile("rport-norm-")
existing <- file.path(base, "existing-dir")
stopifnot(dir.create(base), dir.create(existing))
missing <- file.path(base, "missing-entry")

paths <- normalizePath(c(existing, missing), mustWork = FALSE)
print(length(paths))
cat("exists-dir:", dir.exists(paths[1]), "\n", sep = " ")
cat("abs-suffix:", grepl("/existing-dir$", paths[1]), "\n", sep = " ")
cat("missing-passthrough:", identical(paths[2], missing), "\n", sep = " ")

named <- normalizePath(path = existing, mustWork = TRUE, winslash = "/")
cat("named-canonical:", identical(named, paths[1]), "\n", sep = " ")

print(is.na(normalizePath(NA_character_, mustWork = FALSE)))
err <- tryCatch(
  normalizePath(missing, mustWork = TRUE),
  error = function(e) paste("ERR", grepl("No such file or directory", conditionMessage(e)))
)
print(err)
