function(nrow=1,ncol=1,widths=unit(rep(1,ncol),'null'),heights=unit(rep(1,nrow),'null'),default.units='null',respect=FALSE,just='centre') {
  if (length(nrow)!=1L || length(ncol)!=1L || is.na(nrow) || is.na(ncol) || nrow<1 || ncol<1 || nrow!=as.integer(nrow) || ncol!=as.integer(ncol)) stop('invalid layout dimensions')
  if (!(is.logical(respect) || is.numeric(respect)) || any(is.na(respect)) || (!is.matrix(respect) && length(respect)!=1L)) stop('invalid layout respect')
  if (!is.unit(widths)) widths<-unit(widths,default.units)
  if (!is.unit(heights)) heights<-unit(heights,default.units)
  structure(list(nrow=nrow,ncol=ncol,widths=widths,heights=heights,respect=as.numeric(respect)!=0,respect.matrix=is.matrix(respect),just=just),class='layout')
}
