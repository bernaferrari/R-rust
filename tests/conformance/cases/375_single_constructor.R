show_single <- function(x) {
  print(x)
  cat(typeof(x), length(x), isTRUE(attr(x, "Csingle")), "\n")
}

show_single(single())
show_single(single(0))
show_single(single(3))
show_single(single(length = 2))
