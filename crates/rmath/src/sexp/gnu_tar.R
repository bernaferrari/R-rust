{
sq <- function(x) paste0("'", gsub("'", "'\\''", x, fixed = TRUE), "'")

tar <- function(tarfile, files = NULL,
                compression = "none", compression_level = 6,
                tar = Sys.getenv("tar"), extra_flags = "")
{
    if (!nzchar(tar) || tar %in% c("internal", "tar"))
        bin <- "/usr/bin/tar"
    else
        bin <- tar
    if (is.null(files))
        files <- list.files()
    if (is.list(files))
        files <- unlist(files, use.names = FALSE)
    files <- as.character(files)
    files <- files[nzchar(files) & !files %in% c(".", "..")]
    missing <- !file.exists(files)
    if (any(missing))
        warning(sprintf("file '%s' not found", files[missing][1L]), domain = NA)
    files <- files[!missing]
    if (!length(files))
        return(invisible(1L))
    flags <- switch(compression,
                    none = "-cf",
                    gzip = "-zcf",
                    bzip2 = "-jcf",
                    xz = "-Jcf",
                    "-cf")
    cmd <- paste("COPYFILE_DISABLE=1", bin,
                 if (nzchar(extra_flags)) extra_flags else "",
                 flags, sq(tarfile),
                 paste(sq(files), collapse = " "))
    invisible(system(cmd))
}

untar <- function(tarfile, files = NULL, list = FALSE, exdir = ".",
                  compressed = NA, extras = NULL, verbose = FALSE,
                  restore_times = TRUE, tar = Sys.getenv("TAR"), ...)
{
    if (!nzchar(tar) || tar %in% c("internal", "tar"))
        bin <- "/usr/bin/tar"
    else
        bin <- tar
    if (isTRUE(list)) {
        cmd <- paste("COPYFILE_DISABLE=1", bin, "-tf", sq(tarfile))
        return(system(cmd, intern = TRUE))
    }
    cmd <- paste("COPYFILE_DISABLE=1", bin, "-xf", sq(tarfile),
                 "-C", sq(exdir))
    invisible(system(cmd))
}

tar
}
