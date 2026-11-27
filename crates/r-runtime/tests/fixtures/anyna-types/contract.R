cases <- list(null=NULL, raw=as.raw(1), symbol=quote(a), language=quote(a+b), expression=expression(a), environment=baseenv(), closure=function() 1, builtin=sum, special=quote)
for (n in names(cases)) for (nested in c(FALSE, TRUE)) {
 x <- cases[[n]]
 if (nested) x <- list(x)
 ans <- tryCatch(as.character(anyNA(x, recursive=TRUE)), error=function(e) paste0("ERROR:", conditionMessage(e)))
 cat(n, nested, ans, "\n", sep="|")
}
