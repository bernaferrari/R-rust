# Capture original installed GNU objects; never execute the Rust runtime.
args <- commandArgs(TRUE)
stopifnot(length(args) == 2L, identical(as.character(getRversion()), "4.7.0"), identical(R.version[["svn rev"]], "90451"))
package <- args[[1L]]; out <- args[[2L]]
stopifnot(package %in% c("methods", "utils", "tools", "grDevices", "graphics", "stats", "stats4"))
dir.create(out, recursive=TRUE, showWarnings=FALSE)
p <- system.file(package=package)
stopifnot(nzchar(p))
stopifnot(file.copy(file.path(p,"R",paste0(package,".rdb")), file.path(out,paste0(package,".rdb")), overwrite=TRUE))
if (file.exists(file.path(p,"R","sysdata.rdb"))) {
    stopifnot(file.copy(file.path(p,"R","sysdata.rdb"),file.path(out,"sysdata.rdb"),overwrite=TRUE))
    saveRDS(readRDS(file.path(p,"R","sysdata.rdx")),file.path(out,"sysdata-index.rds"),version=2,compress=FALSE)
}
index <- readRDS(file.path(p,"R",paste0(package,".rdx")))
saveRDS(index,file.path(out,"index.rds"),version=2,compress=FALSE)
ns <- asNamespace(package)
saveRDS(sort(getNamespaceExports(ns)),file.path(out,"exports.rds"),version=2,compress=FALSE)
info <- get(".__NAMESPACE__.",ns)
saveRDS(get("imports",info),file.path(out,"imports.rds"),version=2,compress=FALSE)
saveRDS(get("S3methods",info),file.path(out,"S3methods.rds"),version=2,compress=FALSE)
for (name in c("DESCRIPTION","NAMESPACE")) stopifnot(file.copy(file.path(p,name),file.path(out,name),overwrite=TRUE))
assignments <- as.list(body(base:::lazyLoadDBexec))
selected <- Filter(function(x) is.call(x) && identical(x[[1L]],as.name("<-")) && identical(x[[2L]],as.name("envhook")),assignments)
stopifnot(length(selected)==1L)
dput(selected[[1L]][[3L]],file=file.path(out,"envhook.R"))

# Keep package examples available to runtimes whose package images have no
# filesystem library tree. Store the exact output of GNU tools::Rd2ex, indexed
# by every help alias, so `example()` can source it from a textConnection.
aliases <- readRDS(file.path(p, "help", "aliases.rds"))
topics <- names(aliases)
example_index <- file(file.path(out, "examples.index"), open="wt", encoding="UTF-8")
example_data <- file(file.path(out, "examples.data"), open="wb")
offset <- 0
example_docs <- 0L
for (rdfile in unique(unname(aliases))) {
    topic <- topics[match(rdfile, unname(aliases))]
    help_file <- do.call(utils::help, list(topic=topic, package=package, help_type="text"))
    stopifnot(length(help_file) == 1L)
    rd <- utils:::.getHelpFile(help_file)
    tmp <- tempfile("rport-rd-example-")
    tools::Rd2ex(rd, tmp)
    lines <- if (file.exists(tmp)) readLines(tmp, warn=FALSE, encoding="UTF-8") else character()
    if (any(grepl("^### \\*\\* Examples$", lines))) {
        bytes <- charToRaw(enc2utf8(paste0(paste(lines, collapse="\n"), "\n")))
        len <- length(bytes)
        writeBin(bytes, example_data)
        for (alias in topics[unname(aliases) == rdfile]) {
            stopifnot(!grepl("[\t\r\n]", alias))
            writeLines(paste(alias, offset, len, sep="\t"), example_index, useBytes=TRUE)
        }
        offset <- offset + len
        example_docs <- example_docs + 1L
    }
    unlink(tmp)
}
close(example_index)
close(example_data)
cat(paste0("Pinned GNU ", package, " ", R.version$platform, " variables ", length(index$variables), " references ", length(index$references), " exports ", length(getNamespaceExports(ns)), " example docs ", example_docs, " bytes ", offset, "\n"))
