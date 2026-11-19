project_path <- (function() {
    work <- tempfile("analysis-project-")
    stopifnot(dir.create(work))
    on.exit(unlink(work, recursive = TRUE))
    input <- file.path(work, "input")
    output <- file.path(work, "run")
    stopifnot(!dir.create(file.path(input, "code"), showWarnings = FALSE),
              dir.create(file.path(input, "code"), recursive = TRUE),
              dir.create(file.path(input, "empty")),
              dir.create(output))
    writeLines(c("summarize <- function(records) aggregate(score ~ group, records, mean)",
                 "labels <- c(ctrl = 'Control', trt = 'Treatment')"),
               file.path(input, "code", "summary.R"))
    writeLines(c("source('code/summary.R', local = TRUE)",
                 "records <- read.csv('observations.csv', stringsAsFactors = FALSE)",
                 "result <- summarize(records)",
                 "result$label <- unname(labels[result$group])",
                 "write.csv(result, 'report.csv', row.names = FALSE)"),
               file.path(input, "run.R"))
    writeLines(c("group,score", "trt,4", "ctrl,2", "trt,8", "ctrl,4"),
               file.path(input, "observations.csv"))

    stopifnot(file.copy(input, output, recursive = TRUE))
    project <- file.path(output, "input")
    stopifnot(dir.exists(file.path(project, "empty")),
              identical(list.files(project, recursive = TRUE),
                        c("code/summary.R", "observations.csv", "run.R")))
    caller <- getwd()
    environment <- new.env()
    source(file.path(project, "run.R"), local = environment, chdir = TRUE)
    stopifnot(identical(getwd(), caller),
              identical(environment$result$score, c(3, 6)),
              !file.exists(file.path(input, "report.csv")))
    report <- file.path(project, "report.csv")
    stopifnot(identical(readLines(report), c('"group","score","label"',
                                              '"ctrl",3,"Control"', '"trt",6,"Treatment"')))

    duplicate <- file.path(work, "archived report.csv")
    stopifnot(file.copy(report, duplicate),
              identical(readLines(duplicate), readLines(report)),
              !file.copy(report, duplicate))
    writeLines("stop('project error')", file.path(input, "error.R"))
    writeLines("kept", file.path(project, "observations.csv"))
    stopifnot(!file.copy(input, output, recursive = TRUE, overwrite = FALSE),
              identical(readLines(file.path(project, "observations.csv")), "kept"),
              file.exists(file.path(project, "error.R")))
    caught <- tryCatch({
        source(file.path(project, "error.R"), chdir = TRUE)
        FALSE
    }, error = function(error) identical(conditionMessage(error), "project error"))
    stopifnot(caught, identical(getwd(), caller),
              file.copy(input, output, recursive = TRUE, overwrite = TRUE),
              identical(readLines(file.path(project, "observations.csv")),
                        readLines(file.path(input, "observations.csv"))))
    cat("Copied and ran a multi-file analysis with unchanged inputs\n")
    work
})()
stopifnot(!dir.exists(project_path))
