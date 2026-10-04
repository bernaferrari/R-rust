# Independent GNU contract; run with the authenticated pinned Rscript --vanilla.
args <- commandArgs(TRUE)
stopifnot(length(args) == 1L)
assets <- args[[1L]]
namespace <- asNamespace("datasets")
lazy <- getNamespaceInfo(namespace, "lazydata")
attached <- as.environment("package:datasets")
names <- sort(ls(lazy, all.names = TRUE), method = "radix")
stopifnot(length(names) == 108L, length(ls(attached)) == 108L,
          length(getNamespaceExports(namespace)) == 0L,
          identical(parent.env(lazy), baseenv()),
          !environmentIsLocked(lazy), environmentIsLocked(attached))
before <- substitute(mtcars, attached)
expected_code <- quote(lazyLoadDBfetch(KEY, datafile, compressed, envhook))
expected_code[[2L]] <- c(59685L, 1102L)
stopifnot(identical(before, expected_code))
first <- system.time(value <- get("mtcars", attached))
stopifnot(identical(value, datasets::mtcars),
          identical(before, substitute(mtcars, attached)))
private_error <- tryCatch({ datasets:::mtcars; NULL }, error = conditionMessage)
stopifnot(identical(private_error, "object 'mtcars' not found"))
all_time <- system.time(values <- mget(names, lazy, inherits = FALSE))
expected <- new.env(parent = emptyenv())
eager_time <- system.time(loaded <- load(file.path(assets, "all.rda"), envir = expected))
stopifnot(identical(sort(loaded, method = "radix"), names),
          all(vapply(names, function(name) {
              identical(get(name, expected), values[[name]])
          }, logical(1L))),
          is.data.frame(substitute(mtcars, expected)))
topics <- readRDS(file.path(assets, "Rdata.rds"))
for (topic in names(topics)) {
    target <- new.env(parent = emptyenv())
    result <- withVisible(data(list = topic, package = "datasets", envir = target))
    stopifnot(identical(result$value, topic), identical(result$visible, FALSE),
              identical(sort(ls(target), method = "radix"),
                        sort(topics[[topic]], method = "radix")),
              all(vapply(topics[[topic]], function(name) {
                  identical(get(name, target), values[[name]])
              }, logical(1L))))
}
copy <- datasets::mtcars
copy$mpg[[1L]] <- -99
stopifnot(identical(datasets::mtcars$mpg[[1L]], 21),
          identical(substitute(mtcars, attached), before))
model <- lm(mpg ~ wt + cyl, data = datasets::mtcars)
stopifnot(isTRUE(all.equal(unname(coef(model)),
    c(39.686261480253, -3.190972138983, -1.507794968259), tolerance = 1e-10)))
cat("first_mtcars_elapsed=", first[["elapsed"]], "\n", sep = "")
cat("force_all_elapsed=", all_time[["elapsed"]], "\n", sep = "")
cat("eager_workspace_elapsed=", eager_time[["elapsed"]], "\n", sep = "")
cat("eager_limit=substitute returns a data.frame instead of preserved promise code\n")
cat("Pinned datasets contract complete: 108 objects, 91 topics, lazy code, namespace, COW, model\n")
