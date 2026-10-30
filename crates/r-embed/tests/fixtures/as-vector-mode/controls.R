values <- list(NULL, character(), NA_character_, "unknown-mode", c("integer", "double"), "integer", "double", "function", "any")
for (i in seq_along(values)) {
    mode <- values[[i]]
    result <- tryCatch({
        x <- as.vector(1L, mode)
        if (is.function(x)) paste0("function:", x())
        else paste0(typeof(x), ":", paste(capture.output(dput(x)), collapse=" "))
    }, error=function(e) paste0("ERROR:", conditionMessage(e)))
    cat(i, "|", result, "\n", sep="")
}
x <- structure(list(a=1), foo=42)
for (mode in c("any", "list", "expression")) {
    result <- tryCatch(paste(capture.output(dput(as.vector(x, mode))), collapse=" "),
                       error=function(e) paste0("ERROR:", conditionMessage(e)))
    cat("list-", mode, "|", result, "\n", sep="")
}
y <- structure(1:2, names=c("a", "b"))
cat("atomic-any|", paste(capture.output(dput(as.vector(y, "any"))), collapse=" "), "\n", sep="")
for (value in list(quote(foo), new.env(), function() 1)) {
    result <- tryCatch(paste(capture.output(dput(as.vector(value, "any"))), collapse=" "),
                       error=function(e) paste0("ERROR:", conditionMessage(e)))
    cat("other-any|", result, "\n", sep="")
}
