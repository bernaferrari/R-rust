x <- c(1, 2, 3, 2, 1, 2, 3, 2, 1, 2)
cat(round(ar.burg(x, aic = FALSE, order.max = 1)$ar, 4), "\n", sep = "")
