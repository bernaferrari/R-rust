use super::*;
use crate::library::grid::{
    alloc_vector, checked, duplicate, get_attrib, inherits, integer_elt, logical_elt, protect,
    real_elt,
};

pub fn L_raster(
    raster: SEXP,
    x: SEXP,
    y: SEXP,
    w: SEXP,
    h: SEXP,
    hjust: SEXP,
    vjust: SEXP,
    interpolate: SEXP,
) -> SEXP {
    {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, "transparent");

        let (vpWidthCM, vpHeightCM, transform, rotationAngle) = getViewportTransform(currentvp, dd);
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gp_is_scalar = [-1i32; 15];
        let gc_buf = R_GE_gcontext::zeroed();
        let gc_cache_buf = R_GE_gcontext::zeroed();
        let gc = Some(&gc_buf);
        let gc_cache = Some(&gc_cache_buf);
        initGContext(currentgp, gc, dd, &mut gp_is_scalar, gc_cache);

        let n = LENGTH(raster);
        if n <= 0 {
            Rf_error_cstr(c"Empty raster");
        }

        let image: Vec<c_uint> =
            if inherits(raster, c"nativeRaster") && TYPEOF(raster) == SEXPTYPE::INTSXP {
                (0..n)
                    .map(|i| integer_elt(raster, i as R_xlen_t) as c_uint)
                    .collect()
            } else {
                (0..n)
                    .map(|i| {
                        // SAFETY: `raster` is a live object; `RGBpar3` is the engine
                        // colour parser bridge over its element data.
                        unsafe { RGBpar3(raster as *mut c_void, i, R_TRANWHITE as c_uint) }
                    })
                    .collect()
            };

        let dim = get_attrib(raster, R_DimSymbol());
        if TYPEOF(dim) != SEXPTYPE::INTSXP || LENGTH(dim) < 2 {
            Rf_error_cstr(c"invalid raster dimensions");
        }

        let mut maxn = unitLength(x);
        maxn = maxn.max(unitLength(y));
        maxn = maxn.max(unitLength(w));
        maxn = maxn.max(unitLength(h));
        let hjust_len = LENGTH(hjust).max(1) as usize;
        let vjust_len = LENGTH(vjust).max(1) as usize;
        let interp_len = LENGTH(interpolate).max(1) as usize;

        GEMode(1, dd);
        for i in 0..maxn as usize {
            updateGContext(currentgp, i as c_int, gc, dd, &mut gp_is_scalar, gc_cache);

            let mut xx: c_double;
            let mut yy: c_double;
            (xx, yy) = transformLocn(
                x, y, i as c_int, vpc, gc, vpWidthCM, vpHeightCM, dd, &transform,
            );
            let mut ww = transformWidthtoINCHES(w, i as c_int, vpc, gc, vpWidthCM, vpHeightCM, dd);
            let mut hh = transformHeighttoINCHES(h, i as c_int, vpc, gc, vpWidthCM, vpHeightCM, dd);
            let hjust_i = if TYPEOF(hjust) == SEXPTYPE::REALSXP && LENGTH(hjust) > 0 {
                real_elt(hjust, (i % hjust_len) as R_xlen_t)
            } else {
                0.0
            };
            let vjust_i = if TYPEOF(vjust) == SEXPTYPE::REALSXP && LENGTH(vjust) > 0 {
                real_elt(vjust, (i % vjust_len) as R_xlen_t)
            } else {
                0.0
            };
            let interp_i = if TYPEOF(interpolate) == SEXPTYPE::LGLSXP && LENGTH(interpolate) > 0 {
                logical_elt(interpolate, (i % interp_len) as R_xlen_t)
            } else {
                0
            };

            if rotationAngle == 0.0 {
                xx = justifyX(xx, ww, hjust_i);
                yy = justifyY(yy, hh, vjust_i);

                xx = toDeviceX(xx, GE_INCHES, dd);
                yy = toDeviceY(yy, GE_INCHES, dd);
                ww = toDeviceWidth(ww, GE_INCHES, dd);
                hh = toDeviceHeight(hh, GE_INCHES, dd);
                if xx.is_finite() && yy.is_finite() && ww.is_finite() && hh.is_finite() {
                    GERaster(
                        &image,
                        integer_elt(dim, 1),
                        integer_elt(dim, 0),
                        xx,
                        yy,
                        ww,
                        hh,
                        rotationAngle,
                        interp_i,
                        gc,
                        dd,
                    );
                }
            } else {
                let (xadj, yadj) = justification(ww, hh, hjust_i, vjust_i);
                let xadjInches = unit(xadj, L_INCHES);
                let _xadj_guard = protect(xadjInches);
                let yadjInches = unit(yadj, L_INCHES);
                let _yadj_guard = protect(yadjInches);
                let (dw, dh) = transformDimn(
                    xadjInches,
                    yadjInches,
                    0,
                    vpc,
                    gc,
                    vpWidthCM,
                    vpHeightCM,
                    dd,
                    rotationAngle,
                );
                let mut xbl = xx + dw;
                let mut ybl = yy + dh;

                xbl = toDeviceX(xbl, GE_INCHES, dd);
                ybl = toDeviceY(ybl, GE_INCHES, dd);
                ww = toDeviceWidth(ww, GE_INCHES, dd);
                hh = toDeviceHeight(hh, GE_INCHES, dd);
                if xbl.is_finite() && ybl.is_finite() && ww.is_finite() && hh.is_finite() {
                    GERaster(
                        &image,
                        integer_elt(dim, 1),
                        integer_elt(dim, 0),
                        xbl,
                        ybl,
                        ww,
                        hh,
                        rotationAngle,
                        interp_i,
                        gc,
                        dd,
                    );
                }
            }
        }
        GEMode(0, dd);
        R_NilValue()
    }
}

/* ==============================
 * L_cap
 * ============================== */

pub fn L_cap() -> SEXP {
    {
        let dd = getDevice();
        let raster = GECap(dd);
        let _raster_guard = protect(raster);
        if isNull(raster) {
            R_NilValue()
        } else {
            let image = alloc_vector(SEXPTYPE::STRSXP, LENGTH(raster));
            let _image_guard = protect(image);
            image
        }
    }
}

/* ==============================
 * gridText (internal)
 * ============================== */
