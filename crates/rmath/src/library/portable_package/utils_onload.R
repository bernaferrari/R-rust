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
    tryCatch(hook("<builtin:utils>", "utils"),
             error = function(e) warning("utils .onLoad: ", conditionMessage(e)))
    ov <- tryCatch(get("osVersion", hooks, inherits=FALSE), error = function(e) NULL)
    if (!is.character(ov) || length(ov) != 1L || !nzchar(ov)) {
        ## .onLoad may abort before assigning osVersion; derive it directly
        ## (GNU utils sessionInfo.R Darwin branch).
        ov <- tryCatch({
            ver <- readLines("/System/Library/CoreServices/SystemVersion.plist")
            ind <- grep("ProductUserVisibleVersion", ver)
            ver <- sub("</string>$", "", sub(".*<string>", "", ver[ind + 1L]))
            ver1 <- strsplit(ver, ".", fixed = TRUE)[[1L]]
            if (ver1[1L] == "10")
                sprintf("%s %s %s",
                        if (as.numeric(ver1[2L]) < 12) "OS X" else "macOS", "", ver)
            else if (ver1[1L] %in% c(11:15, 26:27))
                sprintf("macOS %s %s",
                        switch(ver1[1L],
                               "11" = "Big Sur", "12" = "Monterey", "13" = "Ventura",
                               "14" = "Sonoma", "15" = "Sequoia",
                               "26" = "Tahoe", "27" = "Golden Gate"),
                        ver)
            else sprintf("macOS %s", ver)
        }, error = function(e) { NULL })
    }
    if (is.character(ov) && length(ov) == 1L && nzchar(ov))
        assign("osVersion", ov, envir=original)
})
