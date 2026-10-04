options(digits = 7)
controls <- list(
  pairlist_integer_na = pairlist(NA_integer_, 3L),
  list_integer_na = list(NA_integer_, 3L),
  pairlist_nested_integer = pairlist(list(3L)),
  list_nested_integer = list(list(3L)),
  pairlist_attributes = pairlist(structure(c(3L, NA_integer_), names = c("x", "y"), class = "custom", note = "kept")),
  list_precision = list(1.2345678901234567, c(1.2345678901234567, 9.8765432109876543))
)
for (name in names(controls)) {
  cat(name, "\n", sep = "")
  cat("input="); dput(controls[[name]])
  cat("output="); dput(as.character(controls[[name]]))
}
cat("precision_pairlist_equivalence=", identical(as.character(pairlist(1.2345678901234567, c(1.2345678901234567, 9.8765432109876543))), as.character(controls$list_precision)), "\n", sep = "")
expected <- list(
  c("NA", "3"), c("NA", "3"), "list(3)", "list(3)", "c(3, NA)",
  c("1.23456789012346", "c(1.23456789012346, 9.87654321098765)")
)
stopifnot(all(vapply(seq_along(controls), function(i) identical(as.character(controls[[i]]), expected[[i]]), logical(1))))
TRUE
