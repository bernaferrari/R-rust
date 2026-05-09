print(summary(c(1, 2, 3, NA)))
print(summary(numeric(0)))
print(summary(c(TRUE, FALSE, NA)))
print(identical(
  unclass(summary(character(0))),
  c(Length = 0L, N.unique = 0L, N.blank = 0L, Min.nchar = NA_integer_, Max.nchar = NA_integer_)
))
print(identical(
  unclass(summary(c("b", "a", "b", NA))),
  c(Length = 4L, N.unique = 2L, N.blank = 0L, Min.nchar = 1L, Max.nchar = 1L, NAs = 1L)
))
