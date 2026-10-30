emit <- function(label, expr) {
    result <- tryCatch(paste(capture.output(dput(eval(expr))), collapse=" "),
                       error=function(e) paste0("ERROR:", conditionMessage(e)))
    cat(label, "|", result, "\n", sep="")
}
values <- list(NULL, character(), "é", c("é", "日本"), c("é", "日本", "🙂", "λ"), 1:2)
for (i in seq_along(values)) for (n in c(0L, 2L, 3L)) for (mode in c("structure", "attr", "names")) {
    value <- values[[i]]
    target <- array(seq_len(n), dim=n)
    emit(paste("array", i, n, mode, sep=":"), quote({
        if (mode == "structure") target <- structure(target, names=value)
        else if (mode == "attr") attr(target, "names") <- value
        else names(target) <- value
        attributes(target)
    }))
}
for (mode in c("structure", "attr", "names")) for (i in 1:2) {
    value <- list(NULL, "é")[[i]]
    target <- array(1:2, dim=2, dimnames=list(axis=c("a", "b")))
    emit(paste("existing", mode, i, sep=":"), quote({
        if (mode == "structure") target <- structure(target, names=value)
        else if (mode == "attr") attr(target, "names") <- value
        else names(target) <- value
        attributes(target)
    }))
}
