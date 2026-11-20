library(grid)
pdf("safety-source.pdf", width = 6, height = 4)
dev.control(displaylist = "enable")
grid.newpage()
grid.text("Recorded geometry")
original <- recordPlot()
invisible(dev.off())

metadata <- as.list(attr(original, "rport.grid.commands"))
bad <- original
attr(bad, "rport.grid.commands") <- as.pairlist(c(list(draw = "invalid"), metadata))

pdf("mixed-source.pdf", width = 6, height = 4)
dev.control(displaylist = "enable")
plot.new()
grid.text("Mixed geometry")
mixed <- recordPlot()
invisible(dev.off())

pdf("safety.pdf", width = 8, height = 5)
grid.text("Destination survives", y = .75)
invalid <- tryCatch({replayPlot(bad); ""}, error = conditionMessage)
missing <- tryCatch({replayPlot(mixed); ""}, error = conditionMessage)
stopifnot(grepl("invalid grid object", invalid, fixed = TRUE),
          grepl("without complete grid commands", missing, fixed = TRUE))
grid.text("Drawing can continue", y = .25)
invisible(dev.off())
