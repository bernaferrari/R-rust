local({
    for (package in c("methods", "utils", "stats", "graphics", "grDevices", "tools")) {
        cat(package, "\n")
        table <- getNamespaceInfo(package, "S3methods")
        print(dim(table))
        print(identical(table, getNamespaceInfo(asNamespace(package), "S3methods")))
    }
    print(identical(getS3method("predict", "lm"), stats:::predict.lm))
    print(is.function(getS3method("print", "data.frame")))
    gc()
    print(identical(getS3method("predict", "lm"), stats:::predict.lm))
})
