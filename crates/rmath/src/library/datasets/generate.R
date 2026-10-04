# Run through generate.py, which verifies the pinned runtime and hashes inputs.
args <- commandArgs(TRUE)
stopifnot(length(args) == 1L)
out <- args[[1L]]
dir.create(out, recursive = TRUE, showWarnings = FALSE)
package <- find.package("datasets")
index <- readRDS(file.path(package, "data", "Rdata.rdx"))
topics <- readRDS(file.path(package, "data", "Rdata.rds"))
listing <- readRDS(file.path(package, "Meta", "data.rds"))
lazy <- getNamespaceInfo("datasets", "lazydata")
objects <- sort(ls(lazy, all.names = TRUE), method = "radix")
stopifnot(length(objects) == 108L, length(topics) == 91L,
          nrow(listing) == 108L, length(index$references) == 0L,
          identical(as.integer(index$compressed), 3L),
          identical(sort(names(index$variables), method = "radix"), objects),
          length(getNamespaceExports("datasets")) == 0L)
write_tsv <- function(value, name) {
    write.table(value, file.path(out, name), sep = "\t", row.names = FALSE,
                quote = TRUE, qmethod = "double", na = "")
}
write_tsv(data.frame(package = package), "package.tsv")
write_tsv(data.frame(item = listing[, 1L], title = listing[, 2L]), "index.tsv")
write_tsv(do.call(rbind, lapply(names(topics), function(topic) {
    data.frame(topic = topic, object = topics[[topic]])
})), "topics.tsv")
write_tsv(do.call(rbind, lapply(objects, function(name) {
    value <- get(name, lazy, inherits = FALSE)
    key <- index$variables[[name]]
    data.frame(name = name, type = typeof(value), length = length(value),
               class = paste(class(value), collapse = ","),
               dimensions = paste(dim(value), collapse = ","),
               attributes = paste(names(attributes(value)), collapse = ","),
               offset = key[[1L]], bytes = key[[2L]])
})), "objects.tsv")
# A complete independently serialized expected graph, including attributes.
# Version 2 avoids platform-specific ALTREP encodings while retaining values.
save(list = objects, envir = lazy, file = file.path(out, "all.rda"),
     version = 2L, compress = FALSE)
saveRDS(mget(objects, lazy, inherits = FALSE), file.path(out, "values.rds"),
        version = 2L, compress = FALSE)
for (name in c("Rdata.rdb", "Rdata.rdx", "Rdata.rds")) {
    stopifnot(file.copy(file.path(package, "data", name),
                       file.path(out, name), overwrite = FALSE))
}
stopifnot(file.copy(file.path(package, "Meta", "data.rds"),
                   file.path(out, "data-index.rds"), overwrite = FALSE))
cat("Pinned datasets export complete: 108 objects, 91 topics, 108 index rows\n")
