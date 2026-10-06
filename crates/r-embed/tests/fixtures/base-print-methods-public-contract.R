local({
    for (name in c("print.function", "print.data.frame")) {
        method <- getExportedValue("base", name)
        print(typeof(method))
        print(identical(environment(method), .BaseNamespaceEnv))
        print(names(formals(method)))
    }
    print(identical(getS3method("print", "function"), base::print.function))
    print(identical(getS3method("print", "data.frame"), base::print.data.frame))
    f <- eval(parse(text=c("function(x = 1) {", "    # retained source comment",
                          "    x + 1", "}"), keep.source=TRUE))
    for (useSource in c(TRUE, FALSE)) {
        text <- capture.output(print.function(f, useSource=useSource))
        print(any(grepl("retained source comment", text, fixed=TRUE)))
        print(identical(text, capture.output(print.default(f, useSource=useSource))))
        invisible(capture.output(result <- withVisible(print.function(f, useSource=useSource))))
        print(result$visible)
        print(identical(result$value, f))
    }
    print(identical(capture.output(print.function(sum)),
                    capture.output(print.default(sum))))
    compiled <- compiler::cmpfun(f)
    print(any(grepl("retained source comment",
                   capture.output(print.function(compiled, useSource=FALSE)), fixed=TRUE)))
    print(compiled(4))
    x <- data.frame(number=c(1.25, NA, 300), text=c("a", "long", NA),
                    row.names=c("first", "second", "third"))
    for (method in list(print.data.frame, compiler::cmpfun(print.data.frame))) {
        method(x, digits=3)
        method(x, quote=TRUE, row.names=FALSE, max=4)
        method(x, row.names=c("one", "two", "three"))
        method(x[FALSE, , drop=FALSE])
        method(x[, FALSE, drop=FALSE])
        result <- withVisible(method(x, max=0))
        print(result$visible)
        print(identical(result$value, x))
        print(tryCatch(method(x, max=Inf), error=conditionMessage))
    }
    for (encode in c(TRUE, FALSE)) {
        formatted <- format(c("NA", NA_character_, "x"), na.encode=encode)
        print(formatted)
        print(is.na(formatted))
    }
    for (quote in c(TRUE, FALSE)) {
        for (max in c(0, 1, 3, 6)) {
            print(matrix(c("a", NA_character_, "NA", "long", "b", "c"), 2, 3),
                  quote=quote, max=max)
        }
    }
    for (value in c(Inf, -Inf, NaN, NA_real_)) {
        print(tryCatch(stop("number: ", value), error=conditionMessage))
    }
    gc()
    print(identical(getS3method("print", "data.frame"), base::print.data.frame))
})
