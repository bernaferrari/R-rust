local({
    previous <- options(useFancyQuotes = FALSE)
    on.exit(options(previous))
    original <- utils::data
    stopifnot(identical(environment(original), asNamespace("utils")),
              identical(original, data))
    target <- new.env()
    loaded <- withVisible(utils::data(BJsales, package = "datasets", envir = target))
    stopifnot(identical(loaded$value, "BJsales"), !loaded$visible,
              identical(sort(ls(target)), c("BJsales", "BJsales.lead")),
              identical(target$BJsales, datasets::BJsales),
              identical(target$BJsales.lead, datasets::BJsales.lead))
    preserved <- new.env()
    preserved$BJsales <- "caller value"
    warnings <- character()
    withCallingHandlers(
        utils::data(list = "BJsales", package = "datasets", envir = preserved,
                    overwrite = FALSE),
        warning = function(c) {
            warnings <<- c(warnings, conditionMessage(c))
            invokeRestart("muffleWarning")
        })
    stopifnot(identical(preserved$BJsales, "caller value"),
              identical(preserved$BJsales.lead, datasets::BJsales.lead),
              identical(warnings, "an object named 'BJsales' already exists and will not be overwritten"))
    warnings <- character()
    missing <- withCallingHandlers(
        withVisible(utils::data(list = "rport_missing_data", package = "datasets", envir = target)),
        warning = function(c) {
            warnings <<- c(warnings, conditionMessage(c))
            invokeRestart("muffleWarning")
        })
    identical(missing$value, "rport_missing_data") && !missing$visible &&
        identical(warnings, "data set 'rport_missing_data' not found") &&
        identical(sort(ls(target)), c("BJsales", "BJsales.lead")) &&
        identical(original, utils::data)
})
