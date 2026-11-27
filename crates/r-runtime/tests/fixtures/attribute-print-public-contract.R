print.attributeFail <- function(x, ...) {
    cat("METHOD-STDOUT\n")
    message("METHOD-STDERR")
    stop("attribute failure")
}
run <- function(x) invisible(tryCatch(print(x), error = function(e) {
    cat("CAUGHT: ", conditionMessage(e), "\n", sep = "")
}))
cat("INTEGER\n")
run(structure(1L, before = 2L, bad = structure(3L, class = "attributeFail"), after = 4L))
cat("LIST\n")
run(structure(list(1L), before = 2L, bad = structure(3L, class = "attributeFail"), after = 4L))
cat("PAIRLIST\n")
run(structure(pairlist(1L), before = 2L, bad = structure(3L, class = "attributeFail"), after = 4L))
cat("MATRIX\n")
run(structure(matrix(1:4, 2L), before = 2L, bad = structure(3L, class = "attributeFail"), after = 4L))
cat("EMPTY-LIST\n")
run(structure(list(), before = 2L, bad = structure(3L, class = "attributeFail"), after = 4L))
setClass("attributeS4Fail", slots = c(payload = "integer"))
setMethod("show", "attributeS4Fail", function(object) {
    cat("SHOW-STDOUT\n")
    message("SHOW-STDERR")
    stop("show attribute failure")
})
cat("S4-ATTRIBUTE\n")
run(structure(1L, before = 2L, bad = new("attributeS4Fail", payload = 3L), after = 4L))

print.attributeMutate <- function(x, ...) {
    if (action == "clear-all") attributes(env) <<- NULL
    if (action == "remove-next") attr(env, "later") <<- NULL
    if (action == "replace-next") attr(env, "later") <<- structure(list(payload = 99L), class = "attributeKept")
    gc(full = TRUE)
    cat("MUTATOR\n")
}
print.attributeKept <- function(x, ...) {
    cat("KEPT: ", paste(x$payload, collapse = ","), "\n", sep = "")
}
for (action in c("clear-all", "remove-next", "replace-next")) {
    cat(action, "\n", sep = "")
    env <- new.env(parent = emptyenv())
    attr(env, "first") <- structure(1L, class = "attributeMutate")
    attr(env, "later") <- structure(list(payload = c(17L, 29L)), class = "attributeKept")
    lines <- capture.output(print(env))
    # The environment address is not a stable contract; preserve all attribute
    # output and check the mutation itself independently.
    cat(lines[-1L], sep = "\n")
    cat("\n")
    stopifnot(if (action == "clear-all") is.null(attributes(env)) else if (action == "remove-next") is.null(attr(env, "later")) else identical(attr(env, "later")$payload, 99L))
}

stopifnot(identical(1L+1L,2L))

print.attributeSuccess <- function(x, ...) {
    cat("SUCCESS-STDOUT\n")
    message("SUCCESS-STDERR")
    invisible(x)
}
values <- list(integer = 1L, real = 1.5, logical = TRUE, complex = 1+2i,
               text = "value", raw = as.raw(1L), empty_integer = integer(),
               empty_real = numeric(), empty_logical = logical(),
               empty_complex = complex(), empty_raw = raw(),
               empty_named_list = structure(list(), names = character()),
               expression = expression(1+2), array = array(1:8, c(2L,2L,2L)))
for (name in names(values)) {
    cat("SUCCESS-", name, "\n", sep = "")
    print(structure(values[[name]], nested = list(a = structure(3L, class = "attributeSuccess"), b = 4L), plain = 5L))
}
stopifnot(identical(1L+1L,2L))
