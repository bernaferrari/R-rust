values <- list(pairlist(a=1,b=2), pairlist(a='é',b='日本'), pairlist(a=TRUE,b=NA), pairlist(a=1:2,b=NULL), pairlist(a=quote(foo),b=3.5))
for (n in 0:3) for (i in seq_along(values)) for (mode in c('attr','structure','replacement')) {
  target <- seq_len(n); value <- values[[i]]
  result <- tryCatch({
    if (mode=='attr') attr(target,'names') <- value
    else if (mode=='structure') target <- structure(target,names=value)
    else names(target) <- value
    paste(capture.output(dput(names(target))),collapse=' ')
  }, error=function(e) paste0('ERROR:',conditionMessage(e)))
  cat(n,':',i,':',mode,'|',result,'\n',sep='')
}
