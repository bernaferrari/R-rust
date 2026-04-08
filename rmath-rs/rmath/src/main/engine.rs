//! Port of R's src/main/engine.c -- R Graphics Engine.
//!
//! Original source: src/main/engine.c (~4,017 lines)
//!
//! This file implements R's graphics engine, providing the interface between
//! graphics devices and graphics systems (like base graphics and grid).

use std::cell::Cell;
use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int, c_uint, c_void};
use std::ptr;

use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of registered graphics systems.
pub const MAX_GRAPHICS_SYSTEMS: c_int = 24;

/// R Graphics Engine version number.
pub const R_GE_version: c_int = 17;

/// Device API version constants
pub const R_GE_definitions: c_int = 13;
pub const R_GE_deviceClip: c_int = 14;
pub const R_GE_group: c_int = 15;
pub const R_GE_glyphs: c_int = 16;
pub const R_GE_fontVar: c_int = 17;

/// GEevent enum values
pub const GE_InitState: c_int = 0;
pub const GE_FinaliseState: c_int = 1;
pub const GE_SaveState: c_int = 2;
pub const GE_RestoreState: c_int = 6;
pub const GE_CopyState: c_int = 3;
pub const GE_SaveSnapshotState: c_int = 4;
pub const GE_RestoreSnapshotState: c_int = 5;
pub const GE_CheckPlot: c_int = 7;
pub const GE_ScalePS: c_int = 8;

/// GEUnit enum values
pub const GE_DEVICE: c_int = 0;
pub const GE_NDC: c_int = 1;
pub const GE_INCHES: c_int = 2;
pub const GE_CM: c_int = 3;

/// LTY constants (R internal line type encoding as 4-bit integers in a word)
pub const LTY_BLANK: c_uint = !0u32; // -1 in C, all bits set
pub const LTY_SOLID: c_uint = 0;
pub const LTY_DASHED: c_uint = 4 + (4 << 4);
pub const LTY_DOTTED: c_uint = 1 + (3 << 4);
pub const LTY_DOTDASH: c_uint = 1 + (3 << 4) + (4 << 8) + (3 << 12);
pub const LTY_LONGDASH: c_uint = 7 + (3 << 4);
pub const LTY_TWODASH: c_uint = 2 + (2 << 4) + (6 << 8) + (2 << 12);

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

/// Degrees to radians conversion factor
pub const DEG2RAD: c_double = 0.01745329251994329576;

// ---------------------------------------------------------------------------
// Colour macros (matching R_ext/GraphicsDevice.h)
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn R_RGB(r: u32, g: u32, b: u32) -> c_uint {
    (r) | ((g) << 8) | ((b) << 16) | 0xFF000000
}

#[inline]
pub(crate) fn R_RGBA(r: u32, g: u32, b: u32, a: u32) -> c_uint {
    (r) | ((g) << 8) | ((b) << 16) | ((a) << 24)
}

#[inline]
fn R_RED(col: c_uint) -> c_uint {
    (col) & 255
}
#[inline]
fn R_GREEN(col: c_uint) -> c_uint {
    ((col) >> 8) & 255
}
#[inline]
fn R_BLUE(col: c_uint) -> c_uint {
    ((col) >> 16) & 255
}
#[inline]
fn R_ALPHA(col: c_uint) -> c_uint {
    ((col) >> 24) & 255
}
#[inline]
fn R_TRANSPARENT(col: c_uint) -> bool {
    R_ALPHA(col) == 0
}

/// Check if a value is finite (replaces R_FINITE)
#[inline]
fn r_finite(x: c_double) -> bool {
    x.is_finite()
}

// ---------------------------------------------------------------------------
// Number of registered graphics systems (mutable static)
// ---------------------------------------------------------------------------

thread_local! { static numGraphicsSystems: Cell<c_int> = Cell::new(0); }

// ---------------------------------------------------------------------------
// R_GE_gcontext -- Graphics context structure
// ---------------------------------------------------------------------------

/// R's graphics parameter context, passed between graphics systems,
/// the graphics engine, and graphics devices.
///
/// This matches the C struct R_GE_gcontext from GraphicsEngine.h.
#[repr(C)]
pub struct R_GE_gcontext {
    pub col: c_int,
    pub fill: c_int,
    pub gamma: c_double,
    pub lwd: c_double,
    pub lty: c_int,
    pub lend: c_int,
    pub ljoin: c_int,
    pub lmitre: c_double,
    pub cex: c_double,
    pub ps: c_double,
    pub lineheight: c_double,
    pub fontface: c_int,
    pub fontfamily: [c_char; 201],
    pub patternFill: SEXP,
}

impl Default for R_GE_gcontext {
    fn default() -> Self {
        R_GE_gcontext {
            col: 1,
            fill: -1, // NA_INTEGER equivalent for transparent
            gamma: 1.0,
            lwd: 1.0,
            lty: LTY_SOLID as c_int,
            lend: GE_ROUND_CAP,
            ljoin: GE_ROUND_JOIN,
            lmitre: 10.0,
            cex: 1.0,
            ps: 12.0,
            lineheight: 1.2,
            fontface: 1,
            fontfamily: [0; 201],
            patternFill: ptr::null_mut(),
        }
    }
}

/// Pointer to graphics context
pub type pGEcontext = *const R_GE_gcontext;

// ---------------------------------------------------------------------------
// DevDesc -- Device descriptor function pointer table
// ---------------------------------------------------------------------------

/// Device function pointer table. All fields are Option<> to handle
/// devices that don't implement all features (NULL function pointers in C).
///
/// This matches the C struct _DevDesc from GraphicsDevice.h.
#[repr(C)]
pub struct DevDesc {
    // Device physical characteristics
    pub left: c_double,
    pub right: c_double,
    pub bottom: c_double,
    pub top: c_double,
    pub clipLeft: c_double,
    pub clipRight: c_double,
    pub clipBottom: c_double,
    pub clipTop: c_double,
    pub xCharOffset: c_double,
    pub yCharOffset: c_double,
    pub yLineBias: c_double,
    pub ipr: [c_double; 2],
    pub cra: [c_double; 2],
    pub gamma: c_double,

    // Device capabilities
    pub canClip: c_int, // Rboolean
    pub canChangeGamma: c_int,
    pub canHAdj: c_int,

    // Device initial settings
    pub startps: c_double,
    pub startcol: c_int,
    pub startfill: c_int,
    pub startlty: c_int,
    pub startfont: c_int,
    pub startgamma: c_double,

    // Device-specific info
    pub deviceSpecific: *mut c_void,

    // Display list toggle
    pub displayListOn: c_int,

    // Event handling
    pub canGenMouseDown: c_int,
    pub canGenMouseMove: c_int,
    pub canGenMouseUp: c_int,
    pub canGenKeybd: c_int,
    pub canGenIdle: c_int,
    pub gettingEvent: c_int,

    // Device procedures (function pointers)
    pub activate: Option<unsafe extern "C" fn(*const DevDesc)>,
    pub circle:
        Option<unsafe extern "C" fn(c_double, c_double, c_double, pGEcontext, *mut DevDesc)>,
    pub clip: Option<unsafe extern "C" fn(c_double, c_double, c_double, c_double, *mut DevDesc)>,
    pub close: Option<unsafe extern "C" fn(*mut DevDesc)>,
    pub deactivate: Option<unsafe extern "C" fn(*const DevDesc)>,
    pub locator: Option<unsafe extern "C" fn(*mut c_double, *mut c_double, *mut DevDesc) -> c_int>,
    pub line: Option<
        unsafe extern "C" fn(c_double, c_double, c_double, c_double, pGEcontext, *mut DevDesc),
    >,
    pub metricInfo: Option<
        unsafe extern "C" fn(
            c_int,
            pGEcontext,
            *mut c_double,
            *mut c_double,
            *mut c_double,
            *mut DevDesc,
        ),
    >,
    pub mode: Option<unsafe extern "C" fn(c_int, *mut DevDesc)>,
    pub newPage: Option<unsafe extern "C" fn(pGEcontext, *mut DevDesc)>,
    pub polygon: Option<
        unsafe extern "C" fn(c_int, *const c_double, *const c_double, pGEcontext, *mut DevDesc),
    >,
    pub polyline: Option<
        unsafe extern "C" fn(c_int, *const c_double, *const c_double, pGEcontext, *mut DevDesc),
    >,
    pub rect: Option<
        unsafe extern "C" fn(c_double, c_double, c_double, c_double, pGEcontext, *mut DevDesc),
    >,
    pub path: Option<
        unsafe extern "C" fn(
            *mut c_double,
            *mut c_double,
            c_int,
            *mut c_int,
            c_int,
            pGEcontext,
            *mut DevDesc,
        ),
    >,
    pub raster: Option<
        unsafe extern "C" fn(
            *mut c_uint,
            c_int,
            c_int,
            c_double,
            c_double,
            c_double,
            c_double,
            c_double,
            c_int,
            pGEcontext,
            *mut DevDesc,
        ),
    >,
    pub cap: Option<unsafe extern "C" fn(*mut DevDesc) -> SEXP>,
    pub size: Option<
        unsafe extern "C" fn(
            *mut c_double,
            *mut c_double,
            *mut c_double,
            *mut c_double,
            *mut DevDesc,
        ),
    >,
    pub strWidth: Option<unsafe extern "C" fn(*const c_char, pGEcontext, *mut DevDesc) -> c_double>,
    pub text: Option<
        unsafe extern "C" fn(
            c_double,
            c_double,
            *const c_char,
            c_double,
            c_double,
            pGEcontext,
            *mut DevDesc,
        ),
    >,
    pub onExit: Option<unsafe extern "C" fn(*mut DevDesc)>,
    pub getEvent: Option<unsafe extern "C" fn(SEXP, *const c_char) -> SEXP>,

    // newFrameConfirm
    pub newFrameConfirm: Option<unsafe extern "C" fn(*mut DevDesc) -> c_int>,

    // UTF-8 text support
    pub hasTextUTF8: c_int,
    pub textUTF8: Option<
        unsafe extern "C" fn(
            c_double,
            c_double,
            *const c_char,
            c_double,
            c_double,
            pGEcontext,
            *mut DevDesc,
        ),
    >,
    pub strWidthUTF8:
        Option<unsafe extern "C" fn(*const c_char, pGEcontext, *mut DevDesc) -> c_double>,
    pub wantSymbolUTF8: c_int,

    pub useRotatedTextInContour: c_int,

    // Event environment
    pub eventEnv: SEXP,
    pub eventHelper: Option<unsafe extern "C" fn(*mut DevDesc, c_int)>,
    pub holdflush: Option<unsafe extern "C" fn(*mut DevDesc, c_int) -> c_int>,

    // Device capabilities (0 = NA/unset)
    pub haveTransparency: c_int,
    pub haveTransparentBg: c_int,
    pub haveRaster: c_int,
    pub haveCapture: c_int,
    pub haveLocator: c_int,

    // Since R_GE_definitions (v13)
    pub setPattern: Option<unsafe extern "C" fn(SEXP, *mut DevDesc) -> SEXP>,
    pub releasePattern: Option<unsafe extern "C" fn(SEXP, *mut DevDesc)>,
    pub setClipPath: Option<unsafe extern "C" fn(SEXP, SEXP, *mut DevDesc) -> SEXP>,
    pub releaseClipPath: Option<unsafe extern "C" fn(SEXP, *mut DevDesc)>,
    pub setMask: Option<unsafe extern "C" fn(SEXP, SEXP, *mut DevDesc) -> SEXP>,
    pub releaseMask: Option<unsafe extern "C" fn(SEXP, *mut DevDesc)>,
    pub deviceVersion: c_int,

    // Since R_GE_deviceClip (v14)
    pub deviceClip: c_int,

    // Since R_GE_group (v15)
    pub defineGroup: Option<unsafe extern "C" fn(SEXP, c_int, SEXP, *mut DevDesc) -> SEXP>,
    pub useGroup: Option<unsafe extern "C" fn(SEXP, SEXP, *mut DevDesc)>,
    pub releaseGroup: Option<unsafe extern "C" fn(SEXP, *mut DevDesc)>,
    pub stroke: Option<unsafe extern "C" fn(SEXP, pGEcontext, *mut DevDesc)>,
    pub fill: Option<unsafe extern "C" fn(SEXP, c_int, pGEcontext, *mut DevDesc)>,
    pub fillStroke: Option<unsafe extern "C" fn(SEXP, c_int, pGEcontext, *mut DevDesc)>,
    pub capabilities: Option<unsafe extern "C" fn(SEXP) -> SEXP>,

    // Since R_GE_glyphs (v16)
    pub glyph: Option<
        unsafe extern "C" fn(
            c_int,
            *const c_int,
            *const c_double,
            *const c_double,
            SEXP,
            c_double,
            c_int,
            c_double,
            *mut DevDesc,
        ),
    >,

    // Reserved for future expansion
    pub reserved: [c_char; 64],
}

/// Pointer to device descriptor
pub type pDevDesc = *mut DevDesc;

// ---------------------------------------------------------------------------
// GESystemDesc -- Graphics system registration info
// ---------------------------------------------------------------------------

/// Callback type for graphics system events.
pub type GEcallback = Option<unsafe extern "C" fn(c_int, *mut GEDevDesc, SEXP) -> SEXP>;

/// Per-system registration info.
#[repr(C)]
pub struct GESystemDesc {
    pub systemSpecific: *mut c_void,
    pub callback: GEcallback,
}

// ---------------------------------------------------------------------------
// GEDevDesc -- Graphics engine device descriptor
// ---------------------------------------------------------------------------

/// The full graphics engine device descriptor, which wraps a DevDesc
/// and adds engine-level bookkeeping (display list, dirty flag, etc.)
#[repr(C)]
pub struct GEDevDesc {
    /// The device descriptor (visible to devices)
    pub dev: pDevDesc,
    /// Display list toggle
    pub displayListOn: c_int,
    /// The display list itself
    pub displayList: SEXP,
    /// Pointer to end of display list
    pub DLlastElt: SEXP,
    /// Saved snapshot for device history
    pub savedSnapshot: SEXP,
    /// Has the device received output?
    pub dirty: c_int,
    /// Should graphics calls be recorded?
    pub recordGraphics: c_int,
    /// Device lock flag
    pub lock: c_int,
    /// Per-system state
    pub gesd: [*mut GESystemDesc; MAX_GRAPHICS_SYSTEMS as usize],
    /// Per-device ask setting
    pub ask: c_int,
    /// Is device appending a path?
    pub appending: c_int,
}

/// Pointer to GEDevDesc
pub type pGEDevDesc = *mut GEDevDesc;

// ---------------------------------------------------------------------------
// GEStructID enum (used in Rinternals.h for struct identification)
// ---------------------------------------------------------------------------

pub const GEStructID: c_int = 42;

// ---------------------------------------------------------------------------
// Registered systems array
// ---------------------------------------------------------------------------

thread_local! { static registeredSystems: Cell<[*mut GESystemDesc; MAX_GRAPHICS_SYSTEMS as usize]> = Cell::new([ptr::null_mut(); MAX_GRAPHICS_SYSTEMS as usize]); }

// ---------------------------------------------------------------------------
// R_GE_getVersion / R_GE_checkVersionOrDie
// ---------------------------------------------------------------------------

pub unsafe fn R_GE_getVersion() -> c_int {
    R_GE_version
}

pub unsafe fn R_GE_checkVersionOrDie(version: c_int) {
    if version != R_GE_version {
        // In full implementation, call error(). Silently ignore for now.
    }
}

// ---------------------------------------------------------------------------
// Internal helper: unregisterOne
// ---------------------------------------------------------------------------

unsafe fn unregisterOne(dd: pGEDevDesc, systemNumber: c_int) {
    let idx = systemNumber as usize;
    if idx < MAX_GRAPHICS_SYSTEMS as usize {
        if !(*dd).gesd[idx].is_null() {
            let gesd_ptr = (*dd).gesd[idx];
            if let Some(cb) = (*gesd_ptr).callback {
                let _ = cb(GE_FinaliseState, dd, R_NilValue());
            }
            // Free the GESystemDesc
            let _ = Box::from_raw(gesd_ptr);
            (*dd).gesd[idx] = ptr::null_mut();
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helper: registerOne
// ---------------------------------------------------------------------------

unsafe fn registerOne(dd: pGEDevDesc, systemNumber: c_int, cb: GEcallback) {
    let idx = systemNumber as usize;
    if idx >= MAX_GRAPHICS_SYSTEMS as usize {
        return;
    }
    let gesd = Box::new(GESystemDesc {
        systemSpecific: ptr::null_mut(),
        callback: cb,
    });
    (*dd).gesd[idx] = Box::into_raw(gesd);
    if let Some(cb) = cb {
        let result = cb(GE_InitState, dd, R_NilValue());
        if result == R_NilValue() {
            // Tidy up on failure
            let _ = Box::from_raw((*dd).gesd[idx]);
            (*dd).gesd[idx] = ptr::null_mut();
        }
    }
}

// ---------------------------------------------------------------------------
// GEdestroyDevDesc
// ---------------------------------------------------------------------------

pub unsafe fn GEdestroyDevDesc(dd: *mut c_void) {
    if dd.is_null() {
        return;
    }
    let gdd = dd as pGEDevDesc;
    for i in 0..MAX_GRAPHICS_SYSTEMS {
        unregisterOne(gdd, i);
    }
    if !(*gdd).dev.is_null() {
        let _ = Box::from_raw((*gdd).dev);
        (*gdd).dev = ptr::null_mut();
    }
    let _ = Box::from_raw(gdd);
}

// ---------------------------------------------------------------------------
// GEsystemState
// ---------------------------------------------------------------------------

pub unsafe fn GEsystemState(dd: *mut c_void, index: c_int) -> *mut c_void {
    if dd.is_null() || index < 0 || index >= MAX_GRAPHICS_SYSTEMS {
        return ptr::null_mut();
    }
    let gdd = dd as pGEDevDesc;
    let idx = index as usize;
    if (*gdd).gesd[idx].is_null() {
        return ptr::null_mut();
    }
    (*(*gdd).gesd[idx]).systemSpecific
}

// ---------------------------------------------------------------------------
// GEregisterWithDevice
// ---------------------------------------------------------------------------

pub unsafe fn GEregisterWithDevice(dd: *mut c_void) {
    if dd.is_null() {
        return;
    }
    let gdd = dd as pGEDevDesc;
    for i in 0..MAX_GRAPHICS_SYSTEMS {
        let idx = i as usize;
        let sys = registeredSystems.with(|v| v.get()[idx]);
        if !sys.is_null() {
            let cb = (*sys).callback;
            registerOne(gdd, i, cb);
        }
    }
}

// ---------------------------------------------------------------------------
// GEregisterSystem
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GEregisterSystem(
    cb: Option<unsafe extern "C" fn(c_int, *mut c_void, SEXP) -> SEXP>,
    systemRegisterIndex: *mut c_int,
) {
    if systemRegisterIndex.is_null() {
        return;
    }
    *systemRegisterIndex = 0;
    // Find first NULL slot
    while (*systemRegisterIndex as usize) < MAX_GRAPHICS_SYSTEMS as usize {
        let idx = *systemRegisterIndex as usize;
        if registeredSystems.with(|v| v.get()[idx]).is_null() {
            break;
        }
        *systemRegisterIndex += 1;
    }
    // Store the registration info
    // SAFETY: C API passes *mut c_void but GESystemDesc stores *mut GEDevDesc.
    // They have the same ABI (both are raw pointers), so transmuting the function pointer is safe.
    let cb_typed: GEcallback = std::mem::transmute(cb);
    let gesd = Box::new(GESystemDesc {
        systemSpecific: ptr::null_mut(),
        callback: cb_typed,
    });
    registeredSystems.with(|v| {
        let mut arr = v.get();
        arr[idx] = Box::into_raw(gesd);
        v.set(arr);
    });
    numGraphicsSystems.with(|v| v.set(v.get() + 1));
}

// ---------------------------------------------------------------------------
// GEunregisterSystem
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GEunregisterSystem(registerIndex: c_int) {
    if registerIndex < 0 {
        return;
    }
    if numGraphicsSystems.with(|v| v.get()) == 0 {
        return;
    }
    let idx = registerIndex as usize;
    let sys = registeredSystems.with(|v| v.get()[idx]);
    if idx < MAX_GRAPHICS_SYSTEMS as usize && !sys.is_null() {
        let _ = Box::from_raw(sys);
        registeredSystems.with(|v| {
            let mut arr = v.get();
            arr[idx] = ptr::null_mut();
            v.set(arr);
        });
    }
    numGraphicsSystems.with(|v| v.set(v.get() - 1));
}

// ---------------------------------------------------------------------------
// GEhandleEvent
// ---------------------------------------------------------------------------

pub unsafe fn GEhandleEvent(event: c_int, dev: *mut c_void, data: SEXP) -> SEXP {
    for i in 0..MAX_GRAPHICS_SYSTEMS {
        let idx = i as usize;
        let sys = registeredSystems.with(|v| v.get()[idx]);
        if !sys.is_null() {
            if let Some(cb) = (*sys).callback {
                // We need a pGEDevDesc, but we only have pDevDesc here.
                // In the full implementation, desc2GEDesc maps pDevDesc -> pGEDevDesc.
                // For now, call with null.
                let _ = cb(event, ptr::null_mut(), data);
            }
        }
    }
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Coordinate transformations
// ---------------------------------------------------------------------------

/// Helper: safely access DevDesc fields from a void pointer.
/// Returns None if the pointer is null.
#[inline]
unsafe fn dev_ptr(dd: *mut c_void) -> Option<*mut DevDesc> {
    if dd.is_null() {
        return None;
    }
    let gdd = dd as pGEDevDesc;
    if (*gdd).dev.is_null() {
        return None;
    }
    Some((*gdd).dev)
}

pub unsafe fn fromDeviceX(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    let mut result = value;
    if let Some(d) = dev_ptr(dd) {
        match to {
            GE_DEVICE => {}
            GE_NDC => {
                let dev_left = (*d).left;
                let dev_right = (*d).right;
                if (dev_right - dev_left).abs() > 0.0 {
                    result = (result - dev_left) / (dev_right - dev_left);
                }
            }
            GE_INCHES => {
                let dev_left = (*d).left;
                let dev_right = (*d).right;
                if (dev_right - dev_left).abs() > 0.0 {
                    result = (result - dev_left) / (dev_right - dev_left)
                        * (dev_right - dev_left).abs()
                        * (*d).ipr[0];
                }
            }
            GE_CM => {
                let dev_left = (*d).left;
                let dev_right = (*d).right;
                if (dev_right - dev_left).abs() > 0.0 {
                    result = (result - dev_left) / (dev_right - dev_left)
                        * (dev_right - dev_left).abs()
                        * (*d).ipr[0]
                        * 2.54;
                }
            }
            _ => {}
        }
    }
    result
}

pub unsafe fn toDeviceX(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    let mut result = value;
    if let Some(d) = dev_ptr(dd) {
        // Fall-through switch (C uses fall-through intentionally)
        match from {
            GE_CM => {
                result = result / 2.54;
                // fall through to GE_INCHES
                let dev_left = (*d).left;
                let dev_right = (*d).right;
                if (dev_right - dev_left).abs() > 0.0 {
                    result = (result / (*d).ipr[0]) / (dev_right - dev_left).abs();
                }
                // fall through to GE_NDC
                result = dev_left + result * (dev_right - dev_left);
            }
            GE_INCHES => {
                let dev_left = (*d).left;
                let dev_right = (*d).right;
                if (dev_right - dev_left).abs() > 0.0 {
                    result = (result / (*d).ipr[0]) / (dev_right - dev_left).abs();
                }
                result = dev_left + result * (dev_right - dev_left);
            }
            GE_NDC => {
                let dev_left = (*d).left;
                let dev_right = (*d).right;
                result = dev_left + result * (dev_right - dev_left);
            }
            GE_DEVICE => {}
            _ => {}
        }
    }
    result
}

#[unsafe(no_mangle)]
pub unsafe fn fromDeviceY(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    let mut result = value;
    if let Some(d) = dev_ptr(dd) {
        match to {
            GE_DEVICE => {}
            GE_NDC => {
                let dev_bottom = (*d).bottom;
                let dev_top = (*d).top;
                if (dev_top - dev_bottom).abs() > 0.0 {
                    result = (result - dev_bottom) / (dev_top - dev_bottom);
                }
            }
            GE_INCHES => {
                let dev_bottom = (*d).bottom;
                let dev_top = (*d).top;
                if (dev_top - dev_bottom).abs() > 0.0 {
                    result = (result - dev_bottom) / (dev_top - dev_bottom)
                        * (dev_top - dev_bottom).abs()
                        * (*d).ipr[1];
                }
            }
            GE_CM => {
                let dev_bottom = (*d).bottom;
                let dev_top = (*d).top;
                if (dev_top - dev_bottom).abs() > 0.0 {
                    result = (result - dev_bottom) / (dev_top - dev_bottom)
                        * (dev_top - dev_bottom).abs()
                        * (*d).ipr[1]
                        * 2.54;
                }
            }
            _ => {}
        }
    }
    result
}

pub unsafe fn toDeviceY(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    let mut result = value;
    if let Some(d) = dev_ptr(dd) {
        match from {
            GE_CM => {
                result = result / 2.54;
                let dev_bottom = (*d).bottom;
                let dev_top = (*d).top;
                if (dev_top - dev_bottom).abs() > 0.0 {
                    result = (result / (*d).ipr[1]) / (dev_top - dev_bottom).abs();
                }
                result = dev_bottom + result * (dev_top - dev_bottom);
            }
            GE_INCHES => {
                let dev_bottom = (*d).bottom;
                let dev_top = (*d).top;
                if (dev_top - dev_bottom).abs() > 0.0 {
                    result = (result / (*d).ipr[1]) / (dev_top - dev_bottom).abs();
                }
                result = dev_bottom + result * (dev_top - dev_bottom);
            }
            GE_NDC => {
                let dev_bottom = (*d).bottom;
                let dev_top = (*d).top;
                result = dev_bottom + result * (dev_top - dev_bottom);
            }
            GE_DEVICE => {}
            _ => {}
        }
    }
    result
}

pub unsafe fn fromDeviceWidth(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    let mut result = value;
    if let Some(d) = dev_ptr(dd) {
        match to {
            GE_DEVICE => {}
            GE_NDC => {
                let dev_right = (*d).right;
                let dev_left = (*d).left;
                if (dev_right - dev_left).abs() > 0.0 {
                    result = result / (dev_right - dev_left);
                }
            }
            GE_INCHES => {
                result = result * (*d).ipr[0];
            }
            GE_CM => {
                result = result * (*d).ipr[0] * 2.54;
            }
            _ => {}
        }
    }
    result
}

#[unsafe(no_mangle)]
pub unsafe fn toDeviceWidth(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    let mut result = value;
    if let Some(d) = dev_ptr(dd) {
        match from {
            GE_CM => {
                result = result / 2.54;
                let dev_right = (*d).right;
                let dev_left = (*d).left;
                if (dev_right - dev_left).abs() > 0.0 {
                    result = (result / (*d).ipr[0]) / (dev_right - dev_left).abs();
                }
                result = result * (dev_right - dev_left);
            }
            GE_INCHES => {
                let dev_right = (*d).right;
                let dev_left = (*d).left;
                if (dev_right - dev_left).abs() > 0.0 {
                    result = (result / (*d).ipr[0]) / (dev_right - dev_left).abs();
                }
                result = result * (dev_right - dev_left);
            }
            GE_NDC => {
                let dev_right = (*d).right;
                let dev_left = (*d).left;
                result = result * (dev_right - dev_left);
            }
            GE_DEVICE => {}
            _ => {}
        }
    }
    result
}

pub unsafe fn fromDeviceHeight(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    let mut result = value;
    if let Some(d) = dev_ptr(dd) {
        match to {
            GE_DEVICE => {}
            GE_NDC => {
                let dev_top = (*d).top;
                let dev_bottom = (*d).bottom;
                if (dev_top - dev_bottom).abs() > 0.0 {
                    result = result / (dev_top - dev_bottom);
                }
            }
            GE_INCHES => {
                result = result * (*d).ipr[1];
            }
            GE_CM => {
                result = result * (*d).ipr[1] * 2.54;
            }
            _ => {}
        }
    }
    result
}

#[unsafe(no_mangle)]
pub unsafe fn toDeviceHeight(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    let mut result = value;
    if let Some(d) = dev_ptr(dd) {
        match from {
            GE_CM => {
                result = result / 2.54;
                let dev_top = (*d).top;
                let dev_bottom = (*d).bottom;
                if (dev_top - dev_bottom).abs() > 0.0 {
                    result = (result / (*d).ipr[1]) / (dev_top - dev_bottom).abs();
                }
                result = result * (dev_top - dev_bottom);
            }
            GE_INCHES => {
                let dev_top = (*d).top;
                let dev_bottom = (*d).bottom;
                if (dev_top - dev_bottom).abs() > 0.0 {
                    result = (result / (*d).ipr[1]) / (dev_top - dev_bottom).abs();
                }
                result = result * (dev_top - dev_bottom);
            }
            GE_NDC => {
                let dev_top = (*d).top;
                let dev_bottom = (*d).bottom;
                result = result * (dev_top - dev_bottom);
            }
            GE_DEVICE => {}
            _ => {}
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Line end / join parameter functions
// ---------------------------------------------------------------------------

struct LineEND {
    name: &'static str,
    end: c_int,
}

static LINEEND_TABLE: [LineEND; 3] = [
    LineEND {
        name: "round",
        end: GE_ROUND_CAP,
    },
    LineEND {
        name: "butt",
        end: GE_BUTT_CAP,
    },
    LineEND {
        name: "square",
        end: GE_SQUARE_CAP,
    },
];

pub unsafe fn GE_LENDpar(value: SEXP, ind: c_int) -> c_int {
    // Stub: return default round cap
    // Full implementation parses SEXP string/integer/real
    GE_ROUND_CAP
}

pub unsafe fn GE_LENDget(lend: c_int) -> SEXP {
    // Stub: return nil
    R_NilValue()
}

struct LineJOIN {
    name: &'static str,
    join: c_int,
}

static LINEJOIN_TABLE: [LineJOIN; 3] = [
    LineJOIN {
        name: "round",
        join: GE_ROUND_JOIN,
    },
    LineJOIN {
        name: "mitre",
        join: GE_MITRE_JOIN,
    },
    LineJOIN {
        name: "bevel",
        join: GE_BEVEL_JOIN,
    },
];

pub unsafe fn GE_LJOINpar(value: SEXP, ind: c_int) -> c_int {
    GE_ROUND_JOIN
}

pub unsafe fn GE_LJOINget(ljoin: c_int) -> SEXP {
    R_NilValue()
}

// ---------------------------------------------------------------------------
// GESetClip
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GESetClip(x1: c_double, y1: c_double, x2: c_double, y2: c_double, dd: *mut c_void) {
    if let Some(d) = dev_ptr(dd) {
        let dx1 = (*d).left;
        let dx2 = (*d).right;
        let dy1 = (*d).bottom;
        let dy2 = (*d).top;

        let mut nx1 = x1;
        let mut nx2 = x2;
        let mut ny1 = y1;
        let mut ny2 = y2;

        // Clip to device region
        if dx1 <= dx2 {
            nx1 = nx1.max(dx1);
            nx2 = nx2.min(dx2);
        } else {
            nx1 = nx1.min(dx1);
            nx2 = nx2.max(dx2);
        }
        if dy1 <= dy2 {
            ny1 = ny1.max(dy1);
            ny2 = ny2.min(dy2);
        } else {
            ny1 = ny1.min(dy1);
            ny2 = ny2.max(dy2);
        }

        // Call device clip if available
        if let Some(clip_fn) = (*d).clip {
            clip_fn(nx1, nx2, ny1, ny2, d);
        }

        // Record clip rect settings
        (*d).clipLeft = nx1.min(nx2);
        (*d).clipRight = nx1.max(nx2);
        (*d).clipTop = ny1.max(ny2);
        (*d).clipBottom = ny1.min(ny2);
    }
}

// ---------------------------------------------------------------------------
// GELine
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GELine(
    x1: c_double,
    y1: c_double,
    x2: c_double,
    y2: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if let Some(d) = dev_ptr(dd) {
        if let Some(line_fn) = (*d).line {
            line_fn(x1, y1, x2, y2, gc as pGEcontext, d);
        }
    }
}

// ---------------------------------------------------------------------------
// GEPolyline
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GEPolyline(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if let Some(d) = dev_ptr(dd) {
        if let Some(polyline_fn) = (*d).polyline {
            polyline_fn(n, x, y, gc as pGEcontext, d);
        }
    }
}

// ---------------------------------------------------------------------------
// GEPolygon
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GEPolygon(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if let Some(d) = dev_ptr(dd) {
        if let Some(polygon_fn) = (*d).polygon {
            polygon_fn(n, x, y, gc as pGEcontext, d);
        }
    }
}

// ---------------------------------------------------------------------------
// GECircle
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GECircle(
    x: c_double,
    y: c_double,
    radius: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if radius <= 0.0 {
        return;
    }
    if let Some(d) = dev_ptr(dd) {
        if let Some(circle_fn) = (*d).circle {
            circle_fn(x, y, radius, gc as pGEcontext, d);
        }
    }
}

// ---------------------------------------------------------------------------
// GERect
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GERect(
    x0: c_double,
    y0: c_double,
    x1: c_double,
    y1: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if let Some(d) = dev_ptr(dd) {
        if let Some(rect_fn) = (*d).rect {
            rect_fn(x0, y0, x1, y1, gc as pGEcontext, d);
        }
    }
}

// ---------------------------------------------------------------------------
// GEPath
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GEPath(
    x: *mut c_double,
    y: *mut c_double,
    npoly: c_int,
    nper: *mut c_int,
    winding: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if let Some(d) = dev_ptr(dd) {
        if let Some(path_fn) = (*d).path {
            if npoly > 0 {
                path_fn(x, y, npoly, nper, winding, gc as pGEcontext, d);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GERaster
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
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
    if width != 0.0 && height != 0.0 {
        if let Some(d) = dev_ptr(dd) {
            if let Some(raster_fn) = (*d).raster {
                raster_fn(
                    raster,
                    w,
                    h,
                    x,
                    y,
                    width,
                    height,
                    angle,
                    interpolate,
                    gc as pGEcontext,
                    d,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GECap
// ---------------------------------------------------------------------------

pub unsafe fn GECap(dd: *mut c_void) -> SEXP {
    if let Some(d) = dev_ptr(dd) {
        if let Some(cap_fn) = (*d).cap {
            return cap_fn(d);
        }
    }
    R_NilValue()
}

// ---------------------------------------------------------------------------
// GEText
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
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
    if let Some(d) = dev_ptr(dd) {
        // Determine which text function to use
        let has_utf8 = (*d).hasTextUTF8 != 0;
        let text_fn = if has_utf8 { (*d).textUTF8 } else { (*d).text };

        if let Some(fn_) = text_fn {
            fn_(x, y, str, rot, 0.0, gc as pGEcontext, d);
        }
    }
}

// ---------------------------------------------------------------------------
// GEXspline
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
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
    // Stub: call compute_open/closed_spline then draw
    R_NilValue()
}

// ---------------------------------------------------------------------------
// GEMode
// ---------------------------------------------------------------------------

pub unsafe fn GEMode(mode: c_int, dd: *mut c_void) {
    if let Some(d) = dev_ptr(dd) {
        if let Some(mode_fn) = (*d).mode {
            mode_fn(mode, d);
        }
    }
}

// ---------------------------------------------------------------------------
// GESymbol -- Draw one of the R special plotting symbols
// ---------------------------------------------------------------------------


const SMALL: c_double = 0.25;
const RADIUS: c_double = 0.375;
const SQRC: c_double = 0.88622692545275801364;
const DMDC: c_double = 1.25331413731550025119;
const TRC0: c_double = 1.55512030155621416073;
const TRC1: c_double = 1.34677368708859836060;
const TRC2: c_double = 0.77756015077810708036;

pub unsafe fn GESymbol(
    x: c_double,
    y: c_double,
    pch: c_int,
    size: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if pch == c_int::MIN {
        // NA_INTEGER - do nothing
        return;
    }

    let xx: [c_double; 4] = [0.0; 4];
    let yy: [c_double; 4] = [0.0; 4];

    if pch >= 0 && pch <= 255 {
        if pch == ('.' as c_int) {
            // pch="." -- draw a filled rect
            let xc = size * 0.5;
            let yc = xc;
            GERect(x - xc, y - yc, x + xc, y + yc, gc, dd);
        } else {
            // Single character -- draw as text
            let str_bytes = [pch as u8 as c_char, 0];
            GEText(x, y, str_bytes.as_ptr(), 0, f64::NAN, f64::NAN, 0.0, gc, dd);
        }
    } else if pch > 255 {
        // Invalid pch for locale
    } else {
        // Negative pch: Unicode point -- draw as text
        // (handled by the else branch above in real code)
    }
}

// ---------------------------------------------------------------------------
// GEPretty
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GEPretty(lo: *mut c_double, up: *mut c_double, ndiv: *mut c_int) {
    // Stub: R_pretty is in appl/pretty.c, not yet ported
    // For now, just do basic pretty axis calculation
    if lo.is_null() || up.is_null() || ndiv.is_null() {
        return;
    }
    let lo_val = *lo;
    let up_val = *up;
    if !r_finite(lo_val) || !r_finite(up_val) || *ndiv <= 0 {
        return;
    }
    if lo_val >= up_val {
        return;
    }
    // Simple pretty calculation
    let range = up_val - lo_val;
    let nice_step = range / (*ndiv as c_double);
    *lo = lo_val;
    *up = up_val;
}

// ---------------------------------------------------------------------------
// GEMetricInfo
// ---------------------------------------------------------------------------

pub unsafe fn GEMetricInfo(
    c: c_int,
    gc: *const c_void,
    ascent: *mut c_double,
    descent: *mut c_double,
    width: *mut c_double,
    dd: *mut c_void,
) {
    if let Some(d) = dev_ptr(dd) {
        if let Some(mi_fn) = (*d).metricInfo {
            mi_fn(c, gc as pGEcontext, ascent, descent, width, d);
            return;
        }
    }
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

// ---------------------------------------------------------------------------
// GEStrWidth
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GEStrWidth(
    str: *const c_char,
    enc: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) -> c_double {
    if let Some(d) = dev_ptr(dd) {
        let has_utf8 = (*d).hasTextUTF8 != 0;
        let sw_fn = if has_utf8 {
            (*d).strWidthUTF8
        } else {
            (*d).strWidth
        };
        if let Some(fn_) = sw_fn {
            return fn_(str, gc as pGEcontext, d);
        }
    }
    0.0
}

// ---------------------------------------------------------------------------
// GEStrHeight
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GEStrHeight(
    str: *const c_char,
    enc: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) -> c_double {
    if let Some(d) = dev_ptr(dd) {
        // Height based on cra[1] adjusted for current pointsize
        let mut h = 0.0;
        if let Some(gc_ref) = (gc as pGEcontext).as_ref() {
            let startps = (*d).startps;
            if startps.abs() > 0.0 {
                h = gc_ref.lineheight * gc_ref.cex * (*d).cra[1] * gc_ref.ps / startps;
            }
        }
        // Count lines and add line spacing
        if !str.is_null() {
            let s = CStr::from_ptr(str);
            let bytes = s.to_bytes();
            let n = bytes.iter().filter(|&&b| b == b'\n').count();
            h += n as c_double * (*d).cra[1];
        }
        return h;
    }
    0.0
}

// ---------------------------------------------------------------------------
// GEStrMetric
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GEStrMetric(
    str: *const c_char,
    enc: c_int,
    gc: *const c_void,
    ascent: *mut c_double,
    descent: *mut c_double,
    width: *mut c_double,
    dd: *mut c_void,
) {
    *ascent = 0.0;
    *descent = 0.0;
    *width = 0.0;

    if let Some(d) = dev_ptr(dd) {
        *ascent = GEStrHeight(str, enc, gc, dd);
        *width = GEStrWidth(str, enc, gc, dd);
    }
}

// ---------------------------------------------------------------------------
// GENewPage
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GENewPage(gc: *const c_void, dd: *mut c_void) {
    if let Some(d) = dev_ptr(dd) {
        let gdd = dd as pGEDevDesc;
        (*gdd).appending = 0;
        if let Some(np_fn) = (*d).newPage {
            np_fn(gc as pGEcontext, d);
        }
    }
}

// ---------------------------------------------------------------------------
// GEdeviceDirty / GEdirtyDevice / GEcleanDevice
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe fn GEdeviceDirty(dd: *mut c_void) -> c_int {
    if dd.is_null() {
        return 0;
    }
    let gdd = dd as pGEDevDesc;
    (*gdd).dirty
}

pub unsafe fn GEdirtyDevice(dd: *mut c_void) {
    if !dd.is_null() {
        let gdd = dd as pGEDevDesc;
        (*gdd).dirty = 1;
    }
}

pub(crate) unsafe fn GEcleanDevice(dd: *mut c_void) {
    if !dd.is_null() {
        let gdd = dd as pGEDevDesc;
        (*gdd).dirty = 0;
    }
}

// ---------------------------------------------------------------------------
// GEcheckState
// ---------------------------------------------------------------------------

pub unsafe fn GEcheckState(dd: *mut c_void) -> c_int {
    1 // TRUE -- stub, always returns valid
}

// ---------------------------------------------------------------------------
// GErecording
// ---------------------------------------------------------------------------

pub unsafe fn GErecording(call: SEXP, dd: *mut c_void) -> c_int {
    if dd.is_null() || call == R_NilValue() {
        return 0;
    }
    let gdd = dd as pGEDevDesc;
    if (*gdd).recordGraphics != 0 { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// GErecordGraphicOperation
// ---------------------------------------------------------------------------

pub unsafe fn GErecordGraphicOperation(op: SEXP, args: SEXP, dd: *mut c_void) {
    // Stub: display list recording requires SEXP allocation (CONS, list2, etc.)
}

// ---------------------------------------------------------------------------
// GEinitDisplayList
// ---------------------------------------------------------------------------

pub unsafe fn GEinitDisplayList(dd: *mut c_void) {
    if dd.is_null() {
        return;
    }
    let gdd = dd as pGEDevDesc;
    (*gdd).savedSnapshot = GEcreateSnapshot(dd);
    (*gdd).displayList = R_NilValue();
    (*gdd).DLlastElt = R_NilValue();
}

// ---------------------------------------------------------------------------
// GEplayDisplayList
// ---------------------------------------------------------------------------

pub unsafe fn GEplayDisplayList(dd: *mut c_void) {
    // Stub: display list replay requires SEXP evaluation infrastructure
}

// ---------------------------------------------------------------------------
// GEcopyDisplayList
// ---------------------------------------------------------------------------

pub unsafe fn GEcopyDisplayList(fromDevice: c_int) {
    // Stub: requires GEcurrentDevice, GEgetDevice, display list copy
}

// ---------------------------------------------------------------------------
// GEcreateSnapshot
// ---------------------------------------------------------------------------

pub unsafe fn GEcreateSnapshot(dd: *mut c_void) -> SEXP {
    R_NilValue()
}

// ---------------------------------------------------------------------------
// GEplaySnapshot
// ---------------------------------------------------------------------------

pub unsafe fn GEplaySnapshot(snapshot: SEXP, dd: *mut c_void) {
    // Stub: requires display list duplication and replay
}

// ---------------------------------------------------------------------------
// do_getSnapshot / do_playSnapshot
// ---------------------------------------------------------------------------

pub unsafe fn do_getSnapshot(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    R_NilValue()
}

pub unsafe fn do_playSnapshot(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    R_NilValue()
}

// ---------------------------------------------------------------------------
// do_recordGraphics
// ---------------------------------------------------------------------------

pub unsafe fn do_recordGraphics(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    R_NilValue()
}

// ---------------------------------------------------------------------------
// GEonExit
// ---------------------------------------------------------------------------

pub unsafe fn GEonExit() {
    // Stub: requires iterating over all devices to reset recording
}

// ---------------------------------------------------------------------------
// GEstring_to_pch
// ---------------------------------------------------------------------------

pub unsafe fn GEstring_to_pch(pch: SEXP) -> c_int {
    c_int::MIN // NA_INTEGER
}

// ---------------------------------------------------------------------------
// GE_LTYpar / GE_LTYget
// ---------------------------------------------------------------------------

struct LineTYPE {
    name: &'static str,
    pattern: c_uint,
}

static LINETYPE_TABLE: [LineTYPE; 7] = [
    LineTYPE {
        name: "blank",
        pattern: LTY_BLANK,
    },
    LineTYPE {
        name: "solid",
        pattern: LTY_SOLID,
    },
    LineTYPE {
        name: "dashed",
        pattern: LTY_DASHED,
    },
    LineTYPE {
        name: "dotted",
        pattern: LTY_DOTTED,
    },
    LineTYPE {
        name: "dotdash",
        pattern: LTY_DOTDASH,
    },
    LineTYPE {
        name: "longdash",
        pattern: LTY_LONGDASH,
    },
    LineTYPE {
        name: "twodash",
        pattern: LTY_TWODASH,
    },
];

pub unsafe fn GE_LTYpar(value: SEXP, ind: c_int) -> c_uint {
    LTY_SOLID
}

pub unsafe fn GE_LTYget(lty: c_uint) -> SEXP {
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Raster image operations
// ---------------------------------------------------------------------------

pub unsafe fn R_GE_rasterScale(
    sraster: *const c_uint,
    sw: c_int,
    sh: c_int,
    draster: *mut c_uint,
    dw: c_int,
    dh: c_int,
) {
    if sraster.is_null() || draster.is_null() || sw <= 0 || sh <= 0 || dw <= 0 || dh <= 0 {
        return;
    }
    for i in 0..dh {
        for j in 0..dw {
            let sy = (i as c_int * sh) / dh;
            let sx = (j as c_int * sw) / dw;
            let pixel = if sx >= 0 && sx < sw && sy >= 0 && sy < sh {
                *sraster.add((sy * sw + sx) as usize)
            } else {
                0
            };
            *draster.add((i as usize * dw as usize) + j as usize) = pixel;
        }
    }
}

pub unsafe fn R_GE_rasterInterpolate(
    sraster: *const c_uint,
    sw: c_int,
    sh: c_int,
    draster: *mut c_uint,
    dw: c_int,
    dh: c_int,
) {
    if sraster.is_null() || draster.is_null() || sw <= 0 || sh <= 0 || dw <= 0 || dh <= 0 {
        return;
    }
    let scx = (16.0 * sw as c_double) / dw as c_double;
    let scy = (16.0 * sh as c_double) / dh as c_double;
    let wm2 = sw - 2;
    let hm2 = sh - 2;

    for i in 0..dh {
        let ypm = (scy * i as c_double - 8.0).max(0.0) as c_int;
        let yp = ypm >> 4;
        let yf = ypm & 0x0f;
        let dline = i as usize * dw as usize;
        let sline = yp as usize * sw as usize;

        for j in 0..dw {
            let xpm = (scx * j as c_double - 8.0).max(0.0) as c_int;
            let xp = xpm >> 4;
            let xf = xpm & 0x0f;

            let p1 = if (xp as isize) < (sw as isize) && (yp as isize) < (sh as isize) {
                *sraster.add(sline + xp as usize)
            } else {
                0
            };

            let p2 = if (xp + 1) < sw && (yp as isize) < (sh as isize) {
                *sraster.add(sline + (xp + 1) as usize)
            } else {
                p1
            };

            let p3 = if (xp as isize) < (sw as isize) && (yp + 1) < sh {
                *sraster.add(sline + sw as usize + xp as usize)
            } else {
                p1
            };

            let p4 = if (xp + 1) < sw && (yp + 1) < sh {
                *sraster.add(sline + sw as usize + (xp + 1) as usize)
            } else {
                p1
            };

            let area00 = (16 - xf) * (16 - yf);
            let area10 = xf * (16 - yf);
            let area01 = (16 - xf) * yf;
            let area11 = xf * yf;

            let v00r = area00 * R_RED(p1) as c_int;
            let v00g = area00 * R_GREEN(p1) as c_int;
            let v00b = area00 * R_BLUE(p1) as c_int;
            let v00a = area00 * R_ALPHA(p1) as c_int;

            let v10r = area10 * R_RED(p2) as c_int;
            let v10g = area10 * R_GREEN(p2) as c_int;
            let v10b = area10 * R_BLUE(p2) as c_int;
            let v10a = area10 * R_ALPHA(p2) as c_int;

            let v01r = area01 * R_RED(p3) as c_int;
            let v01g = area01 * R_GREEN(p3) as c_int;
            let v01b = area01 * R_BLUE(p3) as c_int;
            let v01a = area01 * R_ALPHA(p3) as c_int;

            let v11r = area11 * R_RED(p4) as c_int;
            let v11g = area11 * R_GREEN(p4) as c_int;
            let v11b = area11 * R_BLUE(p4) as c_int;
            let v11a = area11 * R_ALPHA(p4) as c_int;

            let pixel = (((v00r + v10r + v01r + v11r + 128) >> 8) as c_uint & 0x000000ff_u32)
                | ((v00g + v10g + v01g + v11g + 128) as c_uint & 0x0000ff00_u32)
                | (((v00b + v10b + v01b + v11b + 128) << 8) as c_uint & 0x00ff0000_u32)
                | (((v00a + v10a + v01a + v11a + 128) << 16) as c_uint & 0xff000000_u32);

            *draster.add(dline + j as usize) = pixel;
        }
    }
}

pub unsafe fn R_GE_rasterRotatedSize(
    w: c_int,
    h: c_int,
    angle: c_double,
    wnew: *mut c_int,
    hnew: *mut c_int,
) {
    let diag = ((w * w + h * h) as f64).sqrt();
    let theta = (h as f64).atan2(w as f64);
    let trx1 = diag * (theta + angle).cos();
    let trx2 = diag * (theta - angle).cos();
    let try1 = diag * (theta + angle).sin();
    let try2 = diag * (angle - theta).sin();

    let mut nw = (trx1.abs().max(trx2.abs()) + 0.5) as c_int;
    let mut nh = (try1.abs().max(try2.abs()) + 0.5) as c_int;

    // Ensure rotated image is not smaller than original
    nw = nw.max(w);
    nh = nh.max(h);

    if !wnew.is_null() {
        *wnew = nw;
    }
    if !hnew.is_null() {
        *hnew = nh;
    }
}

pub unsafe fn R_GE_rasterRotatedOffset(
    w: c_int,
    h: c_int,
    angle: c_double,
    botleft: c_int,
    xoff: *mut c_double,
    yoff: *mut c_double,
) {
    let hypot = 0.5 * ((w * w + h * h) as f64).sqrt();
    let theta;
    let dw;
    let dh;

    if botleft != 0 {
        theta = std::f64::consts::PI + (h as f64).atan2(w as f64);
        dw = hypot * (theta + angle).cos();
        dh = hypot * (theta + angle).sin();
        if !xoff.is_null() {
            *xoff = dw + w as f64 / 2.0;
        }
        if !yoff.is_null() {
            *yoff = dh + h as f64 / 2.0;
        }
    } else {
        theta = -std::f64::consts::PI - (h as f64).atan2(w as f64);
        dw = hypot * (theta + angle).cos();
        dh = hypot * (theta + angle).sin();
        if !xoff.is_null() {
            *xoff = dw + w as f64 / 2.0;
        }
        if !yoff.is_null() {
            *yoff = dh - h as f64 / 2.0;
        }
    }
}

pub unsafe fn R_GE_rasterResizeForRotation(
    sraster: *const c_uint,
    w: c_int,
    h: c_int,
    newRaster: *mut c_uint,
    wnew: c_int,
    hnew: c_int,
    gc: *const c_void,
) {
    if newRaster.is_null() || w <= 0 || h <= 0 || wnew <= 0 || hnew <= 0 {
        return;
    }
    let fill = if let Some(gc_ref) = (gc as pGEcontext).as_ref() {
        gc_ref.fill as c_uint
    } else {
        R_TRANWHITE
    };

    // Fill with background
    for i in 0..hnew as usize {
        for j in 0..wnew as usize {
            *newRaster.add(i * wnew as usize + j) = fill;
        }
    }

    // Copy source into center
    let xoff = (wnew - w) / 2;
    let yoff = (hnew - h) / 2;
    for i in 0..h {
        for j in 0..w {
            let inew = i + yoff;
            let jnew = j + xoff;
            if inew >= 0 && inew < hnew && jnew >= 0 && jnew < wnew {
                *newRaster.add(inew as usize * wnew as usize + jnew as usize) =
                    *sraster.add((i * w + j) as usize);
            }
        }
    }
}

pub unsafe fn R_GE_rasterRotate(
    sraster: *const c_uint,
    w: c_int,
    h: c_int,
    angle: c_double,
    draster: *mut c_uint,
    gc: *const c_void,
    smoothAlpha: c_int,
) {
    if sraster.is_null() || draster.is_null() || w <= 0 || h <= 0 {
        return;
    }
    let fill = if let Some(gc_ref) = (gc as pGEcontext).as_ref() {
        gc_ref.fill as c_uint
    } else {
        R_TRANWHITE
    };

    // R uses clockwise angle; convert to anticlockwise for our calc
    let a = -angle;
    let xcen = w / 2;
    let ycen = h / 2;
    let wm2 = w - 2;
    let hm2 = h - 2;
    let sina = 16.0 * a.sin();
    let cosa = 16.0 * a.cos();

    for i in 0..h {
        let ydif = ycen - i;
        let dline = i as usize * w as usize;
        for j in 0..w {
            let xdif = xcen - j;
            let xpm = (-xdif as c_double * cosa - ydif as c_double * sina) as c_int;
            let ypm = (-ydif as c_double * cosa + xdif as c_double * sina) as c_int;
            let xp = xcen + (xpm >> 4);
            let yp = ycen + (ypm >> 4);
            let xf = xpm & 0x0f;
            let yf = ypm & 0x0f;

            if xp < 0 || yp < 0 || xp > wm2 || yp > hm2 {
                *draster.add(dline + j as usize) = fill;
                continue;
            }

            let sline = yp as usize * w as usize;
            let w00 = *sraster.add(sline + xp as usize);
            let w10 = *sraster.add(sline + (xp + 1) as usize);
            let w01 = *sraster.add(sline + w as usize + xp as usize);
            let w11 = *sraster.add(sline + w as usize + (xp + 1) as usize);

            let rval = ((16 - xf) * (16 - yf) * R_RED(w00) as c_int
                + xf * (16 - yf) * R_RED(w10) as c_int
                + (16 - xf) * yf * R_RED(w01) as c_int
                + xf * yf * R_RED(w11) as c_int
                + 128)
                / 256;
            let gval = ((16 - xf) * (16 - yf) * R_GREEN(w00) as c_int
                + xf * (16 - yf) * R_GREEN(w10) as c_int
                + (16 - xf) * yf * R_GREEN(w01) as c_int
                + xf * yf * R_GREEN(w11) as c_int
                + 128)
                / 256;
            let bval = ((16 - xf) * (16 - yf) * R_BLUE(w00) as c_int
                + xf * (16 - yf) * R_BLUE(w10) as c_int
                + (16 - xf) * yf * R_BLUE(w01) as c_int
                + xf * yf * R_BLUE(w11) as c_int
                + 128)
                / 256;

            let aval = if smoothAlpha != 0 {
                ((16 - xf) * (16 - yf) * R_ALPHA(w00) as c_int
                    + xf * (16 - yf) * R_ALPHA(w10) as c_int
                    + (16 - xf) * yf * R_ALPHA(w01) as c_int
                    + xf * yf * R_ALPHA(w11) as c_int
                    + 128)
                    / 256
            } else {
                let a00 = R_ALPHA(w00) as c_int;
                let a10 = R_ALPHA(w10) as c_int;
                let a01 = R_ALPHA(w01) as c_int;
                let a11 = R_ALPHA(w11) as c_int;
                a00.max(a10).max(a01).max(a11)
            };

            let pixel = R_RGBA(
                rval.clamp(0, 255) as u32,
                gval.clamp(0, 255) as u32,
                bval.clamp(0, 255) as u32,
                aval.clamp(0, 255) as u32,
            );
            *draster.add(dline + j as usize) = pixel;
        }
    }
}

// ---------------------------------------------------------------------------
// Path drawing (GEgroup API)
// ---------------------------------------------------------------------------

pub unsafe fn GEStroke(path: SEXP, gc: *const c_void, dd: *mut c_void) {
    if let Some(d) = dev_ptr(dd) {
        if (*d).deviceVersion >= R_GE_group {
            if let Some(stroke_fn) = (*d).stroke {
                stroke_fn(path, gc as pGEcontext, d);
            }
        }
    }
}

pub unsafe fn GEFill(path: SEXP, rule: c_int, gc: *const c_void, dd: *mut c_void) {
    if let Some(d) = dev_ptr(dd) {
        if (*d).deviceVersion >= R_GE_group {
            if let Some(fill_fn) = (*d).fill {
                fill_fn(path, rule, gc as pGEcontext, d);
            }
        }
    }
}

pub unsafe fn GEFillStroke(path: SEXP, rule: c_int, gc: *const c_void, dd: *mut c_void) {
    if let Some(d) = dev_ptr(dd) {
        if (*d).deviceVersion >= R_GE_group {
            if let Some(fs_fn) = (*d).fillStroke {
                fs_fn(path, rule, gc as pGEcontext, d);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Glyph info API
// ---------------------------------------------------------------------------

pub unsafe fn R_GE_glyphInfoGlyphs(glyphInfo: SEXP) -> SEXP {
    R_NilValue()
}

pub unsafe fn R_GE_glyphInfoFonts(glyphInfo: SEXP) -> SEXP {
    R_NilValue()
}

pub unsafe fn R_GE_glyphID(glyphs: SEXP) -> SEXP {
    R_NilValue()
}

pub unsafe fn R_GE_glyphX(glyphs: SEXP) -> SEXP {
    R_NilValue()
}

pub unsafe fn R_GE_glyphY(glyphs: SEXP) -> SEXP {
    R_NilValue()
}

pub unsafe fn R_GE_glyphFont(glyphs: SEXP) -> SEXP {
    R_NilValue()
}

pub unsafe fn R_GE_glyphSize(glyphs: SEXP) -> SEXP {
    R_NilValue()
}

pub unsafe fn R_GE_glyphColour(glyphs: SEXP) -> SEXP {
    R_NilValue()
}

pub unsafe fn R_GE_glyphRotation(glyphs: SEXP) -> SEXP {
    R_NilValue()
}

pub unsafe fn R_GE_hasGlyphRotation(glyphs: SEXP) -> c_int {
    0 // FALSE
}

// ---------------------------------------------------------------------------
// Glyph font info API
// ---------------------------------------------------------------------------

pub unsafe fn R_GE_glyphFontFile(glyphFont: SEXP) -> *const c_char {
    ptr::null()
}

pub unsafe fn R_GE_glyphFontIndex(glyphFont: SEXP) -> c_int {
    0
}

pub unsafe fn R_GE_glyphFontFamily(glyphFont: SEXP) -> *const c_char {
    ptr::null()
}

pub unsafe fn R_GE_glyphFontWeight(glyphFont: SEXP) -> c_double {
    0.0
}

pub unsafe fn R_GE_glyphFontStyle(glyphFont: SEXP) -> c_int {
    0
}

pub unsafe fn R_GE_glyphFontPSname(glyphFont: SEXP) -> *const c_char {
    ptr::null()
}

pub unsafe fn R_GE_glyphFontNumVar(glyphFont: SEXP) -> c_int {
    0
}

pub unsafe fn R_GE_glyphFontVarAxis(glyphFont: SEXP, index: c_int) -> *const c_char {
    ptr::null()
}

pub unsafe fn R_GE_glyphFontVarValue(glyphFont: SEXP, index: c_int) -> c_double {
    0.0
}

pub unsafe fn R_GE_glyphFontVarFormatted(
    glyphFont: SEXP,
    index: c_int,
) -> *const c_char {
    ptr::null()
}

// ---------------------------------------------------------------------------
// GEGlyph
// ---------------------------------------------------------------------------

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
    if let Some(d) = dev_ptr(dd) {
        if (*d).deviceVersion >= R_GE_glyphs {
            if let Some(glyph_fn) = (*d).glyph {
                glyph_fn(n, glyphs, x, y, font, size, colour, rot, d);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Rf_eval_with_gd
// ---------------------------------------------------------------------------

pub unsafe fn Rf_eval_with_gd(e: SEXP, rho: SEXP, dd: *mut c_void) -> SEXP {
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Module-private helper stubs (not #[unsafe(no_mangle)])
// ---------------------------------------------------------------------------

pub(crate) unsafe fn compute_open_spline(
    n: c_int,
    x: *mut c_double,
    y: *mut c_double,
    s: *mut c_double,
    repEnds: c_int,
    precision: c_int,
    dd: *mut c_void,
) {
    // Stub: xspline computation
}

pub(crate) unsafe fn compute_closed_spline(
    n: c_int,
    x: *mut c_double,
    y: *mut c_double,
    s: *mut c_double,
    precision: c_int,
    dd: *mut c_void,
) {
    // Stub: xspline computation
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_R_GE_getVersion() {
        unsafe {
            assert_eq!(R_GE_getVersion(), R_GE_version);
        }
    }

    #[test]
    fn test_R_GE_checkVersionOrDie_matching() {
        unsafe {
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
    fn test_GEMetricInfo_returns_zeros() {
        unsafe {
            let mut a = 1.0;
            let mut d = 1.0;
            let mut w = 1.0;
            GEMetricInfo(77, ptr::null(), &mut a, &mut d, &mut w, ptr::null_mut());
            assert_eq!(a, 0.0);
            assert_eq!(d, 0.0);
            assert_eq!(w, 0.0);
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
    fn test_GEcheckState_returns_true() {
        unsafe {
            assert_eq!(GEcheckState(ptr::null_mut()), 1);
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
    fn test_GE_LTYpar_returns_solid() {
        unsafe {
            assert_eq!(GE_LTYpar(ptr::null_mut(), 0), LTY_SOLID);
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
            // 100x200 image rotated by 0.5 rad; bounding box must be >= original
            R_GE_rasterRotatedSize(100, 200, 0.5, &mut wnew, &mut hnew);
            assert_eq!(wnew, 184);
            assert_eq!(hnew, 223);
        }
    }

    #[test]
    fn test_R_GE_rasterRotatedOffset() {
        unsafe {
            let mut xoff = 0.0;
            let mut yoff = 0.0;
            R_GE_rasterRotatedOffset(100, 200, 0.5, 1, &mut xoff, &mut yoff);
            // botleft=1: offset from bottom-left corner
            assert!((xoff - 54.06342576590164).abs() < 1e-10);
            assert!((yoff - (-11.72953311924742)).abs() < 1e-10);
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
    fn test_LEND_LJOIN_stubs() {
        unsafe {
            assert_eq!(GE_LENDpar(ptr::null_mut(), 0), GE_ROUND_CAP);
            assert_eq!(GE_LJOINpar(ptr::null_mut(), 0), GE_ROUND_JOIN);
            assert_eq!(GE_LENDget(0), R_NilValue());
            assert_eq!(GE_LJOINget(0), R_NilValue());
        }
    }

    #[test]
    fn test_constants() {
        assert_eq!(R_GE_version, 17);
        assert_eq!(MAX_GRAPHICS_SYSTEMS, 24);
        assert_eq!(GE_DEVICE, 0);
        assert_eq!(GE_NDC, 1);
        assert_eq!(GE_INCHES, 2);
        assert_eq!(GE_CM, 3);
        assert_eq!(R_TRANWHITE, 0x00FFFFFF);
        assert_eq!(LTY_SOLID, 0);
        assert_eq!(LTY_DASHED, 4 + (4 << 4));
        assert_eq!(LTY_DOTTED, 1 + (3 << 4));
    }

    #[test]
    fn test_raster_scale() {
        unsafe {
            let src = [0xFF0000FFu32, 0x00FF0000u32];
            let mut dst = [0u32; 4];
            R_GE_rasterScale(src.as_ptr(), 2, 1, dst.as_mut_ptr(), 4, 2);
            assert_eq!(dst[0], 0xFF0000FF);
            assert_eq!(dst[1], 0xFF0000FF);
            assert_eq!(dst[2], 0x00FF0000);
            assert_eq!(dst[3], 0x00FF0000);
        }
    }

    #[test]
    fn test_colour_macros() {
        // R_RGB: r | (g<<8) | (b<<16) | (a<<24) with alpha=0xFF
        assert_eq!(R_RGB(255, 0, 0), 0xFF0000FF_u32); // r=255, g=0, b=0, a=255
        assert_eq!(R_RED(0x000000FF_u32), 255); // red in bits 0-7
        assert_eq!(R_GREEN(0x0000FF00_u32), 255); // green in bits 8-15
        assert_eq!(R_BLUE(0x00FF0000_u32), 255); // blue in bits 16-23
        assert_eq!(R_ALPHA(0xFF000000_u32), 255); // alpha in bits 24-31
        assert!(R_TRANSPARENT(0x00FFFFFF_u32)); // alpha=0 => transparent
        assert!(!R_TRANSPARENT(0xFFFFFFFF_u32)); // alpha=255 => not transparent
    }

    #[test]
    fn test_gcontext_default() {
        let gc = R_GE_gcontext::default();
        assert_eq!(gc.col, 1);
        assert_eq!(gc.lwd, 1.0);
        assert_eq!(gc.ps, 12.0);
        assert_eq!(gc.fontface, 1);
    }

    #[test]
    fn test_GESetClip_null_dd() {
        // Should not crash with null dd
        unsafe {
            GESetClip(0.0, 0.0, 100.0, 100.0, ptr::null_mut());
        }
    }

    #[test]
    fn test_drawing_functions_null_dd() {
        unsafe {
            GELine(0.0, 0.0, 1.0, 1.0, ptr::null(), ptr::null_mut());
            GEPolyline(0, ptr::null(), ptr::null(), ptr::null(), ptr::null_mut());
            GEPolygon(0, ptr::null(), ptr::null(), ptr::null(), ptr::null_mut());
            GECircle(0.0, 0.0, 10.0, ptr::null(), ptr::null_mut());
            GERect(0.0, 0.0, 1.0, 1.0, ptr::null(), ptr::null_mut());
            GEText(
                0.0,
                0.0,
                ptr::null(),
                0,
                0.0,
                0.0,
                0.0,
                ptr::null(),
                ptr::null_mut(),
            );
            GEMode(1, ptr::null_mut());
            GENewPage(ptr::null(), ptr::null_mut());
        }
    }
}
