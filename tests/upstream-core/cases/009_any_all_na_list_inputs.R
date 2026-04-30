## Curated from r-source/tests/any-all.R:
## direct any()/all() truth-table, NA behavior, and helper/do.call paths.
print(any(c(TRUE, FALSE)))
print(all(c(TRUE, FALSE)))
print(any(c(FALSE, FALSE)))
print(all(c(FALSE, FALSE)))
print(any(c(TRUE, TRUE)))
print(all(c(TRUE, TRUE)))
print(any(c(FALSE, TRUE)))
print(all(c(FALSE, TRUE)))
print(any(c(NA, FALSE)))
print(all(c(NA, FALSE)))
print(any(c(NA, FALSE), na.rm = TRUE))
print(all(c(NA, TRUE)))
print(all(c(NA, TRUE), na.rm = TRUE))
print(any(c(TRUE, NA, FALSE), na.rm = TRUE))
print(all(c(TRUE, NA, FALSE), na.rm = TRUE))

cases <- list(
  list(input = c(TRUE, FALSE), any = TRUE, all = FALSE),
  list(input = c(FALSE, FALSE), any = FALSE, all = FALSE),
  list(input = c(NA, FALSE), any = NA, all = FALSE),
  list(input = c(NA, TRUE), any = TRUE, all = NA),
  list(input = list(FALSE, NA), any = NA, all = FALSE),
  list(input = list(TRUE, NA), any = TRUE, all = NA)
)

run <- function(f, input, na.rm = FALSE) {
  if (is.list(input)) {
    do.call(f, c(input, list(na.rm = na.rm)))
  } else {
    f(input, na.rm = na.rm)
  }
}

print(deparse(substitute(any)))

check_case <- function(case, name, zed) print(identical(case[[name]], run(zed, case$input)))

check_case(cases[[1]], "any", any)
check_case(cases[[1]], "all", all)
check_case(cases[[2]], "any", any)
check_case(cases[[2]], "all", all)
check_case(cases[[3]], "any", any)
check_case(cases[[3]], "all", all)
check_case(cases[[4]], "any", any)
check_case(cases[[4]], "all", all)
check_case(cases[[5]], "any", any)
check_case(cases[[5]], "all", all)
check_case(cases[[6]], "any", any)
check_case(cases[[6]], "all", all)

print(run(any, list(FALSE, NA), na.rm = TRUE))
print(run(all, list(TRUE, NA), na.rm = TRUE))
