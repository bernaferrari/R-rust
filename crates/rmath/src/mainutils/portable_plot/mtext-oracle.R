# Independently execute with the pinned GNU oracle and its real PDF device.
# Arguments: compiled GNU-only probe library, new output directory.
args <- commandArgs(TRUE)
stopifnot(length(args) == 2L)
dyn.load(args[1L])
dir.create(args[2L], recursive=TRUE, showWarnings=FALSE)
pdf(file=file.path(args[2L], "device.pdf"), width=7, height=7,
          family="Helvetica", pointsize=12)
par(mar=c(5.1, 4.1, 4.1, 2.1), oma=c(2, 2, 2, 2))
plot.new()
plot.window(xlim=c(0, 1), ylim=c(0, 1), xaxs="i", yaxs="i")
geometry <- c(usr=par("usr"), plt=par("plt"), fig=par("fig"),
              din=par("din"), csi=par("csi"), mex=par("mex"),
              ps=par("ps"), cex=par("cex"),
              plotX=grconvertX(par("usr")[1:2], "user", "device"),
              plotY=grconvertY(par("usr")[3:4], "user", "device"))
dput(geometry, file=file.path(args[2L], "geometry.R"))

probe <- function(id, ...) {
    invisible(.Call("rport_trace_start", file.path(args[2L], paste0(id, ".tsv"))))
    result <- tryCatch(withCallingHandlers(withVisible(mtext(...)),
        warning=function(w) { cat("warning:", conditionMessage(w), "\n"); invokeRestart("muffleWarning") }),
        error=function(e) conditionMessage(e))
    invisible(.Call("rport_trace_stop"))
    cat(id, "\n")
    if (is.character(result)) print(result)
    else print(c(null=is.null(result$value), visible=result$visible))
}
for (outer in c(FALSE, TRUE)) {
    for (side in 1:4) {
        for (las in 0:3) {
            probe(paste0("side", side, "-las", las, "-outer", as.integer(outer)),
                  "label", side=side, las=las, line=.5, outer=outer)
        }
    }
}
probe("recycle", c("a", "b", NA_character_), side=1:4, line=c(0, 1),
      at=c(.25, .75), adj=c(0, .5, 1), padj=c(0, .5, 1),
      cex=c(.5, 1, 2), col=c("red", NA), font=1:4)
par(cex=2, col="green", font=2)
probe("cex-missing", "label", cex=NA)
probe("cex-replace", "label", cex=1)
probe("colors-missing", c("a", "b"), col=c(NA, "blue"))
probe("missing-text", c(NA_character_, "present"))
probe("empty-string", "")
probe("math", expression(alpha^2), side=4, las=1)
for (name in c("text", "side", "line", "outer", "at", "adj", "padj", "cex", "col", "font")) {
    supplied <- list(text="label")
    supplied[[name]] <- numeric()
    do.call(probe, c(list(id=paste0("empty-", name)), supplied))
}
for (side in c(0, 5, NA)) probe(paste0("invalid-side-", side), "label", side=side)
for (cex in c(-1, 0, Inf)) probe(paste0("cex-", cex), "label", cex=cex)
probe("character-side", "label", side="2")
probe("bad-side-coercion", "label", side="bad")
probe("invalid-las", "label", las=4)
probe("invalid-line", "label", line=NA)
probe("invalid-at", "label", at=Inf)
probe("after-errors", "label", side=1)
invisible(dev.off())
