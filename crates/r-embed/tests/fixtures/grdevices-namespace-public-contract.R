local({
    stopifnot(identical(ls(envir = .GlobalEnv, all.names = TRUE), character()))
    namespace <- loadNamespace("grDevices")
    original <- grDevices::dev.new
    stopifnot(identical(environment(original), namespace),
              identical(original, get("dev.new", as.environment("package:grDevices"))),
              identical(getNamespace("grDevices"), namespace))
    detach("package:grDevices")
    stopifnot(identical(getNamespace("grDevices"), namespace),
              identical(grDevices::dev.new, original))
    gc()
    library(grDevices)
    stopifnot(identical(original, get("dev.new", as.environment("package:grDevices"))),
              identical(environment(grDevices::devAskNewPage), namespace),
              !exists("dev.new", .GlobalEnv, inherits = FALSE))
    TRUE
})
