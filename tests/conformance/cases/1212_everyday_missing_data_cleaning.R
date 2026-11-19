source <- paste(c("sample,value,control", "a,10,2", "b,NA,1", "c,NaN,4",
                  "d,Inf,5", "e,-Inf,6", "f,16,4", "g,8,NA", "h,9,3"), collapse = "\n")
measurements <- read.csv(text = source, colClasses = c("character", "numeric", "numeric"))
rejected <- with(measurements, !is.finite(value) | !is.finite(control))
stopifnot(sum(is.na(measurements$value)) == 2L,
          sum(is.nan(measurements$value)) == 1L,
          sum(is.infinite(measurements$value)) == 2L,
          sum(rejected) == 5L)
clean <- measurements[!rejected, ]
clean$adjusted <- with(clean, value / control)
stopifnot(identical(clean$sample, c("a", "f", "h")),
          identical(clean$adjusted, c(5, 4, 3)),
          mean(clean$adjusted) == 4,
          median(clean$adjusted) == 4,
          identical(range(clean$adjusted), c(3, 5)))
cat("Missing and non-finite measurements are excluded from the analysis\n")
