#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/engine.c -- R Graphics Engine.
//!
//! Original source: src/main/engine.c (~4,017 lines)
//!
//! This file implements R's graphics engine, providing the interface between
//! graphics devices and graphics systems (like base graphics and grid).
//! All functions are stubs returning safe defaults until the full graphics
//! subsystem is implemented.

use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int, c_uint, c_void};
use std::ptr;

use crate::sexp::accessors::{CHAR, INTEGER, LENGTH, LOGICAL, REAL, STRING_ELT, TYPEOF};
use crate::sexp::constructors::Rf_mkString;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE, NA_INTEGER, NA_LOGICAL};
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of registered graphics systems.
pub const MAX_GRAPHICS_SYSTEMS: c_int = 10;

/// R Graphics Engine version number.
pub const R_GE_version: c_int = 16;

/// GEevent enum values
pub const GE_InitState: c_int = 0;
pub const GE_FinaliseState: c_int = 1;
pub const GE_SaveState: c_int = 2;
pub const GE_RestoreState: c_int = 3;
pub const GE_CopyState: c_int = 4;
pub const GE_CheckPlot: c_int = 5;
pub const GE_SaveSnapshotState: c_int = 6;
pub const GE_RestoreSnapshotState: c_int = 7;

/// GEUnit enum values
pub const GE_DEVICE: c_int = 0;
pub const GE_NDC: c_int = 1;
pub const GE_INCHES: c_int = 2;
pub const GE_CM: c_int = 3;

/// Device version constants
pub const R_GE_deviceClip: c_int = 14;
pub const R_GE_group: c_int = 15;
pub const R_GE_glyphs: c_int = 16;

/// LTY constants
pub const LTY_BLANK: c_int = -1;
pub const LTY_SOLID: c_int = 1;
pub const LTY_DASHED: c_int = 2;
pub const LTY_DOTTED: c_int = 3;
pub const LTY_DOTDASH: c_int = 4;
pub const LTY_LONGDASH: c_int = 5;
pub const LTY_TWODASH: c_int = 6;

/// R_GE_lineend enum values
pub const GE_ROUND_CAP: c_int = 1;
pub const GE_BUTT_CAP: c_int = 2;
pub const GE_SQUARE_CAP: c_int = 3;

/// R_GE_linejoin enum values
pub const GE_ROUND_JOIN: c_int = 1;
pub const GE_MITRE_JOIN: c_int = 2;
pub const GE_BEVEL_JOIN: c_int = 3;

/// R transparent white color value.
pub const R_TRANWHITE: c_uint = 0x00FFFFFF;

// ---------------------------------------------------------------------------
// Number of registered graphics systems (mutable static)
// ---------------------------------------------------------------------------

thread_local! { static numGraphicsSystems: Cell<c_int> = Cell::new(0); }

#[inline]
fn wrap_index(len: c_int, ind: c_int) -> usize {
    ind.rem_euclid(len.max(1)) as usize
}

unsafe fn sexp_string_at(value: SEXP, ind: c_int) -> Option<String> {
    if value.is_null() || TYPEOF(value) != SEXPTYPE::STRSXP.as_c_int() || LENGTH(value) == 0 {
        return None;
    }
    let idx = wrap_index(LENGTH(value), ind) as R_xlen_t;
    let cstr = CStr::from_ptr(CHAR(STRING_ELT(value, idx)));
    Some(cstr.to_string_lossy().to_ascii_lowercase())
}

unsafe fn sexp_int_at(value: SEXP, ind: c_int) -> Option<c_int> {
    if value.is_null() || LENGTH(value) == 0 {
        return None;
    }
    let idx = wrap_index(LENGTH(value), ind);
    match TYPEOF(value) {
        t if t == SEXPTYPE::INTSXP.as_c_int() => {
            let x = *INTEGER(value).add(idx);
            if x == NA_INTEGER { None } else { Some(x) }
        }
        t if t == SEXPTYPE::LGLSXP.as_c_int() => {
            let x = *LOGICAL(value).add(idx);
            if x == NA_LOGICAL { None } else { Some(x) }
        }
        t if t == SEXPTYPE::REALSXP.as_c_int() => {
            let x = *REAL(value).add(idx);
            if x.is_finite() { Some(x as c_int) } else { None }
        }
        _ => None,
    }
}

fn parse_lend_name(name: &str) -> Option<c_int> {
    if name.starts_with("round") {
        Some(GE_ROUND_CAP)
    } else if name.starts_with("butt") {
        Some(GE_BUTT_CAP)
    } else if name.starts_with("square") {
        Some(GE_SQUARE_CAP)
    } else {
        None
    }
}

fn parse_ljoin_name(name: &str) -> Option<c_int> {
    if name.starts_with("round") {
        Some(GE_ROUND_JOIN)
    } else if name.starts_with("mitre") || name.starts_with("miter") {
        Some(GE_MITRE_JOIN)
    } else if name.starts_with("bevel") {
        Some(GE_BEVEL_JOIN)
    } else {
        None
    }
}

fn parse_lty_name(name: &str) -> Option<c_uint> {
    match name {
        "blank" => Some(LTY_BLANK as c_uint),
        "solid" => Some(LTY_SOLID as c_uint),
        "dashed" => Some(LTY_DASHED as c_uint),
        "dotted" => Some(LTY_DOTTED as c_uint),
        "dotdash" => Some(LTY_DOTDASH as c_uint),
        "longdash" => Some(LTY_LONGDASH as c_uint),
        "twodash" => Some(LTY_TWODASH as c_uint),
        _ => None,
    }
}

fn parse_lty_hex_digit(b: u8) -> Option<c_uint> {
    match b {
        b'1'..=b'9' => Some((b - b'0') as c_uint),
        b'a'..=b'f' => Some((b - b'a' + 10) as c_uint),
        b'A'..=b'F' => Some((b - b'A' + 10) as c_uint),
        _ => None,
    }
}

fn parse_lty_hex_pattern(spec: &str) -> Option<c_uint> {
    let bytes = spec.as_bytes();
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    let mut pattern: c_uint = 0;
    for b in bytes {
        let digit = parse_lty_hex_digit(*b)?;
        pattern = (pattern << 4) | digit;
    }
    Some(pattern)
}

// ---------------------------------------------------------------------------
// R_GE_getVersion / R_GE_checkVersionOrDie
// ---------------------------------------------------------------------------

/// Return the current graphics engine version number.
pub unsafe fn R_GE_getVersion() -> c_int {
    R_GE_version
}

/// Check that the given version matches the current engine version; panic on mismatch.
pub unsafe fn R_GE_checkVersionOrDie(version: c_int) {
    if version != R_GE_version {
        // In full implementation, this would call error().
        //  ignore silently.
    }
}

// ---------------------------------------------------------------------------
// GEdestroyDevDesc
// ---------------------------------------------------------------------------

/// Destroy a graphics device description, freeing all associated resources.
pub unsafe fn GEdestroyDevDesc(dd: *mut c_void) {
    let _ = dd;
}

// ---------------------------------------------------------------------------
// GEsystemState
// ---------------------------------------------------------------------------

/// Return the system-specific state for a graphics system.
pub unsafe fn GEsystemState(dd: *mut c_void, index: c_int) -> *mut c_void {
    ptr::null_mut()
}

// ---------------------------------------------------------------------------
// GEregisterWithDevice
// ---------------------------------------------------------------------------

/// Register all current graphics systems with a new device.
pub unsafe fn GEregisterWithDevice(dd: *mut c_void) {
    let _ = dd;
    // No devices in headless mode
}

// ---------------------------------------------------------------------------
// GEregisterSystem
// ---------------------------------------------------------------------------

/// Register a new graphics system with the engine.
pub unsafe fn GEregisterSystem(
    cb: Option<unsafe extern "C" fn(c_int, *mut c_void, SEXP) -> SEXP>,
    systemRegisterIndex: *mut c_int,
) {
    unsafe {
        let _ = cb;
        numGraphicsSystems.with(|v| {
            let current = v.get();
            if current >= MAX_GRAPHICS_SYSTEMS {
                return;
            }
            if !systemRegisterIndex.is_null() {
                *systemRegisterIndex = current;
            }
            // Increment the count of registered graphics systems
            v.set(current + 1);
            // Wire-up with any active devices. In headless mode there are no devices,
            // but call the hook to keep behavior consistent with the original C API.
            GEregisterWithDevice(ptr::null_mut());
        });
    }
}

// ---------------------------------------------------------------------------
// GEunregisterSystem
// ---------------------------------------------------------------------------

/// Unregister a graphics system from the engine.
pub unsafe fn GEunregisterSystem(registerIndex: c_int) {
    let _ = registerIndex;
    numGraphicsSystems.with(|v| {
        let current = v.get();
        if current > 0 {
            v.set(current - 1);
        }
    });
}

// ---------------------------------------------------------------------------
// GEhandleEvent
// ---------------------------------------------------------------------------

/// Handle a graphics event, forwarding to all registered systems.
pub unsafe fn GEhandleEvent(event: c_int, dev: *mut c_void, data: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// Coordinate transformation stubs
// ---------------------------------------------------------------------------

/// Convert X coordinate from device units to the specified unit.
pub unsafe fn fromDeviceX(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    value
}

/// Convert X coordinate from the specified unit to device units.
pub unsafe fn toDeviceX(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    value
}

/// Convert Y coordinate from device units to the specified unit.
pub unsafe fn fromDeviceY(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    value
}

/// Convert Y coordinate from the specified unit to device units.
pub unsafe fn toDeviceY(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    value
}

/// Convert width from device units to the specified unit.
pub unsafe fn fromDeviceWidth(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    value
}

/// Convert width from the specified unit to device units.
pub unsafe fn toDeviceWidth(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    value
}

/// Convert height from device units to the specified unit.
pub unsafe fn fromDeviceHeight(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    value
}

/// Convert height from the specified unit to device units.
pub unsafe fn toDeviceHeight(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    value
}

// ---------------------------------------------------------------------------
// Line end / join parameter functions
// ---------------------------------------------------------------------------

/// Parse a line end specification from an R SEXP value.
pub unsafe fn GE_LENDpar(value: SEXP, ind: c_int) -> c_int {
    if let Some(name) = sexp_string_at(value, ind) {
        if let Some(parsed) = parse_lend_name(&name) {
            return parsed;
        }
    }
    match sexp_int_at(value, ind) {
        Some(1) => GE_ROUND_CAP,
        Some(2) => GE_BUTT_CAP,
        Some(3) => GE_SQUARE_CAP,
        _ => GE_ROUND_CAP,
    }
}

/// Convert a line end code to an R string.
pub unsafe fn GE_LENDget(lend: c_int) -> SEXP {
    match lend {
        GE_BUTT_CAP => Rf_mkString(c"butt".as_ptr()),
        GE_SQUARE_CAP => Rf_mkString(c"square".as_ptr()),
        _ => Rf_mkString(c"round".as_ptr()),
    }
}

/// Parse a line join specification from an R SEXP value.
pub unsafe fn GE_LJOINpar(value: SEXP, ind: c_int) -> c_int {
    if let Some(name) = sexp_string_at(value, ind) {
        if let Some(parsed) = parse_ljoin_name(&name) {
            return parsed;
        }
    }
    match sexp_int_at(value, ind) {
        Some(1) => GE_ROUND_JOIN,
        Some(2) => GE_MITRE_JOIN,
        Some(3) => GE_BEVEL_JOIN,
        _ => GE_ROUND_JOIN,
    }
}

/// Convert a line join code to an R string.
pub unsafe fn GE_LJOINget(ljoin: c_int) -> SEXP {
    match ljoin {
        GE_MITRE_JOIN => Rf_mkString(c"mitre".as_ptr()),
        GE_BEVEL_JOIN => Rf_mkString(c"bevel".as_ptr()),
        _ => Rf_mkString(c"round".as_ptr()),
    }
}

// ---------------------------------------------------------------------------
// GESetClip
// ---------------------------------------------------------------------------

/// Set the clipping rectangle on the current device.
pub unsafe fn GESetClip(x1: c_double, y1: c_double, x2: c_double, y2: c_double, dd: *mut c_void) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GELine
// ---------------------------------------------------------------------------

/// Draw a line segment on the device, with clipping.
pub unsafe fn GELine(
    x1: c_double,
    y1: c_double,
    x2: c_double,
    y2: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GEPolyline
// ---------------------------------------------------------------------------

/// Draw a polyline on the device, with clipping.
pub unsafe fn GEPolyline(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GEPolygon
// ---------------------------------------------------------------------------

/// Draw a filled polygon on the device, with clipping.
pub unsafe fn GEPolygon(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GECircle
// ---------------------------------------------------------------------------

/// Draw a circle on the device, with clipping.
pub unsafe fn GECircle(
    x: c_double,
    y: c_double,
    radius: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GERect
// ---------------------------------------------------------------------------

/// Draw a rectangle on the device, with clipping.
pub unsafe fn GERect(
    x0: c_double,
    y0: c_double,
    x1: c_double,
    y1: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GEPath
// ---------------------------------------------------------------------------

/// Draw a multi-polygon path on the device.
pub unsafe fn GEPath(
    x: *mut c_double,
    y: *mut c_double,
    npoly: c_int,
    nper: *mut c_int,
    winding: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GERaster
// ---------------------------------------------------------------------------

/// Draw a raster image on the device.
pub unsafe fn GERaster(
    raster: *mut c_uint,
    w: c_int,
    h: c_int,
    x: c_double,
    y: c_double,
    width: c_double,
    height: c_double,
    angle: c_double,
    interpolate: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GECap
// ---------------------------------------------------------------------------

/// Capture the current device contents as a raster image.
pub unsafe fn GECap(dd: *mut c_void) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// GEText
// ---------------------------------------------------------------------------

/// Draw text on the device, with clipping and rotation support.
pub unsafe fn GEText(
    x: c_double,
    y: c_double,
    str: *const c_char,
    enc: c_int,
    xc: c_double,
    yc: c_double,
    rot: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GEXspline
// ---------------------------------------------------------------------------

/// Draw an X-spline (smooth curve through control points) on the device.
pub unsafe fn GEXspline(
    n: c_int,
    x: *mut c_double,
    y: *mut c_double,
    s: *mut c_double,
    open: c_int,
    repEnds: c_int,
    draw: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// GEMode
// ---------------------------------------------------------------------------

/// Set the graphics mode on a device.
pub unsafe fn GEMode(mode: c_int, dd: *mut c_void) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GESymbol
// ---------------------------------------------------------------------------

/// Draw a plotting symbol on the device.
pub unsafe fn GESymbol(
    x: c_double,
    y: c_double,
    pch: c_int,
    size: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GEPretty
// ---------------------------------------------------------------------------

/// Calculate pretty axis tick positions (wrapper around R_pretty).
pub unsafe fn GEPretty(lo: *mut c_double, up: *mut c_double, ndiv: *mut c_int) {
    // Headless: no rendering -- in full implementation, calls R_pretty()
}

// ---------------------------------------------------------------------------
// GEMetricInfo
// ---------------------------------------------------------------------------

/// Get metric information (ascent, descent, width) for a character.
pub unsafe fn GEMetricInfo(
    c: c_int,
    gc: *const c_void,
    ascent: *mut c_double,
    descent: *mut c_double,
    width: *mut c_double,
    dd: *mut c_void,
) {
    unsafe {
        // Headless: return sensible defaults to enable layout in headless mode.
        if !ascent.is_null() {
            *ascent = 0.8;
        }
        if !descent.is_null() {
            *descent = 0.2;
        }
        if !width.is_null() {
            *width = 0.5;
        }
    }
}

// ---------------------------------------------------------------------------
// GEStrWidth
// ---------------------------------------------------------------------------

/// Get the width of a string in device coordinates.
pub unsafe fn GEStrWidth(
    str: *const c_char,
    enc: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) -> c_double {
    0.0
}

// ---------------------------------------------------------------------------
// GEStrHeight
// ---------------------------------------------------------------------------

/// Get the height of a string in device coordinates.
pub unsafe fn GEStrHeight(
    str: *const c_char,
    enc: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) -> c_double {
    0.0
}

// ---------------------------------------------------------------------------
// GEStrMetric
// ---------------------------------------------------------------------------

/// Get metric information for a string (ascent, descent, width).
pub unsafe fn GEStrMetric(
    str: *const c_char,
    enc: c_int,
    gc: *const c_void,
    ascent: *mut c_double,
    descent: *mut c_double,
    width: *mut c_double,
    dd: *mut c_void,
) {
    unsafe {
        // Headless: return zeros
        if !ascent.is_null() {
            *ascent = 0.0;
        }
        if !descent.is_null() {
            *descent = 0.0;
        }
        if !width.is_null() {
            *width = 0.0;
        }
    }
}

// ---------------------------------------------------------------------------
// GENewPage
// ---------------------------------------------------------------------------

/// Start a new page on the device.
pub unsafe fn GENewPage(gc: *const c_void, dd: *mut c_void) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GEdeviceDirty / GEdirtyDevice / GEcleanDevice
// ---------------------------------------------------------------------------

/// Check whether a device has received output from any graphics system.
pub unsafe fn GEdeviceDirty(dd: *mut c_void) -> c_int {
    0 // FALSE
}

/// Mark a device as having received output.
pub unsafe fn GEdirtyDevice(dd: *mut c_void) {
    // Headless: no rendering
}

/// Mark a device as clean (no output recorded).
pub(crate) unsafe fn GEcleanDevice(dd: *mut c_void) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GEcheckState
// ---------------------------------------------------------------------------

/// Check whether all registered graphics systems are in a valid state.
pub unsafe fn GEcheckState(dd: *mut c_void) -> c_int {
    // Headless: if no device is provided, consider state OK (0)
    if dd.is_null() {
        0 // FALSE in R's convention means OK for headless checks
    } else {
        1 // TRUE: there is a device/state to check
    }
}

// ---------------------------------------------------------------------------
// GErecording
// ---------------------------------------------------------------------------

/// Check whether graphics operations should be recorded for replay.
pub unsafe fn GErecording(call: SEXP, dd: *mut c_void) -> c_int {
    0 // FALSE
}

// ---------------------------------------------------------------------------
// GErecordGraphicOperation
// ---------------------------------------------------------------------------

/// Record a graphics operation for display list replay.
pub unsafe fn GErecordGraphicOperation(op: SEXP, args: SEXP, dd: *mut c_void) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GEinitDisplayList
// ---------------------------------------------------------------------------

/// Initialize the display list for a device.
pub unsafe fn GEinitDisplayList(dd: *mut c_void) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GEplayDisplayList
// ---------------------------------------------------------------------------

/// Replay the display list on a device.
pub unsafe fn GEplayDisplayList(dd: *mut c_void) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GEcopyDisplayList
// ---------------------------------------------------------------------------

/// Copy the display list from one device to another.
pub unsafe fn GEcopyDisplayList(fromDevice: c_int) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GEcreateSnapshot
// ---------------------------------------------------------------------------

/// Create a snapshot of the current display, including graphics system state.
pub unsafe fn GEcreateSnapshot(dd: *mut c_void) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// GEplaySnapshot
// ---------------------------------------------------------------------------

/// Recreate a saved display from a snapshot.
pub unsafe fn GEplaySnapshot(snapshot: SEXP, dd: *mut c_void) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// do_getSnapshot / do_playSnapshot
// ---------------------------------------------------------------------------

/// recordPlot() -- R internal entry point.
pub unsafe fn do_getSnapshot(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// replayPlot() -- R internal entry point.
pub unsafe fn do_playSnapshot(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// do_recordGraphics
// ---------------------------------------------------------------------------

/// .Internal(recordGraphics(...)) -- R internal entry point.
pub unsafe fn do_recordGraphics(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// GEonExit
// ---------------------------------------------------------------------------

/// Reset graphics state on error/interrupt.
pub unsafe fn GEonExit() {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GEstring_to_pch
// ---------------------------------------------------------------------------

/// Convert a single-character string SEXP to a pch integer code.
pub unsafe fn GEstring_to_pch(pch: SEXP) -> c_int {
    if pch.is_null() || TYPEOF(pch) != SEXPTYPE::STRSXP.as_c_int() || LENGTH(pch) == 0 {
        return NA_INTEGER;
    }
    let ch = CStr::from_ptr(CHAR(STRING_ELT(pch, 0)))
        .to_string_lossy()
        .chars()
        .next();
    ch.map_or(NA_INTEGER, |c| c as c_int)
}

// ---------------------------------------------------------------------------
// GE_LTYpar / GE_LTYget
// ---------------------------------------------------------------------------

/// Parse a line type specification from an R SEXP value.
pub unsafe fn GE_LTYpar(value: SEXP, ind: c_int) -> c_uint {
    if let Some(name) = sexp_string_at(value, ind) {
        if let Some(named) = parse_lty_name(&name) {
            return named;
        }
        if let Some(custom) = parse_lty_hex_pattern(&name) {
            return custom;
        }
        return LTY_SOLID as c_uint;
    }
    match sexp_int_at(value, ind) {
        Some(0) => LTY_BLANK as c_uint,
        Some(1) => LTY_SOLID as c_uint,
        Some(2) => LTY_DASHED as c_uint,
        Some(3) => LTY_DOTTED as c_uint,
        Some(4) => LTY_DOTDASH as c_uint,
        Some(5) => LTY_LONGDASH as c_uint,
        Some(6) => LTY_TWODASH as c_uint,
        _ => LTY_SOLID as c_uint,
    }
}

/// Convert a line type code to an R string.
pub unsafe fn GE_LTYget(lty: c_uint) -> SEXP {
    let named = match lty as c_int {
        LTY_BLANK => Some("blank"),
        LTY_SOLID => Some("solid"),
        LTY_DASHED => Some("dashed"),
        LTY_DOTTED => Some("dotted"),
        LTY_DOTDASH => Some("dotdash"),
        LTY_LONGDASH => Some("longdash"),
        LTY_TWODASH => Some("twodash"),
        _ => None,
    };
    if let Some(name) = named {
        return Rf_mkString(CString::new(name).expect("lty name contains no NUL").as_ptr());
    }
    let custom = CString::new(format!("{lty:x}")).expect("hex lty contains no NUL");
    Rf_mkString(custom.as_ptr())
}

// ---------------------------------------------------------------------------
// Raster image operations
// ---------------------------------------------------------------------------

/// Scale a raster image using nearest-neighbour interpolation.
pub unsafe fn R_GE_rasterScale(
    sraster: *const c_uint,
    sw: c_int,
    sh: c_int,
    draster: *mut c_uint,
    dw: c_int,
    dh: c_int,
) {
    // Headless: no rendering
}

/// Scale a raster image using bilinear interpolation.
pub unsafe fn R_GE_rasterInterpolate(
    sraster: *const c_uint,
    sw: c_int,
    sh: c_int,
    draster: *mut c_uint,
    dw: c_int,
    dh: c_int,
) {
    // Headless: no rendering
}

/// Calculate the size needed for a rotated raster image.
pub unsafe fn R_GE_rasterRotatedSize(
    w: c_int,
    h: c_int,
    angle: c_double,
    wnew: *mut c_int,
    hnew: *mut c_int,
) {
    unsafe {
        if !wnew.is_null() {
            *wnew = w;
        }
        if !hnew.is_null() {
            *hnew = h;
        }
    }
}

/// Calculate the offset for a rotated raster image.
pub unsafe fn R_GE_rasterRotatedOffset(
    w: c_int,
    h: c_int,
    angle: c_double,
    botleft: c_int,
    xoff: *mut c_double,
    yoff: *mut c_double,
) {
    unsafe {
        if !xoff.is_null() {
            *xoff = 0.0;
        }
        if !yoff.is_null() {
            *yoff = 0.0;
        }
    }
}

/// Copy a raster image into the middle of a larger raster (for rotation).
pub unsafe fn R_GE_rasterResizeForRotation(
    sraster: *const c_uint,
    w: c_int,
    h: c_int,
    newRaster: *mut c_uint,
    wnew: c_int,
    hnew: c_int,
    gc: *const c_void,
) {
    // Headless: no rendering
}

/// Rotate a raster image.
pub unsafe fn R_GE_rasterRotate(
    sraster: *const c_uint,
    w: c_int,
    h: c_int,
    angle: c_double,
    draster: *mut c_uint,
    gc: *const c_void,
    smoothAlpha: c_int,
) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// Path drawing (GEgroup API)
// ---------------------------------------------------------------------------

/// Stroke (outline) a path on the device.
pub unsafe fn GEStroke(path: SEXP, gc: *const c_void, dd: *mut c_void) {
    // Headless: no rendering
}

/// Fill a path on the device.
pub unsafe fn GEFill(path: SEXP, rule: c_int, gc: *const c_void, dd: *mut c_void) {
    // Headless: no rendering
}

/// Fill and stroke a path on the device.
pub unsafe fn GEFillStroke(path: SEXP, rule: c_int, gc: *const c_void, dd: *mut c_void) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// Glyph info API
// ---------------------------------------------------------------------------

/// Get the glyphs component from a glyphInfo SEXP.
pub unsafe fn R_GE_glyphInfoGlyphs(glyphInfo: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Get the fonts component from a glyphInfo SEXP.
pub unsafe fn R_GE_glyphInfoFonts(glyphInfo: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Get the glyph IDs from a glyphs SEXP.
pub unsafe fn R_GE_glyphID(glyphs: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Get the glyph X positions from a glyphs SEXP.
pub unsafe fn R_GE_glyphX(glyphs: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Get the glyph Y positions from a glyphs SEXP.
pub unsafe fn R_GE_glyphY(glyphs: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Get the glyph font indices from a glyphs SEXP.
pub unsafe fn R_GE_glyphFont(glyphs: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Get the glyph sizes from a glyphs SEXP.
pub unsafe fn R_GE_glyphSize(glyphs: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Get the glyph colours from a glyphs SEXP.
pub unsafe fn R_GE_glyphColour(glyphs: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Get the glyph rotations from a glyphs SEXP.
pub unsafe fn R_GE_glyphRotation(glyphs: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Check whether a glyphs SEXP has rotation information.
pub unsafe fn R_GE_hasGlyphRotation(glyphs: SEXP) -> c_int {
    0 // FALSE
}

// ---------------------------------------------------------------------------
// Glyph font info API
// ---------------------------------------------------------------------------

/// Get the font file path from a glyphFont SEXP.
pub unsafe fn R_GE_glyphFontFile(glyphFont: SEXP) -> *const c_char {
    ptr::null()
}

/// Get the font index from a glyphFont SEXP.
pub unsafe fn R_GE_glyphFontIndex(glyphFont: SEXP) -> c_int {
    0
}

/// Get the font family name from a glyphFont SEXP.
pub unsafe fn R_GE_glyphFontFamily(glyphFont: SEXP) -> *const c_char {
    ptr::null()
}

/// Get the font weight from a glyphFont SEXP.
pub unsafe fn R_GE_glyphFontWeight(glyphFont: SEXP) -> c_double {
    0.0
}

/// Get the font style from a glyphFont SEXP.
pub unsafe fn R_GE_glyphFontStyle(glyphFont: SEXP) -> c_int {
    0
}

/// Get the font PostScript name from a glyphFont SEXP.
pub unsafe fn R_GE_glyphFontPSname(glyphFont: SEXP) -> *const c_char {
    ptr::null()
}

/// Get the number of font variation axes from a glyphFont SEXP.
pub unsafe fn R_GE_glyphFontNumVar(glyphFont: SEXP) -> c_int {
    0
}

/// Get the axis name for a font variation axis.
pub unsafe fn R_GE_glyphFontVarAxis(glyphFont: SEXP, index: c_int) -> *const c_char {
    ptr::null()
}

/// Get the axis value for a font variation axis.
pub unsafe fn R_GE_glyphFontVarValue(glyphFont: SEXP, index: c_int) -> c_double {
    0.0
}

/// Get the formatted value for a font variation axis.
pub unsafe fn R_GE_glyphFontVarFormatted(glyphFont: SEXP, index: c_int) -> *const c_char {
    ptr::null()
}

// ---------------------------------------------------------------------------
// GEGlyph
// ---------------------------------------------------------------------------

/// Draw glyph(s) on the device.
pub unsafe fn GEGlyph(
    n: c_int,
    glyphs: *const c_int,
    x: *const c_double,
    y: *const c_double,
    font: SEXP,
    size: c_double,
    colour: c_int,
    rot: c_double,
    dd: *mut c_void,
) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// Rf_eval_with_gd (eval_with_gd in C)
// ---------------------------------------------------------------------------

/// Evaluate an expression within a graphics device context (with device locking).
pub unsafe fn Rf_eval_with_gd(e: SEXP, rho: SEXP, dd: *mut c_void) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// Module-private helper stubs (not #[unsafe(no_mangle)], avoid duplicate symbols)
// ---------------------------------------------------------------------------

/// Internal helper: compute open spline (from xspline.c).
pub(crate) unsafe fn compute_open_spline(
    n: c_int,
    x: *mut c_double,
    y: *mut c_double,
    s: *mut c_double,
    repEnds: c_int,
    precision: c_int,
    dd: *mut c_void,
) {
    // Headless: no rendering
}

/// Internal helper: compute closed spline (from xspline.c).
pub(crate) unsafe fn compute_closed_spline(
    n: c_int,
    x: *mut c_double,
    y: *mut c_double,
    s: *mut c_double,
    precision: c_int,
    dd: *mut c_void,
) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::accessors::STRING_ELT;
    use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector, Rf_mkChar, Rf_mkString};
    use crate::sexp::ffi::{R_xlen_t, SEXPTYPE};

    unsafe fn make_string_vector(values: &[&str]) -> SEXP {
        let v = Rf_allocVector(SEXPTYPE::STRSXP, values.len() as c_int);
        for (i, value) in values.iter().enumerate() {
            let c = std::ffi::CString::new(*value).expect("test string contains no NUL");
            crate::sexp::accessors::SET_STRING_ELT(v, i as R_xlen_t, Rf_mkChar(c.as_ptr()));
        }
        v
    }

    #[test]
    fn test_R_GE_getVersion() {
        unsafe {
            assert_eq!(R_GE_getVersion(), R_GE_version);
        }
    }

    #[test]
    fn test_R_GE_checkVersionOrDie_matching() {
        unsafe {
            // Should not panic when version matches
            R_GE_checkVersionOrDie(R_GE_version);
        }
    }

    #[test]
    fn test_coordinate_transforms_passthrough() {
        unsafe {
            let val = 1.0;
            assert_eq!(fromDeviceX(val, GE_DEVICE, ptr::null_mut()), 1.0);
            assert_eq!(toDeviceX(val, GE_DEVICE, ptr::null_mut()), 1.0);
            assert_eq!(fromDeviceY(val, GE_INCHES, ptr::null_mut()), 1.0);
            assert_eq!(toDeviceY(val, GE_INCHES, ptr::null_mut()), 1.0);
            assert_eq!(fromDeviceWidth(val, GE_CM, ptr::null_mut()), 1.0);
            assert_eq!(toDeviceWidth(val, GE_CM, ptr::null_mut()), 1.0);
            assert_eq!(fromDeviceHeight(val, GE_NDC, ptr::null_mut()), 1.0);
            assert_eq!(toDeviceHeight(val, GE_NDC, ptr::null_mut()), 1.0);
        }
    }

    #[test]
    fn test_GEhandleEvent_returns_nil() {
        unsafe {
            let result = GEhandleEvent(0, ptr::null_mut(), ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_GEMetricInfo_returns_defaults() {
        unsafe {
            let mut a = 1.0;
            let mut d = 1.0;
            let mut w = 1.0;
            GEMetricInfo(77, ptr::null(), &mut a, &mut d, &mut w, ptr::null_mut());
            assert_eq!(a, 0.8);
            assert_eq!(d, 0.2);
            assert_eq!(w, 0.5);
        }
    }

    #[test]
    fn test_GEStrWidth_returns_zero() {
        unsafe {
            assert_eq!(
                GEStrWidth(ptr::null(), 0, ptr::null(), ptr::null_mut()),
                0.0
            );
        }
    }

    #[test]
    fn test_GEStrHeight_returns_zero() {
        unsafe {
            assert_eq!(
                GEStrHeight(ptr::null(), 0, ptr::null(), ptr::null_mut()),
                0.0
            );
        }
    }

    #[test]
    fn test_GEStrMetric_returns_zeros() {
        unsafe {
            let mut a = 1.0;
            let mut d = 1.0;
            let mut w = 1.0;
            GEStrMetric(
                ptr::null(),
                0,
                ptr::null(),
                &mut a,
                &mut d,
                &mut w,
                ptr::null_mut(),
            );
            assert_eq!(a, 0.0);
            assert_eq!(d, 0.0);
            assert_eq!(w, 0.0);
        }
    }

    #[test]
    fn test_GEdeviceDirty_returns_false() {
        unsafe {
            assert_eq!(GEdeviceDirty(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_GEcheckState_returns_false_for_null() {
        unsafe {
            // null device = headless, returns FALSE (0) meaning no device state to check
            assert_eq!(GEcheckState(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_GErecording_returns_false() {
        unsafe {
            assert_eq!(GErecording(ptr::null_mut(), ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_GEstring_to_pch_returns_na() {
        unsafe {
            assert_eq!(GEstring_to_pch(ptr::null_mut()), c_int::MIN);
        }
    }

    #[test]
    fn test_GEstring_to_pch_reads_first_character() {
        unsafe {
            let pch = Rf_mkString(c"A".as_ptr());
            assert_eq!(GEstring_to_pch(pch), 'A' as c_int);
        }
    }

    #[test]
    fn test_GE_LTYpar_parses_named_and_hex_styles() {
        unsafe {
            let named = make_string_vector(&["dotted"]);
            let custom = make_string_vector(&["3313"]);
            assert_eq!(GE_LTYpar(named, 0), LTY_DOTTED as c_uint);
            assert_eq!(GE_LTYpar(custom, 0), 0x3313);
        }
    }

    #[test]
    fn test_GE_LTYpar_numeric_mapping() {
        unsafe {
            let numeric = Rf_ScalarInteger(2);
            assert_eq!(GE_LTYpar(numeric, 0), LTY_DASHED as c_uint);
        }
    }

    #[test]
    fn test_GE_LTYget_round_trips_named_values() {
        unsafe {
            let out = GE_LTYget(LTY_LONGDASH as c_uint);
            let name = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(out, 0)))
                .to_str()
                .expect("valid UTF-8");
            assert_eq!(name, "longdash");
        }
    }

    #[test]
    fn test_GECap_returns_nil() {
        unsafe {
            let result = GECap(ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_GEXspline_returns_nil() {
        unsafe {
            let result = GEXspline(
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                1,
                0,
                1,
                ptr::null(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_GEdos_return_nil() {
        unsafe {
            assert_eq!(
                do_getSnapshot(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut()
                ),
                R_NilValue()
            );
            assert_eq!(
                do_playSnapshot(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut()
                ),
                R_NilValue()
            );
            assert_eq!(
                do_recordGraphics(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut()
                ),
                R_NilValue()
            );
        }
    }

    #[test]
    fn test_GEGlyphInfo_stubs_return_nil() {
        unsafe {
            assert_eq!(R_GE_glyphInfoGlyphs(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphInfoFonts(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphID(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphX(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphY(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphFont(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphSize(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphColour(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphRotation(ptr::null_mut()), R_NilValue());
        }
    }

    #[test]
    fn test_GEGlyphFontInfo_stubs() {
        unsafe {
            assert_eq!(R_GE_hasGlyphRotation(ptr::null_mut()), 0);
            assert_eq!(R_GE_glyphFontFile(ptr::null_mut()), ptr::null());
            assert_eq!(R_GE_glyphFontIndex(ptr::null_mut()), 0);
            assert_eq!(R_GE_glyphFontFamily(ptr::null_mut()), ptr::null());
            assert_eq!(R_GE_glyphFontWeight(ptr::null_mut()), 0.0);
            assert_eq!(R_GE_glyphFontStyle(ptr::null_mut()), 0);
            assert_eq!(R_GE_glyphFontPSname(ptr::null_mut()), ptr::null());
            assert_eq!(R_GE_glyphFontNumVar(ptr::null_mut()), 0);
            assert_eq!(R_GE_glyphFontVarAxis(ptr::null_mut(), 0), ptr::null());
            assert_eq!(R_GE_glyphFontVarValue(ptr::null_mut(), 0), 0.0);
            assert_eq!(R_GE_glyphFontVarFormatted(ptr::null_mut(), 0), ptr::null());
        }
    }

    #[test]
    fn test_R_GE_rasterRotatedSize() {
        unsafe {
            let mut wnew: c_int = 0;
            let mut hnew: c_int = 0;
            R_GE_rasterRotatedSize(100, 200, 0.5, &mut wnew, &mut hnew);
            assert_eq!(wnew, 100);
            assert_eq!(hnew, 200);
        }
    }

    #[test]
    fn test_R_GE_rasterRotatedOffset() {
        unsafe {
            let mut xoff = 1.0;
            let mut yoff = 1.0;
            R_GE_rasterRotatedOffset(100, 200, 0.5, 1, &mut xoff, &mut yoff);
            assert_eq!(xoff, 0.0);
            assert_eq!(yoff, 0.0);
        }
    }

    #[test]
    fn test_Rf_eval_with_gd_returns_nil() {
        unsafe {
            let result = Rf_eval_with_gd(ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_LEND_LJOIN_parse_and_get() {
        unsafe {
            let lend = make_string_vector(&["square"]);
            let ljoin = make_string_vector(&["bevel"]);
            assert_eq!(GE_LENDpar(lend, 0), GE_SQUARE_CAP);
            assert_eq!(GE_LJOINpar(ljoin, 0), GE_BEVEL_JOIN);

            let lend_name = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(GE_LENDget(GE_BUTT_CAP), 0)))
                .to_str()
                .expect("valid UTF-8");
            let ljoin_name =
                std::ffi::CStr::from_ptr(CHAR(STRING_ELT(GE_LJOINget(GE_MITRE_JOIN), 0)))
                    .to_str()
                    .expect("valid UTF-8");
            assert_eq!(lend_name, "butt");
            assert_eq!(ljoin_name, "mitre");
        }
    }
}
