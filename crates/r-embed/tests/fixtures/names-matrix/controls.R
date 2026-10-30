emit <- function(label, expr) {
    result <- tryCatch(paste(capture.output(dput(eval(expr))), collapse=" "),
                       error=function(e) paste0("ERROR:", conditionMessage(e)))
    cat(label, "|", result, "\n", sep="")
}
values <- list(NULL, character(), "é", c("é", "日本"), 1:2, letters[1:7], letters[1:6])
for (n in c(0L, 2L, 3L)) for (i in seq_along(values)) for (mode in c("structure", "attr", "names")) {
    value <- values[[i]]
    target <- matrix(seq_len(n * 2L), nrow=n, ncol=2L)
    emit(paste(n, i, mode, sep=":"), quote({
        if (mode == "structure") target <- structure(target, names=value)
        else if (mode == "attr") attr(target, "names") <- value
        else names(target) <- value
        attributes(target)
    }))
}
for (i in seq_along(values)) for (mode in c("structure", "attr", "names")) {
    value <- values[[i]]
    target <- array(1:6, dim=c(1L, 2L, 3L))
    emit(paste("rank3", i, mode, sep=":"), quote({
        if (mode == "structure") target <- structure(target, names=value)
        else if (mode == "attr") attr(target, "names") <- value
        else names(target) <- value
        attributes(target)
    }))
}
