local({
    print(c("print.default", "print.data.frame") %in% methods("print"))
    print("summary.data.frame" %in% methods("summary"))
    print("summary.data.frame" %in% methods(class = "data.frame"))
    print("as.data.frame" %in% suppressWarnings(methods("as")))
    print(c(typeof(summary.data.frame), typeof(getS3method("summary", "data.frame"))))
    print(identical(summary.data.frame, get("summary.data.frame", baseenv())))
    print(is.function(methods::.S4methods))
    print(identical(methods::.S4methods, getExportedValue("methods", ".S4methods")))
    print(".S4methods" %in% getNamespaceExports("methods"))
    ns <- asNamespace("methods")
    print(isNamespace(ns))
    print(identical(ns, getNamespace("methods")))
    info <- get(".__NAMESPACE__.", ns, inherits = FALSE)
    print(identical(info, get(".__NAMESPACE__.", ns, inherits = FALSE)))
    exports <- getNamespaceInfo(ns, "exports")
    assign(".rportAlias", "callNextMethod", exports)
    print(identical(getExportedValue(ns, ".rportAlias"), methods::callNextMethod))
    print(identical(methods::.rportAlias, methods::callNextMethod))
    rm(".rportAlias", envir = exports)
    for (args in list(list(ns = "methods", name = character()),
                      list(ns = NULL, name = "is"),
                      list(ns = new.env(), name = "is"))) {
        print(tryCatch(do.call(getExportedValue, args), error = function(e) conditionMessage(e)))
    }
    private <- tryCatch(methods::.getMethodsTable, error = function(e) conditionMessage(e))
    print(private)
    for (x in list(structure(c("a", "bb"), names = c("x", "y"), foo = 1),
                   matrix(c("a", "bb", NA, "NA"), 2, 2,
                          dimnames = list(c("r", "s"), c("u", "v"))),
                   array(character(), c(0, 2, 2)))) {
        print(format(x))
        dput(attributes(format(x)))
    }
    for (x in list(data.frame(), data.frame(a = numeric()),
                   data.frame(a = c(1, 3, NA), b = factor(c("u", "v", "u"))),
                   data.frame(a = c(TRUE, NA, FALSE), b = c("one", "two", "three")),
                   data.frame(a = I(matrix(1:6, 3, 2))))) {
        value <- summary(x)
        print(value)
        print(list(dim = dim(value), dimnames = dimnames(value), class = class(value)))
        print(identical(value, summary.data.frame(x)))
    }
    TRUE
})
