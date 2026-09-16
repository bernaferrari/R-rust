f <- function() invisible()
x <- f()
cat(is.null(x), "\n", sep = "")
cat(typeof(x), "\n", sep = "")
g <- function(mm = "A") {
    mm <- mm
    invisible()
}
y <- g(mm = "B")
cat(is.null(y), "\n", sep = "")
