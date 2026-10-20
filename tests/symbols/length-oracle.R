local({
    ascii <- strrep('x', 10000L)
    utf8 <- strrep('é', 5000L)
    valid <- identical(as.character(as.name(ascii)), ascii) &&
             identical(as.character(as.name(utf8)), utf8)
    ascii_error <- tryCatch(as.name(paste0(ascii, 'x')),
                            error=function(e) conditionMessage(e))
    utf8_error <- tryCatch(as.name(paste0(utf8, 'é')),
                           error=function(e) conditionMessage(e))
    valid && identical(ascii_error, 'variable names are limited to 10000 bytes') &&
             identical(utf8_error, 'variable names are limited to 10000 bytes')
})
