f <- tempfile()
con <- file(f, "wb")
writeBin(as.raw(c(0x61, 0x00, 0x62, 0x0a, 0x63, 0x0a, 0x0a)), con)
close(con)

print(paste(readLines(f, warn = FALSE, skipNul = TRUE), collapse = "/"))
print(paste(readLines(f, warn = FALSE, skipNul = FALSE), collapse = "/"))
print(grepl("URL scheme unsupported", conditionMessage(tryCatch(url(f, "r"), error = function(e) e))))

invisible(unlink(f))
