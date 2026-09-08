/* Test-only GNU R plotmath oracle.  It runs R's real plotmath.c against the
 * bundled DejaVu Sans family through a tiny metric-only graphics device. */
#include <R.h>
#include <Rinternals.h>
#include <R_ext/GraphicsEngine.h>
#include <R_ext/GraphicsDevice.h>
#include <R_ext/Rdynload.h>
#include <ft2build.h>
#include FT_FREETYPE_H
#include <stdlib.h>
#include <stdio.h>

#ifndef RPORT_DEJAVU_DIR
#define RPORT_DEJAVU_DIR "."
#endif

static FT_Library library;
static FT_Face faces[4];
static const char *names[4] = {
    "/DejaVuSans.ttf",
    "/DejaVuSans-Bold.ttf",
    "/DejaVuSans-Oblique.ttf",
    "/DejaVuSans-BoldOblique.ttf"
};

static void metric_info(int c, const pGEcontext gc, double *ascent,
                        double *descent, double *width, pDevDesc dd) {
    int face = gc->fontface - 1;
    if (face < 0 || face > 3) face = 0;
    FT_Face f = faces[face];
    FT_ULong codepoint = (FT_ULong)(c < 0 ? -c : c);
    if (gc->fontface == 5) {
        char in[2] = {(char)c, 0}, out[16] = {0};
        AdobeSymbol2utf8(out, in, sizeof(out), FALSE);
        const unsigned char *u = (const unsigned char *)out;
        if (u[0] < 0x80) codepoint = u[0];
        else if ((u[0] & 0xe0) == 0xc0) codepoint = ((u[0] & 0x1f) << 6) | (u[1] & 0x3f);
        else if ((u[0] & 0xf0) == 0xe0) codepoint = ((u[0] & 0x0f) << 12) | ((u[1] & 0x3f) << 6) | (u[2] & 0x3f);
    }
    if (codepoint && !FT_Get_Char_Index(f, codepoint))
        Rf_error("oracle font lacks U+%04lx", codepoint);
    if (FT_Load_Char(f, codepoint, FT_LOAD_NO_SCALE)) {
        *ascent = *descent = *width = 0;
        return;
    }
    double scale = (gc->ps > 0 ? gc->ps : 12.0) * (gc->cex > 0 ? gc->cex : 1.0)
        / (double)f->units_per_EM;
    FT_Glyph_Metrics m = f->glyph->metrics;
    *ascent = m.horiBearingY * scale;
    *descent = (m.height - m.horiBearingY) * scale;
    *width = m.horiAdvance * scale;
}

static SEXP oracle_metrics(SEXP expr, SEXP point_size) {
    if (!library) {
        if (FT_Init_FreeType(&library)) Rf_error("FreeType init failed");
        const char *directory = getenv("RPORT_DEJAVU_DIR");
        if (!directory) directory = RPORT_DEJAVU_DIR;
        for (int i = 0; i < 4; ++i) {
            char path[4096];
            if (snprintf(path, sizeof(path), "%s%s", directory, names[i]) >= sizeof(path))
                Rf_error("font directory is too long");
            if (FT_New_Face(library, path, 0, &faces[i]))
                Rf_error("cannot load DejaVu face %s", path);
        }
    }
    if (TYPEOF(expr) == EXPRSXP) expr = VECTOR_ELT(expr, 0);
    pDevDesc dev = GEcreateDD();
    if (!dev) Rf_error("GEcreateDD failed");
    dev->metricInfo = metric_info;
    dev->startps = 12;
    /* Device coordinates and callbacks are points; ipr is inches per point. */
    dev->ipr[0] = dev->ipr[1] = 1.0 / 72.0;
    dev->left = dev->bottom = 0;
    dev->right = dev->top = 504;
    pGEDevDesc ge = GEcreateDevDesc(dev);
    if (!ge) { GEfreeDD(dev); Rf_error("GEcreateDevDesc failed"); }
    R_GE_gcontext gc = {0};
    gc.ps = Rf_asReal(point_size);
    if (!R_FINITE(gc.ps) || gc.ps <= 0) Rf_error("invalid point size");
    gc.cex = 1;
    gc.fontface = 1;
    ge->dev->metricInfo = metric_info;
    double width = GEExpressionWidth(expr, &gc, ge);
    double height = GEExpressionHeight(expr, &gc, ge);
    GEdestroyDevDesc(ge);
    SEXP out = PROTECT(allocVector(REALSXP, 2));
    REAL(out)[0] = width / 72.0;
    REAL(out)[1] = height / 72.0;
    UNPROTECT(1);
    return out;
}

static const R_CallMethodDef methods[] = {
    {"plotmath_font_oracle", (DL_FUNC)&oracle_metrics, 2},
    {NULL, NULL, 0}
};
void R_init_plotmath_font_oracle(DllInfo *dll) {
    R_registerRoutines(dll, NULL, methods, NULL, NULL);
    R_useDynamicSymbols(dll, FALSE);
}
