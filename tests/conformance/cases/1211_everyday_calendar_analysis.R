zone <- "America/New_York"
received <- c("2024-03-10 04:30:00", "2024-03-10 05:30:00", "2024-03-10 06:30:00",
              "2024-03-10 07:30:00", "2024-03-11 04:30:00")
timestamp <- as.POSIXct(received, format = "%Y-%m-%d %H:%M:%S", tz = "UTC")
stopifnot(all(c(zone, "Europe/Berlin") %in% OlsonNames()),
          identical(format(timestamp, "%H:%M %z", tz = "US/Eastern"),
                    format(timestamp, "%H:%M %z", tz = zone)),
          format(timestamp[1], "%H:%M %z", tz = "Europe/Berlin") == "05:30 +0100",
          identical(format(timestamp, "%Y-%m-%d %H:%M %z", tz = zone),
                    c("2024-03-09 23:30 -0500", "2024-03-10 00:30 -0500",
                      "2024-03-10 01:30 -0500", "2024-03-10 03:30 -0400",
                      "2024-03-11 00:30 -0400")))
observations <- data.frame(day = as.Date(timestamp, tz = zone), units = c(2L, 4L, 8L, 16L, 32L))
daily <- aggregate(units ~ day, observations, sum)
stopifnot(identical(daily$day, as.Date(c("2024-03-09", "2024-03-10", "2024-03-11"))),
          identical(daily$units, c(2L, 28L, 32L)),
          as.numeric(difftime(timestamp[4], timestamp[3], units = "hours")) == 1)
cat("Local timestamps and daily summaries agree across daylight saving time\n")
