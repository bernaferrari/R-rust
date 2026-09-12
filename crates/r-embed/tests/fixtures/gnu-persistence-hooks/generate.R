dir.create("crates/r-embed/tests/fixtures/gnu-persistence-hooks", recursive = TRUE, showWarnings = FALSE)
base <- "crates/r-embed/tests/fixtures/gnu-persistence-hooks"
e <- new.env()
e$x <- 7L
wire <- serialize(e, NULL, refhook = function(x) "token")
writeBin(wire, file.path(base, "persistent-token.rds"))
z <- unserialize(serialize(e, NULL, refhook = function(x) NULL))
stopifnot(identical(z$x, 7L))
cat("null-fallback=TRUE\n")
replacement <- new.env()
stopifnot(identical(unserialize(wire, refhook = function(x) replacement), replacement))
cat("replacement-identity=TRUE\n")
bad <- tryCatch(serialize(e, NULL, refhook = function(x) 1L), error = function(err) TRUE)
empty <- tryCatch(serialize(e, NULL, refhook = function(x) character()), error = function(err) TRUE)
stopifnot(identical(c(bad, empty), c(TRUE, TRUE)))
cat("invalid-results=TRUE\n")
stopifnot(identical(tryCatch(unserialize(wire), error = function(err) conditionMessage(err)), "no restore method available"))
cat("missing-restore=TRUE\n")
hits <- 0L
repeated <- serialize(list(e, e), NULL, refhook = function(x) {
    hits <<- hits + 1L
    "token"
})
z <- unserialize(repeated, refhook = function(x) replacement)
stopifnot(identical(c(hits, identical(z[[1]], z[[2]])), c(2L, TRUE)))
cat("repeated-references=2|identity=TRUE\n")
gctorture(TRUE)
gc_raw <- serialize(list(e, e), NULL, refhook = function(x) {
    invisible(gc())
    "token"
})
gctorture(FALSE)
stopifnot(is.raw(gc_raw))
cat("gc-callback=TRUE\n")
for (mode in list(c(FALSE, TRUE), c(FALSE, FALSE), c(TRUE, TRUE))) {
    for (version in c(2L, 3L)) {
        encoded <- serialize(e, NULL, ascii = mode[1], xdr = mode[2], version = version, refhook = function(x) "token")
        restored <- unserialize(encoded, refhook = function(x) {stopifnot(identical(x, "token")); replacement})
        stopifnot(is.raw(encoded), identical(restored, replacement))
        cat(sprintf("format-ascii-%s-xdr-%s-version-%d=TRUE\n", mode[1], mode[2], version))
    }
}
atomic_hits <- 0L
invisible(serialize(list(1:3, "text", TRUE), NULL, refhook = function(x) {
    atomic_hits <<- atomic_hits + 1L
    "token"
}))
stopifnot(identical(atomic_hits, 0L))
cat("atomic-excluded=TRUE\n")

stopifnot(identical(unserialize(wire, refhook=function(x)NULL), NULL))
stopifnot(identical(unserialize(wire, refhook=function(x)quote(a+b)), quote(a+b)))
cat("restore-null-and-language=TRUE\n")
hits<-0L
invisible(serialize(list(globalenv(),baseenv(),emptyenv(),1L),NULL,refhook=function(x){hits<<-hits+1L;"token"}))
e<-new.env();e$.packageName<-"ordinary"
invisible(serialize(e,NULL,refhook=function(x){hits<<-hits+1L;"token"}))
stopifnot(identical(hits,1L))
cat("special-environments-excluded=TRUE\n")
