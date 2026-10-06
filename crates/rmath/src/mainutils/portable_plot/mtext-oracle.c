/* GNU-only oracle probe. Keep the original device and metrics; record its
 * actual text callbacks without allocating R objects during drawing. */
#include <R.h>
#include <Rinternals.h>
#include <R_ext/GraphicsEngine.h>
#include <R_ext/GraphicsDevice.h>
#include <R_ext/Rdynload.h>
#include <stdio.h>

typedef void (*text_fn)(double, double, const char *, double, double,
                        const pGEcontext, pDevDesc);
static pDevDesc device;
static FILE *trace;
static text_fn original_text, original_utf8;

static void record(double x, double y, const char *text, double angle,
                   double adj, const pGEcontext gc, pDevDesc dd)
{
    double ascent, descent, width;
    dd->metricInfo('M', gc, &ascent, &descent, &width, dd);
    for (const unsigned char *p = (const unsigned char *)text; *p; ++p)
        fprintf(trace, "%02x", *p);
    fprintf(trace, "\t%.17g\t%.17g\t%.17g\t%.17g\t%d\t%d\t%.17g\t%.17g\t%.17g\t%.17g\t%.17g\t%.17g\n",
            x, y, angle, adj, gc->col, gc->fontface, gc->ps, gc->cex,
            ascent, descent, width, dd->strWidth(text, gc, dd));
}

static void text_callback(double x, double y, const char *text, double angle,
                          double adj, const pGEcontext gc, pDevDesc dd)
{
    record(x, y, text, angle, adj, gc, dd);
    original_text(x, y, text, angle, adj, gc, dd);
}

static void utf8_callback(double x, double y, const char *text, double angle,
                          double adj, const pGEcontext gc, pDevDesc dd)
{
    record(x, y, text, angle, adj, gc, dd);
    original_utf8(x, y, text, angle, adj, gc, dd);
}

static SEXP start(SEXP filename)
{
    if (trace) error("oracle trace already active");
    if (TYPEOF(filename) != STRSXP || XLENGTH(filename) != 1)
        error("oracle trace requires one path");
    device = GEcurrentDevice()->dev;
    if (!device->text || !device->metricInfo)
        error("oracle requires an actual text device");
    trace = fopen(CHAR(STRING_ELT(filename, 0)), "w");
    if (!trace) error("cannot open oracle trace");
    fprintf(trace, "# device\t%.17g\t%.17g\t%.17g\t%.17g\t%.17g\t%.17g\t%.17g\t%.17g\t%.17g\n",
            device->left, device->right, device->bottom, device->top,
            device->ipr[0], device->ipr[1], device->cra[0], device->cra[1],
            device->yLineBias);
    original_text = device->text;
    original_utf8 = device->textUTF8;
    device->text = text_callback;
    if (original_utf8) device->textUTF8 = utf8_callback;
    return R_NilValue;
}

static SEXP stop(void)
{
    if (!trace) error("oracle trace not active");
    if (GEcurrentDevice()->dev != device)
        error("oracle device changed before trace cleanup");
    device->text = original_text;
    device->textUTF8 = original_utf8;
    fclose(trace);
    trace = NULL;
    device = NULL;
    return R_NilValue;
}

void R_init_rport_mtext_oracle(DllInfo *dll)
{
    static const R_CallMethodDef calls[] = {
        {"rport_trace_start", (DL_FUNC)&start, 1},
        {"rport_trace_stop", (DL_FUNC)&stop, 0},
        {NULL, NULL, 0}
    };
    R_registerRoutines(dll, NULL, calls, NULL, NULL);
    R_useDynamicSymbols(dll, FALSE);
}
