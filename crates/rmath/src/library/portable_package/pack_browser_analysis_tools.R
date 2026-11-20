# Generate the compact browser analysis overlay for the pinned tools archive.
args <- commandArgs(TRUE)
stopifnot(length(args) == 2L, identical(R.version[["svn rev"]], "90451"))
source <- args[[1L]]
output <- args[[2L]]
dir.create(output, recursive = TRUE, showWarnings = FALSE)
dropped <- c(".check_packages", ".install_packages", ".build_packages",
    ".check_package_CRAN_incoming", "format.check_package_CRAN_incoming",
    "check_packages_in_dir")
index <- readRDS(file.path(source, "wasm.index.rds"))
db <- file.path(source, "wasm.data")
directory <- file.path(source, "wasm.blocks")
input <- readBin(db, raw(), n = file.size(db))
end <- matrix(readBin(directory, integer(), n = file.size(directory)/4L, endian = "big"), nrow = 2L)
stopifnot(identical(end[, 1L], c(0L, 0L)), end[2L, ncol(end)] == length(input))
start <- end[, -ncol(end), drop=FALSE]
end <- end[, -1L, drop=FALSE]
original_keys <- c(index$variables, index$references)
keys <- unname(index$variables[dropped])
ns <- asNamespace("tools")
overlay <- file(file.path(output, "wasm.replacements"), "wb")
# Bind the overlay to the original database, directory and index. A gzip
# footer supplies the standard CRC-32 byte order without extra packages.
tmp <- tempfile("rport-tools-crc")
gz <- gzfile(tmp, "wb", compression = 1L)
for (name in c("wasm.data", "wasm.blocks", "wasm.index.rds")) {
    path <- file.path(source, name)
    writeBin(readBin(path, raw(), n = file.size(path)), gz)
}
close(gz)
encoded <- readBin(tmp, raw(), n = file.size(tmp))
unlink(tmp)
writeBin(tail(encoded, 8L)[1:4], overlay)
out_blocks <- file(file.path(output, "wasm.blocks"), "wb")
writeBin(c(0L, 0L), out_blocks, size = 4L, endian = "big")
data_end <- 0L
plain_end <- 0L
blocks <- vapply(keys, function(key) {
    i <- match(key[[1L]], start[1L, ])
    stopifnot(!is.na(i), end[1L, i] == sum(key))
    stopifnot(sum(vapply(original_keys, identical, logical(1), key)) == 1L)
    i
}, 0L)
stopifnot(!anyDuplicated(blocks), !anyDuplicated(dropped))
lengths <- end[1L, ] - start[1L, ]
block_new_starts <- integer(ncol(start))
cut_sites <- integer(length(dropped))
say <- function(...) cat(..., sep = "\t")
for (block in seq_len(ncol(start))) {
    block_new_starts[[block]] <- plain_end
    bytes <- input[seq.int(start[2L, block] + 1L, end[2L, block])]
    slot <- match(block, blocks)
    if (!is.na(slot)) {
        name <- dropped[[slot]]
        stub <- get(name, ns)
        stopifnot(is.function(stub))
        body(stub) <- substitute(stop(MSG, call. = FALSE),
            list(MSG = sprintf("tools maintenance function '%s' is unavailable in the slim browser build. Use the full runtime.", name)))
        environment(stub) <- baseenv()
        code <- serialize(stub, NULL, version = 3)
        lengths[[block]] <- length(code)
        tmp <- tempfile("rport-tools-slim")
        gz <- gzfile(tmp, "wb", compression=9L)
        writeBin(code, gz)
        close(gz)
        packed <- readBin(tmp, raw(), n = file.size(tmp))
        unlink(tmp)
        stopifnot(identical(memDecompress(packed, "gzip"), code))
        stopifnot(length(packed) < length(bytes))
        say(name, length(bytes), length(packed), "\n")
        writeBin(as.integer(c(start[2L, block], end[2L, block], length(packed))),
            overlay, size = 4L, endian = "big")
        writeBin(packed, overlay)
        bytes <- packed
        cut_sites[[slot]] <- block
    }
    plain_end <- plain_end + lengths[[block]]
    data_end <- data_end + length(bytes)
    writeBin(c(plain_end, data_end), out_blocks, size = 4L, endian = "big")
}
close(overlay)
close(out_blocks)
remap <- function(key) {
    block <- findInterval(key[[1L]], start[1L, ])
    stopifnot(block > 0L, sum(key) <= end[1L, block])
    if (block %in% cut_sites) {
        stopifnot(identical(c(as.integer(start[1L, block]), as.integer(end[1L, block] - start[1L, block])), key))
        return(c(block_new_starts[[block]], lengths[[block]]))
    }
    as.integer(c(block_new_starts[[block]] + key[[1L]] - start[1L, block], key[[2L]]))
}
index$variables <- lapply(index$variables, remap)
index$references <- lapply(index$references, remap)
saveRDS(index, file.path(output, "wasm.index.rds"), version = 2, compress = FALSE)
