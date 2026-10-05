# Pinned GNU C_plot_window admission before GScale.
# src/library/graphics/src/plot.c, bac583951b728e97b9786804d3b4081f0fe18df5.
rows <- read.delim(commandArgs(TRUE)[1L], header=FALSE, quote="", comment.char="",
                   col.names=c("program","expected"), stringsAsFactors=FALSE)
pdf(NULL)
for (i in seq_len(nrow(rows))) {
    actual <- tryCatch({eval(parse(text=rows$program[i])); "OK"},
                       error=function(e)conditionMessage(e))
    stopifnot(identical(actual,rows$expected[i]))
}
dev.off()
cat(nrow(rows), "GNU finite-limit errors, explicit overrides and recovery: PASS\n")
