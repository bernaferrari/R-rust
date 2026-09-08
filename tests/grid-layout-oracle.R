#!/usr/bin/env Rscript
library(grid)
png(filename=tempfile(fileext='.png'), width=600, height=400, res=100)


measure <- function(name, layout, positions, just='centre') {
  grid.newpage(); pushViewport(viewport(layout=layout, just=just))
  out <- lapply(positions, function(pos) {
    pushViewport(viewport(layout.pos.row=pos[c(1,3)], layout.pos.col=pos[c(2,4)]))
    loc <- deviceLoc(unit(0,'npc'), unit(0,'npc'), valueOnly=TRUE)
    z <- c(width=convertWidth(unit(1,'npc'),'inches',TRUE), height=convertHeight(unit(1,'npc'),'inches',TRUE), left=loc$x, bottom=loc$y)
    popViewport(); z
  })
  popViewport()
  list(name=name, valid=layout$respect, cells=do.call(rbind,out))
}

cases <- list(
  measure('wide-respect-true', grid.layout(1,2,widths=unit(c(1,2),'null'),respect=TRUE), list(c(1,1,1,1),c(1,2,1,2))),
  measure('tall-respect-true', grid.layout(2,1,heights=unit(c(1,2),'null'),respect=TRUE), list(c(1,1,1,1),c(2,1,2,1))),
  measure('selective-2x3', grid.layout(2,3,widths=unit(c(1,2,3),'null'),heights=unit(c(1,1),'null'),respect=matrix(c(TRUE,FALSE,FALSE,FALSE,TRUE,FALSE),2,3)), list(c(1,1,1,1),c(1,2,1,2),c(2,1,2,1),c(2,2,2,2))),
  measure('fixed-null', grid.layout(1,3,widths=unit.c(unit(1,'inches'),unit(c(1,2),'null')),heights=unit(1,'null')), list(c(1,1,1,1),c(1,2,1,2),c(1,3,1,3))),
  measure('no-respect', grid.layout(1,2,widths=unit(c(1,2),'null')), list(c(1,1,1,1),c(1,2,1,2))),
  measure('numeric-just', grid.layout(1,2,widths=unit(c(1,1),'inches'), just=c(.25,.75)), list(c(1,1,1,1),c(1,2,1,2))),
  measure('span-cols', grid.layout(2,3,widths=unit(c(1,2,1),'null')), list(c(1,1,2,2),c(2,3,2,3))),
  measure('pure-fixed-centered', grid.layout(1,2,widths=unit(c(1,1),'inches')), list(c(1,1,1,1),c(1,2,1,2))),
  measure('pure-fixed-oversized', grid.layout(1,2,widths=unit(c(5,7),'inches')), list(c(1,1,1,1),c(1,2,1,2)))
)
print(cases)

invisible(dev.off())
