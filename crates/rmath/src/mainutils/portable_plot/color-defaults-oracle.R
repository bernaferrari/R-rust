# Pinned GNU r90451 FixupCol/C_rect output controls. No optional Cairo required.
# Primary source: src/library/graphics/src/plot.c, FixupCol and C_rect.
check_rect <- function(col,border,filled=FALSE,red.border=TRUE) {
 file <- tempfile(fileext=".pdf")
 pdf(file,compress=FALSE)
 par(fg="red");plot.new();plot.window(c(0,1),c(0,1))
 rect(.2,.2,.8,.8,col=col,border=border)
 dev.off()
 lines <- readLines(file,warn=FALSE,skipNul=TRUE)
 unlink(file)
 begin <- which(lines=="stream")[1L];end <- which(lines=="endstream")[1L]
 content <- lines[seq.int(begin+1L,end-1L)]
 if (red.border) stopifnot(any(content=="1.000 0.000 0.000 SCN"))
 if (filled) stopifnot(any(content=="0.000 0.000 1.000 scn"),any(trimws(content)=="B"))
 else if (red.border) stopifnot(any(trimws(content)=="S"),!any(trimws(content) %in% c("B","f")))
 else stopifnot(!any(trimws(content) %in% c("B","f","S")))
}
for (value in list(character(),integer(),double(),logical(),list(),raw())) check_rect(value,value)
check_rect("blue",NULL,filled=TRUE)
check_rect(NA_character_,NA_character_,red.border=FALSE)
pdf(NULL);plot.new();plot.window(c(0,1),c(0,1))
message <- tryCatch(rect(.2,.2,.8,.8,col="not_an_actual_color"),error=conditionMessage)
stopifnot(grepl("invalid color name",message,fixed=TRUE))
rect(.2,.2,.8,.8,col=character(),border=character());dev.off()
cat("GNU empty-color6 + NULL/current-foreground + NA + same-device recovery: PASS\n")
