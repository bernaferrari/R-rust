local({
    measurements <- read.csv("observations.csv")
    fit <- lm(yield ~ manure + irrigated, data = measurements)
    irrigation <- unname(coef(fit)[["irrigatedTRUE"]])
    future <- data.frame(manure = 8, irrigated = c(FALSE, TRUE))
    estimates <- unname(predict(fit, future))
    summary <- data.frame(Scenario = c("Standard", "Irrigated"), Estimate = round(estimates, 3))
    write.csv(summary, "forecast.csv", row.names = FALSE)
    rows <- sprintf("<tr><td>%s</td><td>%.3f</td></tr>", summary$Scenario, estimates)
    writeLines(c(
        "<!doctype html>",
        "<html lang=\"en\"><head><meta charset=\"utf-8\"><title>Field trial</title></head><body>",
        "<h1>Field trial</h1>",
        sprintf("<p>Estimated irrigation effect: %.3f</p>", irrigation),
        "<table><caption>Predicted yield</caption><thead><tr><th>Scenario</th><th>Yield</th></tr></thead><tbody>",
        rows, "</tbody></table>",
        '<p><a href="forecast.csv">Forecast data</a> <a href="field-trial.pdf">Field trial figure</a></p>',
        "</body></html>"
    ), "index.html")
    pdf("field-trial.pdf", width = 6, height = 4)
    colors <- ifelse(measurements$irrigated, "firebrick", "navy")
    plot(measurements$manure, measurements$yield, xlim = c(3, 9), ylim = c(6, 26),
         pch = 19, col = colors, main = "Field trial", xlab = "Manure applied", ylab = "Crop yield")
    for (water in c(FALSE, TRUE)) {
        x <- 3:9
        lines(x, predict(fit, data.frame(manure = x, irrigated = water)),
              col = if (water) "firebrick" else "navy", lwd = 2)
    }
    invisible(dev.off())
})
