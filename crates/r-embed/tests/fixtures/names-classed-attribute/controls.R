values <- list(
  factor(c('é', '日本')),
  ordered(c('é', '日本')),
  structure(factor(c('é', '日本')), marker='m', names=c('a','b')),
  structure(1:2, class='demo', marker='m'),
  structure(c('é', '日本'), class='demo', marker='m'),
  factor(character()),
  structure(character(), class='demo')
)
for (n in 0:3) for (shape in c('vector', 'array', 'matrix')) for (i in seq_along(values)) for (mode in c('attr', 'structure')) {
  target <- switch(shape, vector=seq_len(n), array=array(seq_len(n), dim=n), matrix=matrix(seq_len(n), nrow=1))
  value <- values[[i]]
  result <- tryCatch({
    if (mode == 'attr') attr(target, 'names') <- value
    else target <- structure(target, names=value)
    paste(capture.output(dput(names(target))), collapse=' ')
  }, error=function(e) paste0('ERROR:', conditionMessage(e)))
  cat(n, ':', shape, ':', i, ':', mode, '|', result, '\n', sep='')
}
