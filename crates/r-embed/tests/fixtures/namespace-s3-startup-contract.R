local({
    for (package in c("methods", "utils", "stats", "graphics", "grDevices", "datasets")) {
        path <- path.package(package)
        print(is.character(path) && length(path) == 1L && !is.na(path) && nzchar(path))
        print(identical(path, attr(as.environment(paste0("package:", package)), "path")))
        print(identical(path, getNamespaceInfo(package, "path")))
    }
    print(identical(.packages(all.available = TRUE, lib.loc = character()), character()))
    print(identical(path.package(character()), character()))
    print(is.null(path.package("notapackage", quiet = TRUE)))
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
