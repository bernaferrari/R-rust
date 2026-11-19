Sys.setenv(TZ = "UTC")
original <- as.POSIXlt(setNames(as.Date(c("1989-03-02", "1991-11-27", NA)), letters[1:3]))
relabeled <- as.POSIXlt(original, tz = "Etc/GMT-5")
converted <- as.POSIXlt(as.POSIXct(original), tz = "Etc/GMT-5")
stopifnot(identical(typeof(difftime), "closure"))
check <- function(result, values, unit, keys = letters[1:3]) {
  stopifnot(inherits(result, "difftime"), identical(names(result), keys),
            identical(as.numeric(result), values), identical(units(result), unit))
}
check(relabeled - original, c(-5, -5, NA_real_), "hours")
check(original - relabeled, c(5, 5, NA_real_), "hours")
check(converted - original, c(0, 0, NA_real_), "secs")
check(difftime(relabeled, original, units = "mins"), c(-300, -300, NA_real_), "mins")
check(difftime(relabeled, original, tz = "UTC"), c(0, 0, NA_real_), "secs")
check(difftime(original, original[0]), numeric(), "secs", character())
stopifnot(identical(Sys.getenv("TZ"), "UTC"))
cat("POSIXlt differences honor the operands' time zones\n")
