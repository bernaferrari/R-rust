local({
    if ("package:utils" %in% search()) detach("package:utils")
    messages <- character()
    classes <- list()
    record <- function(c) {
        messages <<- c(messages, conditionMessage(c))
        classes[[length(classes) + 1L]] <<- class(c)
        invokeRestart("muffleMessage")
    }
    loaded <- withVisible(withCallingHandlers(require(utils), message = record))
    stopifnot(identical(loaded$value, TRUE), !loaded$visible,
              "package:utils" %in% search(),
              identical(messages, "Loading required package: utils\n"),
              identical(classes[[1L]], c("packageStartupMessage", "simpleMessage", "message", "condition")))
    attached <- withCallingHandlers(require("utils", character.only = TRUE), message = record)
    detach("package:utils")
    quiet <- withCallingHandlers(require(utils, quietly = TRUE), message = record)
    identical(attached, TRUE) && identical(quiet, TRUE) &&
        "package:utils" %in% search() &&
        identical(messages, "Loading required package: utils\n") && length(classes) == 1L
})
