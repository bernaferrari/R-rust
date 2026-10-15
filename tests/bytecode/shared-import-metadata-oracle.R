args <- commandArgs(trailingOnly = TRUE)
stopifnot(length(args) == 1L)
saveRDS(list(compiler::cmpfun(function(x) x + 1L),
             compiler::cmpfun(function(x) x * 2L)),
        args[[1L]], compress = FALSE)
