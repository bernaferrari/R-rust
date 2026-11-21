use super::*;
use crate::library::grid::{
    alloc_vector, char_ptr, duplicate, install, integer_elt, logical_elt, protect, real_elt,
    set_integer_elt, set_logical_elt, set_real_elt, set_vector_elt, string_elt, vector_elt,
};

pub fn L_initGrid(GridEvalEnv: SEXP) -> SEXP {
    set_grid_eval_env(GridEvalEnv);
    // GEregisterSystem(gridCallback, &mut gridRegisterIndex);
    R_NilValue()
}

/* ==============================
 * L_killGrid
 * ============================== */

pub fn L_killGrid() -> SEXP {
    // GEunregisterSystem(gridRegisterIndex);
    R_NilValue()
}

/* ==============================
 * dirtyGridDevice
 * ============================== */

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn dirtyGridDevice(dd: pGEDevDesc) {
    {
        // SAFETY: forwarded raw pointers satisfy `GEdeviceDirty`'s contract per this function's own `# Safety` requirements.
        unsafe { GEdeviceDirty(dd) };
    }
}

/* ==============================
 * L_gridDirty
 * ============================== */

pub fn L_gridDirty() -> SEXP {
    {
        let dd = unsafe { getDevice() };
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        unsafe { dirtyGridDevice(dd) };
        R_NilValue()
    }
}

/* ==============================
 * getViewportContext
 * ============================== */

pub(crate) fn getViewportContext(vp: SEXP, vpc: &mut LViewportContext) {
    fillViewportContextFromViewport(vp, vpc);
}

/* ==============================
 * L_currentViewport
 * ============================== */

pub fn L_currentViewport() -> SEXP {
    {
        let dd = unsafe { getDevice() };
        gridStateElement(dd, GSS_VP)
    }
}

/* ==============================
 * doSetViewport
 * ============================== */

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn doSetViewport(vp: SEXP, topLevelVP: c_int, pushing: c_int, dd: pGEDevDesc) -> SEXP {
    {
        let mut xx1: c_double;
        let mut yy1: c_double;
        let mut xx2: c_double;
        let mut yy2: c_double;

        let (devWidthCM, devHeightCM) = unsafe { getDeviceSize(dd) };

        if topLevelVP == 0 && pushing != 0 {
            let parent = gridStateElement(dd, GSS_VP);
            set_vector_elt(vp, PVP_PARENT as R_xlen_t, parent);
            // SAFETY: `parent`'s children environment and the interned name
            // symbol are live objects lent by the viewport above.
            unsafe {
                defineVar(
                    installTrChar(string_elt(vector_elt(vp, VP_NAME as R_xlen_t), 0)),
                    vp,
                    vector_elt(parent, PVP_CHILDREN as R_xlen_t),
                );
            }
        }

        // SAFETY: forwarded raw pointers satisfy `calcViewportTransform`'s contract per this function's own `# Safety` requirements.
        unsafe {
            calcViewportTransform(
                vp,
                viewportParent(vp),
                topLevelVP == 0 && !deviceChanged(devWidthCM, devHeightCM, viewportParent(vp)),
                dd,
            )
        };

        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && logical_elt(resolving_path, 0) != 0
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
            let currentClip = alloc_vector(SEXPTYPE::REALSXP, 4);
            let _current_clip_guard = protect(currentClip);
            set_real_elt(
                currentClip,
                (0) as R_xlen_t,
                real_elt(parentClip, (0) as R_xlen_t),
            );
            set_real_elt(
                currentClip,
                (1) as R_xlen_t,
                real_elt(parentClip, (1) as R_xlen_t),
            );
            set_real_elt(
                currentClip,
                (2) as R_xlen_t,
                real_elt(parentClip, (2) as R_xlen_t),
            );
            set_real_elt(
                currentClip,
                (3) as R_xlen_t,
                real_elt(parentClip, (3) as R_xlen_t),
            );
            set_vector_elt(vp, PVP_CLIPRECT as R_xlen_t, currentClip);
        } else {
            if viewportClip(vp) == NA_LOGICAL {
                // SAFETY: forwarded raw pointers satisfy `toDeviceX`'s contract per this function's own `# Safety` requirements.
                xx1 = unsafe { toDeviceX(-0.5 * devWidthCM / 2.54, GE_INCHES, dd) };
                // SAFETY: forwarded raw pointers satisfy `toDeviceY`'s contract per this function's own `# Safety` requirements.
                yy1 = unsafe { toDeviceY(-0.5 * devHeightCM / 2.54, GE_INCHES, dd) };
                // SAFETY: forwarded raw pointers satisfy `toDeviceX`'s contract per this function's own `# Safety` requirements.
                xx2 = unsafe { toDeviceX(1.5 * devWidthCM / 2.54, GE_INCHES, dd) };
                // SAFETY: forwarded raw pointers satisfy `toDeviceY`'s contract per this function's own `# Safety` requirements.
                yy2 = unsafe { toDeviceY(1.5 * devHeightCM / 2.54, GE_INCHES, dd) };
                // SAFETY: forwarded raw pointers satisfy `GESetClip`'s contract per this function's own `# Safety` requirements.
                unsafe { GESetClip(xx1, yy1, xx2, yy2, dd) };
            } else if viewportClip(vp) != 0 {
                let rotationAngle = if TYPEOF(viewportRotation(vp)) == SEXPTYPE::REALSXP
                    && LENGTH(viewportRotation(vp)) > 0
                {
                    real_elt(viewportRotation(vp), 0)
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
                    xx1 = real_elt(parentClip, (0) as R_xlen_t);
                    yy1 = real_elt(parentClip, (1) as R_xlen_t);
                    xx2 = real_elt(parentClip, (2) as R_xlen_t);
                    yy2 = real_elt(parentClip, (3) as R_xlen_t);
                } else {
                    let mut transform: LTransform = [[0.0; 3]; 3];
                    for i in 0..3usize {
                        for j in 0..3usize {
                            transform[i][j] =
                                real_elt(viewportTransform(vp), (i + 3 * j) as R_xlen_t);
                        }
                    }
                    let vpWidthCM = real_elt(viewportWidthCM(vp), 0);
                    let vpHeightCM = real_elt(viewportHeightCM(vp), 0);
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
                    let gc_buf: [u8; 256] = [0; 256];
                    let gc = gc_buf.as_ptr() as pGEcontext;
                    gcontextFromViewport(vp, gc, dd);
                    (xx1, yy1) =
                        // SAFETY: forwarded raw pointers satisfy `transformLocn`'s contract per this function's own `# Safety` requirements.
                        unsafe { transformLocn(x1, y1, 0, vpc, gc, vpWidthCM, vpHeightCM, dd, &transform) };
                    (xx2, yy2) =
                        // SAFETY: forwarded raw pointers satisfy `transformLocn`'s contract per this function's own `# Safety` requirements.
                        unsafe { transformLocn(x2, y2, 0, vpc, gc, vpWidthCM, vpHeightCM, dd, &transform) };
                    // SAFETY: forwarded raw pointers satisfy `toDeviceX`'s contract per this function's own `# Safety` requirements.
                    xx1 = unsafe { toDeviceX(xx1, GE_INCHES, dd) };
                    // SAFETY: forwarded raw pointers satisfy `toDeviceY`'s contract per this function's own `# Safety` requirements.
                    yy1 = unsafe { toDeviceY(yy1, GE_INCHES, dd) };
                    // SAFETY: forwarded raw pointers satisfy `toDeviceX`'s contract per this function's own `# Safety` requirements.
                    xx2 = unsafe { toDeviceX(xx2, GE_INCHES, dd) };
                    // SAFETY: forwarded raw pointers satisfy `toDeviceY`'s contract per this function's own `# Safety` requirements.
                    yy2 = unsafe { toDeviceY(yy2, GE_INCHES, dd) };
                    // SAFETY: forwarded raw pointers satisfy `GESetClip`'s contract per this function's own `# Safety` requirements.
                    unsafe { GESetClip(xx1, yy1, xx2, yy2, dd) };
                }
            } else {
                let parentClip = viewportClipRect(viewportParent(vp));
                let _parent_clip_guard = protect(parentClip);
                xx1 = real_elt(parentClip, (0) as R_xlen_t);
                yy1 = real_elt(parentClip, (1) as R_xlen_t);
                xx2 = real_elt(parentClip, (2) as R_xlen_t);
                yy2 = real_elt(parentClip, (3) as R_xlen_t);
                let parentClipPath = vector_elt(viewportParent(vp), PVP_CLIPPATH as R_xlen_t);
                let _parent_clip_path_guard = protect(parentClipPath);
                if isClipPath(parentClipPath) {
                    set_vector_elt(vp, PVP_CLIPPATH as R_xlen_t, parentClipPath);
                }
                if pushing == 0 && !isClipPath(parentClipPath) {
                    // SAFETY: forwarded raw pointers satisfy `GESetClip`'s contract per this function's own `# Safety` requirements.
                    unsafe { GESetClip(xx1, yy1, xx2, yy2, dd) };
                }
            }

            let currentClip = alloc_vector(SEXPTYPE::REALSXP, 4);
            let _current_clip_guard = protect(currentClip);
            set_real_elt(currentClip, (0) as R_xlen_t, xx1);
            set_real_elt(currentClip, (1) as R_xlen_t, yy1);
            set_real_elt(currentClip, (2) as R_xlen_t, xx2);
            set_real_elt(currentClip, (3) as R_xlen_t, yy2);
            set_vector_elt(vp, PVP_CLIPRECT as R_xlen_t, currentClip);
        }

        if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && logical_elt(resolving_path, 0) != 0
        {
            // Masks are ignored when resolving a clipping path.
        } else if isMask(viewportMaskSXP(vp)) {
            // Resolve after doSetViewport() once this viewport is current.
        } else if viewportMask(vp) {
            set_vector_elt(
                vp,
                PVP_MASK as R_xlen_t,
                vector_elt(viewportParent(vp), PVP_MASK as R_xlen_t),
            );
        } else {
            set_vector_elt(vp, PVP_MASK as R_xlen_t, R_NilValue());
            resolveMask(R_NilValue(), dd);
        }

        let widthCM = alloc_vector(SEXPTYPE::REALSXP, 1);
        let _width_guard = protect(widthCM);
        set_real_elt(widthCM, 0, devWidthCM);
        set_vector_elt(vp, PVP_DEVWIDTHCM as R_xlen_t, widthCM);

        let heightCM = alloc_vector(SEXPTYPE::REALSXP, 1);
        let _height_guard = protect(heightCM);
        set_real_elt(heightCM, 0, devHeightCM);
        set_vector_elt(vp, PVP_DEVHEIGHTCM as R_xlen_t, heightCM);

        vp
    }
}

/* ==============================
 * L_setviewport
 * ============================== */

pub fn L_setviewport(invp: SEXP, hasParent: SEXP) -> SEXP {
    {
        let dd = unsafe { getDevice() };
        let vp = duplicate(invp);
        let _vp_guard = protect(vp);

        // SAFETY: `vp` is a live object; `install` returns a live symbol.
        let fcall = lang2(install("pushedvp"), vp);
        let _fcall_guard = protect(fcall);
        let pushedvp = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _pushedvp_guard = protect(pushedvp);
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let pushedvp = unsafe {
            doSetViewport(
                pushedvp,
                if logical_elt(hasParent, 0) != 0 { 0 } else { 1 },
                1,
                dd,
            )
        };

        setGridStateElement(dd, GSS_VP, pushedvp);

        {
            let vpgp = vector_elt(pushedvp, VP_GP as R_xlen_t);
            let _vpgp_guard = protect(vpgp);
            // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
            let fill = unsafe { getListElement(vpgp, c"fill".as_ptr() as *mut c_char) };
            if fill != R_NilValue() {
                resolveGPar(vpgp, 1);
                let pushed_gp = vector_elt(pushedvp, PVP_GPAR as R_xlen_t);
                set_vector_elt(
                    pushed_gp,
                    GP_FILL as R_xlen_t,
                    // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                    unsafe { getListElement(vpgp, c"fill".as_ptr() as *mut c_char) },
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
                    && logical_elt(resolving_path, 0) != 0
                {
                    Rf_warning(
                        c"Clipping paths within a (clipping) path are not honoured".as_ptr(),
                    );
                    set_vector_elt(pushedvp, PVP_CLIPPATH as R_xlen_t, R_NilValue());
                } else {
                    let resolvedclip = resolveClipPath(clip, dd);
                    let _resolvedclip_guard = protect(resolvedclip);
                    set_vector_elt(pushedvp, PVP_CLIPPATH as R_xlen_t, resolvedclip);
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
                    && logical_elt(resolving_path, 0) != 0
                {
                    Rf_warning(c"Masks within a (clipping) path are not honoured".as_ptr());
                    set_vector_elt(pushedvp, PVP_MASK as R_xlen_t, R_NilValue());
                } else {
                    let resolvedmask = resolveMask(mask, dd);
                    let _resolvedmask_guard = protect(resolvedmask);
                    set_vector_elt(pushedvp, PVP_MASK as R_xlen_t, resolvedmask);
                }
            }
        }

        R_NilValue()
    }
}

/* ==============================
 * Viewport search helpers
 * ============================== */

pub(crate) fn noChildren(children: SEXP) -> bool {
    {
        let fcall = lang2(install("no.children"), children);
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        let r = asBool(result) != 0;
        r
    }
}

pub(crate) fn childExists(name: SEXP, children: SEXP) -> bool {
    {
        let fcall = lang3(install("child.exists"), name, children);
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        let r = asBool(result) != 0;
        r
    }
}

pub(crate) fn childList(children: SEXP) -> SEXP {
    {
        let fcall = lang2(install("child.list"), children);
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        result
    }
}

pub(crate) fn pathMatch(path: SEXP, pathsofar: SEXP, strict: SEXP) -> bool {
    {
        let fcall = unsafe { lang4(install("pathMatch"), path, pathsofar, strict) };
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        let r = asBool(result) != 0;
        r
    }
}

pub(crate) fn growPath(pathsofar: SEXP, name: SEXP) -> SEXP {
    {
        if isNull(pathsofar) {
            return name;
        }
        let fcall = lang3(install("growPath"), pathsofar, name);
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        result
    }
}

pub(crate) fn findViewport(name: SEXP, strict: SEXP, vp: SEXP, depth: c_int) -> SEXP {
    {
        let result = alloc_vector(SEXPTYPE::VECSXP, 2);
        let _result_guard = protect(result);
        let zeroDepth = alloc_vector(SEXPTYPE::INTSXP, 1);
        let _zero_depth_guard = protect(zeroDepth);
        set_integer_elt(zeroDepth, 0, 0);
        let curDepth = alloc_vector(SEXPTYPE::INTSXP, 1);
        let _cur_depth_guard = protect(curDepth);
        set_integer_elt(curDepth, 0, depth);

        if noChildren(viewportChildren(vp)) {
            set_vector_elt(result, 0 as R_xlen_t, zeroDepth);
            set_vector_elt(result, 1 as R_xlen_t, R_NilValue());
        } else if childExists(name, viewportChildren(vp)) {
            set_vector_elt(result, 0 as R_xlen_t, curDepth);
            set_vector_elt(result, 1 as R_xlen_t, unsafe {
                findVar(installTrChar(string_elt(name, 0)), viewportChildren(vp))
            });
        } else {
            if logical_elt(strict, 0) != 0 {
                set_vector_elt(result, 0 as R_xlen_t, zeroDepth);
                set_vector_elt(result, 1 as R_xlen_t, R_NilValue());
            } else {
                let found = findInChildren(name, strict, viewportChildren(vp), depth + 1);
                let _found_guard = protect(found);
                set_vector_elt(result, 0 as R_xlen_t, vector_elt(found, 0 as R_xlen_t));
                set_vector_elt(result, 1 as R_xlen_t, vector_elt(found, 1 as R_xlen_t));
            }
        }
        result
    }
}

pub(crate) fn findInChildren(name: SEXP, strict: SEXP, children: SEXP, depth: c_int) -> SEXP {
    {
        let childnames = childList(children);
        let _childnames_guard = protect(childnames);
        let n = LENGTH(childnames);
        let mut count: c_int = 0;
        let mut found = false;
        let mut result = R_NilValue();
        while count < n && !found {
            let child = unsafe {
                findVar(
                    installTrChar(string_elt(childnames, count as R_xlen_t)),
                    children,
                )
            };
            let _child_guard = protect(child);
            if !isNull(child) {
                result = findViewport(name, strict, child, depth);
                found = integer_elt(vector_elt(result, 0 as R_xlen_t), 0) > 0;
            }
            count += 1;
        }
        if !found {
            let temp = alloc_vector(SEXPTYPE::VECSXP, 2);
            let _temp_guard = protect(temp);
            let zeroDepth = alloc_vector(SEXPTYPE::INTSXP, 1);
            let _zero_depth_guard = protect(zeroDepth);
            set_integer_elt(zeroDepth, 0, 0);
            set_vector_elt(temp, 0 as R_xlen_t, zeroDepth);
            set_vector_elt(temp, 1 as R_xlen_t, R_NilValue());
            result = temp;
        }
        result
    }
}

pub(crate) fn findvppathInChildren(
    path: SEXP,
    name: SEXP,
    strict: SEXP,
    pathsofar: SEXP,
    children: SEXP,
    depth: c_int,
) -> SEXP {
    {
        let childnames = childList(children);
        let _childnames_guard = protect(childnames);
        let n = LENGTH(childnames);
        let mut count: c_int = 0;
        let mut found = false;
        let mut result = R_NilValue();
        while count < n && !found {
            let vp = unsafe {
                findVar(
                    installTrChar(string_elt(childnames, count as R_xlen_t)),
                    children,
                )
            };
            let _vp_guard = protect(vp);
            let newpathsofar = growPath(pathsofar, vector_elt(vp, VP_NAME as R_xlen_t));
            let _newpathsofar_guard = protect(newpathsofar);
            result = findvppath(path, name, strict, newpathsofar, vp, depth);
            found = integer_elt(vector_elt(result, 0 as R_xlen_t), 0) > 0;
            count += 1;
        }
        if !found {
            let temp = alloc_vector(SEXPTYPE::VECSXP, 2);
            let _temp_guard = protect(temp);
            let zeroDepth = alloc_vector(SEXPTYPE::INTSXP, 1);
            let _zero_depth_guard = protect(zeroDepth);
            set_integer_elt(zeroDepth, 0, 0);
            set_vector_elt(temp, 0 as R_xlen_t, zeroDepth);
            set_vector_elt(temp, 1 as R_xlen_t, R_NilValue());
            result = temp;
        }
        result
    }
}

pub(crate) fn findvppath(
    path: SEXP,
    name: SEXP,
    strict: SEXP,
    pathsofar: SEXP,
    vp: SEXP,
    depth: c_int,
) -> SEXP {
    {
        let result = alloc_vector(SEXPTYPE::VECSXP, 2);
        let _result_guard = protect(result);
        let zeroDepth = alloc_vector(SEXPTYPE::INTSXP, 1);
        let _zero_depth_guard = protect(zeroDepth);
        set_integer_elt(zeroDepth, 0, 0);
        let curDepth = alloc_vector(SEXPTYPE::INTSXP, 1);
        let _cur_depth_guard = protect(curDepth);
        set_integer_elt(curDepth, 0, depth);

        if noChildren(viewportChildren(vp)) {
            set_vector_elt(result, 0 as R_xlen_t, zeroDepth);
            set_vector_elt(result, 1 as R_xlen_t, R_NilValue());
        } else if childExists(name, viewportChildren(vp)) && pathMatch(path, pathsofar, strict) {
            set_vector_elt(result, 0 as R_xlen_t, curDepth);
            set_vector_elt(result, 1 as R_xlen_t, unsafe {
                findVar(installTrChar(string_elt(name, 0)), viewportChildren(vp))
            });
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
            set_vector_elt(result, 0 as R_xlen_t, vector_elt(found, 0 as R_xlen_t));
            set_vector_elt(result, 1 as R_xlen_t, vector_elt(found, 1 as R_xlen_t));
        }
        result
    }
}

/* ==============================
 * L_downviewport
 * ============================== */

pub fn L_downviewport(name: SEXP, strict: SEXP) -> SEXP {
    {
        let dd = unsafe { getDevice() };
        let gvp = gridStateElement(dd, GSS_VP);
        let found = findViewport(name, strict, gvp, 1);
        let _found_guard = protect(found);
        if integer_elt(vector_elt(found, 0 as R_xlen_t), 0) > 0 {
            // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
            let vp = unsafe { doSetViewport(vector_elt(found, 1 as R_xlen_t), 0, 0, dd) };
            setGridStateElement(dd, GSS_VP, vp);
            {
                let clip = vector_elt(vp, PVP_CLIPPATH as R_xlen_t);
                let _clip_guard = protect(clip);
                if isClipPath(clip) {
                    let resolvedclip = resolveClipPath(clip, dd);
                    let _resolvedclip_guard = protect(resolvedclip);
                    set_vector_elt(vp, PVP_CLIPPATH as R_xlen_t, resolvedclip);
                }
            }
            {
                let mask = vector_elt(vp, PVP_MASK as R_xlen_t);
                let _mask_guard = protect(mask);
                if isMask(mask) {
                    let resolvedmask = resolveMask(mask, dd);
                    let _resolvedmask_guard = protect(resolvedmask);
                    set_vector_elt(vp, PVP_MASK as R_xlen_t, resolvedmask);
                }
            }
            vector_elt(found, 0 as R_xlen_t)
        } else {
            Rf_error1(
                c"Viewport '%s' was not found".as_ptr(),
                char_ptr(string_elt(name, 0)),
            );

            R_NilValue()
        }
    }
}

/* ==============================
 * L_downvppath
 * ============================== */

pub fn L_downvppath(path: SEXP, name: SEXP, strict: SEXP) -> SEXP {
    {
        let dd = unsafe { getDevice() };
        let gvp = gridStateElement(dd, GSS_VP);
        let found = findvppath(path, name, strict, R_NilValue(), gvp, 1);
        let _found_guard = protect(found);
        if integer_elt(vector_elt(found, 0 as R_xlen_t), 0) > 0 {
            // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
            let vp = unsafe { doSetViewport(vector_elt(found, 1 as R_xlen_t), 0, 0, dd) };
            setGridStateElement(dd, GSS_VP, vp);
            {
                let clip = vector_elt(vp, PVP_CLIPPATH as R_xlen_t);
                let _clip_guard = protect(clip);
                if isClipPath(clip) {
                    let resolvedclip = resolveClipPath(clip, dd);
                    let _resolvedclip_guard = protect(resolvedclip);
                    set_vector_elt(vp, PVP_CLIPPATH as R_xlen_t, resolvedclip);
                }
            }
            {
                let mask = vector_elt(vp, PVP_MASK as R_xlen_t);
                let _mask_guard = protect(mask);
                if isMask(mask) {
                    let resolvedmask = resolveMask(mask, dd);
                    let _resolvedmask_guard = protect(resolvedmask);
                    set_vector_elt(vp, PVP_MASK as R_xlen_t, resolvedmask);
                }
            }
            vector_elt(found, 0 as R_xlen_t)
        } else {
            Rf_error1(
                c"Viewport '%s' was not found".as_ptr(),
                char_ptr(string_elt(name, 0)),
            );

            R_NilValue()
        }
    }
}

/* ==============================
 * L_unsetviewport
 * ============================== */

pub fn L_unsetviewport(n: SEXP) -> SEXP {
    {
        let dd = unsafe { getDevice() };
        let mut gvp = gridStateElement(dd, GSS_VP);
        let mut newvp = vector_elt(gvp, PVP_PARENT as R_xlen_t);
        if isNull(newvp) {
            Rf_error(
                c"cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)".as_ptr(),
            );
        }
        for _i in 1..integer_elt(n, 0) {
            gvp = newvp;
            newvp = vector_elt(gvp, PVP_PARENT as R_xlen_t);
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
            let false0 = alloc_vector(SEXPTYPE::LGLSXP, 1);
            let _false_guard = protect(false0);
            set_logical_elt(false0, 0, 0);
            let fcall = unsafe {
                lang4(
                    install("remove"),
                    vector_elt(gvp, VP_NAME as R_xlen_t),
                    vector_elt(newvp, PVP_CHILDREN as R_xlen_t),
                    false0,
                )
            };
            let _fcall_guard = protect(fcall);
            let mut t = fcall;
            t = CDR(CDR(t));
            unsafe { SET_TAG(t, install("envir")) };
            t = CDR(t);
            unsafe { SET_TAG(t, install("inherits")) };
            Rf_eval_with_gd(fcall, grid_eval_env(), dd);
        }

        let (devWidthCM, devHeightCM) = unsafe { getDeviceSize(dd) };
        if deviceChanged(devWidthCM, devHeightCM, newvp) {
            // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
            unsafe { calcViewportTransform(newvp, viewportParent(newvp), true, dd) };
        }
        setGridStateElement(dd, GSS_GPAR, vector_elt(gvp, PVP_PARENTGPAR as R_xlen_t));
        setGridStateElement(dd, GSS_VP, newvp);

        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if !(TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && logical_elt(resolving_path, 0) != 0)
        {
            let parentClip = viewportClipRect(newvp);
            let _parent_clip_guard = protect(parentClip);
            let parentClipPath = vector_elt(newvp, PVP_CLIPPATH as R_xlen_t);
            let _parent_clip_path_guard = protect(parentClipPath);
            if isClipPath(parentClipPath) {
                resolveClipPath(parentClipPath, dd);
            } else {
                let xx1 = real_elt(parentClip, (0) as R_xlen_t);
                let yy1 = real_elt(parentClip, (1) as R_xlen_t);
                let xx2 = real_elt(parentClip, (2) as R_xlen_t);
                let yy2 = real_elt(parentClip, (3) as R_xlen_t);
                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                unsafe { GESetClip(xx1, yy1, xx2, yy2, dd) };
            }
        }
        if !(TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && logical_elt(resolving_path, 0) != 0)
        {
            resolveMask(vector_elt(newvp, PVP_MASK as R_xlen_t), dd);
        }

        set_vector_elt(gvp, PVP_PARENT as R_xlen_t, R_NilValue());
        R_NilValue()
    }
}

/* ==============================
 * L_upviewport
 * ============================== */

pub fn L_upviewport(n: SEXP) -> SEXP {
    {
        let dd = unsafe { getDevice() };
        let mut gvp = gridStateElement(dd, GSS_VP);
        let mut newvp = vector_elt(gvp, PVP_PARENT as R_xlen_t);
        if isNull(newvp) {
            Rf_error(
                c"cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)".as_ptr(),
            );
        }
        for _i in 1..integer_elt(n, 0) {
            gvp = newvp;
            newvp = vector_elt(gvp, PVP_PARENT as R_xlen_t);
            if isNull(newvp) {
                Rf_error(
                    c"cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)"
                        .as_ptr(),
                );
            }
        }

        let (devWidthCM, devHeightCM) = unsafe { getDeviceSize(dd) };
        if deviceChanged(devWidthCM, devHeightCM, newvp) {
            // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
            unsafe { calcViewportTransform(newvp, viewportParent(newvp), true, dd) };
        }
        setGridStateElement(dd, GSS_GPAR, vector_elt(gvp, PVP_PARENTGPAR as R_xlen_t));
        setGridStateElement(dd, GSS_VP, newvp);

        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if !(TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && logical_elt(resolving_path, 0) != 0)
        {
            let parentClip = viewportClipRect(newvp);
            let _parent_clip_guard = protect(parentClip);
            let parentClipPath = vector_elt(newvp, PVP_CLIPPATH as R_xlen_t);
            let _parent_clip_path_guard = protect(parentClipPath);
            if isClipPath(parentClipPath) {
                resolveClipPath(parentClipPath, dd);
            } else {
                let xx1 = real_elt(parentClip, (0) as R_xlen_t);
                let yy1 = real_elt(parentClip, (1) as R_xlen_t);
                let xx2 = real_elt(parentClip, (2) as R_xlen_t);
                let yy2 = real_elt(parentClip, (3) as R_xlen_t);
                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                unsafe { GESetClip(xx1, yy1, xx2, yy2, dd) };
            }
        }
        if !(TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && logical_elt(resolving_path, 0) != 0)
        {
            resolveMask(vector_elt(newvp, PVP_MASK as R_xlen_t), dd);
        }
        R_NilValue()
    }
}

/* ==============================
 * Display list accessors
 * ============================== */
