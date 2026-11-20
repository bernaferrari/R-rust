library(grid)
replay_case <- function(case) {
    pdf(NULL, width = 6, height = 4)
    dev.control(displaylist = "enable")
    switch(case,
        "grob-viewport" = grid.draw(textGrob("Marker", vp = viewport(width = .8))),
        "unrecorded-push" = {pushViewport(viewport(width = .8), recording = FALSE); grid.text("Marker")},
        "unrecorded-page" = {grid.newpage(recording = FALSE); grid.text("Marker")},
        "late-push" = {grid.text("Marker"); pushViewport(viewport(width = .8))})
    recorded <- serialize(recordPlot(), NULL)
    invisible(dev.off())
    pdf(paste0(case, ".pdf"), width = 8, height = 5)
    replayPlot(unserialize(recorded))
    invisible(dev.off())
}
for (case in c("grob-viewport", "unrecorded-push", "unrecorded-page", "late-push")) {
    replay_case(case)
}
