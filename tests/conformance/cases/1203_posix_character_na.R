with_origin <- as.POSIXlt(c(NA, 0), origin = "1970-01-01", tz = "UTC")
stopifnot(identical(as.character(with_origin), c(NA_character_, "1970-01-01")),
          identical(as.character(with_origin, digits = 15), c(NA_character_, "1970-01-01")))
value <- .POSIXct(c(0, NA_real_, NaN, -Inf, Inf), tz = "UTC")
parts <- as.POSIXlt(value, tz = "UTC")
expected <- c("1970-01-01", NA_character_, "NaN", "-Inf", "Inf")
stopifnot(identical(as.character(value), expected),
          identical(as.character(parts, digits = 15), expected),
          identical(as.character(parts), expected))
characters <- as.character(parts[c(1, 2, 4, 5)])
stopifnot(identical(as.numeric(as.POSIXlt(characters, tz = "UTC")),
                    c(0, NA_real_, -Inf, Inf)))
cat("POSIX character conversion preserves missing values\n")
