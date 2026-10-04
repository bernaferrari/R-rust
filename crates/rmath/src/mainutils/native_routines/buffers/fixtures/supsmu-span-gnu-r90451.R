# GNU R devel r90451, source bac583951b728e97b9786804d3b4081f0fe18df5.
# Rscript --vanilla this-file.R reproduces the adjacent TSV.
# Explicit C collation pins plain character comparison, independently of parsing.
invisible(Sys.setlocale('LC_COLLATE', 'C'))
fmt <- function(x) paste(format(x, digits=17, scientific=TRUE, trim=TRUE), collapse=',')
x <- (0:23)/23
y <- sin(x*6)+x*.25+rep(c(.03,-.02),12)
cases <- list(decimal='0.25', trailing='0.25 ', zeros='00.25', exponent='0e3',
              hex='0x1p-2', cv='cv', dot='.25', leading=' 0.25', plus='+0.25',
              too_high='1e-1', junk='0.5foo', empty_text='', text_na='NA',
              empty=character(), multiple=c('cv','0.25'), missing=NA_character_,
              real_empty=numeric(), real_multiple=c(.25,.5), real_missing=NA_real_,
              real_nan=NaN, real_inf=Inf)
cat('case\terror\twarnings\toutput_x\toutput_y\n')
for (label in names(cases)) {
  warnings <- character(); error <- ''
  out <- withCallingHandlers(tryCatch(stats::supsmu(x,y,span=cases[[label]]),
             error=function(e) {error <<- conditionMessage(e); NULL}),
             warning=function(w) {warnings <<- c(warnings, conditionMessage(w));
                                  invokeRestart('muffleWarning')})
  cat(label,error,paste(warnings,collapse='|'),if(is.null(out))''else fmt(out$x),
      if(is.null(out))''else fmt(out$y),sep='\t');cat('\n')
}
