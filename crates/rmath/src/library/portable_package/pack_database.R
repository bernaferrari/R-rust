# Repack a pinned GNU lazy-load database into independently readable Wasm blocks.
args <- commandArgs(TRUE)
stopifnot(length(args) == 3L, identical(R.version[["svn rev"]], "90451"))
package <- args[[3L]]
source <- args[[1L]]
out <- args[[2L]]
dir.create(out, recursive=TRUE, showWarnings=FALSE)

index <- readRDS(file.path(source, "index.rds"))
stopifnot(identical(as.integer(index$compressed), 1L))
keys <- c(unname(index$variables), unname(index$references))
key_id <- function(key) paste(key, collapse=":")
ids <- vapply(keys, key_id, "")
records <- keys[!duplicated(ids)]
records <- records[order(vapply(records, `[[`, 0L, 1L))]
remapped <- setNames(vector("list", length(records)), vapply(records, key_id, ""))

database <- file.path(source, paste0(package, ".rdb"))
input <- file(database, "rb")
data <- file(file.path(out, "wasm.data"), "wb")
blocks <- file(file.path(out, "wasm.blocks"), "wb")
logical_end <- 0L
packed_end <- 0L
pending <- list()
pending_bytes <- 0L
block_bytes <- 64L * 1024L
writeBin(c(logical_end, packed_end), blocks, size=4L, endian="big")
tmp <- tempfile("rport-wasm-block-")
flush_block <- function() {
    plain <- do.call(c, pending)
    stream <- gzfile(tmp, "wb", compression=9L)
    writeBin(plain, stream)
    close(stream)
    packed <- readBin(tmp, raw(), n=file.size(tmp))
    stopifnot(identical(memDecompress(packed, type="gzip"), plain))
    writeBin(packed, data)
    logical_end <<- logical_end + length(plain)
    packed_end <<- packed_end + length(packed)
    writeBin(c(logical_end, packed_end), blocks, size=4L, endian="big")
    pending <<- list()
    pending_bytes <<- 0L
}

original_end <- 0L
for (key in records) {
    stopifnot(length(key) == 2L, key[[1L]] == original_end, key[[2L]] > 4L)
    original_end <- original_end + key[[2L]]
    expected <- readBin(input, integer(), n=1L, size=4L, endian="big")
    packed <- readBin(input, raw(), n=key[[2L]] - 4L)
    stopifnot(expected > 0L, length(packed) == key[[2L]] - 4L)
    plain <- memDecompress(packed, type="gzip")
    stopifnot(length(plain) == expected)
    if (pending_bytes > 0L && pending_bytes + expected > block_bytes) flush_block()
    remapped[[key_id(key)]] <- as.integer(c(logical_end + pending_bytes, expected))
    pending[[length(pending) + 1L]] <- plain
    pending_bytes <- pending_bytes + expected
}
flush_block()
stopifnot(original_end == file.size(database))
close(blocks)
close(data)
close(input)
unlink(tmp)

remap <- function(key) remapped[[key_id(key)]]
index$variables <- lapply(index$variables, remap)
index$references <- lapply(index$references, remap)
index$compressed <- FALSE
saveRDS(index, file.path(out, "wasm.index.rds"), version=2, compress=FALSE)
cat(length(records), "GNU", package, "records;", logical_end, "logical bytes;", packed_end, "packed bytes\n")
