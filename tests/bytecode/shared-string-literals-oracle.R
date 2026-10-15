out <- commandArgs(trailingOnly = TRUE)[[1L]]
functions <- list(
    a = compiler::cmpfun(function() "shared code literal"),
    b = compiler::cmpfun(function() "shared code literal")
)
saveRDS(functions, out, compress = FALSE, version = 3L)
