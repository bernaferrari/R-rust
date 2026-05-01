check_case <- function(case, foo) {
  print(deparse(substitute(foo)))
}
check_case(NULL, any)

check_braced <- function(x) {
  print(deparse(substitute(x)))
}
check_braced(all)

identity_arg <- function(x) x
nested <- function(case, foo) {
  identity_arg(case)
  print(deparse(substitute(foo)))
}
nested(NULL, all)
