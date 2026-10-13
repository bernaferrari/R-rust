use super::*;

pub unsafe fn L_raster(
    raster: SEXP,
    x: SEXP,
    y: SEXP,
    w: SEXP,
    h: SEXP,
    hjust: SEXP,
    vjust: SEXP,
    interpolate: SEXP,
) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, c"transparent".as_ptr());

        let mut vpWidthCM: c_double = 0.0;
        let mut vpHeightCM: c_double = 0.0;
        let mut rotationAngle: c_double = 0.0;
        let mut transform: LTransform = [[0.0; 3]; 3];
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);

        let n = LENGTH(raster);
        if n <= 0 {
            Rf_error(c"Empty raster".as_ptr());
        }

        let mut image_owned: Vec<c_uint> = Vec::new();
        let image: *mut c_uint = if Rf_inherits(raster, c"nativeRaster".as_ptr()) != 0
            && TYPEOF(raster) == SEXPTYPE::INTSXP
        {
            INTEGER(raster) as *mut c_uint
        } else {
            image_owned = vec![0; n as usize];
            for i in 0..n as usize {
                image_owned[i] = RGBpar3(raster as *mut c_void, i as c_int, R_TRANWHITE as c_uint);
            }
            image_owned.as_mut_ptr()
        };

        let dim = getAttrib(raster, R_DimSymbol());
        if TYPEOF(dim) != SEXPTYPE::INTSXP || LENGTH(dim) < 2 {
            Rf_error(c"invalid raster dimensions".as_ptr());
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
            updateGContext(
                currentgp,
                i as c_int,
                gc,
                dd,
                gp_is_scalar.as_mut_ptr(),
                gc_cache,
            );

            let mut xx: c_double = 0.0;
            let mut yy: c_double = 0.0;
            transformLocn(
                x,
                y,
                i as c_int,
                vpc,
                gc,
                vpWidthCM,
                vpHeightCM,
                dd,
                &mut transform,
                &mut xx,
                &mut yy,
            );
            let mut ww = transformWidthtoINCHES(w, i as c_int, vpc, gc, vpWidthCM, vpHeightCM, dd);
            let mut hh = transformHeighttoINCHES(h, i as c_int, vpc, gc, vpWidthCM, vpHeightCM, dd);
            let hjust_i = if TYPEOF(hjust) == SEXPTYPE::REALSXP && LENGTH(hjust) > 0 {
                *REAL(hjust).add(i % hjust_len)
            } else {
                0.0
            };
            let vjust_i = if TYPEOF(vjust) == SEXPTYPE::REALSXP && LENGTH(vjust) > 0 {
                *REAL(vjust).add(i % vjust_len)
            } else {
                0.0
            };
            let interp_i = if TYPEOF(interpolate) == SEXPTYPE::LGLSXP && LENGTH(interpolate) > 0 {
                *LOGICAL(interpolate).add(i % interp_len)
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
                        image,
                        *INTEGER(dim).add(1),
                        *INTEGER(dim).add(0),
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
                let mut xadj: c_double = 0.0;
                let mut yadj: c_double = 0.0;
                justification(ww, hh, hjust_i, vjust_i, &mut xadj, &mut yadj);
                let xadjInches = unit(xadj, L_INCHES);
                let _xadj_guard = protect(xadjInches);
                let yadjInches = unit(yadj, L_INCHES);
                let _yadj_guard = protect(yadjInches);
                let mut dw: c_double = 0.0;
                let mut dh: c_double = 0.0;
                transformDimn(
                    xadjInches,
                    yadjInches,
                    0,
                    vpc,
                    gc,
                    vpWidthCM,
                    vpHeightCM,
                    dd,
                    rotationAngle,
                    &mut dw,
                    &mut dh,
                );
                let mut xbl = xx + dw;
                let mut ybl = yy + dh;
                xbl = toDeviceX(xbl, GE_INCHES, dd);
                ybl = toDeviceY(ybl, GE_INCHES, dd);
                ww = toDeviceWidth(ww, GE_INCHES, dd);
                hh = toDeviceHeight(hh, GE_INCHES, dd);
                if xbl.is_finite() && ybl.is_finite() && ww.is_finite() && hh.is_finite() {
                    GERaster(
                        image,
                        *INTEGER(dim).add(1),
                        *INTEGER(dim).add(0),
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

pub unsafe fn L_cap() -> SEXP {
    unsafe {
        let dd = getDevice();
        let raster = GECap(dd);
        let _raster_guard = protect(raster);
        if isNull(raster) {
            R_NilValue()
        } else {
            let image = Rf_allocVector(SEXPTYPE::STRSXP, LENGTH(raster));
            let _image_guard = protect(image);
            image
        }
    }
}

/* ==============================
 * gridText (internal)
 * ============================== */
