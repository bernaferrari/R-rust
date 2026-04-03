#ifndef R_H
#define R_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>

/* Fundamental types - EXACT ABI MATCH */
typedef int32_t Rboolean;
typedef void*   SEXP;
typedef int32_t SEXPTYPE;

#define TRUE  1
#define FALSE 0

#define R_NilValue   ((SEXP)0)
#define R_UnboundValue ((SEXP)1)

/* Entry points */
void Rf_initEmbeddedR(int argc, char **argv);
void Rf_endEmbeddedR(int fatal);

SEXP Rf_eval(SEXP e, SEXP rho);
SEXP Rf_applyClosure(SEXP call, SEXP op, SEXP args, SEXP rho);

SEXP Rf_protect(SEXP s);
void Rf_unprotect(int n);

/* Allocation */
SEXP Rf_allocVector(SEXPTYPE type, int n);
SEXP Rf_allocList(int n);

/* Error handling */
void Rf_error(const char *msg, ...);
void Rf_warning(const char *msg, ...);

#ifdef __cplusplus
}
#endif

#endif /* R_H */
