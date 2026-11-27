#include <R.h>
#include <Rinternals.h>
#include <R_ext/Altrep.h>
#include <R_ext/Rdynload.h>
static R_altrep_class_t cls;
static R_xlen_t len(SEXP x) { return 1; }
static int probe_elt(SEXP x, R_xlen_t i) { R_gc(); SET_INTEGER_ELT(R_altrep_data1(x), 0, 99); return 1; }
SEXP probe(void) {
 SEXP scalar=PROTECT(ScalarInteger(2));
 SEXP list=PROTECT(allocVector(VECSXP,1)); SET_VECTOR_ELT(list,0,scalar);
 SEXP first=PROTECT(R_new_altrep(cls,scalar,R_NilValue));
 SEXP tail=PROTECT(cons(list,R_NilValue));
 SEXP input=PROTECT(cons(first,tail));
 SEXP target=PROTECT(allocVector(INTSXP,2));
 setAttrib(target,R_NamesSymbol,input);
 SEXP out=PROTECT(allocVector(VECSXP,2)); SET_VECTOR_ELT(out,0,getAttrib(target,R_NamesSymbol)); SET_VECTOR_ELT(out,1,scalar);
 UNPROTECT(7); return out;
}
void R_init_probe(DllInfo *dll) { cls=R_make_altinteger_class("probe", "probe", dll); R_set_altrep_Length_method(cls,len); R_set_altinteger_Elt_method(cls,probe_elt); }
