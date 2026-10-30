emit <- function(label, expr) {
    result <- tryCatch(paste(capture.output(dput(eval(expr))), collapse=" "),
                       error=function(e) paste0("ERROR:", conditionMessage(e)))
    cat(label, "|", result, "\n", sep="")
}
values <- list(NULL, character(), "é", c("é", "日本"),
               c("é", "日本", "🙂", "λ"), 1:2,
               c(TRUE, NA), c(2.5, NA_real_), as.raw(1:2), c(1+2i, NA_complex_))
for (i in seq_along(values)) for (n in c(0L, 2L, 3L)) for (mode in c("structure", "attr", "names")) {
    value <- values[[i]]
    target <- seq_len(n)
    emit(paste(i, n, mode, sep=":"), quote({
        if (mode == "structure") target <- structure(target, names=value)
        else if (mode == "attr") attr(target, "names") <- value
        else names(target) <- value
        names(target)
    }))
}
