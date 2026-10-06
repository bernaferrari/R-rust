report <- function(expr) {
    tryCatch(eval(substitute(expr),parent.frame()),error=function(e) {
        cat(conditionMessage(e),'\n',sep='')
        cat(paste(deparse(conditionCall(e)),collapse=' '),'\n',sep='')
    })
}
report(unserialize(1L))
report(unserialize(1))
report(unserialize(c(1L,2L)))
report(unserialize(structure(1L,class='other')))
value <- list(numbers=c(a=1L,b=NA_integer_),matrix=matrix(1:4,2L),nested=list(TRUE,'text'))
bytes <- serialize(value,NULL)
invisible(gc())
print(identical(unserialize(bytes),value))
local({
    con <- rawConnection(bytes,'rb')
    invisible(gc())
    on.exit(close(con))
    print(identical(unserialize(con),value))
})
local({
    f <- tempfile()
    on.exit(unlink(f))
    writeBin(bytes,f)
    con <- file(f,'rb')
    invisible(gc())
    on.exit(close(con),add=TRUE)
    print(identical(unserialize(con),value))
})
