x <- 2
as.logical.flag_class <- function(x, ...) stop('must not dispatch as.logical')
flags <- list(false=FALSE, true=TRUE, na=NA, empty_logical=logical(0),
  int_zero=0L, int_one=1L, int_na=NA_integer_, empty_integer=integer(0),
  negative_zero=-0, double_one=1, double_nan=NaN, double_na=NA_real_,
  double_infinite=Inf, empty_double=numeric(0), complex_zero=0+0i,
  complex_imaginary=1i, complex_na=NA_complex_, empty_complex=complex(0),
  raw_zero=as.raw(0), raw_one=as.raw(1), empty_raw=raw(0),
  upper_F='F', lower_f='f', false_upper='FALSE', false_title='False',
  false_lower='false', false_mixed='FaLsE', false_leading=' FALSE',
  false_trailing='FALSE ', true_string='TRUE', unknown='unknown',
  string_na=NA_character_, empty_string_vector=character(0),
  multi_false=c(FALSE,TRUE), multi_true=c(TRUE,FALSE),
  multi_string_false=c('FALSE','TRUE'), null=NULL, list=list(FALSE),
  expression=expression(FALSE), pairlist=pairlist(FALSE),
  symbol=quote(FALSE_SYMBOL), closure=function()NULL, environment=new.env(),
  classed_zero=structure(0,class='flag_class'), compact_huge_zero=0:1000000000,
  unicode_unknown='FALSEé', long_unknown=paste0('FALSE',strrep('x',100000)))
for (name in names(flags)) {
  ans <- tryCatch(deriv(quote(x^2), 'x', hessian=flags[[name]]),
                  error=function(e) conditionMessage(e))
  cat(name, ':', sep='')
  if (is.character(ans)) cat('error:', ans, '\n', sep='') else {
    value <- eval(ans)
    cat(as.numeric(value), ':', as.numeric(attr(value,'gradient')), ':',sep='')
    h <- attr(value,'hessian')
    if (is.null(h)) cat('none\n') else cat(as.numeric(h),'\n',sep='')
  }
}
