# GNU R base/R/funprog.R; GPL-2.0-or-later, R Core Team.
function(f, x) { ind <- as.logical(unlist(lapply(x, f))); x[which(ind)] }
