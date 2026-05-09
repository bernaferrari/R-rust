show <- function(x) {
  y <- format.data.frame(x)
  print(y)
  cat(typeof(y), length(y), "\n")
}
show(character(0))
show(numeric(0))
show(integer(0))
show(logical(0))
