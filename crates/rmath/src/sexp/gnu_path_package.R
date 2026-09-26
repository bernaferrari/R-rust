{
.rmpkg <- function(pkg) sub("package:", "", pkg, fixed = TRUE)
.packages <- function(all.available = FALSE, lib.loc = NULL) {
    if (is.null(lib.loc)) lib.loc <- .libPaths()
    if (all.available) {
        ans <- character()
        for (lib in lib.loc[file.exists(lib.loc)]) {
            a <- list.files(lib, all.files = FALSE, full.names = FALSE)
            pfile <- file.path(lib, a, "Meta", "package.rds")
            ans <- c(ans, a[file.exists(pfile)])
        }
        return(unique(ans))
    }
    s <- search()
    invisible(.rmpkg(s[startsWith(s, "package:")]))
}
path.package <- function(package = NULL, quiet = FALSE) {
    if (is.null(package)) package <- .packages()
    if (length(package) == 0L) return(character())
    s <- search()
    searchpaths <- lapply(seq_along(s), function(i) attr(as.environment(i), "path"))
    searchpaths[[length(s)]] <- system.file()
    pkgs <- paste0("package:", package)
    pos <- match(pkgs, s)
    if (any(m <- is.na(pos))) {
        if (!quiet) {
            if (all(m)) stop("none of the packages are loaded")
            else warning(sprintf(ngettext(as.integer(sum(m)),
                "package %s is not loaded",
                "packages %s are not loaded"),
                paste(package[m], collapse = ", ")), domain = NA)
        }
        pos <- pos[!m]
    }
    unlist(searchpaths[pos], use.names = FALSE)
}
path.package
}
