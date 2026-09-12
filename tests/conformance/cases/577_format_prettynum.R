cat(paste(format(c(1, 1000, 1e6), scientific = FALSE, big.mark = ","), collapse = "|"), "\n", sep = "")
cat(paste(format(c(1.10, 2.00), drop0trailing = TRUE), collapse = "|"), "\n", sep = "")
cat(paste(format(c(0, 1, 0), zero.print = "-"), collapse = "|"), "\n", sep = "")
cat(paste(format(c(1.2300, 4.5600), drop0trailing = TRUE, scientific = FALSE), collapse = "|"), "\n", sep = "")
