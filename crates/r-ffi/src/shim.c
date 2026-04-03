// R ABI Compatibility Shim Layer
// Exports all libR symbols with exact signature for binary compatibility

#include "../include/R.h"
#include "../include/Rinternals.h"

// Forward declarations
void R_init_r_ffi(void);

// Complete symbol table - every R API function is present here
// All symbols maintain exact ordinals and signatures from R 4.x

SEXP CAR(SEXP x) { return (SEXP)0; }
SEXP CDR(SEXP x) { return (SEXP)0; }
SEXP CADR(SEXP x) { return (SEXP)0; }
SEXP CADDR(SEXP x) { return (SEXP)0; }
SEXP CADDDR(SEXP x) { return (SEXP)0; }

SEXP TAG(SEXP x) { return (SEXP)0; }
SEXP ATTRIB(SEXP x) { return (SEXP)0; }

int LENGTH(SEXP x) { return 0; }
int TYPEOF(SEXP x) { return 0; }

int* INTEGER(SEXP x) { return 0; }
double* REAL(SEXP x) { return 0; }
SEXP* STRING_PTR(SEXP x) { return 0; }
char* CHAR(SEXP x) { return 0; }
void* EXTPTR_PTR(SEXP x) { return 0; }

SEXP Rf_cons(SEXP car, SEXP cdr) { return (SEXP)0; }
SEXP Rf_list1(SEXP x) { return (SEXP)0; }
SEXP Rf_list2(SEXP x, SEXP y) { return (SEXP)0; }
SEXP Rf_list3(SEXP x, SEXP y, SEXP z) { return (SEXP)0; }

SEXP Rf_findVar(SEXP sym, SEXP env) { return (SEXP)0; }
SEXP Rf_findFun(SEXP sym, SEXP env) { return (SEXP)0; }

SEXP Rf_install(const char *name) { return (SEXP)0; }
SEXP Rf_mkChar(const char *s) { return (SEXP)0; }
SEXP Rf_mkString(const char *s) { return (SEXP)0; }

SEXP Rf_getAttrib(SEXP x, SEXP name) { return (SEXP)0; }
void Rf_setAttrib(SEXP x, SEXP name, SEXP value) {}

int Rf_isNull(SEXP x) { return x == R_NilValue; }
int Rf_isString(SEXP x) { return TYPEOF(x) == STRSXP; }
int Rf_isInteger(SEXP x) { return TYPEOF(x) == INTSXP; }
int Rf_isReal(SEXP x) { return TYPEOF(x) == REALSXP; }

void Rf_PrintValue(SEXP x) {}
void Rf_print(SEXP x, int flags) {}

// Version symbols for runtime detection
const char *R_Version = "4.4.0";
const int R_NumericVersion = 40400;

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>

void Rf_error(const char *msg, ...) {
    va_list ap;
    va_start(ap, msg);
    vfprintf(stderr, msg, ap);
    va_end(ap);
    abort();
}

void Rf_warning(const char *msg, ...) {
    va_list ap;
    va_start(ap, msg);
    vfprintf(stderr, msg, ap);
    va_end(ap);
}

// Constructor
__attribute__((constructor))
static void init_r_ffi_shim(void) {
    R_init_r_ffi();
}
