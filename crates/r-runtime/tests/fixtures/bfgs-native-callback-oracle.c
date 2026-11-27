#include <R.h>
#include <Rinternals.h>
#include <R_ext/Applic.h>
#include <math.h>
#pragma STDC FP_CONTRACT OFF
static double objective(int n, double *p, void *state) {
 double x=p[0], y=p[1], residual=y-x*x, distance=1.0-x;
 return 100.0*(residual*residual)+distance*distance;
}
static void gradient(int n, double *p, double *g, void *state) {
 double x=p[0],y=p[1];
 g[0]=-400.0*x*(y-x*x)-2.0*(1.0-x);
 g[1]=200.0*(y-x*x);
}
SEXP rport_native_bfgs_oracle(void) {
 double p[2]={-1.2,1.0},value=NAN;int mask[2]={1,1},nf=0,ng=0,fail=-1;
 vmmin(2,p,&value,objective,gradient,100,0,mask,-INFINITY,1e-8,10,NULL,&nf,&ng,&fail);
 SEXP out=PROTECT(allocVector(REALSXP,6));
 REAL(out)[0]=p[0];REAL(out)[1]=p[1];REAL(out)[2]=value;
 REAL(out)[3]=nf;REAL(out)[4]=ng;REAL(out)[5]=fail;
 UNPROTECT(1);return out;
}
