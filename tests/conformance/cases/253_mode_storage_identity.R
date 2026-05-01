values <- list(
  NULL,
  TRUE,
  1L,
  1.5,
  1 + 2i,
  "x",
  list(1),
  quote(x),
  quote(f(x)),
  expression(1 + 2),
  function(x) x,
  as.raw(1)
)

for (value in values) {
  cat(mode(value), ":", storage.mode(value), "\n")
}

x <- structure(1:2, names = c("a", "b"))
y <- identity(x)
print(y)
print(names(y))
