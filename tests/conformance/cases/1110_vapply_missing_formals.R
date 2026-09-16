f <- function(x, y = 1) x
cat(paste(vapply(formals(f), typeof, ""), collapse = ","), "\n", sep = "")
hasReal <- function(x) {
    if (is.double(x) || is.complex(x)) {
        !all((x == round(x, 3)) | is.na(x))
    } else if (is.logical(x) || is.integer(x) ||
        is.symbol(x) || is.call(x) || is.environment(x) || is.character(x)) {
        FALSE
    } else if (is.recursive(x)) {
        any(vapply(x, hasReal, NA))
    } else {
        FALSE
    }
}
cat(hasReal(formals(f)), "\n")
cat(paste(vapply(formals(f), hasReal, NA), collapse = ","), "\n", sep = "")
