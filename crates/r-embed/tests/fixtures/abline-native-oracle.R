## Execute with the pinned GNU R executable and retain the real PDF paths.
pdf(commandArgs(TRUE)[1L], width = 7, height = 7, compress = FALSE)
cases <- c(
    "abline(1,.5,col='red')",
    "abline(1,1,untf=TRUE,col='red')",
    "abline(h=10,col='red',lwd=3,lty=2)",
    "abline(v=10,col='blue',lwd=2,lty=3)"
)
for (code in cases) {
    par(xaxs = 'i', yaxs = 'i')
    plot.new()
    plot.window(c(1,100), c(1,100), log = 'xy')
    result <- withVisible(eval(parse(text = code)))
    cat(code, ': NULL=', is.null(result$value),
        ', visible=', result$visible, '\n', sep = '')
}
invisible(dev.off())
