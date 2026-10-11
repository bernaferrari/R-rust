file.show <-
    function (..., header = rep("", nfiles), title = "R Information",
              delete.file = FALSE, pager = getOption("pager"), encoding = "")
{
    files <- path.expand(c(...))
    ## skip directories
    didx <- which(file.info(files)$isdir)
    if (length(didx)) {
        warning("directories are ignored")
        files <- files[-didx]
    }
    nfiles <- length(files)
    if(nfiles == 0L)
        return(invisible(NULL))
    ## avoid re-encoding files to the current encoding.
    if(l10n_info()[["UTF-8"]] && encoding == "UTF-8") encoding <- ""
    if(l10n_info()[["Latin-1"]] && encoding == "latin1") encoding <- ""
    if(!is.na(encoding) && nzchar(encoding)) {
        for(i in seq_along(files)) {
            f <- files[i]
            tf <- tempfile()
            tmp <- list(readBin(f, "raw", file.size(f)))
            tmp2 <- try(iconv(tmp, encoding, "", "byte"))
            if(inherits(tmp2, "try-error")) file.copy(f, tf)
            else {
                tmp2 <- strsplit(tmp2, "\r\n?|\n", perl = TRUE)[[1L]]
                writeLines(tmp2, tf)
                files[i] <- tf
            }
        }
        files <- enc2utf8(files)
    }
    .Internal(file.show(files, header, title, delete.file, pager))
}
