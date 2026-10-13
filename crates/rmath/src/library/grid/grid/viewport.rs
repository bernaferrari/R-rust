use super::*;

pub unsafe fn L_initGrid(GridEvalEnv: SEXP) -> SEXP {
    unsafe {
        set_grid_eval_env(GridEvalEnv);
        // GEregisterSystem(gridCallback, &mut gridRegisterIndex);
        R_NilValue()
    }
}

/* ==============================
 * L_killGrid
 * ============================== */

pub unsafe fn L_killGrid() -> SEXP {
    unsafe {
        // GEunregisterSystem(gridRegisterIndex);
        R_NilValue()
    }
}

/* ==============================
 * dirtyGridDevice
 * ============================== */

pub unsafe fn dirtyGridDevice(dd: pGEDevDesc) {
    unsafe {
        GEdeviceDirty(dd);
    }
}

/* ==============================
 * L_gridDirty
 * ============================== */

pub unsafe fn L_gridDirty() -> SEXP {
    unsafe {
        let dd = getDevice();
        dirtyGridDevice(dd);
        R_NilValue()
    }
}

/* ==============================
 * getViewportContext
 * ============================== */

pub(crate) unsafe fn getViewportContext(vp: SEXP, vpc: *mut LViewportContext) {
    unsafe {
        fillViewportContextFromViewport(vp, vpc);
    }
}

/* ==============================
 * L_currentViewport
 * ============================== */

pub unsafe fn L_currentViewport() -> SEXP {
    unsafe {
        let dd = getDevice();
        gridStateElement(dd, GSS_VP)
    }
}

/* ==============================
 * doSetViewport
 * ============================== */

pub unsafe fn doSetViewport(vp: SEXP, topLevelVP: c_int, pushing: c_int, dd: pGEDevDesc) -> SEXP {
    unsafe {
        let mut devWidthCM: c_double = 0.0;
        let mut devHeightCM: c_double = 0.0;
        let mut xx1: c_double = 0.0;
        let mut yy1: c_double = 0.0;
        let mut xx2: c_double = 0.0;
        let mut yy2: c_double = 0.0;

        getDeviceSize(dd, &mut devWidthCM, &mut devHeightCM);

        if topLevelVP == 0 && pushing != 0 {
            let parent = gridStateElement(dd, GSS_VP);
            SET_VECTOR_ELT(vp, PVP_PARENT as R_xlen_t, parent);
            defineVar(
                installTrChar(STRING_ELT(VECTOR_ELT(vp, VP_NAME as R_xlen_t), 0)),
                vp,
                VECTOR_ELT(parent, PVP_CHILDREN as R_xlen_t),
            );
        }

        calcViewportTransform(
            vp,
            viewportParent(vp),
            topLevelVP == 0 && !deviceChanged(devWidthCM, devHeightCM, viewportParent(vp)),
            dd,
        );

        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path) != 0
        {
            if !isClipPath(viewportClipSXP(vp))
                && (viewportClip(vp) == NA_LOGICAL || viewportClip(vp) != 0)
            {
                Rf_warning(
                    c"Turning clipping on or off within a (clipping) path is no honoured".as_ptr(),
                );
            }
        } else if isClipPath(viewportClipSXP(vp)) {
            let parentClip = viewportClipRect(viewportParent(vp));
            let _parent_clip_guard = protect(parentClip);
            let currentClip = Rf_allocVector(SEXPTYPE::REALSXP, 4);
            let _current_clip_guard = protect(currentClip);
            *REAL(currentClip).add(0) = *REAL(parentClip).add(0);
            *REAL(currentClip).add(1) = *REAL(parentClip).add(1);
            *REAL(currentClip).add(2) = *REAL(parentClip).add(2);
            *REAL(currentClip).add(3) = *REAL(parentClip).add(3);
            SET_VECTOR_ELT(vp, PVP_CLIPRECT as R_xlen_t, currentClip);
        } else {
            if viewportClip(vp) == NA_LOGICAL {
                xx1 = toDeviceX(-0.5 * devWidthCM / 2.54, GE_INCHES, dd);
                yy1 = toDeviceY(-0.5 * devHeightCM / 2.54, GE_INCHES, dd);
                xx2 = toDeviceX(1.5 * devWidthCM / 2.54, GE_INCHES, dd);
                yy2 = toDeviceY(1.5 * devHeightCM / 2.54, GE_INCHES, dd);
                GESetClip(xx1, yy1, xx2, yy2, dd);
            } else if viewportClip(vp) != 0 {
                let rotationAngle = if TYPEOF(viewportRotation(vp)) == SEXPTYPE::REALSXP
                    && LENGTH(viewportRotation(vp)) > 0
                {
                    *REAL(viewportRotation(vp))
                } else {
                    0.0
                };
                if rotationAngle != 0.0
                    && rotationAngle != 90.0
                    && rotationAngle != 270.0
                    && rotationAngle != 360.0
                {
                    Rf_warning(c"cannot clip to rotated viewport".as_ptr());
                    let parentClip = viewportClipRect(viewportParent(vp));
                    let _parent_clip_guard = protect(parentClip);
                    xx1 = *REAL(parentClip).add(0);
                    yy1 = *REAL(parentClip).add(1);
                    xx2 = *REAL(parentClip).add(2);
                    yy2 = *REAL(parentClip).add(3);
                } else {
                    let mut transform: LTransform = [[0.0; 3]; 3];
                    for i in 0..3usize {
                        for j in 0..3usize {
                            transform[i][j] = *REAL(viewportTransform(vp)).add(i + 3 * j);
                        }
                    }
                    let vpWidthCM = *REAL(viewportWidthCM(vp));
                    let vpHeightCM = *REAL(viewportHeightCM(vp));
                    let x1 = if topLevelVP == 0 {
                        unit(0.0, L_NPC)
                    } else {
                        unit(-0.5, L_NPC)
                    };
                    let _x1_guard = protect(x1);
                    let y1 = if topLevelVP == 0 {
                        unit(0.0, L_NPC)
                    } else {
                        unit(-0.5, L_NPC)
                    };
                    let _y1_guard = protect(y1);
                    let x2 = if topLevelVP == 0 {
                        unit(1.0, L_NPC)
                    } else {
                        unit(1.5, L_NPC)
                    };
                    let _x2_guard = protect(x2);
                    let y2 = if topLevelVP == 0 {
                        unit(1.0, L_NPC)
                    } else {
                        unit(1.5, L_NPC)
                    };
                    let _y2_guard = protect(y2);
                    let mut vpc = LViewportContext::default();
                    getViewportContext(vp, &mut vpc);
                    let mut gc_buf: [u8; 256] = [0; 256];
                    let gc = gc_buf.as_ptr() as pGEcontext;
                    gcontextFromViewport(vp, gc, dd);
                    transformLocn(
                        x1,
                        y1,
                        0,
                        vpc,
                        gc,
                        vpWidthCM,
                        vpHeightCM,
                        dd,
                        &mut transform,
                        &mut xx1,
                        &mut yy1,
                    );
                    transformLocn(
                        x2,
                        y2,
                        0,
                        vpc,
                        gc,
                        vpWidthCM,
                        vpHeightCM,
                        dd,
                        &mut transform,
                        &mut xx2,
                        &mut yy2,
                    );
                    xx1 = toDeviceX(xx1, GE_INCHES, dd);
                    yy1 = toDeviceY(yy1, GE_INCHES, dd);
                    xx2 = toDeviceX(xx2, GE_INCHES, dd);
                    yy2 = toDeviceY(yy2, GE_INCHES, dd);
                    GESetClip(xx1, yy1, xx2, yy2, dd);
                }
            } else {
                let parentClip = viewportClipRect(viewportParent(vp));
                let _parent_clip_guard = protect(parentClip);
                xx1 = *REAL(parentClip).add(0);
                yy1 = *REAL(parentClip).add(1);
                xx2 = *REAL(parentClip).add(2);
                yy2 = *REAL(parentClip).add(3);
                let parentClipPath = VECTOR_ELT(viewportParent(vp), PVP_CLIPPATH as R_xlen_t);
                let _parent_clip_path_guard = protect(parentClipPath);
                if isClipPath(parentClipPath) {
                    SET_VECTOR_ELT(vp, PVP_CLIPPATH as R_xlen_t, parentClipPath);
                }
                if pushing == 0 && !isClipPath(parentClipPath) {
                    GESetClip(xx1, yy1, xx2, yy2, dd);
                }
            }

            let currentClip = Rf_allocVector(SEXPTYPE::REALSXP, 4);
            let _current_clip_guard = protect(currentClip);
            *REAL(currentClip).add(0) = xx1;
            *REAL(currentClip).add(1) = yy1;
            *REAL(currentClip).add(2) = xx2;
            *REAL(currentClip).add(3) = yy2;
            SET_VECTOR_ELT(vp, PVP_CLIPRECT as R_xlen_t, currentClip);
        }

        if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path) != 0
        {
            // Masks are ignored when resolving a clipping path.
        } else if isMask(viewportMaskSXP(vp)) {
            // Resolve after doSetViewport() once this viewport is current.
        } else if viewportMask(vp) {
            SET_VECTOR_ELT(
                vp,
                PVP_MASK as R_xlen_t,
                VECTOR_ELT(viewportParent(vp), PVP_MASK as R_xlen_t),
            );
        } else {
            SET_VECTOR_ELT(vp, PVP_MASK as R_xlen_t, R_NilValue());
            resolveMask(R_NilValue(), dd);
        }

        let widthCM = Rf_allocVector(SEXPTYPE::REALSXP, 1);
        let _width_guard = protect(widthCM);
        *REAL(widthCM) = devWidthCM;
        SET_VECTOR_ELT(vp, PVP_DEVWIDTHCM as R_xlen_t, widthCM);

        let heightCM = Rf_allocVector(SEXPTYPE::REALSXP, 1);
        let _height_guard = protect(heightCM);
        *REAL(heightCM) = devHeightCM;
        SET_VECTOR_ELT(vp, PVP_DEVHEIGHTCM as R_xlen_t, heightCM);

        vp
    }
}

/* ==============================
 * L_setviewport
 * ============================== */

pub unsafe fn L_setviewport(invp: SEXP, hasParent: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let vp = Rf_duplicate(invp);
        let _vp_guard = protect(vp);

        let fcall = lang2(Rf_install(b"pushedvp\0".as_ptr() as *const c_char), vp);
        let _fcall_guard = protect(fcall);
        let pushedvp = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _pushedvp_guard = protect(pushedvp);
        let pushedvp = doSetViewport(
            pushedvp,
            if *LOGICAL(hasParent) != 0 { 0 } else { 1 },
            1,
            dd,
        );

        setGridStateElement(dd, GSS_VP, pushedvp);

        {
            let vpgp = VECTOR_ELT(pushedvp, VP_GP as R_xlen_t);
            let _vpgp_guard = protect(vpgp);
            let fill = getListElement(vpgp, c"fill".as_ptr() as *mut c_char);
            if fill != R_NilValue() {
                resolveGPar(vpgp, 1);
                let pushed_gp = VECTOR_ELT(pushedvp, PVP_GPAR as R_xlen_t);
                SET_VECTOR_ELT(
                    pushed_gp,
                    GP_FILL as R_xlen_t,
                    getListElement(vpgp, c"fill".as_ptr() as *mut c_char),
                );
                setGridStateElement(dd, GSS_GPAR, pushed_gp);
            }
        }

        {
            let clip = viewportClipSXP(pushedvp);
            let _clip_guard = protect(clip);
            if isClipPath(clip) {
                let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
                if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
                    && LENGTH(resolving_path) > 0
                    && *LOGICAL(resolving_path) != 0
                {
                    Rf_warning(
                        c"Clipping paths within a (clipping) path are not honoured".as_ptr(),
                    );
                    SET_VECTOR_ELT(pushedvp, PVP_CLIPPATH as R_xlen_t, R_NilValue());
                } else {
                    let resolvedclip = resolveClipPath(clip, dd);
                    let _resolvedclip_guard = protect(resolvedclip);
                    SET_VECTOR_ELT(pushedvp, PVP_CLIPPATH as R_xlen_t, resolvedclip);
                }
            }
        }

        {
            let mask = viewportMaskSXP(pushedvp);
            let _mask_guard = protect(mask);
            if isMask(mask) {
                let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
                if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
                    && LENGTH(resolving_path) > 0
                    && *LOGICAL(resolving_path) != 0
                {
                    Rf_warning(c"Masks within a (clipping) path are not honoured".as_ptr());
                    SET_VECTOR_ELT(pushedvp, PVP_MASK as R_xlen_t, R_NilValue());
                } else {
                    let resolvedmask = resolveMask(mask, dd);
                    let _resolvedmask_guard = protect(resolvedmask);
                    SET_VECTOR_ELT(pushedvp, PVP_MASK as R_xlen_t, resolvedmask);
                }
            }
        }

        R_NilValue()
    }
}

/* ==============================
 * Viewport search helpers
 * ============================== */

pub(crate) unsafe fn noChildren(children: SEXP) -> bool {
    unsafe {
        let fcall = lang2(
            Rf_install(b"no.children\0".as_ptr() as *const c_char),
            children,
        );
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        let r = asBool(result) != 0;
        r
    }
}

pub(crate) unsafe fn childExists(name: SEXP, children: SEXP) -> bool {
    unsafe {
        let fcall = lang3(
            Rf_install(b"child.exists\0".as_ptr() as *const c_char),
            name,
            children,
        );
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        let r = asBool(result) != 0;
        r
    }
}

pub(crate) unsafe fn childList(children: SEXP) -> SEXP {
    unsafe {
        let fcall = lang2(
            Rf_install(b"child.list\0".as_ptr() as *const c_char),
            children,
        );
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        result
    }
}

pub(crate) unsafe fn pathMatch(path: SEXP, pathsofar: SEXP, strict: SEXP) -> bool {
    unsafe {
        let fcall = lang4(
            Rf_install(b"pathMatch\0".as_ptr() as *const c_char),
            path,
            pathsofar,
            strict,
        );
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        let r = asBool(result) != 0;
        r
    }
}

pub(crate) unsafe fn growPath(pathsofar: SEXP, name: SEXP) -> SEXP {
    unsafe {
        if isNull(pathsofar) {
            return name;
        }
        let fcall = lang3(
            Rf_install(b"growPath\0".as_ptr() as *const c_char),
            pathsofar,
            name,
        );
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        result
    }
}

pub(crate) unsafe fn findViewport(name: SEXP, strict: SEXP, vp: SEXP, depth: c_int) -> SEXP {
    unsafe {
        let result = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _result_guard = protect(result);
        let zeroDepth = Rf_allocVector(SEXPTYPE::INTSXP, 1);
        let _zero_depth_guard = protect(zeroDepth);
        *INTEGER(zeroDepth) = 0;
        let curDepth = Rf_allocVector(SEXPTYPE::INTSXP, 1);
        let _cur_depth_guard = protect(curDepth);
        *INTEGER(curDepth) = depth;

        if noChildren(viewportChildren(vp)) {
            SET_VECTOR_ELT(result, 0 as R_xlen_t, zeroDepth);
            SET_VECTOR_ELT(result, 1 as R_xlen_t, R_NilValue());
        } else if childExists(name, viewportChildren(vp)) {
            SET_VECTOR_ELT(result, 0 as R_xlen_t, curDepth);
            SET_VECTOR_ELT(
                result,
                1 as R_xlen_t,
                findVar(installTrChar(STRING_ELT(name, 0)), viewportChildren(vp)),
            );
        } else {
            if *LOGICAL(strict) != 0 {
                SET_VECTOR_ELT(result, 0 as R_xlen_t, zeroDepth);
                SET_VECTOR_ELT(result, 1 as R_xlen_t, R_NilValue());
            } else {
                let found = findInChildren(name, strict, viewportChildren(vp), depth + 1);
                let _found_guard = protect(found);
                SET_VECTOR_ELT(result, 0 as R_xlen_t, VECTOR_ELT(found, 0 as R_xlen_t));
                SET_VECTOR_ELT(result, 1 as R_xlen_t, VECTOR_ELT(found, 1 as R_xlen_t));
            }
        }
        result
    }
}

pub(crate) unsafe fn findInChildren(
    name: SEXP,
    strict: SEXP,
    children: SEXP,
    depth: c_int,
) -> SEXP {
    unsafe {
        let childnames = childList(children);
        let _childnames_guard = protect(childnames);
        let n = LENGTH(childnames);
        let mut count: c_int = 0;
        let mut found = false;
        let mut result = R_NilValue();
        while count < n && !found {
            let child = findVar(
                installTrChar(STRING_ELT(childnames, count as R_xlen_t)),
                children,
            );
            let _child_guard = protect(child);
            if !isNull(child) {
                result = findViewport(name, strict, child, depth);
                found = *INTEGER(VECTOR_ELT(result, 0 as R_xlen_t)) > 0;
            }
            count += 1;
        }
        if !found {
            let temp = Rf_allocVector(SEXPTYPE::VECSXP, 2);
            let _temp_guard = protect(temp);
            let zeroDepth = Rf_allocVector(SEXPTYPE::INTSXP, 1);
            let _zero_depth_guard = protect(zeroDepth);
            *INTEGER(zeroDepth) = 0;
            SET_VECTOR_ELT(temp, 0 as R_xlen_t, zeroDepth);
            SET_VECTOR_ELT(temp, 1 as R_xlen_t, R_NilValue());
            result = temp;
        }
        result
    }
}

pub(crate) unsafe fn findvppathInChildren(
    path: SEXP,
    name: SEXP,
    strict: SEXP,
    pathsofar: SEXP,
    children: SEXP,
    depth: c_int,
) -> SEXP {
    unsafe {
        let childnames = childList(children);
        let _childnames_guard = protect(childnames);
        let n = LENGTH(childnames);
        let mut count: c_int = 0;
        let mut found = false;
        let mut result = R_NilValue();
        while count < n && !found {
            let vp = findVar(
                installTrChar(STRING_ELT(childnames, count as R_xlen_t)),
                children,
            );
            let _vp_guard = protect(vp);
            let newpathsofar = growPath(pathsofar, VECTOR_ELT(vp, VP_NAME as R_xlen_t));
            let _newpathsofar_guard = protect(newpathsofar);
            result = findvppath(path, name, strict, newpathsofar, vp, depth);
            found = *INTEGER(VECTOR_ELT(result, 0 as R_xlen_t)) > 0;
            count += 1;
        }
        if !found {
            let temp = Rf_allocVector(SEXPTYPE::VECSXP, 2);
            let _temp_guard = protect(temp);
            let zeroDepth = Rf_allocVector(SEXPTYPE::INTSXP, 1);
            let _zero_depth_guard = protect(zeroDepth);
            *INTEGER(zeroDepth) = 0;
            SET_VECTOR_ELT(temp, 0 as R_xlen_t, zeroDepth);
            SET_VECTOR_ELT(temp, 1 as R_xlen_t, R_NilValue());
            result = temp;
        }
        result
    }
}

pub(crate) unsafe fn findvppath(
    path: SEXP,
    name: SEXP,
    strict: SEXP,
    pathsofar: SEXP,
    vp: SEXP,
    depth: c_int,
) -> SEXP {
    unsafe {
        let result = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _result_guard = protect(result);
        let zeroDepth = Rf_allocVector(SEXPTYPE::INTSXP, 1);
        let _zero_depth_guard = protect(zeroDepth);
        *INTEGER(zeroDepth) = 0;
        let curDepth = Rf_allocVector(SEXPTYPE::INTSXP, 1);
        let _cur_depth_guard = protect(curDepth);
        *INTEGER(curDepth) = depth;

        if noChildren(viewportChildren(vp)) {
            SET_VECTOR_ELT(result, 0 as R_xlen_t, zeroDepth);
            SET_VECTOR_ELT(result, 1 as R_xlen_t, R_NilValue());
        } else if childExists(name, viewportChildren(vp)) && pathMatch(path, pathsofar, strict) {
            SET_VECTOR_ELT(result, 0 as R_xlen_t, curDepth);
            SET_VECTOR_ELT(
                result,
                1 as R_xlen_t,
                findVar(installTrChar(STRING_ELT(name, 0)), viewportChildren(vp)),
            );
        } else {
            let found = findvppathInChildren(
                path,
                name,
                strict,
                pathsofar,
                viewportChildren(vp),
                depth + 1,
            );
            let _found_guard = protect(found);
            SET_VECTOR_ELT(result, 0 as R_xlen_t, VECTOR_ELT(found, 0 as R_xlen_t));
            SET_VECTOR_ELT(result, 1 as R_xlen_t, VECTOR_ELT(found, 1 as R_xlen_t));
        }
        result
    }
}

/* ==============================
 * L_downviewport
 * ============================== */

pub unsafe fn L_downviewport(name: SEXP, strict: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let gvp = gridStateElement(dd, GSS_VP);
        let found = findViewport(name, strict, gvp, 1);
        let _found_guard = protect(found);
        if *INTEGER(VECTOR_ELT(found, 0 as R_xlen_t)) > 0 {
            let vp = doSetViewport(VECTOR_ELT(found, 1 as R_xlen_t), 0, 0, dd);
            setGridStateElement(dd, GSS_VP, vp);
            {
                let clip = VECTOR_ELT(vp, PVP_CLIPPATH as R_xlen_t);
                let _clip_guard = protect(clip);
                if isClipPath(clip) {
                    let resolvedclip = resolveClipPath(clip, dd);
                    let _resolvedclip_guard = protect(resolvedclip);
                    SET_VECTOR_ELT(vp, PVP_CLIPPATH as R_xlen_t, resolvedclip);
                }
            }
            {
                let mask = VECTOR_ELT(vp, PVP_MASK as R_xlen_t);
                let _mask_guard = protect(mask);
                if isMask(mask) {
                    let resolvedmask = resolveMask(mask, dd);
                    let _resolvedmask_guard = protect(resolvedmask);
                    SET_VECTOR_ELT(vp, PVP_MASK as R_xlen_t, resolvedmask);
                }
            }
            VECTOR_ELT(found, 0 as R_xlen_t)
        } else {
            Rf_error1(
                c"Viewport '%s' was not found".as_ptr(),
                CHAR(STRING_ELT(name, 0)),
            );
            R_NilValue()
        }
    }
}

/* ==============================
 * L_downvppath
 * ============================== */

pub unsafe fn L_downvppath(path: SEXP, name: SEXP, strict: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let gvp = gridStateElement(dd, GSS_VP);
        let found = findvppath(path, name, strict, R_NilValue(), gvp, 1);
        let _found_guard = protect(found);
        if *INTEGER(VECTOR_ELT(found, 0 as R_xlen_t)) > 0 {
            let vp = doSetViewport(VECTOR_ELT(found, 1 as R_xlen_t), 0, 0, dd);
            setGridStateElement(dd, GSS_VP, vp);
            {
                let clip = VECTOR_ELT(vp, PVP_CLIPPATH as R_xlen_t);
                let _clip_guard = protect(clip);
                if isClipPath(clip) {
                    let resolvedclip = resolveClipPath(clip, dd);
                    let _resolvedclip_guard = protect(resolvedclip);
                    SET_VECTOR_ELT(vp, PVP_CLIPPATH as R_xlen_t, resolvedclip);
                }
            }
            {
                let mask = VECTOR_ELT(vp, PVP_MASK as R_xlen_t);
                let _mask_guard = protect(mask);
                if isMask(mask) {
                    let resolvedmask = resolveMask(mask, dd);
                    let _resolvedmask_guard = protect(resolvedmask);
                    SET_VECTOR_ELT(vp, PVP_MASK as R_xlen_t, resolvedmask);
                }
            }
            VECTOR_ELT(found, 0 as R_xlen_t)
        } else {
            Rf_error1(
                c"Viewport '%s' was not found".as_ptr(),
                CHAR(STRING_ELT(name, 0)),
            );
            R_NilValue()
        }
    }
}

/* ==============================
 * L_unsetviewport
 * ============================== */

pub unsafe fn L_unsetviewport(n: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let mut gvp = gridStateElement(dd, GSS_VP);
        let mut newvp = VECTOR_ELT(gvp, PVP_PARENT as R_xlen_t);
        if isNull(newvp) {
            Rf_error(
                c"cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)".as_ptr(),
            );
        }
        for _i in 1..*INTEGER(n) {
            gvp = newvp;
            newvp = VECTOR_ELT(gvp, PVP_PARENT as R_xlen_t);
            if isNull(newvp) {
                Rf_error(
                    c"cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)"
                        .as_ptr(),
                );
            }
        }

        let _gvp_guard = protect(gvp);
        let _newvp_guard = protect(newvp);
        {
            let false0 = Rf_allocVector(SEXPTYPE::LGLSXP, 1);
            let _false_guard = protect(false0);
            *LOGICAL(false0) = 0;
            let fcall = lang4(
                Rf_install(c"remove".as_ptr()),
                VECTOR_ELT(gvp, VP_NAME as R_xlen_t),
                VECTOR_ELT(newvp, PVP_CHILDREN as R_xlen_t),
                false0,
            );
            let _fcall_guard = protect(fcall);
            let mut t = fcall;
            t = CDR(CDR(t));
            SET_TAG(t, Rf_install(c"envir".as_ptr()));
            t = CDR(t);
            SET_TAG(t, Rf_install(c"inherits".as_ptr()));
            Rf_eval_with_gd(fcall, grid_eval_env(), dd);
        }

        let mut devWidthCM: c_double = 0.0;
        let mut devHeightCM: c_double = 0.0;
        getDeviceSize(dd, &mut devWidthCM, &mut devHeightCM);
        if deviceChanged(devWidthCM, devHeightCM, newvp) {
            calcViewportTransform(newvp, viewportParent(newvp), true, dd);
        }
        setGridStateElement(dd, GSS_GPAR, VECTOR_ELT(gvp, PVP_PARENTGPAR as R_xlen_t));
        setGridStateElement(dd, GSS_VP, newvp);

        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if !(TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path) != 0)
        {
            let parentClip = viewportClipRect(newvp);
            let _parent_clip_guard = protect(parentClip);
            let parentClipPath = VECTOR_ELT(newvp, PVP_CLIPPATH as R_xlen_t);
            let _parent_clip_path_guard = protect(parentClipPath);
            if isClipPath(parentClipPath) {
                resolveClipPath(parentClipPath, dd);
            } else {
                let xx1 = *REAL(parentClip).add(0);
                let yy1 = *REAL(parentClip).add(1);
                let xx2 = *REAL(parentClip).add(2);
                let yy2 = *REAL(parentClip).add(3);
                GESetClip(xx1, yy1, xx2, yy2, dd);
            }
        }
        if !(TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path) != 0)
        {
            resolveMask(VECTOR_ELT(newvp, PVP_MASK as R_xlen_t), dd);
        }

        SET_VECTOR_ELT(gvp, PVP_PARENT as R_xlen_t, R_NilValue());
        R_NilValue()
    }
}

/* ==============================
 * L_upviewport
 * ============================== */

pub unsafe fn L_upviewport(n: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let mut gvp = gridStateElement(dd, GSS_VP);
        let mut newvp = VECTOR_ELT(gvp, PVP_PARENT as R_xlen_t);
        if isNull(newvp) {
            Rf_error(
                c"cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)".as_ptr(),
            );
        }
        for _i in 1..*INTEGER(n) {
            gvp = newvp;
            newvp = VECTOR_ELT(gvp, PVP_PARENT as R_xlen_t);
            if isNull(newvp) {
                Rf_error(
                    c"cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)"
                        .as_ptr(),
                );
            }
        }

        let mut devWidthCM: c_double = 0.0;
        let mut devHeightCM: c_double = 0.0;
        getDeviceSize(dd, &mut devWidthCM, &mut devHeightCM);
        if deviceChanged(devWidthCM, devHeightCM, newvp) {
            calcViewportTransform(newvp, viewportParent(newvp), true, dd);
        }
        setGridStateElement(dd, GSS_GPAR, VECTOR_ELT(gvp, PVP_PARENTGPAR as R_xlen_t));
        setGridStateElement(dd, GSS_VP, newvp);

        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if !(TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path) != 0)
        {
            let parentClip = viewportClipRect(newvp);
            let _parent_clip_guard = protect(parentClip);
            let parentClipPath = VECTOR_ELT(newvp, PVP_CLIPPATH as R_xlen_t);
            let _parent_clip_path_guard = protect(parentClipPath);
            if isClipPath(parentClipPath) {
                resolveClipPath(parentClipPath, dd);
            } else {
                let xx1 = *REAL(parentClip).add(0);
                let yy1 = *REAL(parentClip).add(1);
                let xx2 = *REAL(parentClip).add(2);
                let yy2 = *REAL(parentClip).add(3);
                GESetClip(xx1, yy1, xx2, yy2, dd);
            }
        }
        if !(TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path) != 0)
        {
            resolveMask(VECTOR_ELT(newvp, PVP_MASK as R_xlen_t), dd);
        }
        R_NilValue()
    }
}

/* ==============================
 * Display list accessors
 * ============================== */
