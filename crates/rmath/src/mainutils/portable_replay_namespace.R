if (!isTRUE(attr(replayPlot, "rport.portable.recording"))) {
    replayPlot <- local({
        original <- replayPlot
        eval(substitute(function(x, reloadPkgs = FALSE) {
            # The owned scene codec is raw data, not GNU's display-list object.
            # Admit it before GNU's restoreRecordedPlot indexes that object.
            if (is.raw(x) && inherits(x, "recordedplot")) {
                if (reloadPkgs)
                    stop("replayPlot package reload is not supported")
                return(invisible(.External2(C_playSnapshot, x)))
            }
            ORIGINAL(x, reloadPkgs)
        }, list(ORIGINAL = original)), envir = environment(original))
    })
    attr(replayPlot, "rport.portable.recording") <- TRUE
}
