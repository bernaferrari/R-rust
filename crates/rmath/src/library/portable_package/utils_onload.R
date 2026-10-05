# Keep GNU's original hook and metadata helpers, with a private metadata-only
# implementation of their constant uname requests. No shell process is started
# and neither the public system binding nor the namespace helpers are replaced.
local({
    original <- environment(.onLoad)
    hooks <- new.env(parent=original)
    hooks$system <- function(command, intern=FALSE, ...) {
        info <- Sys.info()
        if (!isTRUE(intern)) stop("utils initialization requires captured metadata")
        if (identical(command, "uname -sr 2>/dev/null||echo darwin"))
            return(paste(info[["sysname"]], info[["release"]]))
        if (identical(command, "uname -a"))
            return(paste(info[["sysname"]], info[["nodename"]], info[["release"]], info[["version"]], info[["machine"]]))
        if (identical(command, "uname -r")) return(info[["release"]])
        stop("unsupported utils initialization metadata request")
    }
    for (name in c("defaultUserAgent", ".osVersion")) {
        helper <- get(name, original, inherits=FALSE)
        environment(helper) <- hooks
        assign(name, helper, envir=hooks)
    }
    hook <- get(".onLoad", original, inherits=FALSE)
    environment(hook) <- hooks
    hook("<builtin:utils>", "utils")
    assign("osVersion", get("osVersion", hooks, inherits=FALSE), envir=original)
})
