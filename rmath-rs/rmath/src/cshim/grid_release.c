#include <Rinternals.h>
#include <R_ext/GraphicsEngine.h>

void rmath_grid_release_pattern(pGEDevDesc dd, SEXP ref) {
    if (dd == NULL || dd->dev == NULL || dd->dev->releasePattern == NULL) {
        return;
    }
    dd->dev->releasePattern(ref, dd->dev);
}

void rmath_grid_release_clip_path(pGEDevDesc dd, SEXP ref) {
    if (dd == NULL || dd->dev == NULL || dd->dev->releaseClipPath == NULL) {
        return;
    }
    dd->dev->releaseClipPath(ref, dd->dev);
}

void rmath_grid_release_mask(pGEDevDesc dd, SEXP ref) {
    if (dd == NULL || dd->dev == NULL || dd->dev->releaseMask == NULL) {
        return;
    }
    dd->dev->releaseMask(ref, dd->dev);
}

void rmath_grid_release_group(pGEDevDesc dd, SEXP ref) {
    if (dd == NULL || dd->dev == NULL || dd->dev->releaseGroup == NULL) {
        return;
    }
    dd->dev->releaseGroup(ref, dd->dev);
}

void rmath_grid_release_definitions(pGEDevDesc dd, int clear_groups) {
    if (dd == NULL || dd->dev == NULL) {
        return;
    }

    rmath_grid_release_pattern(dd, R_NilValue);
    rmath_grid_release_clip_path(dd, R_NilValue);
    rmath_grid_release_mask(dd, R_NilValue);

    if (clear_groups && dd->dev->deviceVersion > R_GE_group) {
        rmath_grid_release_group(dd, R_NilValue);
    }
}
