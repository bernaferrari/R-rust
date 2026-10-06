# Build the accompanying C source with the pinned GNU R CMD SHLIB.
# Execute this script with the resulting library path as its sole argument.
args <- commandArgs(TRUE)
stopifnot(length(args) == 1L)
dyn.load(args[1L])
x <- .Call("rport_native_bfgs_oracle")
cat(paste(sprintf("%.17g", x), collapse=","), "\n", sep="")
