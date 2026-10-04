# GNU R devel r90451, source bac583951b728e97b9786804d3b4081f0fe18df5.
# Rscript --vanilla this-file.R reproduces the adjacent TSV.
# Calls only the confirmed five-argument Fortran registration, with valid models.
fmt <- function(x) paste(format(x,digits=17,scientific=TRUE,trim=TRUE),collapse=',')
routine <- get('C_pppred',asNamespace('stats'))
cat('case\targument\ttype\tinput\toutput\n')
one <- function(label,x,smod,scratch=if(smod[5]>0)2L*smod[4]else 0L,alias=FALSE) {
  np <- nrow(x);q <- as.integer(smod[3]+.1)
  input <- list(as.integer(c(np,991L)),c(as.double(x),901.25),
                c(as.double(smod),902.5),rep(3.5,np*q+1L),rep(7.75,scratch+1L))
  if(alias) input[[5]] <- input[[3]]
  out <- do.call(.Fortran,c(list(routine),input))
  stopifnot(length(out)==5L)
  for(i in seq_along(input)) {
    cat(label,i-1L,typeof(input[[i]]),fmt(input[[i]]),fmt(out[[i]]),sep='\t');cat('\n')
  }
  invisible(out)
}
train <- matrix(seq(-1,1,length.out=24),ncol=1)
response <- sin(train[,1]*3)+(1:24)/100
fit <- stats::ppr(train,response,nterms=1,optlevel=0)
pred <- matrix(c(-2,-.2,0,.7,2),ncol=1)
out <- one('trained',pred,fit$smod)
stopifnot(isTRUE(all.equal(out[[4]][1:5],as.double(predict(fit,pred)),tolerance=1e-13)))
one('zero-observations',matrix(double(),nrow=0,ncol=1),fit$smod)
one('shared-model-workspace',pred,fit$smod,alias=TRUE)
two <- cbind(train,cos(train[,1]*2))
fit2 <- stats::ppr(two,cbind(response,cos(train[,1]*3)),nterms=2,optlevel=0)
pred2 <- cbind(pred,cos(pred[,1]*2))
out <- one('multi-response',pred2,fit2$smod)
stopifnot(isTRUE(all.equal(out[[4]][1:10],as.double(predict(fit2,pred2)),tolerance=1e-13)))
constant <- stats::ppr(train,rep(3,24),nterms=1,optlevel=0)
stopifnot(constant$smod[5]==0)
one('zero-terms-full',pred,constant$smod,scratch=0L)
one('zero-terms-short',pred,constant$smod[1:7],scratch=0L)
# Explicit valid packed curves exercise endpoint, tie, permutation and singleton
# contracts independently of the training algorithm. Layout: m,p,q,n,mu,means,ys,
# projection coefficients, response coefficients, curve values, projections.
single <- c(1,2,2,1,1,2,4,2,1,.5,3,2,7,0)
one('singleton-curve',cbind(-1:2,0:3),single)
ties <- c(1,1,1,6,1,0,1,1,1,20,0,10,21,1,11,2,0,1,2,0,1)
one('tied-projections',matrix(c(-1,0,.5,1,1.5,2,3),ncol=1),ties)
descending <- c(1,1,1,24,1,0,1,1,1,24:1,24:1)
one('descending-long',matrix(c(0,1,1.5,8,15.5,24,25),ncol=1),descending)
# The zero-term path genuinely does not read x or sorting workspace.
empty <- .Fortran(routine,5L,double(),as.double(constant$smod[1:7]),double(5),double())
stopifnot(all(empty[[4]]==3),length(empty[[2]])==0,length(empty[[5]])==0)
