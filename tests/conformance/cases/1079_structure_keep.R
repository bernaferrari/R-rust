X <- matrix(1:4, 2, 2, dimnames = list(c("A", "B"), 1:2))
w <- NULL
withCallingHandlers(
  val <- structure(1:4, .Dim = c(2L, 2L), .Dimnames = list(c("A", "B"), c("1", "2"))),
  warning = function(w) {
    w <<- w
    invokeRestart("muffleWarning")
  }
)
cat(inherits(w, "deprecatedWarning"), "\n", sep = "")
cat(identical(val, X), "\n", sep = "")
cat(is.na(rank(c(1, NA), na.last = "keep")[2]), "\n", sep = "")
