f <- function(...) {
  print(...length())
  print(...names()[2])
  print(...elt(1))
  print(...elt(2))
}

f(10, b = 20)
