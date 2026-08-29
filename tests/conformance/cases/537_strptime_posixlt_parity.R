Sys.setenv(TZ = "UTC")
invisible(Sys.setlocale("LC_TIME", "C"))

## Basic %Y-%m-%d parse: POSIXlt shape, classes, components (stock r90447)
x <- strptime("2020-02-01", "%Y-%m-%d")
cat(paste(class(x), collapse = "|"), "\n", sep = "")
cat(x$year + 1900, x$mon + 1, x$mday, x$hour, x$min, x$sec, "\n", sep = "|")
cat(x$wday, x$yday, x$isdst, "\n", sep = "|")
cat(x$zone, x$gmtoff, "\n", sep = "|")
cat(paste(attributes(x)$tzone, collapse = "|"), "\n", sep = "")

## Fractional seconds via %OS (plain %OS agrees across stock/trunk)
y <- strptime("2020-02-01 12:34:56.789", "%Y-%m-%d %H:%M:%OS")
cat(y$sec, y$min, y$hour, y$mday, "\n", sep = "|")

## Out-of-range and Inf %OS (r90409)
cat(is.na(strptime("100", "%OS3")$sec), "\n", sep = "")
cat(is.na(strptime("999", "%OS3")$sec), "\n", sep = "")
z <- strptime("Inf", "%OS")
cat(is.na(z$sec), is.infinite(z$sec), z$isdst, "\n", sep = "|")
## PR#19124: strptime("0", "%w") works in valid cases in a C locale
w <- strptime("0", "%w")
cat(w$wday, "\n", sep = "")

## %j day-of-year fills mon/mday; 366 is valid in a leap year
d <- strptime("2020-060", "%Y-%j")
cat(d$year + 1900, d$mon + 1, d$mday, d$yday, "\n", sep = "|")
d2 <- strptime("2020-366", "%Y-%j")
cat(d2$mon + 1, d2$mday, d2$yday, "\n", sep = "|")

## %U/%W week parity with %w (week reconciliation, r90447 yday fix)
u1 <- strptime("0 1", "%U %w")
w1 <- strptime("0 1", "%W %w")
cat(u1$mon + 1, u1$mday, u1$yday, "\n", sep = "|")
cat(w1$mon + 1, w1$mday, w1$yday, "\n", sep = "|")
u2 <- strptime("10 1", "%W %w")
cat(u2$mon + 1, u2$mday, u2$yday, "\n", sep = "|")
u3 <- strptime("2020 05 3", "%Y %U %w")
w3 <- strptime("2020 05 3", "%Y %W %w")
cat(u3$mon + 1, u3$mday, u3$yday, u3$wday, "\n", sep = "|")
cat(w3$mon + 1, w3$mday, w3$yday, w3$wday, "\n", sep = "|")

## %z offset parsing (RFC 822 form)
oz <- strptime("2020-02-01 05:00:00 +0800", "%Y-%m-%d %H:%M:%S %z")
cat(oz$hour, oz$min, oz$gmtoff, "\n", sep = "|")

## Name matching, %p, %% literals and whitespace handling
cat(strptime("Saturday", "%A")$wday, "\n", sep = "")
s1 <- strptime("Sun 2020-02-01", "%a %Y-%m-%d")
cat(s1$wday, s1$year + 1900, s1$mon + 1, s1$mday, "\n", sep = "|")
s4 <- strptime("2020-02-01x%", "%Y-%m-%dx%%")
cat(s4$year + 1900, s4$mon + 1, s4$mday, "\n", sep = "|")
s2 <- strptime("Feb 1 2020", "%b %d %Y")
cat(s2$mon + 1, s2$mday, s2$year + 1900, "\n", sep = "|")
s3 <- strptime("12:34:56 PM", "%I:%M:%S %p")
cat(s3$hour, s3$min, s3$sec, "\n", sep = "|")

## Vector recycling and NA propagation
v <- strptime(c("2020-02-01", "bad", "2020-13-01"), "%Y-%m-%d")
cat(is.na(v$year), "\n", sep = "|")
cat(v$year[1] + 1900, v$mon[1] + 1, v$mday[1], "\n", sep = "|")
cat(is.na(strptime("2020-02-01 25:00", "%Y-%m-%d %H:%M")$hour), "\n", sep = "")
