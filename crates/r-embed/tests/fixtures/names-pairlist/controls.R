values <- list(pairlist(a=1,b=2), pairlist(a='é',b='日本'), pairlist(a=TRUE,b=NA), pairlist(a=1:2,b=NULL), pairlist(a=quote(foo),b=3.5), pairlist(a=character(),b=1), pairlist(a=list(1),b=as.raw(2)), pairlist(a=1+2i,b=NA_character_))
for (n in 0:3) for (shape in c('vector','array','matrix')) for (i in seq_along(values)) for (mode in c('attr','structure','replacement')) {
  target <- switch(shape, vector=seq_len(n), array=array(seq_len(n),dim=n), matrix=matrix(seq_len(n),nrow=1)); value <- values[[i]]
  result <- tryCatch({
    if (mode=='attr') attr(target,'names') <- value
    else if (mode=='structure') target <- structure(target,names=value)
    else names(target) <- value
    paste(capture.output(dput(names(target))),collapse=' ')
  }, error=function(e) paste0('ERROR:',conditionMessage(e)))
  cat(n,':',shape,':',i,':',mode,'|',result,'\n',sep='')
}
