# Independent controls for the named-file constructor regression.
# Run with the pinned oracle/r-oracle.json GNU R executable.
run <- function() {
    directory <- tempfile("rport-named-file-")
    dir.create(directory)
    previous <- setwd(directory)
    on.exit({ setwd(previous); unlink(directory, recursive = TRUE) })
    path <- file.path(tempdir(), paste0("Rf", Sys.getpid(), "rport-named-connection"))
    on.exit(unlink(path), add = TRUE)
    writeLines("original", path)
    connection <- file(path)
    stopifnot(!isOpen(connection), identical(summary(connection)$mode, "r"),
              identical(readLines(path), "original"))
    close(connection)
    connection <- file("named-open.txt", open = "w")
    writeLines("hello", connection)
    close(connection)
    stopifnot(identical(readLines("named-open.txt", warn = FALSE), "hello"))
    connection <- file("")
    stopifnot(isOpen(connection), isOpen(connection, "r"), isOpen(connection, "w"))
    close(connection)
    cat("Named/deferred/open and anonymous read-write GNU controls: PASS\n")
}
run()
