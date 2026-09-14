s <- factor.scope(
  attr(terms(y ~ a + b), "factors"),
  list(add = attr(terms(y ~ a + b + a:b), "factors"))
)
cat(paste(s$add, collapse = ","), "\n", sep = "")
