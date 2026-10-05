stopifnot(getRversion() == '4.7.0', R.version[['svn rev']] == '90451')
cases <- c(
 '.Internal(parent.env(1))',
 '.Internal(parent.env(NULL))',
 '.Internal(parent.env(emptyenv()))',
 '.Internal(parent.env())',
 '.Internal(parent.env(baseenv(), baseenv()))',
 '.Internal(`parent.env<-`(NULL, baseenv()))',
 '.Internal(`parent.env<-`(new.env(), 1))',
 '.Internal(`parent.env<-`(new.env(), NULL))',
 '.Internal(`parent.env<-`(emptyenv(), baseenv()))',
 '.Internal(`parent.env<-`(baseenv(), globalenv()))',
 '.Internal(`parent.env<-`(asNamespace("base"), globalenv()))',
 '.Internal(`parent.env<-`(parent.env(asNamespace("methods")), globalenv()))',
 'local({ e <- new.env(); p <- new.env(parent=emptyenv()); lockEnvironment(e); identical(.Internal(`parent.env<-`(e,p)),e) && identical(parent.env(e),p) })',
 'local({ e <- new.env(parent=emptyenv()); p <- new.env(parent=e); .Internal(`parent.env<-`(e,p)) })'
)
cat('expression\tresult\n')
for (code in cases) {
 result <- tryCatch({value <- eval(parse(text=code)); if (is.logical(value)) as.character(value) else 'OK'}, error=function(e) conditionMessage(e))
 cat(code, result, sep='\t'); cat('\n')
}
