#include <Rinternals.h>
#include <R_ext/GraphicsEngine.h>
#include <math.h>
#include <string.h>

#ifndef CE_NATIVE
#define CE_NATIVE 0
#define CE_UTF8 1
#define CE_LATIN1 2
#define CE_BYTES 3
#define CE_SYMBOL 5
#endif

static double ge_width_for_line(const char *line, const pGEcontext gc, pGEDevDesc dd, Rboolean utf8)
{
    if (dd == NULL || dd->dev == NULL) {
        return 0.0;
    }
    if (utf8 && dd->dev->strWidthUTF8 != NULL) {
        return dd->dev->strWidthUTF8(line, gc, dd->dev);
    }
    if (dd->dev->strWidth != NULL) {
        return dd->dev->strWidth(line, gc, dd->dev);
    }
    return 0.0;
}

static double ge_max_line_width(const char *str, const pGEcontext gc, pGEDevDesc dd, Rboolean utf8)
{
    if (str == NULL || *str == '\0') {
        return 0.0;
    }

    double width = 0.0;
    const char *line = str;
    const char *p = str;
    while (1) {
        if (*p == '\n' || *p == '\0') {
            size_t len = (size_t)(p - line);
            char *buf = (char *) R_alloc(len + 1, sizeof(char));
            memcpy(buf, line, len);
            buf[len] = '\0';
            double w = ge_width_for_line(buf, gc, dd, utf8);
            if (w > width) {
                width = w;
            }
            if (*p == '\0') {
                break;
            }
            line = p + 1;
        }
        ++p;
    }
    return width;
}

void rmath_ge_set_clip(double x1, double y1, double x2, double y2, pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL || dd->dev->clip == NULL) {
        return;
    }
    dd->dev->clip(x1, x2, y1, y2, dd->dev);
}

void rmath_ge_line(double x1, double y1, double x2, double y2, const pGEcontext gc, pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL || dd->dev->line == NULL) {
        return;
    }
    dd->dev->line(x1, y1, x2, y2, gc, dd->dev);
}

void rmath_ge_polyline(int n, double *x, double *y, const pGEcontext gc, pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL || dd->dev->polyline == NULL) {
        return;
    }
    dd->dev->polyline(n, x, y, gc, dd->dev);
}

void rmath_ge_polygon(int n, double *x, double *y, const pGEcontext gc, pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL || dd->dev->polygon == NULL) {
        return;
    }
    dd->dev->polygon(n, x, y, gc, dd->dev);
}

void rmath_ge_circle(double x, double y, double radius, const pGEcontext gc, pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL || dd->dev->circle == NULL) {
        return;
    }
    dd->dev->circle(x, y, radius, gc, dd->dev);
}

void rmath_ge_rect(double x0, double y0, double x1, double y1, const pGEcontext gc, pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL || dd->dev->rect == NULL) {
        return;
    }
    dd->dev->rect(x0, y0, x1, y1, gc, dd->dev);
}

void rmath_ge_path(double *x, double *y, int npoly, int *nper, int winding, const pGEcontext gc, pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL || dd->dev->path == NULL) {
        return;
    }
    dd->dev->path(x, y, npoly, nper, winding ? TRUE : FALSE, gc, dd->dev);
}

void rmath_ge_raster(unsigned int *raster,
                     int w,
                     int h,
                     double x,
                     double y,
                     double width,
                     double height,
                     double angle,
                     int interpolate,
                     const pGEcontext gc,
                     pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL || dd->dev->raster == NULL) {
        return;
    }
    dd->dev->raster(raster, w, h, x, y, width, height, angle,
                    interpolate ? TRUE : FALSE, gc, dd->dev);
}

void rmath_ge_text(double x,
                   double y,
                   const char *str,
                   double rot,
                   double hadj,
                   const pGEcontext gc,
                   pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL || dd->dev->text == NULL) {
        return;
    }
    dd->dev->text(x, y, str, rot, hadj, gc, dd->dev);
}

void rmath_ge_text_with_encoding(double x,
                                 double y,
                                 const char *str,
                                 int enc,
                                 double rot,
                                 double hadj,
                                 const pGEcontext gc,
                                 pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL) {
        return;
    }
    if (dd->dev->hasTextUTF8 == TRUE && dd->dev->textUTF8 != NULL && enc != CE_NATIVE) {
        dd->dev->textUTF8(x, y, str, rot, hadj, gc, dd->dev);
        return;
    }
    if (dd->dev->text != NULL) {
        dd->dev->text(x, y, str, rot, hadj, gc, dd->dev);
    }
}

void rmath_ge_mode(int mode, pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL || dd->dev->mode == NULL) {
        return;
    }
    dd->dev->mode(mode, dd->dev);
}

void rmath_ge_new_page(const pGEcontext gc, pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL || dd->dev->newPage == NULL) {
        return;
    }
    dd->dev->newPage(gc, dd->dev);
}

void rmath_ge_stroke(SEXP path, const pGEcontext gc, pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL || dd->dev->stroke == NULL) {
        return;
    }
    dd->dev->stroke(path, gc, dd->dev);
}

void rmath_ge_fill(SEXP path, int rule, const pGEcontext gc, pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL || dd->dev->fill == NULL) {
        return;
    }
    dd->dev->fill(path, rule, gc, dd->dev);
}

void rmath_ge_fill_stroke(SEXP path, int rule, const pGEcontext gc, pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL || dd->dev->fillStroke == NULL) {
        return;
    }
    dd->dev->fillStroke(path, rule, gc, dd->dev);
}

int rmath_ge_device_dirty(pGEDevDesc dd) {
    if (dd == NULL) {
        return FALSE;
    }
    return dd->dirty;
}

void rmath_ge_mark_dirty(pGEDevDesc dd) {
    if (dd == NULL) {
        return;
    }
    dd->dirty = TRUE;
}

void rmath_ge_mark_clean(pGEDevDesc dd) {
    if (dd == NULL) {
        return;
    }
    dd->dirty = FALSE;
}

int rmath_ge_recording(pGEDevDesc dd) {
    if (dd == NULL) {
        return FALSE;
    }
    return dd->recordGraphics;
}

void rmath_ge_set_recording(pGEDevDesc dd, int value) {
    if (dd == NULL) {
        return;
    }
    dd->recordGraphics = value ? TRUE : FALSE;
}

double rmath_ge_device_left(pGEDevDesc dd) {
    return (dd != NULL && dd->dev != NULL) ? dd->dev->left : 0.0;
}

double rmath_ge_device_right(pGEDevDesc dd) {
    return (dd != NULL && dd->dev != NULL) ? dd->dev->right : 0.0;
}

double rmath_ge_device_bottom(pGEDevDesc dd) {
    return (dd != NULL && dd->dev != NULL) ? dd->dev->bottom : 0.0;
}

double rmath_ge_device_top(pGEDevDesc dd) {
    return (dd != NULL && dd->dev != NULL) ? dd->dev->top : 0.0;
}

double rmath_ge_device_ipr_x(pGEDevDesc dd) {
    return (dd != NULL && dd->dev != NULL) ? dd->dev->ipr[0] : 0.0;
}

double rmath_ge_device_ipr_y(pGEDevDesc dd) {
    return (dd != NULL && dd->dev != NULL) ? dd->dev->ipr[1] : 0.0;
}

double rmath_ge_device_cra_y(pGEDevDesc dd) {
    return (dd != NULL && dd->dev != NULL) ? dd->dev->cra[1] : 0.0;
}

double rmath_ge_device_startps(pGEDevDesc dd) {
    return (dd != NULL && dd->dev != NULL) ? dd->dev->startps : 1.0;
}

int rmath_ge_device_has_text_utf8(pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL) {
        return FALSE;
    }
    return dd->dev->hasTextUTF8;
}

int rmath_ge_device_want_symbol_utf8(pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL) {
        return FALSE;
    }
    return dd->dev->wantSymbolUTF8;
}

int rmath_ge_device_version(pGEDevDesc dd) {
    if (dd == NULL || dd->dev == NULL) {
        return 0;
    }
    return dd->dev->deviceVersion;
}

int rmath_ge_gc_fontface(const pGEcontext gc) {
    return (gc != NULL) ? gc->fontface : 0;
}

double rmath_ge_gc_cex(const pGEcontext gc) {
    return (gc != NULL) ? gc->cex : 1.0;
}

double rmath_ge_gc_ps(const pGEcontext gc) {
    return (gc != NULL) ? gc->ps : 12.0;
}

double rmath_ge_gc_lineheight(const pGEcontext gc) {
    return (gc != NULL) ? gc->lineheight : 1.0;
}

const char *rmath_ge_gc_fontfamily(const pGEcontext gc) {
    return (gc != NULL) ? gc->fontfamily : NULL;
}

double rmath_ge_from_device_x(double value, int to, pGEDevDesc dd)
{
    double result = value;
    if (dd == NULL || dd->dev == NULL) {
        return result;
    }
    switch (to) {
    case GE_DEVICE:
        break;
    case GE_NDC:
        result = (result - dd->dev->left) / (dd->dev->right - dd->dev->left);
        break;
    case GE_INCHES:
        result = (result - dd->dev->left) / (dd->dev->right - dd->dev->left) *
            fabs(dd->dev->right - dd->dev->left) * dd->dev->ipr[0];
        break;
    case GE_CM:
        result = (result - dd->dev->left) / (dd->dev->right - dd->dev->left) *
            fabs(dd->dev->right - dd->dev->left) * dd->dev->ipr[0] * 2.54;
        break;
    }
    return result;
}

double rmath_ge_to_device_x(double value, int from, pGEDevDesc dd)
{
    double result = value;
    if (dd == NULL || dd->dev == NULL) {
        return result;
    }
    switch (from) {
    case GE_CM:
        result = result / 2.54;
    case GE_INCHES:
        result = (result / dd->dev->ipr[0]) / fabs(dd->dev->right - dd->dev->left);
    case GE_NDC:
        result = dd->dev->left + result * (dd->dev->right - dd->dev->left);
    case GE_DEVICE:
        break;
    }
    return result;
}

double rmath_ge_from_device_y(double value, int to, pGEDevDesc dd)
{
    double result = value;
    if (dd == NULL || dd->dev == NULL) {
        return result;
    }
    switch (to) {
    case GE_DEVICE:
        break;
    case GE_NDC:
        result = (result - dd->dev->bottom) / (dd->dev->top - dd->dev->bottom);
        break;
    case GE_INCHES:
        result = (result - dd->dev->bottom) / (dd->dev->top - dd->dev->bottom) *
            fabs(dd->dev->top - dd->dev->bottom) * dd->dev->ipr[1];
        break;
    case GE_CM:
        result = (result - dd->dev->bottom) / (dd->dev->top - dd->dev->bottom) *
            fabs(dd->dev->top - dd->dev->bottom) * dd->dev->ipr[1] * 2.54;
        break;
    }
    return result;
}

double rmath_ge_to_device_y(double value, int from, pGEDevDesc dd)
{
    double result = value;
    if (dd == NULL || dd->dev == NULL) {
        return result;
    }
    switch (from) {
    case GE_CM:
        result = result / 2.54;
    case GE_INCHES:
        result = (result / dd->dev->ipr[1]) / fabs(dd->dev->top - dd->dev->bottom);
    case GE_NDC:
        result = dd->dev->bottom + result * (dd->dev->top - dd->dev->bottom);
    case GE_DEVICE:
        break;
    }
    return result;
}

double rmath_ge_from_device_width(double value, int to, pGEDevDesc dd)
{
    double result = value;
    if (dd == NULL || dd->dev == NULL) {
        return result;
    }
    switch (to) {
    case GE_DEVICE:
        break;
    case GE_NDC:
        result = result / (dd->dev->right - dd->dev->left);
        break;
    case GE_INCHES:
        result = result * dd->dev->ipr[0];
        break;
    case GE_CM:
        result = result * dd->dev->ipr[0] * 2.54;
        break;
    }
    return result;
}

double rmath_ge_to_device_width(double value, int from, pGEDevDesc dd)
{
    double result = value;
    if (dd == NULL || dd->dev == NULL) {
        return result;
    }
    switch (from) {
    case GE_CM:
        result = result / 2.54;
    case GE_INCHES:
        result = (result / dd->dev->ipr[0]) / fabs(dd->dev->right - dd->dev->left);
    case GE_NDC:
        result = result * (dd->dev->right - dd->dev->left);
    case GE_DEVICE:
        break;
    }
    return result;
}

double rmath_ge_from_device_height(double value, int to, pGEDevDesc dd)
{
    double result = value;
    if (dd == NULL || dd->dev == NULL) {
        return result;
    }
    switch (to) {
    case GE_DEVICE:
        break;
    case GE_NDC:
        result = result / (dd->dev->top - dd->dev->bottom);
        break;
    case GE_INCHES:
        result = result * dd->dev->ipr[1];
        break;
    case GE_CM:
        result = result * dd->dev->ipr[1] * 2.54;
        break;
    }
    return result;
}

double rmath_ge_to_device_height(double value, int from, pGEDevDesc dd)
{
    double result = value;
    if (dd == NULL || dd->dev == NULL) {
        return result;
    }
    switch (from) {
    case GE_CM:
        result = result / 2.54;
    case GE_INCHES:
        result = (result / dd->dev->ipr[1]) / fabs(dd->dev->top - dd->dev->bottom);
    case GE_NDC:
        result = result * (dd->dev->top - dd->dev->bottom);
    case GE_DEVICE:
        break;
    }
    return result;
}

void rmath_ge_metric_info(int c, const pGEcontext gc,
                          double *ascent, double *descent, double *width,
                          pGEDevDesc dd)
{
    if (ascent != NULL) *ascent = 0.0;
    if (descent != NULL) *descent = 0.0;
    if (width != NULL) *width = 0.0;
    if (dd == NULL || dd->dev == NULL || dd->dev->metricInfo == NULL) {
        return;
    }
    dd->dev->metricInfo(c, gc, ascent, descent, width, dd->dev);
}

double rmath_ge_str_width(const char *str, int enc, const pGEcontext gc, pGEDevDesc dd)
{
    if (str == NULL || *str == '\0') {
        return 0.0;
    }
    return ge_max_line_width(str, gc, dd, FALSE);
}

double rmath_ge_str_width_utf8(const char *str, const pGEcontext gc, pGEDevDesc dd)
{
    if (str == NULL || *str == '\0') {
        return 0.0;
    }
    return ge_max_line_width(str, gc, dd, TRUE);
}

double rmath_ge_str_height(const char *str, int enc, const pGEcontext gc, pGEDevDesc dd)
{
    if (str == NULL || *str == '\0') {
        return 0.0;
    }
    int n = 0;
    for (const char *s = str; *s != '\0'; ++s) {
        if (*s == '\n') {
            n++;
        }
    }
    double asc = 0.0, dsc = 0.0, wid = 0.0;
    rmath_ge_metric_info('M', gc, &asc, &dsc, &wid, dd);
    double lineheight = 1.0;
    if (gc != NULL && dd != NULL && dd->dev != NULL && dd->dev->startps != 0.0) {
        lineheight = gc->lineheight * gc->cex * dd->dev->cra[1] * gc->ps / dd->dev->startps;
    }
    if (asc == 0.0 && dsc == 0.0 && wid == 0.0) {
        asc = lineheight;
    }
    return n * lineheight + asc;
}

void rmath_ge_str_metric(const char *str, int enc, const pGEcontext gc,
                         double *ascent, double *descent, double *width,
                         pGEDevDesc dd)
{
    if (ascent != NULL) *ascent = 0.0;
    if (descent != NULL) *descent = 0.0;
    if (width != NULL) *width = 0.0;
    if (str == NULL || *str == '\0') {
        return;
    }
    double asc = 0.0, dsc = 0.0, wid = 0.0;
    rmath_ge_metric_info('M', gc, &asc, &dsc, &wid, dd);
    double lineheight = rmath_ge_str_height(str, enc, gc, dd);
    if (ascent != NULL) *ascent = asc + (lineheight - asc);
    if (descent != NULL) *descent = dsc;
    if (width != NULL) *width = rmath_ge_str_width(str, enc, gc, dd);
}

void rmath_ge_symbol(double x, double y, int pch, double size,
                     const pGEcontext gc, pGEDevDesc dd)
{
    double r, xc, yc;
    double xx[4], yy[4];
    unsigned int maxchar = (mbcslocale && gc != NULL && gc->fontface != 5) ? 127 : 255;
    pGEcontext use_gc = (pGEcontext) gc;
    R_GE_gcontext mutable_gc;

    if (pch == NA_INTEGER) {
        return;
    } else if (pch < 0) {
        if (gc != NULL && gc->fontface == 5) {
            error("use of negative pch with symbol font is invalid");
        }
        char str[16];
        size_t res = Rf_ucstoutf8(str, (unsigned int) (-pch));
        str[res] = '\0';
        rmath_ge_text_with_encoding(x, y, str, CE_UTF8, 0.0, NA_REAL, gc, dd);
    } else if (' ' <= pch && pch <= (int) maxchar) {
        if (pch == '.') {
            if (gc != NULL) {
                mutable_gc = *gc;
                mutable_gc.fill = mutable_gc.col;
                mutable_gc.col = R_TRANWHITE;
                use_gc = &mutable_gc;
            }
            xc = size * fabs(rmath_ge_to_device_width(0.005, GE_INCHES, dd));
            yc = size * fabs(rmath_ge_to_device_height(0.005, GE_INCHES, dd));
            if (size > 0 && xc < 0.5) xc = 0.5;
            if (size > 0 && yc < 0.5) yc = 0.5;
            rmath_ge_rect(x - xc, y - yc, x + xc, y + yc, use_gc, dd);
        } else {
            char str[2] = { (char) pch, '\0' };
            rmath_ge_text_with_encoding(x, y, str,
                                        (gc != NULL && gc->fontface == 5) ? CE_SYMBOL : CE_NATIVE,
                                        0.0, NA_REAL, gc, dd);
        }
    } else if (pch > (int) maxchar) {
        warning("pch value '%d' is invalid in this locale", pch);
    } else {
        double gstr0 = rmath_ge_from_device_width(size, GE_INCHES, dd);
        switch (pch) {
        case 0:
            xc = rmath_ge_to_device_width(0.375 * gstr0, GE_INCHES, dd);
            yc = rmath_ge_to_device_height(0.375 * gstr0, GE_INCHES, dd);
            if (gc != NULL) {
                mutable_gc = *gc;
                mutable_gc.fill = R_TRANWHITE;
                use_gc = &mutable_gc;
            }
            rmath_ge_rect(x - xc, y - yc, x + xc, y + yc, use_gc, dd);
            break;
        case 1:
            xc = 0.375 * size;
            if (gc != NULL) {
                mutable_gc = *gc;
                mutable_gc.fill = R_TRANWHITE;
                use_gc = &mutable_gc;
            }
            rmath_ge_circle(x, y, xc, use_gc, dd);
            break;
        case 2:
            xc = 0.375 * gstr0;
            r = rmath_ge_to_device_height(1.55512030155621416073 * xc, GE_INCHES, dd);
            yc = rmath_ge_to_device_height(0.77756015077810708036 * xc, GE_INCHES, dd);
            xc = rmath_ge_to_device_width(1.34677368708859836060 * xc, GE_INCHES, dd);
            xx[0] = x; yy[0] = y + r;
            xx[1] = x + xc; yy[1] = y - yc;
            xx[2] = x - xc; yy[2] = y - yc;
            if (gc != NULL) {
                mutable_gc = *gc;
                mutable_gc.fill = R_TRANWHITE;
                use_gc = &mutable_gc;
            }
            rmath_ge_polygon(3, xx, yy, use_gc, dd);
            break;
        case 3:
            xc = rmath_ge_to_device_width(M_SQRT2 * 0.375 * gstr0, GE_INCHES, dd);
            yc = rmath_ge_to_device_height(M_SQRT2 * 0.375 * gstr0, GE_INCHES, dd);
            rmath_ge_line(x - xc, y, x + xc, y, gc, dd);
            rmath_ge_line(x, y - yc, x, y + yc, gc, dd);
            break;
        case 4:
            xc = rmath_ge_to_device_width(0.375 * gstr0, GE_INCHES, dd);
            yc = rmath_ge_to_device_height(0.375 * gstr0, GE_INCHES, dd);
            rmath_ge_line(x - xc, y - yc, x + xc, y + yc, gc, dd);
            rmath_ge_line(x - xc, y + yc, x + xc, y - yc, gc, dd);
            break;
        case 5:
            xc = rmath_ge_to_device_width(M_SQRT2 * 0.375 * gstr0, GE_INCHES, dd);
            yc = rmath_ge_to_device_height(M_SQRT2 * 0.375 * gstr0, GE_INCHES, dd);
            xx[0] = x - xc; yy[0] = y;
            xx[1] = x; yy[1] = y + yc;
            xx[2] = x + xc; yy[2] = y;
            xx[3] = x; yy[3] = y - yc;
            if (gc != NULL) {
                mutable_gc = *gc;
                mutable_gc.fill = R_TRANWHITE;
                use_gc = &mutable_gc;
            }
            rmath_ge_polygon(4, xx, yy, use_gc, dd);
            break;
        case 6:
            xc = 0.375 * gstr0;
            r = rmath_ge_to_device_height(1.55512030155621416073 * xc, GE_INCHES, dd);
            yc = rmath_ge_to_device_height(0.77756015077810708036 * xc, GE_INCHES, dd);
            xc = rmath_ge_to_device_width(1.34677368708859836060 * xc, GE_INCHES, dd);
            xx[0] = x; yy[0] = y - r;
            xx[1] = x + xc; yy[1] = y + yc;
            xx[2] = x - xc; yy[2] = y + yc;
            if (gc != NULL) {
                mutable_gc = *gc;
                mutable_gc.fill = R_TRANWHITE;
                use_gc = &mutable_gc;
            }
            rmath_ge_polygon(3, xx, yy, use_gc, dd);
            break;
        case 7:
            xc = 0.375 * gstr0;
            yc = 0.375 * gstr0;
            xx[0] = x; yy[0] = y + yc;
            xx[1] = x + xc; yy[1] = y;
            xx[2] = x; yy[2] = y - yc;
            xx[3] = x - xc; yy[3] = y;
            if (gc != NULL) {
                mutable_gc = *gc;
                mutable_gc.fill = R_TRANWHITE;
                use_gc = &mutable_gc;
            }
            rmath_ge_polygon(4, xx, yy, use_gc, dd);
            break;
        case 8:
            xc = rmath_ge_to_device_width(M_SQRT2 * 0.375 * gstr0, GE_INCHES, dd);
            yc = rmath_ge_to_device_height(M_SQRT2 * 0.375 * gstr0, GE_INCHES, dd);
            rmath_ge_line(x - xc, y, x + xc, y, gc, dd);
            rmath_ge_line(x, y - yc, x, y + yc, gc, dd);
            rmath_ge_line(x - xc, y - yc, x + xc, y + yc, gc, dd);
            rmath_ge_line(x - xc, y + yc, x + xc, y - yc, gc, dd);
            break;
        case 9:
            xc = rmath_ge_to_device_width(M_SQRT2 * 0.375 * gstr0, GE_INCHES, dd);
            yc = rmath_ge_to_device_height(M_SQRT2 * 0.375 * gstr0, GE_INCHES, dd);
            xx[0] = x - xc; yy[0] = y - yc;
            xx[1] = x + xc; yy[1] = y - yc;
            xx[2] = x + xc; yy[2] = y + yc;
            xx[3] = x - xc; yy[3] = y + yc;
            if (gc != NULL) {
                mutable_gc = *gc;
                mutable_gc.fill = R_TRANWHITE;
                use_gc = &mutable_gc;
            }
            rmath_ge_polygon(4, xx, yy, use_gc, dd);
            break;
        case 10:
            rmath_ge_text_with_encoding(x, y, "+", CE_NATIVE, 0.0, NA_REAL, gc, dd);
            break;
        case 11:
            rmath_ge_text_with_encoding(x, y, "x", CE_NATIVE, 0.0, NA_REAL, gc, dd);
            break;
        case 12:
            rmath_ge_line(x, y - size / 2.0, x, y + size / 2.0, gc, dd);
            break;
        case 13:
            rmath_ge_line(x - size / 2.0, y, x + size / 2.0, y, gc, dd);
            break;
        case 14:
            rmath_ge_line(x - size / 2.0, y - size / 2.0, x + size / 2.0, y + size / 2.0, gc, dd);
            break;
        case 15:
            rmath_ge_line(x - size / 2.0, y + size / 2.0, x + size / 2.0, y - size / 2.0, gc, dd);
            break;
        default:
            break;
        }
    }
}

void rmath_ge_raster_scale(unsigned int *sraster, int sw, int sh,
                           unsigned int *draster, int dw, int dh) {
    for (int i = 0; i < dh; i++) {
        for (int j = 0; j < dw; j++) {
            int sy = i * sh / dh;
            int sx = j * sw / dw;
            unsigned int pixel = 0;
            if (sx >= 0 && sx < sw && sy >= 0 && sy < sh) {
                pixel = sraster[sy * sw + sx];
            }
            draster[i * dw + j] = pixel;
        }
    }
}

void rmath_ge_raster_interpolate(unsigned int *sraster, int sw, int sh,
                                 unsigned int *draster, int dw, int dh) {
    double scx = (16. * sw) / dw;
    double scy = (16. * sh) / dh;
    int wm2 = sw - 2;
    int hm2 = sh - 2;
    for (int i = 0; i < dh; i++) {
        int ypm = (int) fmax(0.0, scy * i - 8);
        int yp = ypm >> 4;
        int yf = ypm & 0x0f;
        unsigned int *dline = draster + i * dw;
        unsigned int *sline = sraster + yp * sw;
        for (int j = 0; j < dw; j++) {
            int xpm = (int) fmax(0.0, scx * j - 8);
            int xp = xpm >> 4;
            int xf = xpm & 0x0f;
            unsigned int pixels1 = *(sline + xp);
            unsigned int pixels2, pixels3, pixels4;
            if (xp > wm2 || yp > hm2) {
                if (yp > hm2 && xp <= wm2) {
                    pixels2 = *(sline + xp + 1);
                    pixels3 = pixels1;
                    pixels4 = pixels2;
                } else if (xp > wm2 && yp <= hm2) {
                    pixels2 = pixels1;
                    pixels3 = *(sline + sw + xp);
                    pixels4 = pixels3;
                } else {
                    pixels4 = pixels3 = pixels2 = pixels1;
                }
            } else {
                pixels2 = *(sline + xp + 1);
                pixels3 = *(sline + sw + xp);
                pixels4 = *(sline + sw + xp + 1);
            }
            int area00 = (16 - xf) * (16 - yf);
            int area10 = xf * (16 - yf);
            int area01 = (16 - xf) * yf;
            int area11 = xf * yf;
            int v00r = area00 * R_RED(pixels1);
            int v00g = area00 * R_GREEN(pixels1);
            int v00b = area00 * R_BLUE(pixels1);
            int v00a = area00 * R_ALPHA(pixels1);
            int v10r = area10 * R_RED(pixels2);
            int v10g = area10 * R_GREEN(pixels2);
            int v10b = area10 * R_BLUE(pixels2);
            int v10a = area10 * R_ALPHA(pixels2);
            int v01r = area01 * R_RED(pixels3);
            int v01g = area01 * R_GREEN(pixels3);
            int v01b = area01 * R_BLUE(pixels3);
            int v01a = area01 * R_ALPHA(pixels3);
            int v11r = area11 * R_RED(pixels4);
            int v11g = area11 * R_GREEN(pixels4);
            int v11b = area11 * R_BLUE(pixels4);
            int v11a = area11 * R_ALPHA(pixels4);
            unsigned int pixel = (((v00r + v10r + v01r + v11r + 128) >>  8) & 0x000000ff) |
                                 (((v00g + v10g + v01g + v11g + 128)      ) & 0x0000ff00) |
                                 (((v00b + v10b + v01b + v11b + 128) <<  8) & 0x00ff0000) |
                                 (((v00a + v10a + v01a + v11a + 128) << 16) & 0xff000000);
            *(dline + j) = pixel;
        }
    }
}

void rmath_ge_raster_rotated_size(int w, int h, double angle, int *wnew, int *hnew) {
    double diag = sqrt(w * w + h * h);
    double theta = atan2((double) h, (double) w);
    double trx1 = diag * cos(theta + angle);
    double trx2 = diag * cos(theta - angle);
    double try1 = diag * sin(theta + angle);
    double try2 = diag * sin(angle - theta);
    *wnew = (int) (fmax(fabs(trx1), fabs(trx2)) + 0.5);
    *hnew = (int) (fmax(fabs(try1), fabs(try2)) + 0.5);
    if (*wnew < w) *wnew = w;
    if (*hnew < h) *hnew = h;
}

void rmath_ge_raster_rotated_offset(int w, int h, double angle, int botleft,
                                    double *xoff, double *yoff) {
    double hypot = .5 * sqrt(w * w + h * h);
    double theta, dw, dh;
    if (botleft) {
        theta = M_PI + atan2(h, w);
        dw = hypot * cos(theta + angle);
        dh = hypot * sin(theta + angle);
        *xoff = dw + w / 2.0;
        *yoff = dh + h / 2.0;
    } else {
        theta = -M_PI - atan2(h, w);
        dw = hypot * cos(theta + angle);
        dh = hypot * sin(theta + angle);
        *xoff = dw + w / 2.0;
        *yoff = dh - h / 2.0;
    }
}

void rmath_ge_raster_resize_for_rotation(unsigned int *sraster,
                                         int w, int h,
                                         unsigned int *newRaster,
                                         int wnew, int hnew,
                                         const pGEcontext gc) {
    int xoff = (wnew - w) / 2;
    int yoff = (hnew - h) / 2;
    for (int i = 0; i < hnew; i++) {
        for (int j = 0; j < wnew; j++) {
            newRaster[i * wnew + j] = gc->fill;
        }
    }
    for (int i = 0; i < h; i++) {
        for (int j = 0; j < w; j++) {
            int inew = i + yoff;
            int jnew = j + xoff;
            newRaster[inew * wnew + jnew] = sraster[i * w + j];
        }
    }
}

void rmath_ge_raster_rotate(unsigned int *sraster, int w, int h, double angle,
                            unsigned int *draster, const pGEcontext gc,
                            int smoothAlpha) {
    angle = -angle;
    int xcen = w / 2;
    int wm2 = w - 2;
    int ycen = h / 2;
    int hm2 = h - 2;
    double sina = 16. * sin(angle);
    double cosa = 16. * cos(angle);

    for (int i = 0; i < h; i++) {
        int ydif = ycen - i;
        unsigned int *dline = draster + i * w;
        for (int j = 0; j < w; j++) {
            int xdif = xcen - j;
            int xpm = (int) (-xdif * cosa - ydif * sina);
            int ypm = (int) (-ydif * cosa + xdif * sina);
            int xp = xcen + (xpm >> 4);
            int yp = ycen + (ypm >> 4);
            int xf = xpm & 0x0f;
            int yf = ypm & 0x0f;
            if (xp < 0 || yp < 0 || xp > wm2 || yp > hm2) {
                *(dline + j) = gc->fill;
                continue;
            }
            unsigned int *sline = sraster + yp * w;
            unsigned int word00 = *(sline + xp);
            unsigned int word10 = *(sline + xp + 1);
            unsigned int word01 = *(sline + w + xp);
            unsigned int word11 = *(sline + w + xp + 1);
            int rval = ((16 - xf) * (16 - yf) * R_RED(word00) +
                        xf * (16 - yf) * R_RED(word10) +
                        (16 - xf) * yf * R_RED(word01) +
                        xf * yf * R_RED(word11) + 128) / 256;
            int gval = ((16 - xf) * (16 - yf) * R_GREEN(word00) +
                        xf * (16 - yf) * R_GREEN(word10) +
                        (16 - xf) * yf * R_GREEN(word01) +
                        xf * yf * R_GREEN(word11) + 128) / 256;
            int bval = ((16 - xf) * (16 - yf) * R_BLUE(word00) +
                        xf * (16 - yf) * R_BLUE(word10) +
                        (16 - xf) * yf * R_BLUE(word01) +
                        xf * yf * R_BLUE(word11) + 128) / 256;
            int aval;
            if (smoothAlpha) {
                aval = ((16 - xf) * (16 - yf) * R_ALPHA(word00) +
                        xf * (16 - yf) * R_ALPHA(word10) +
                        (16 - xf) * yf * R_ALPHA(word01) +
                        xf * yf * R_ALPHA(word11) + 128) / 256;
            } else {
                aval = (int) fmax(fmax(R_ALPHA(word00), R_ALPHA(word10)),
                                  fmax(R_ALPHA(word01), R_ALPHA(word11)));
            }
            *(dline + j) = R_RGBA(rval, gval, bval, aval);
        }
    }
}

void rmath_ge_glyph(int n, int *glyphs, double *x, double *y,
                    SEXP font, double size,
                    int colour, double rot, pGEDevDesc dd) {
    if (dd != NULL && dd->dev != NULL && dd->dev->deviceVersion >= R_GE_glyphs && dd->dev->glyph != NULL) {
        dd->dev->glyph(n, glyphs, x, y, font, size, colour, rot, dd->dev);
    }
}
