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
    for (package in c("methods", "utils", "stats", "graphics", "grDevices", "datasets", "tools")) {
        namespace <- asNamespace(package)
        print(identical(environmentName(namespace), package))
        print(is.null(attr(namespace, "name")) && is.null(attr(namespace, "path")))
    }
    print(identical(environmentName(NULL), "") && identical(environmentName(1L), ""))
    print(identical(environmentName(globalenv()), "R_GlobalEnv") &&
          identical(environmentName(emptyenv()), "R_EmptyEnv") &&
          identical(environmentName(baseenv()), "base") &&
          identical(environmentName(.BaseNamespaceEnv), "base"))
    environment <- new.env(parent = baseenv())
    print(identical(environmentName(environment), ""))
    attr(environment, "name") <- c("first", "second")
    print(identical(environmentName(environment), c("first", "second")))
    attr(environment, "name") <- 42L
    print(identical(environmentName(environment), 42L))
    attr(environment, "name") <- NULL
    info <- new.env(parent = baseenv())
    assign(".__NAMESPACE__.", info, environment)
    print(!isNamespace(environment) && identical(environmentName(environment), ""))
    info$spec <- character()
    print(!isNamespace(environment) && identical(environmentName(environment), ""))
    info$spec <- 1L
    print(!isNamespace(environment) && identical(environmentName(environment), ""))
    info$spec <- c(name = "synthetic", version = "1.0")
    print(isNamespace(environment) && identical(environmentName(environment), "synthetic"))
    attr(environment, "name") <- "ordinary name"
    print(identical(environmentName(environment), "synthetic"))
    attr(environment, "name") <- c("package:attached", "second")
    print(identical(environmentName(environment), "package:attached"))
    attr(environment, "name") <- NULL
    rm("spec", envir = info)
    reads <- 0L
    makeActiveBinding("spec", function(value) {
        reads <<- reads + 1L
        gc()
        c(name = "active", version = "1.0")
    }, info)
    print(identical(environmentName(environment), "active") && reads == 2L)
    visible <- stats::t.test
    print(identical(environmentName(environment(visible)), "stats"))
    print(identical(c(isS3method("t.test"), isS3method(f = "t", class = "test")), c(FALSE, FALSE)))
    registerS3method("t", "test", function(x) x)
    print(identical(c(isS3method("t.test"), isS3method(f = "t", class = "test")), c(FALSE, FALSE)))
})
