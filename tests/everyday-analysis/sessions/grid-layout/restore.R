pdf("panels-restored.pdf", width = 8, height = 5)
replayPlot(readRDS("panels.rds"))
invisible(dev.off())
