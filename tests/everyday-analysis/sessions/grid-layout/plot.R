library(grid)
draw_panel <- function(column, fill, label) {
    pushViewport(viewport(layout.pos.row = 1L, layout.pos.col = column))
    grid.rect(y = .55, width = .70, height = .50, gp = gpar(fill = fill, col = NA))
    grid.text(label, y = .19)
    popViewport()
}

pdf("panels.pdf", width = 6, height = 4)
dev.control(displaylist = "enable")
grid.newpage()
pushViewport(viewport(layout = grid.layout(nrow = 1L, ncol = 2L),
                      width = .96, height = .88))
draw_panel(1L, "navy", "Control panel")
draw_panel(2L, "firebrick", "Treatment panel")
popViewport()
figure <- recordPlot()
recorded <- serialize(figure, NULL)
saveRDS(figure, "panels.rds")
invisible(dev.off())

pdf("panels-live.pdf", width = 8, height = 5)
replayPlot(figure)
invisible(dev.off())

pdf("panels-replay.pdf", width = 8, height = 5)
replayPlot(unserialize(recorded))
invisible(dev.off())
cat("Replayed the recorded two-panel grid figure at a different size\n")
