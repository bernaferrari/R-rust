#ifndef R_EXT_RDYNLOAD_H
#define R_EXT_RDYNLOAD_H

#include "../R.h"
#include "../Rinternals.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef void *DllInfo;

DllInfo R_getDllInfo(const char *name);
void R_freeDllInfo(DllInfo info);

SEXP R_libraryLoad(const char *path);
SEXP R_libraryUnload(SEXP dll);

void R_SetExternalPtrAddr(SEXP s, void *addr);
void* R_ExternalPtrAddr(SEXP s);

#ifdef __cplusplus
}
#endif

#endif /* R_EXT_RDYNLOAD_H */
