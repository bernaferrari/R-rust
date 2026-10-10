capture <- function(value) {
  messages <- character()
  value <- withCallingHandlers(value, warning = function(w) {
    messages <<- c(messages, conditionMessage(w))
    invokeRestart("muffleWarning")
  })
  list(value = value, warnings = messages)
}
cases <- list(
  logical = list(pairlist(a = 1L, b = integer(), c = NA_real_), c(TRUE, NA, NA), character()),
  integer = list(pairlist(a = 1.9, b = integer(), c = "bad"), c(1L, NA_integer_, NA_integer_), "NAs introduced by coercion"),
  double = list(pairlist(a = 2L, b = logical(), c = 1 + 2i), c(2, NA_real_, 1), "imaginary parts discarded in coercion"),
  complex = list(pairlist(a = NA_real_, b = integer(), c = "3+4i"), c(complex(real = NA_real_, imaginary = 0), NA_complex_, 3 + 4i), character()),
  raw = list(pairlist(a = -1L, b = 256L, c = NA_integer_, d = integer()), as.raw(c(255, 0, 0, 0)), character())
)
for (mode in names(cases)) {
  actual <- capture(as.vector(cases[[mode]][[1]], mode))
  stopifnot(identical(actual$value, cases[[mode]][[2]]), identical(actual$warnings, cases[[mode]][[3]]))
  cat(mode, "values-and-warnings=PASS\n")
}
for (value in list(pairlist(NULL), pairlist(1:2))) {
  message <- tryCatch(as.integer(value), error = function(e) conditionMessage(e))
  stopifnot(identical(message, "'pairlist' object cannot be coerced to type 'integer'"))
}
stopifnot(identical(as.vector(pairlist(list(1L), expression(x)), "integer"), c(NA_integer_, NA_integer_)))
cat("invalid-and-generic-children=PASS\n")
TRUE
