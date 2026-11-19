observations <- data.frame(
    segment = factor(c("retail", "service", "retail", NA, "service"),
                     levels = c("retail", "service", "unused")),
    region = factor(c("north", "north", "south", "south", "south")),
    revenue = c(1.5, 2, 3, 4, NA_real_))
stopifnot(sum(is.na(observations$segment)) == 1L)

counts <- xtabs(~ segment, observations)
used <- xtabs(~ segment, observations, drop.unused.levels = TRUE)
weighted <- xtabs(revenue ~ segment, observations, na.rm = TRUE)
by_region <- xtabs(revenue ~ segment + region, observations, na.rm = TRUE)
stopifnot(identical(dimnames(counts), list(segment = c("retail", "service", "unused"))),
          identical(as.integer(counts), c(2L, 2L, 0L)),
          identical(dimnames(used), list(segment = c("retail", "service"))),
          identical(dimnames(weighted), dimnames(counts)),
          identical(as.numeric(weighted), c(4.5, 2, 0)),
          identical(dimnames(by_region), c(dimnames(counts), list(region = c("north", "south")))),
          identical(colSums(by_region), c(north = 3.5, south = 3)))

labels <- list(segment = levels(observations$segment))
storage <- array(0L, 3L, dimnames = labels)
storage[] <- as.numeric(weighted)
plain <- c(retail = 2L, service = 1L)
plain[1] <- 2.5
grouped <- tapply(c(1, 2), list(segment = c("a", "b")), sum, default = 0L)
stopifnot(identical(dimnames(storage), labels),
          identical(as.numeric(storage), c(4.5, 2, 0)),
          identical(plain, c(retail = 2.5, service = 1)),
          identical(dimnames(grouped), list(segment = c("a", "b"))))
cat("Categorical counts and weighted summaries agree\n")
