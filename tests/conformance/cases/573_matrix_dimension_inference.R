for (x in list(matrix(1:6, ncol=3), matrix(1:6, nrow=2),
               matrix(numeric()), matrix(numeric(), ncol=2),
               matrix(numeric(), nrow=2), matrix(numeric(), nrow=-.5), suppressWarnings(matrix(1:5, ncol=3)))) {
    cat(paste(dim(x), collapse="x"), ":", paste(as.vector(x), collapse=","), "\n", sep="")
}
