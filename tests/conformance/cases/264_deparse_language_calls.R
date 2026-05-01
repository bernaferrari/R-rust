print(deparse(quote(sum(1, 2))))
print(deparse(quote(1 + 2)))

show_arg <- function(x) {
  print(deparse(substitute(x)))
}
show_arg(sum(1, 2))
show_arg(1 + 2)

print(deparse1(quote(sum(1, 2))))
