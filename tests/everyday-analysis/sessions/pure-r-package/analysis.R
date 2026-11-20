check <- function(op, version) list(op = op, version = package_version(version))
too_new <- check(">=", "3.0.0")
original_libraries <- .libPaths()
empty_library <- tempfile("r6-library-")
stopifnot(dir.create(empty_library), !"R6" %in% loadedNamespaces(),
          inherits(try(loadNamespace("R6", lib.loc = empty_library), silent = TRUE), "try-error"))
message <- tryCatch(loadNamespace("R6", versionCheck = too_new), error = conditionMessage)
stopifnot(is.character(message), grepl("2.6.1 is being loaded", message, fixed = TRUE),
          !requireNamespace("R6", quietly = TRUE, versionCheck = too_new),
          !"R6" %in% loadedNamespaces())
namespace <- loadNamespace("R6", lib.loc = original_libraries, versionCheck = check("==", "2.6.1"))
.libPaths(empty_library)
message <- tryCatch(loadNamespace("R6", versionCheck = too_new), error = conditionMessage)
stopifnot(identical(namespace, loadNamespace("R6", lib.loc = empty_library)),
          identical(getNamespaceVersion(namespace), c(version = "2.6.1")),
          is.character(message), grepl("2.6.1 is already loaded", message, fixed = TRUE),
          !requireNamespace("R6", quietly = TRUE, versionCheck = too_new))
.libPaths(original_libraries)
unlink(empty_library, recursive = TRUE)

suppressPackageStartupMessages(library(R6))
stopifnot(as.character(packageVersion("R6")) == "2.6.1",
          identical(getNamespaceVersion("R6"), c(version = "2.6.1")),
          requireNamespace("R6", quietly = TRUE, versionCheck =
              list(op = "==", version = package_version("2.6.1"))),
          !requireNamespace("R6", quietly = TRUE, versionCheck =
              list(op = ">=", version = package_version("3.0.0"))))
Queue <- R6Class("Queue", public = list(
    items = numeric(),
    add = function(value) self$items <- c(self$items, value),
    mean = function() mean(self$items)
))
queue <- Queue$new()
for (value in c(3, 7, 11)) queue$add(value)
stopifnot(is.R6(queue), identical(queue$items, c(3, 7, 11)), queue$mean() == 7)
cat("Loaded R6 2.6.1; analyzed observations with mean 7\n")
