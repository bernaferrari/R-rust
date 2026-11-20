reject <- function(expr, package, fields) {
    message <- tryCatch({ force(expr); NA_character_ }, error = conditionMessage)
    stopifnot(!is.na(message), all(vapply(fields, grepl, logical(1), x = message, fixed = TRUE)),
              !package %in% loadedNamespaces())
}

.libPaths("wrappers")
reject(loadNamespace("NeedsR6"), "NeedsR6", "R6")
reject(library(NeedsNewR), "NeedsNewR", "9999.0.0")

.libPaths(c("pinned", "wrappers"))
reject(loadNamespace("TooNewImports"), "TooNewImports", c("R6", "3.0.0"))
reject(library(TooNewImports), "TooNewImports", c("R6", "3.0.0"))
reject(library(TooNewDepends), "TooNewDepends", c("R6", "3.0.0"))
stopifnot(!"R6" %in% loadedNamespaces())

stopifnot(UnusedImports::result() == 7L, !"R6" %in% loadedNamespaces())
pinned <- loadNamespace("R6")
stopifnot(identical(unname(getNamespaceVersion(pinned)), "2.6.1"))
.libPaths("wrappers")
stopifnot(NeedsR6::result() == 7L, identical(pinned, asNamespace("R6")))

.libPaths(c("synthetic", "wrappers"))
reject(loadNamespace("TooNewImports"), "TooNewImports", c("2.6.1", "3.0.0"))
stopifnot(identical(pinned, asNamespace("R6")))

.libPaths(c("pinned", "wrappers"))
stopifnot(isNamespace(loadNamespace("TooNewDepends")),
          !"package:R6" %in% search(), !"package:TooNewDepends" %in% search())
cat("Checked dependency refusals, unused imports, retry and loaded namespace identity\n")
