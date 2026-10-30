values <- list(pairlist(a=1,b=2), pairlist(a='é',b='日本'), pairlist(a=TRUE,b=NA), pairlist(a=1:2,b=NULL), pairlist(a=quote(foo),b=3.5), pairlist(a=character(),b=1), pairlist(a=list(1),b=as.raw(2)), pairlist(a=1+2i,b=NA_character_))
for (shape in c('pairlist','language')) for (i in seq_along(values)) for (mode in c('attr','structure','replacement')) {
  target <- if (shape=='pairlist') pairlist(a=1,b=2) else quote(f(a=1,b=2)); value <- values[[i]]
  result <- tryCatch({
    if (mode=='attr') attr(target,'names') <- value
    else if (mode=='structure') target <- structure(target,names=value)
    else names(target) <- value
    paste(capture.output(dput(target)),collapse=' ')
  }, error=function(e) paste0('ERROR:',conditionMessage(e)))
  cat(shape,':',i,':',mode,'|',result,'\n',sep='')
}
