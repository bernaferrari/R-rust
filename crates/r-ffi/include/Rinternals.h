#ifndef RINTERNALS_H
#define RINTERNALS_H

#include "R.h"

#ifdef __cplusplus
extern "C" {
#endif

/* SEXPTYPE values - EXACT ordinal values for ABI */
enum {
    NILSXP     = 0,
    SYMSXP     = 1,
    LISTSXP    = 2,
    CLOSXP     = 3,
    ENVSXP     = 4,
    PROMSXP    = 5,
    LANGSXP    = 6,
    SPECIALSXP = 7,
    BUILTINSXP = 8,
    CHARSXP    = 9,
    LGLSXP     = 10,
    INTSXP     = 13,
    REALSXP    = 14,
    CPLXSXP    = 15,
    STRSXP     = 16,
    DOTSXP     = 17,
    ANYSXP     = 18,
    VECSXP     = 19,
    EXPRSXP    = 20,
    BCODESXP   = 21,
    EXTPTRSXP  = 22,
    WEAKREFSXP = 23,
    RAWSXP     = 24,
    OBJSXP     = 25,
    FUNSXP     = 99
};

/* Routine registration API */
typedef struct {
    const char *name;
    void (*fun)();
    int numArgs;
} R_CMethodDef;

typedef struct {
    const char *name;
    SEXP (*fun)();
    int numArgs;
} R_CallMethodDef;

typedef struct {
    const char *name;
    SEXP (*fun)();
} R_ExternalMethodDef;

typedef struct {
    const char *name;
    void (*fun)();
    int numArgs;
    int visibility;
} R_FortranMethodDef;

void R_registerRoutines(SEXP dll,
                        R_CMethodDef *cEntries,
                        R_CallMethodDef *callEntries,
                        R_FortranMethodDef *fortranEntries,
                        R_ExternalMethodDef *externalEntries);

void R_useDynamicSymbols(SEXP dll, Rboolean value);
void R_forceSymbols(SEXP dll, Rboolean value);

void* R_GetCCallable(const char *pkg, const char *name);
void R_SetCCallable(const char *pkg, const char *name, void *f);

/* Entry point interfaces */
SEXP Rf_doDotCall(SEXP call, SEXP op, SEXP args, SEXP env);
SEXP Rf_doDotExternal(SEXP call, SEXP op, SEXP args, SEXP env);
SEXP Rf_doDotC(SEXP call, SEXP op, SEXP args, SEXP env);
SEXP Rf_doDotFortran(SEXP call, SEXP op, SEXP args, SEXP env);

#ifdef __cplusplus
}
#endif

#endif /* RINTERNALS_H */
