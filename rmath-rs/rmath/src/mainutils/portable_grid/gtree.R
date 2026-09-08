function(...,name=NULL,gp=NULL,vp=NULL,children=NULL,childrenvp=NULL,cl=NULL) {
    if(!is.null(childrenvp)) stop('gTree childrenvp is not supported')
    if(is.null(children)) children<-gList()
    structure(list(name=name,gp=gp,vp=vp,children=children),class=c(cl,'gTree','grob','gDesc'))
}
