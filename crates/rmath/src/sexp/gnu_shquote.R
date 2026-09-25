{
shQuote <- function(string, type = c("sh", "csh", "cmd", "cmd2"))
{
    if(missing(type) && .Platform$OS.type == "windows") type <- "cmd"
    type <- match.arg(type)
    if(type == "cmd") {
        string <- gsub("(\\\\*)\"", "\\1\\1\\\\\"", string)
        string <- sub("(\\\\+)$", "\\1\\1", string)
        paste0("\"", string, "\"", recycle0 = TRUE)
    } else if (type == "cmd2")
        gsub('([()%!^"<>&|])', "^\\1", string)
    else if(!any(grepl("'", string)))
	paste0("'", string, "'", recycle0 = TRUE)
    else if(type == "sh")
	paste0('"', gsub('(["$`\\])', "\\\\\\1", string), '"')
    else if(!any(grepl("([$`])", string)))
	paste0('"', gsub('(["!\\])' , "\\\\\\1", string), '"')
    else
	paste0("'", gsub("'", "'\"'\"'", string, fixed = TRUE), "'")
}
shQuote
}
