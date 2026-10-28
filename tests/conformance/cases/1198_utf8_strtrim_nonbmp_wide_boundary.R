# Pinned GNU R r90451; a wide non-BMP scalar is kept whole at its width boundary.
x <- "a😀b"
values <- lapply(1:4, function(width) strtrim(x, width))
expected <- list("a", "a", "a😀", "a😀b")
raw_values <- lapply(values, charToRaw)
expected_raw <- lapply(expected, charToRaw)
stopifnot(identical(values, expected),
          identical(raw_values, expected_raw),
          identical(vapply(values, nchar, integer(1), type = "width"),
                    c(1L, 1L, 3L, 4L)))
print(TRUE)
