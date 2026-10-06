parent <- new.env(parent = baseenv())
parent$x <- function() 42L
child <- new.env(parent = parent)
child$x <- 1L
stopifnot(identical(tryCatch(get("x", envir = child, mode = "function", inherits = FALSE), error = conditionMessage), "object 'x' of mode 'function' was not found"))
stopifnot(identical(get("x", envir = child, mode = "function", inherits = TRUE), parent$x))
stopifnot(is.null(get0("x", envir = child, mode = "function", inherits = FALSE)))
stopifnot(identical(get0("x", envir = child, mode = "function", inherits = TRUE), parent$x))
stopifnot(identical(get("x", envir = child, mode = "numeric"), 1L))
stopifnot(identical(get0("x", envir = child, mode = "double"), 1L))
fallback <- function() stop("fallback must be returned without invocation")
stopifnot(identical(get0("absent", envir = child, ifnotfound = fallback), fallback))
base_mode <- get("mode", envir = baseenv(), mode = "function", inherits = FALSE)
stopifnot(is.function(base_mode), identical(base_mode(1L), "numeric"))
isolated <- new.env(parent = emptyenv())
stopifnot(is.null(get0("mode", envir = isolated, mode = "function", inherits = TRUE)))
for (operation in c("get", "get0")) {
    counter <- 0L
    e <- new.env()
    delayedAssign("lazy", {counter <<- counter + 1L; function(value) value + 1L}, assign.env = e)
    stopifnot(identical(do.call(operation, list("lazy", envir = e))(41L), 42L), counter == 1L)
    stopifnot(is.function(do.call(operation, list("lazy", envir = e))), counter == 1L)
    delayedAssign("collecting", {rm("collecting", envir = e); gc(); function(value) value + 2L}, assign.env = e)
    selected <- do.call(operation, list("collecting", envir = e, inherits = FALSE))
    invisible(gc())
    stopifnot(identical(selected(40L), 42L), !exists("collecting", envir = e, inherits = FALSE))
    delayedAssign("broken", stop("lazy failure"), assign.env = e)
    stopifnot(identical(tryCatch(do.call(operation, list("broken", envir = e)), error = conditionMessage), "lazy failure"))
}
TRUE
