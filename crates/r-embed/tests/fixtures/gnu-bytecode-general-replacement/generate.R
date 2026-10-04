# Pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5,
# R 4.7.0 development r90451; XDR v2, uncompressed.
stopifnot(getRversion()=='4.7.0')
probes<-list(
 nested=function(){x<-list(c(1L,2L));events<-character();rhsValue<-7L;i<-function(){events<<-c(events,'i');1L};j<-function(){events<<-c(events,'j');2L};rhs<-function(){events<<-c(events,'rhs');rhsValue};result<-withVisible(x[[i()]][j()]<-rhs());list(result=result,x=x,events=events)},
 attribute=function(){x<-list(c(1L,2L));events<-character();rhsValue<-quote(unbound_data);i<-function(){events<<-c(events,'i');1L};name<-function(){events<<-c(events,'name');'note'};rhs<-function(){events<<-c(events,'rhs');rhsValue};result<-withVisible(attr(x[[i()]],name())<-rhs());list(result=result,x=x,events=events)},
 names=function(){x<-list(c(1L,2L));events<-character();rhsValue<-c('left','right');i<-function(){events<<-c(events,'i');1L};rhs<-function(){events<<-c(events,'rhs');rhsValue};result<-withVisible(names(x[[i()]])<-rhs());list(result=result,x=x,events=events)}
)
expected<-list(
 nested=list(result=list(value=7L,visible=FALSE),x=list(c(1L,7L)),events=c('rhs','i','j','i')),
 attribute=list(result=list(value=quote(unbound_data),visible=FALSE),x=list(structure(c(1L,2L),note=quote(unbound_data))),events=c('rhs','i','name','i')),
 names=list(result=list(value=c('left','right'),visible=FALSE),x=list(c(left=1L,right=2L)),events=c('rhs','i','i'))
)
for(nm in names(probes)){
 a<-probes[[nm]]();b<-compiler::cmpfun(probes[[nm]])();stopifnot(identical(a,b),identical(a,expected[[nm]]));cat(nm,'\n');dput(a)
}
out<-'crates/r-embed/tests/fixtures/gnu-bytecode-general-replacement'
functions<-list(nested=function()x[[i()]][j()]<-rhs(), attribute=function()attr(x[[i()]],name())<-rhs(), names=function()names(x[[i()]])<-rhs())
for(nm in names(functions)){environment(functions[[nm]])<-baseenv();saveRDS(compiler::cmpfun(functions[[nm]]),file.path(out,paste0(nm,'.rds')),version=2,compress=FALSE)}
cat('Pinned GNU original/cmpfun general replacement oracle passed\n')

custom <- function(){x<-list(c(1L,2L));events<-character();metadata<-list();rhsValue<-7L;i<-function(){events<<-c(events,'i');1L};j<-function(){events<<-c(events,'j');2L};rhs<-function(){events<<-c(events,'rhs');rhsValue};`[[`<-function(x,which){events<<-c(events,'get');metadata<<-.Primitive('[[<-')(metadata,1L,value=list(substitute(x),substitute(which),.subset2(as.list(sys.call()),1L)));.subset2(x,which)};`[<-`<-function(x,where,value){events<<-c(events,'inner');metadata<<-.Primitive('[[<-')(metadata,2L,value=list(substitute(x),substitute(where),substitute(value),.subset2(as.list(sys.call()),1L)));attr(value,'changed')<-TRUE;.Primitive('[<-')(x,where,value=value)};`[[<-`<-function(x,which,value){events<<-c(events,'outer');metadata<<-.Primitive('[[<-')(metadata,3L,value=list(substitute(x),substitute(which),substitute(value),.subset2(as.list(sys.call()),1L)));.Primitive('[[<-')(x,which,value=value)};result<-withVisible(x[[which=i()]][where=j()]<-rhs());list(result=result,x=x,events=events,metadata=metadata,rhs=rhsValue)}
custom.expected<-list(result=list(value=7L,visible=FALSE),x=list(c(1L,7L)),events=c('rhs','get','i','inner','j','outer','i'),metadata=list(list(as.name('*tmp*'),quote(i()),as.name('[[')),list(as.name('*tmp*'),quote(j()),quote(rhs()),as.name('[<-')),list(as.name('*tmp*'),quote(i()),as.name('*vtmp*'),as.name('[[<-'))),rhs=7L)
stopifnot(identical(custom(),custom.expected),identical(compiler::cmpfun(custom)(),custom.expected))
deep<-function(){x<-list(list(c(1L,2L)));events<-character();i<-function(){events<<-c(events,'i');1L};j<-function(){events<<-c(events,'j');1L};k<-function(){events<<-c(events,'k');2L};rhs<-function(){events<<-c(events,'rhs');7L};r<-withVisible(x[[i()]][[j()]][k()]<-rhs());list(result=r,x=x,events=events)}
expected.deep<-list(result=list(value=7L,visible=FALSE),x=list(list(c(1L,7L))),events=c('rhs','i','j','k','j','i'))
stopifnot(identical(deep(),expected.deep),identical(compiler::cmpfun(deep)(),expected.deep))
cat('Tagged syntax, original RHS sharing and deeper occurrence order assertions passed\n')
enclosing<-function(){x<-list(c(1L,2L));events<-character();rhsValue<-c('left','right');i<-function(){events<<-c(events,'i');1L};rhs<-function(){events<<-c(events,'rhs');x<<-list(c(30L,40L));rhsValue};f<-function(){x<-list(c(50L,60L));r<-withVisible(names(x[[i()]])<<-rhs());list(result=r,local=x)};r<-f();list(result=r$result,local=r$local,x=x,events=events)}
expected.enclosing<-list(result=list(value=c('left','right'),visible=FALSE),local=list(c(50L,60L)),x=list(c(left=30L,right=40L)),events=c('rhs','i','i'))
stopifnot(identical(enclosing(),expected.enclosing),identical(compiler::cmpfun(enclosing)(),expected.enclosing))
cat('Enclosing shadow and RHS-root mutation oracle passed\n')

symbolHeads<-function(){x<-list(c(1L,2L));events<-character();metadata<-list();rhsValue<-7L;i<-function(){events<<-c(events,'i');1L};j<-function(){events<<-c(events,'j');2L};rhs<-function(){events<<-c(events,'rhs');rhsValue};leaf<-function(x,slot){events<<-c(events,'leafGet');metadata<<-.Primitive('[[<-')(metadata,1L,value=list(substitute(x),substitute(slot),.subset2(as.list(sys.call()),1L)));.subset2(x,slot)};`leaf<-`<-function(x,slot,value){events<<-c(events,'leafSet');metadata<<-.Primitive('[[<-')(metadata,3L,value=list(substitute(x),substitute(slot),substitute(value),.subset2(as.list(sys.call()),1L)));.Primitive('[[<-')(x,slot,value=value)};`tip<-`<-function(x,index,value){events<<-c(events,'tipSet');metadata<<-.Primitive('[[<-')(metadata,2L,value=list(substitute(x),substitute(index),substitute(value),.subset2(as.list(sys.call()),1L)));.Primitive('[<-')(x,index,value=value)};r<-withVisible(tip(leaf(x,slot=i()),index=j())<-rhs());list(result=r,x=x,events=events,metadata=metadata)}
expected.symbolHeads<-list(result=list(value=7L,visible=FALSE),x=list(c(1L,7L)),events=c('rhs','leafGet','i','tipSet','j','leafSet','i'),metadata=list(list(as.name('*tmp*'),quote(i()),as.name('leaf')),list(as.name('*tmp*'),quote(j()),quote(rhs()),as.name('tip<-')),list(as.name('*tmp*'),quote(i()),as.name('*vtmp*'),as.name('leaf<-'))))
stopifnot(identical(symbolHeads(),expected.symbolHeads),identical(compiler::cmpfun(symbolHeads)(),expected.symbolHeads))
cat('Custom symbol heads and named syntax oracle passed\n')
symbolFunction<-function()tip(leaf(x,slot=i()),index=j())<-rhs()
environment(symbolFunction)<-baseenv()
saveRDS(compiler::cmpfun(symbolFunction),file.path(out,'symbol_heads.rds'),version=2,compress=FALSE)
