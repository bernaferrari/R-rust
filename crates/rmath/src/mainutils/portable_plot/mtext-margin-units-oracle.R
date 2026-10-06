# Run with the pinned GNU executable, the independently compiled device probe,
# and a fresh output directory. This uses its real 7-inch Helvetica PDF device.
args <- commandArgs(TRUE)
stopifnot(length(args) == 2L)
dyn.load(args[1L])
dir.create(args[2L], recursive=TRUE, showWarnings=FALSE)
pdf(file=file.path(args[2L], "device.pdf"), width=7, height=7,
    family="Helvetica", pointsize=12)
for (cex in c(1, 2, .83)) {
    par(cex=cex, font=1, mar=c(5.1,4.1,4.1,2.1), oma=rep(2,4))
    plot.new()
    plot.window(c(0,1), c(0,1), xaxs="i", yaxs="i")
    invisible(.Call("rport_trace_start", file.path(args[2L], paste0("cex-",cex,".tsv"))))
    mtext("label", side=1, line=.5, at=0, adj=0, padj=0, cex=1)
    invisible(.Call("rport_trace_stop"))
    cat("cex",cex,"csi",par("csi"),"\n")
}
par(cex=1,mar=rep(0,4),oma=rep(0,4))
plot.new()
plot.window(c(0,1), c(0,1), xaxs="i", yaxs="i")
cat("zero-margin",grconvertX(0:1,"user","device"),grconvertY(0:1,"user","device"),"\n")
invisible(dev.off())
