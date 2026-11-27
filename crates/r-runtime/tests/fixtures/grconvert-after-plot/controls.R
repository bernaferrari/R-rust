postplot_result <- local({
  values <- character()
  record <- function(label) {
    values <<- c(values, paste(label, par('cex'), par('csi'), par('mex'),
      grconvertX(1, 'chars', 'device'), grconvertY(1, 'chars', 'device')))
  }
  record('start')
  par(cex=1.2)
  record('set-before')
  plot.new()
  record('frame')
  par(cex=2, mex=.8)
  record('set-after')
  plot.new()
  record('next-frame')
  par(mfrow=c(2,1))
  plot.new()
  record('layout')
  values
})
invisible(NULL)
