pdf('/tmp/rport-raster-margin-gnu.pdf', width=160/72, height=120/72)
for (xpd in c(FALSE,TRUE)) {
 par(mar=rep(1,4),xaxs='i',yaxs='i')
 plot.new();plot.window(c(0,1),c(0,1))
 par(xpd=xpd)
 rasterImage(matrix(c('red','green','blue','black'),nrow=2),.25,0,1.25,1,angle=90,interpolate=FALSE)
 cat('xpd',xpd,'plot',grconvertX(0:1,'user','device'),grconvertY(0:1,'user','device'),'\n')
}
invisible(dev.off())
