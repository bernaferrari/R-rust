emit <- function(label, expr) {
    result <- tryCatch(paste(capture.output(dput(eval(expr))), collapse=" "),
                       error=function(e) paste0("ERROR:", conditionMessage(e)))
    cat(label, "|", result, "\n", sep="")
}
values <- list(structure("é", marker="x", names="名"),
               structure(1:2, marker="x", names=c("p", "q")),
               structure(c("é", "日本"), marker="x"),
               structure(character(), marker="x"))
for (kind in c("vector", "array", "matrix")) for (n in 0:3) for (i in seq_along(values)) for (mode in "names") {
    value <- values[[i]]
    target <- if (kind == "vector") seq_len(n) else if (kind == "array") array(seq_len(n), dim=n) else matrix(seq_len(n), nrow=1L, ncol=n)
    emit(paste(kind, n, i, mode, sep=":"), quote({
        if (mode == "structure") target <- structure(target, names=value)
        else if (mode == "attr") attr(target, "names") <- value
        else names(target) <- value
        list(names=names(target), attributes=attributes(target))
    }))
}
