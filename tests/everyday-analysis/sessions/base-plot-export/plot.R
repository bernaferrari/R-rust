pdf("weekly-observations.pdf", width = 6, height = 4, compress = FALSE)
week <- 1:5
observations <- c(2, 4, 3, 7, 8)
plot(week, observations, xlab = "Week", ylab = "Observations",
     main = "Weekly observations", xlim = c(1, 5), ylim = c(0, 10),
     xaxs = "i", yaxs = "i", pch = 19, col = "navy")
lines(week, fitted(lm(observations ~ week)), col = "firebrick", lwd = 2)
abline(h = 5, col = "grey40", lty = 2)
legend("topleft", c("Observed", "Linear fit"), col = c("navy", "firebrick"),
       pch = c(19, NA), lty = c(NA, 1), bty = "n")
stopifnot(identical(par("usr"), c(1, 5, 0, 10)))
invisible(dev.off())
cat("Exported a layered scatterplot with the same plot limits\n")
