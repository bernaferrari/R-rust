source_time <- as.POSIXlt("9999-12-31 23:59:59", tz = "UTC")
source_time$sec <- 60 - 4^-(1:25)
target <- as.POSIXlt(.POSIXct(numeric(25), tz = "GMT"))
original <- source_time[1:25]
target[1:25] <- original
stopifnot(all(target - original == 0),
          identical(target$sec[1:23], original$sec[1:23]),
          identical(target$sec[24:25], c(0, 0)))
target[[8]] <- original[[8]]
stopifnot(identical(target[[8]]$sec, original[[8]]$sec),
          target[[8]] - original[[8]] == 0)

large <- original[[8]]
large$year <- 300000000L - 1900L
large$sec <- 59.75
whole <- large
whole$sec <- floor(whole$sec)
expected <- as.POSIXlt(as.POSIXct(whole), tz = "GMT")
stopifnot(abs(as.numeric(as.POSIXct(whole))) >= 2^53)
target[1L] <- large
target[[2L]] <- large
stopifnot(identical(target$sec[1:2], rep(expected$sec, 2)),
          all(target[1:2] - expected == 0))
cat("Cross-zone POSIXlt replacement preserves representable precision\n")
