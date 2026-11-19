stopifnot(!exists("samples", inherits = FALSE), !file.exists("input.csv"))
summary <- read.csv("export.csv")
stopifnot(identical(summary$site, c("north", "south")),
          identical(summary$reading, c(17L, 7L)))
cat("Fresh session read both exported site means\n")
