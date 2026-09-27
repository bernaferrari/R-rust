function(lang, unset = "en")
{
    stopifnot(is.character(lang), length(lang) == 1L,
              lang == "C" || grepl("^[a-z][a-z]", lang))
    curLang <- Sys.getenv("LANGUAGE", unset = NA)
    if(is.na(curLang) || !nzchar(curLang))
        curLang <- unset
    if (!capabilities("NLS") || !exists(".popath", inherits = TRUE) || is.na(get(".popath", inherits = TRUE))) {
        warning("no natural language support or missing translations",
                domain = NA)
        return(invisible(structure(curLang, ok = FALSE)))
    }
    if (Sys.getlocale("LC_CTYPE") %in% c("C", "POSIX") &&
        lang != "C") {
        warning("in a C locale: cannot set language", domain = NA)
        return(invisible(structure(curLang, ok = FALSE)))
    }
    ok <- Sys.setenv(LANGUAGE=lang)
    if(!ok)
        warning(gettextf('Sys.setenv(LANGUAGE="%s") may have failed', lang), domain=NA)
    ok. <- isTRUE(bindtextdomain(NULL))
    invisible(structure(curLang, ok = ok && ok.))
}
