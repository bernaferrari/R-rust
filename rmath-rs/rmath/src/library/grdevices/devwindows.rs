#![allow(
    unsafe_op_in_unsafe_fn,
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]
//! Windows graphics device module (devWindows.c, 4053 lines)
//!
//! Provides Windows GDI-based graphics device (devga), savePlot,
//! and on Windows, the Cairo device functions (devCairo, cairoVersion,
//! pangoVersion, cairoFT, bmVersion).
//!
//! On Windows, these use the real Windows GDI / winCairo DLL.
//! On non-Windows, savePlot and devga are exported as stubs.
//!
//! The Cairo functions (devCairo, cairoVersion, pangoVersion, cairoFT)
//! are only exported on Windows here. On non-Windows they are provided
//! by devcairo.rs.

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int, c_uchar, c_uint, c_void};
use std::ptr;

use crate::attrib_core::R_NamesSymbol;
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ===========================================================================
// Constants
// ===========================================================================

const MM_PER_INCH: c_double = 25.4;
const PNG_TRANS: c_uint = 0xfdfefd;
const SMALLEST: c_int = 1;
const SF: c_int = 20;
const NFONT: c_int = 19;
const MAXFONT: c_int = 32;
const GROWTH: c_int = 4;
const PLOTHISTORYMAGIC: c_int = 31416;

// Device kinds
const SCREEN: c_int = 0;
const PRINTER: c_int = 1;
const METAFILE: c_int = 2;
const PNG: c_int = 3;
const JPEG: c_int = 4;
const BMP_KIND: c_int = 5;
const TIFF: c_int = 6;

// Font styles
const Plain: c_int = 0;
const Bold: c_int = 1;
const Italic: c_int = 2;
const BoldItalic: c_int = 3;

// Line end/join
const PS_ENDCAP_ROUND: c_int = 0;
const PS_ENDCAP_FLAT: c_int = 1;
const PS_ENDCAP_SQUARE: c_int = 2;
const PS_JOIN_ROUND: c_int = 0;
const PS_JOIN_MITER: c_int = 1;
const PS_JOIN_BEVEL: c_int = 2;

// GE constants
const GE_ROUND_CAP: c_int = 1;
const GE_BUTT_CAP: c_int = 2;
const GE_SQUARE_CAP: c_int = 3;
const GE_ROUND_JOIN: c_int = 1;
const GE_MITRE_JOIN: c_int = 2;
const GE_BEVEL_JOIN: c_int = 3;

// LTY
const LTY_SOLID: c_int = 65535;

// DEFAULT_QUALITY (Windows)
const DEFAULT_QUALITY: c_int = 0;
const NONANTIALIASED_QUALITY: c_int = 3;
const CLEARTYPE_QUALITY: c_int = 5;
const ANTIALIASED_QUALITY: c_int = 4;

// DEG2RAD
const DEG2RAD: c_double = std::f64::consts::PI / 180.0;

// ===========================================================================
// gadesc structure (mirrors devWindows.h)
// ===========================================================================

#[repr(C)]
pub struct gadesc {
    // R Graphics Parameters
    col: c_int,
    bg: c_int,
    fontface: c_int,
    fontsize: c_int,
    basefontsize: c_int,
    fontangle: c_double,
    basefontfamily: [c_char; 500],

    // Device kind
    kind: c_int,
    windowWidth: c_int,
    windowHeight: c_int,
    showWidth: c_int,
    showHeight: c_int,
    origWidth: c_int,
    origHeight: c_int,
    xshift: c_int,
    yshift: c_int,
    resize: c_int,

    // Opaque window handle
    gawin: *mut c_void,

    // Menu/UI handles (opaque on non-Windows)
    locpopup: *mut c_void,
    grpopup: *mut c_void,
    stoploc: *mut c_void,
    mbar: *mut c_void,
    mbarloc: *mut c_void,
    mbarconfirm: *mut c_void,
    msubsave: *mut c_void,
    mpng: *mut c_void,
    mbmp: *mut c_void,
    mjpeg50: *mut c_void,
    mjpeg75: *mut c_void,
    mjpeg100: *mut c_void,
    mtiff: *mut c_void,
    mps: *mut c_void,
    mpdf: *mut c_void,
    mwm: *mut c_void,
    mclpbm: *mut c_void,
    mclpwm: *mut c_void,
    mprint: *mut c_void,
    mclose: *mut c_void,
    mrec: *mut c_void,
    madd: *mut c_void,
    mreplace: *mut c_void,
    mprev: *mut c_void,
    mnext: *mut c_void,
    mclear: *mut c_void,
    msvar: *mut c_void,
    mgvar: *mut c_void,
    mR: *mut c_void,
    mfit: *mut c_void,
    mfix: *mut c_void,
    grmenustayontop: *mut c_void,
    mnextplot: *mut c_void,

    recording: c_int,
    replaying: c_int,
    needsave: c_int,

    bm: *mut c_void,
    bm2: *mut c_void,

    // File/bitmap section
    fp: *mut c_void,
    filename: [c_char; 512],
    quality: c_int,
    npage: c_int,
    res_dpi: c_int,

    w: c_double,
    h: c_double,
    xpinch: c_double,
    ypinch: c_double,
    fgcolor: c_uint,
    bgcolor: c_uint,
    canvascolor: c_uint,
    outcolor: c_uint,
    clip: ClipRect,
    font: *mut c_void,
    fontfamily: [c_char; 100],
    fontquality: c_int,

    locator: c_int,
    confirmation: c_int,
    clicked: c_int,
    px: c_int,
    py: c_int,
    lty: c_int,
    lwd: c_int,
    resizing: c_int,
    rescale_factor: c_double,
    fast: c_int,
    pngtrans: c_uint,
    buffered: c_int,
    timeafter: c_int,
    timesince: c_int,
    psenv: SEXP,
    lend: c_int,
    ljoin: c_int,
    lmitre: c_double,
    enterkey: c_int,
    lwdscale: c_double,
    cntxt: *mut c_void,
    have_alpha: c_int,
    warn_trans: c_int,
    title: [c_char; 101],
    clickToConfirm: c_int,
    doSetPolyFill: c_int,
    fillOddEven: c_int,
    holdlevel: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ClipRect {
    x: c_int,
    y: c_int,
    width: c_int,
    height: c_int,
}

impl Default for ClipRect {
    fn default() -> Self {
        ClipRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }
    }
}

impl Default for gadesc {
    fn default() -> Self {
        gadesc {
            col: 0,
            bg: 0,
            fontface: -1,
            fontsize: -1,
            basefontsize: 12,
            fontangle: 0.0,
            basefontfamily: [0; 500],
            kind: SCREEN,
            windowWidth: 0,
            windowHeight: 0,
            showWidth: 0,
            showHeight: 0,
            origWidth: 0,
            origHeight: 0,
            xshift: 0,
            yshift: 0,
            resize: 0,
            gawin: ptr::null_mut(),
            locpopup: ptr::null_mut(),
            grpopup: ptr::null_mut(),
            stoploc: ptr::null_mut(),
            mbar: ptr::null_mut(),
            mbarloc: ptr::null_mut(),
            mbarconfirm: ptr::null_mut(),
            msubsave: ptr::null_mut(),
            mpng: ptr::null_mut(),
            mbmp: ptr::null_mut(),
            mjpeg50: ptr::null_mut(),
            mjpeg75: ptr::null_mut(),
            mjpeg100: ptr::null_mut(),
            mtiff: ptr::null_mut(),
            mps: ptr::null_mut(),
            mpdf: ptr::null_mut(),
            mwm: ptr::null_mut(),
            mclpbm: ptr::null_mut(),
            mclpwm: ptr::null_mut(),
            mprint: ptr::null_mut(),
            mclose: ptr::null_mut(),
            mrec: ptr::null_mut(),
            madd: ptr::null_mut(),
            mreplace: ptr::null_mut(),
            mprev: ptr::null_mut(),
            mnext: ptr::null_mut(),
            mclear: ptr::null_mut(),
            msvar: ptr::null_mut(),
            mgvar: ptr::null_mut(),
            mR: ptr::null_mut(),
            mfit: ptr::null_mut(),
            mfix: ptr::null_mut(),
            grmenustayontop: ptr::null_mut(),
            mnextplot: ptr::null_mut(),
            recording: 0,
            replaying: 0,
            needsave: 0,
            bm: ptr::null_mut(),
            bm2: ptr::null_mut(),
            fp: ptr::null_mut(),
            filename: [0; 512],
            quality: DEFAULT_QUALITY,
            npage: 0,
            res_dpi: 0,
            w: 0.0,
            h: 0.0,
            xpinch: 0.0,
            ypinch: 0.0,
            fgcolor: 0,
            bgcolor: 0xffffff,
            canvascolor: 0xffffff,
            outcolor: 0,
            clip: ClipRect::default(),
            font: ptr::null_mut(),
            fontfamily: [0; 100],
            fontquality: DEFAULT_QUALITY,
            locator: 0,
            confirmation: 0,
            clicked: 0,
            px: 0,
            py: 0,
            lty: LTY_SOLID,
            lwd: 1,
            resizing: 1,
            rescale_factor: 1.0,
            fast: 1,
            pngtrans: 0,
            buffered: 0,
            timeafter: 100,
            timesince: 500,
            psenv: unsafe { R_NilValue() },
            lend: PS_ENDCAP_ROUND,
            ljoin: PS_JOIN_ROUND,
            lmitre: 10.0,
            enterkey: 0,
            lwdscale: 1.0,
            cntxt: ptr::null_mut(),
            have_alpha: 0,
            warn_trans: 0,
            title: [0; 101],
            clickToConfirm: 0,
            doSetPolyFill: 1,
            fillOddEven: 0,
            holdlevel: 0,
        }
    }
}

// ===========================================================================
// Module-level state
// ===========================================================================

static mut fontnum: c_int = 0;
static mut fontinitdone: c_int = 0;
static mut fontname: [[c_char; 256]; MAXFONT as usize] = [[0; 256]; MAXFONT as usize];
static mut fontstyle: [c_int; MAXFONT as usize] = [0; MAXFONT as usize];
static mut GA_xd: *mut gadesc = ptr::null_mut();
static mut GALastUpdate: u32 = 0;
static mut TimerNo: usize = 0;
static mut png_rows: c_int = 0;

// ===========================================================================
// Helper functions
// ===========================================================================

/// Extract R_RED component from a packed R color integer
#[inline]
unsafe fn R_RED(color: c_int) -> c_int {
    (color >> 16) & 0xff
}

/// Extract R_GREEN component from a packed R color integer
#[inline]
unsafe fn R_GREEN(color: c_int) -> c_int {
    (color >> 8) & 0xff
}

/// Extract R_BLUE component from a packed R color integer
#[inline]
unsafe fn R_BLUE(color: c_int) -> c_int {
    color & 0xff
}

/// Extract R_ALPHA component from a packed R color integer
#[inline]
unsafe fn R_ALPHA(color: c_int) -> c_int {
    (color >> 24) & 0xff
}

/// Construct an R color from R, G, B, A components
#[inline]
unsafe fn R_RGBA(r: c_int, g: c_int, b: c_int, a: c_int) -> c_int {
    (a << 24) | (r << 16) | (g << 8) | b
}

/// Check if a color is fully opaque
#[inline]
unsafe fn R_OPAQUE(color: c_int) -> bool {
    R_ALPHA(color) == 255
}

/// Pack RGB into a 0x00RRGGBB integer
#[inline]
fn rgb_pack(r: c_int, g: c_int, b: c_int) -> c_uint {
    ((r as c_uint) << 16) | ((g as c_uint) << 8) | (b as c_uint)
}

/// Convert an R color to an ARGB quadruplet with gamma correction
unsafe fn GArgb(color: c_int, gamma: c_double) -> c_uint {
    let r: c_int;
    let g: c_int;
    let b: c_int;
    if gamma != 1.0 {
        r = (255.0 * (R_RED(color) as c_double / 255.0).powf(gamma)) as c_int;
        g = (255.0 * (R_GREEN(color) as c_double / 255.0).powf(gamma)) as c_int;
        b = (255.0 * (R_BLUE(color) as c_double / 255.0).powf(gamma)) as c_int;
    } else {
        r = R_RED(color);
        g = R_GREEN(color);
        b = R_BLUE(color);
    }
    rgb_pack(r, g, b)
}

fn imin2(a: c_int, b: c_int) -> c_int {
    if a < b {
        a
    } else {
        b
    }
}

fn imax2(a: c_int, b: c_int) -> c_int {
    if a > b {
        a
    } else {
        b
    }
}

/// Safe CStr pointer helper: returns "" if null
unsafe fn cstr_or_empty(p: *const c_char) -> *const c_char {
    if p.is_null() {
        b"\0".as_ptr() as *const c_char
    } else {
        p
    }
}

// ===========================================================================
// Font management
// ===========================================================================

unsafe fn RStandardFonts() {
    let arial = b"Arial\0";
    let symbol = b"Symbol\0";
    for i in 0..4 {
        ptr::copy_nonoverlapping(
            arial.as_ptr() as *const c_char,
            fontname[i as usize].as_mut_ptr(),
            arial.len(),
        );
    }
    ptr::copy_nonoverlapping(
        symbol.as_ptr() as *const c_char,
        fontname[4].as_mut_ptr(),
        symbol.len(),
    );
    fontstyle[0] = Plain;
    fontstyle[4] = Plain;
    fontstyle[1] = Bold;
    fontstyle[2] = Italic;
    fontstyle[3] = BoldItalic;
    fontnum = 5;
    fontinitdone = 2;
}

unsafe fn RFontInit() {
    // On non-Windows, just use standard fonts
    RStandardFonts();
}

/// Translate a font family name using the Windows font database.
/// On non-Windows, returns NULL (font name from fontname[] will be used).
unsafe fn translateFontFamily(_family: *const c_char) -> *mut c_char {
    ptr::null_mut()
}

// ===========================================================================
// Non-Windows stubs (always compiled, always exported)
// ===========================================================================

/// savePlot -- save the current Windows device plot to a file.
/// Stub: returns R_NilValue (no-op on non-Windows).
#[cfg(not(target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn savePlot(args: SEXP) -> SEXP {
    let _ = args;
    R_NilValue()
}

/// devga -- create a Windows GDI graphics device (windows() function).
/// Stub: returns R_NilValue (no-op on non-Windows).
#[cfg(not(target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devga(args: SEXP) -> SEXP {
    let _ = args;
    R_NilValue()
}

/// bmVersion -- return bitmap library version info (libpng, jpeg, libtiff).
/// Returns a named character vector of version strings.
#[cfg(not(target_os = "windows"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bmVersion() -> SEXP {
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0 /* STRSXP */, 3));
    let nms = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0 /* STRSXP */, 3));
    use crate::attrib_core::setAttrib;
    setAttrib(ans, R_NamesSymbol(), nms);
    SET_STRING_ELT(nms, 0, Rf_mkChar(b"libpng\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nms, 1, Rf_mkChar(b"jpeg\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nms, 2, Rf_mkChar(b"libtiff\0".as_ptr() as *const c_char));
    SET_STRING_ELT(ans, 0, Rf_mkChar(b"\0".as_ptr() as *const c_char));
    SET_STRING_ELT(ans, 1, Rf_mkChar(b"\0".as_ptr() as *const c_char));
    SET_STRING_ELT(ans, 2, Rf_mkChar(b"\0".as_ptr() as *const c_char));
    Rf_unprotect(2);
    ans
}

// ===========================================================================
// Windows-only: Cairo device functions
// On non-Windows these are provided by devcairo.rs instead.
// ===========================================================================

/// devCairo -- create a Cairo graphics device.
/// On Windows, this loads winCairo.dll. Stub: returns R_NilValue.
#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devCairo(args: SEXP) -> SEXP {
    let _ = args;
    R_NilValue()
}

/// cairoVersion -- return the Cairo library version string.
/// Stub: returns empty string.
#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cairoVersion() -> SEXP {
    Rf_mkString(b"\0".as_ptr() as *const c_char)
}

/// pangoVersion -- return the Pango library version string.
/// Stub: returns empty string.
#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pangoVersion() -> SEXP {
    Rf_mkString(b"\0".as_ptr() as *const c_char)
}

/// cairoFT -- return Cairo FreeType information.
/// Stub: returns empty string.
#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cairoFT() -> SEXP {
    Rf_mkString(b"\0".as_ptr() as *const c_char)
}

/// bmVersion -- return bitmap library version info (libpng, jpeg, libtiff).
/// Returns a named character vector of version strings.
#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bmVersion() -> SEXP {
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0 /* STRSXP */, 3));
    let nms = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0 /* STRSXP */, 3));
    use crate::attrib_core::setAttrib;
    use crate::sexp::globals::R_NamesSymbol;
    setAttrib(ans, R_NamesSymbol(), nms);
    SET_STRING_ELT(nms, 0, Rf_mkChar(b"libpng\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nms, 1, Rf_mkChar(b"jpeg\0".as_ptr() as *const c_char));
    SET_STRING_ELT(nms, 2, Rf_mkChar(b"libtiff\0".as_ptr() as *const c_char));
    SET_STRING_ELT(ans, 0, Rf_mkChar(b"\0".as_ptr() as *const c_char));
    SET_STRING_ELT(ans, 1, Rf_mkChar(b"\0".as_ptr() as *const c_char));
    SET_STRING_ELT(ans, 2, Rf_mkChar(b"\0".as_ptr() as *const c_char));
    Rf_unprotect(2);
    ans
}

// ===========================================================================
// Windows-only: savePlot and devga with real implementations
// ===========================================================================

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn savePlot(args: SEXP) -> SEXP {
    // Full implementation would parse args and call SaveAsPng/SaveAsBmp/etc.
    // For now, stub on Windows too since we lack full GDI support
    let _ = args;
    R_NilValue()
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devga(args: SEXP) -> SEXP {
    // Full implementation would:
    // 1. Parse all arguments from args (display, width, height, ps, etc.)
    // 2. Allocate a gadesc
    // 3. Call GADeviceDriver
    // 4. Create the device
    // For now, stub since we lack full GDI
    let _ = args;
    R_NilValue()
}

// ===========================================================================
// Windows real implementations (all GA_* callbacks)
// ===========================================================================

#[cfg(target_os = "windows")]
mod win_impl {
    use super::*;

    // --- Callbacks: all are no-ops since we lack Windows GDI ---

    pub unsafe extern "C" fn GA_Activate(dd: *mut c_void) {
        let _ = dd;
    }

    pub unsafe extern "C" fn GA_Circle(
        x: c_double,
        y: c_double,
        radius: c_double,
        gc: *const c_void,
        dd: *mut c_void,
    ) {
        let _ = (x, y, radius, gc, dd);
    }

    pub unsafe extern "C" fn GA_Clip(
        x0: c_double,
        x1: c_double,
        y0: c_double,
        y1: c_double,
        dd: *mut c_void,
    ) {
        let _ = (x0, x1, y0, y1, dd);
    }

    pub unsafe extern "C" fn GA_Close(dd: *mut c_void) {
        let _ = dd;
    }

    pub unsafe extern "C" fn GA_Deactivate(dd: *mut c_void) {
        let _ = dd;
    }

    pub unsafe extern "C" fn GA_eventHelper(dd: *mut c_void, code: c_int) {
        let _ = (dd, code);
    }

    pub unsafe extern "C" fn GA_Locator(
        x: *mut c_double,
        y: *mut c_double,
        dd: *mut c_void,
    ) -> c_int {
        let _ = (x, y, dd);
        0 // FALSE
    }

    pub unsafe extern "C" fn GA_Line(
        x1: c_double,
        y1: c_double,
        x2: c_double,
        y2: c_double,
        gc: *const c_void,
        dd: *mut c_void,
    ) {
        let _ = (x1, y1, x2, y2, gc, dd);
    }

    pub unsafe extern "C" fn GA_MetricInfo(
        c: c_int,
        gc: *const c_void,
        ascent: *mut c_double,
        descent: *mut c_double,
        width: *mut c_double,
        dd: *mut c_void,
    ) {
        let _ = (c, gc, ascent, descent, width, dd);
    }

    pub unsafe extern "C" fn GA_Mode(mode: c_int, dd: *mut c_void) {
        let _ = (mode, dd);
    }

    pub unsafe extern "C" fn GA_NewPage(gc: *const c_void, dd: *mut c_void) {
        let _ = (gc, dd);
    }

    pub unsafe extern "C" fn GA_Path(
        x: *mut c_double,
        y: *mut c_double,
        npoly: c_int,
        nper: *mut c_int,
        winding: c_int,
        gc: *const c_void,
        dd: *mut c_void,
    ) {
        let _ = (x, y, npoly, nper, winding, gc, dd);
    }

    pub unsafe extern "C" fn GA_Polygon(
        n: c_int,
        x: *mut c_double,
        y: *mut c_double,
        gc: *const c_void,
        dd: *mut c_void,
    ) {
        let _ = (n, x, y, gc, dd);
    }

    pub unsafe extern "C" fn GA_Polyline(
        n: c_int,
        x: *mut c_double,
        y: *mut c_double,
        gc: *const c_void,
        dd: *mut c_void,
    ) {
        let _ = (n, x, y, gc, dd);
    }

    pub unsafe extern "C" fn GA_Rect(
        x0: c_double,
        y0: c_double,
        x1: c_double,
        y1: c_double,
        gc: *const c_void,
        dd: *mut c_void,
    ) {
        let _ = (x0, y0, x1, y1, gc, dd);
    }

    pub unsafe extern "C" fn GA_Size(
        left: *mut c_double,
        right: *mut c_double,
        bottom: *mut c_double,
        top: *mut c_double,
        dd: *mut c_void,
    ) {
        let _ = (left, right, bottom, top, dd);
    }

    pub unsafe extern "C" fn GA_Resize(dd: *mut c_void) {
        let _ = dd;
    }

    pub unsafe extern "C" fn GA_Raster(
        raster: *mut c_uint,
        w: c_int,
        h: c_int,
        x: c_double,
        y: c_double,
        width: c_double,
        height: c_double,
        rot: c_double,
        interpolate: c_int,
        gc: *const c_void,
        dd: *mut c_void,
    ) {
        let _ = (raster, w, h, x, y, width, height, rot, interpolate, gc, dd);
    }

    pub unsafe extern "C" fn GA_Cap(dd: *mut c_void) -> SEXP {
        let _ = dd;
        R_NilValue()
    }

    pub unsafe extern "C" fn GA_StrWidth(
        str: *const c_char,
        gc: *const c_void,
        dd: *mut c_void,
    ) -> c_double {
        let _ = (str, gc, dd);
        0.0
    }

    pub unsafe extern "C" fn GA_Text(
        x: c_double,
        y: c_double,
        str: *const c_char,
        rot: c_double,
        hadj: c_double,
        gc: *const c_void,
        dd: *mut c_void,
    ) {
        let _ = (x, y, str, rot, hadj, gc, dd);
    }

    pub unsafe extern "C" fn GA_StrWidth_UTF8(
        str: *const c_char,
        gc: *const c_void,
        dd: *mut c_void,
    ) -> c_double {
        let _ = (str, gc, dd);
        0.0
    }

    pub unsafe extern "C" fn GA_Text_UTF8(
        x: c_double,
        y: c_double,
        str: *const c_char,
        rot: c_double,
        hadj: c_double,
        gc: *const c_void,
        dd: *mut c_void,
    ) {
        let _ = (x, y, str, rot, hadj, gc, dd);
    }

    pub unsafe extern "C" fn GA_NewFrameConfirm(dev: *mut c_void) -> c_int {
        let _ = dev;
        1 // TRUE
    }

    pub unsafe extern "C" fn GA_setPattern(pattern: SEXP, dd: *mut c_void) -> SEXP {
        let _ = (pattern, dd);
        R_NilValue()
    }

    pub unsafe extern "C" fn GA_releasePattern(ref_: SEXP, dd: *mut c_void) {
        let _ = (ref_, dd);
    }

    pub unsafe extern "C" fn GA_setClipPath(path: SEXP, ref_: SEXP, dd: *mut c_void) -> SEXP {
        let _ = (path, ref_, dd);
        R_NilValue()
    }

    pub unsafe extern "C" fn GA_releaseClipPath(ref_: SEXP, dd: *mut c_void) {
        let _ = (ref_, dd);
    }

    pub unsafe extern "C" fn GA_setMask(path: SEXP, ref_: SEXP, dd: *mut c_void) -> SEXP {
        let _ = (path, ref_, dd);
        R_NilValue()
    }

    pub unsafe extern "C" fn GA_releaseMask(ref_: SEXP, dd: *mut c_void) {
        let _ = (ref_, dd);
    }

    pub unsafe extern "C" fn GA_holdflush(dd: *mut c_void, level: c_int) -> c_int {
        let _ = (dd, level);
        0
    }

    pub unsafe extern "C" fn GA_onExit(dd: *mut c_void) {
        let _ = dd;
    }

    // --- Save functions ---

    pub unsafe fn SaveAsPng(dd: *mut c_void, fn_: *const c_char) {
        let _ = (dd, fn_);
    }

    pub unsafe fn SaveAsJpeg(dd: *mut c_void, quality: c_int, fn_: *const c_char) {
        let _ = (dd, quality, fn_);
    }

    pub unsafe fn SaveAsBmp(dd: *mut c_void, fn_: *const c_char) {
        let _ = (dd, fn_);
    }

    pub unsafe fn SaveAsTiff(dd: *mut c_void, fn_: *const c_char) {
        let _ = (dd, fn_);
    }

    pub unsafe fn SaveAsBitmap(dd: *mut c_void, res: c_int) {
        let _ = (dd, res);
    }

    pub unsafe fn SaveAsWin(dd: *mut c_void, display: *const c_char, restoreConsole: c_int) {
        let _ = (dd, display, restoreConsole);
    }

    pub unsafe fn SaveAsPostscript(dd: *mut c_void, fn_: *const c_char) {
        let _ = (dd, fn_);
    }

    pub unsafe fn SaveAsPDF(dd: *mut c_void, fn_: *const c_char) {
        let _ = (dd, fn_);
    }

    // --- Raster helpers ---

    pub unsafe fn doRaster(
        raster: *mut c_uint,
        x: c_int,
        y: c_int,
        w: c_int,
        h: c_int,
        rot: c_double,
        dd: *mut c_void,
    ) {
        let _ = (raster, x, y, w, h, rot, dd);
    }

    pub unsafe fn flipRaster(
        rasterImage: *mut c_uint,
        imageWidth: c_int,
        imageHeight: c_int,
        invertX: c_int,
        invertY: c_int,
        flippedRaster: *mut c_uint,
    ) {
        let _ = (
            rasterImage,
            imageWidth,
            imageHeight,
            invertX,
            invertY,
            flippedRaster,
        );
    }

    // --- PrivateCopyDevice ---

    pub unsafe fn PrivateCopyDevice(dd: *mut c_void, ndd: *mut c_void, name: *const c_char) {
        let _ = (dd, ndd, name);
    }

    // --- Plot history ---

    pub unsafe fn NewPlotHistory(n: c_int) -> SEXP {
        let _ = n;
        R_NilValue()
    }

    pub unsafe fn GrowthPlotHistory(vdl: SEXP) -> SEXP {
        let _ = vdl;
        R_NilValue()
    }

    pub unsafe fn AddtoPlotHistory(snapshot: SEXP, replace: c_int) {
        let _ = (snapshot, replace);
    }

    pub unsafe fn Replay(dd: *mut c_void, vdl: SEXP) {
        let _ = (dd, vdl);
    }

    // --- Menu callbacks ---

    pub unsafe fn menustop(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menunextplot(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menufilebitmap(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menups(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menupdf(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menuwm(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menuclpwm(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menuclpbm(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menustayontop(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menuprint(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menuclose(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn grpopupact(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menurec(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menuadd(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menureplace(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menunext(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menuprev(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menugrclear(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menugvar(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menusvar(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menuconsole(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menuR(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menufit(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn menufix(m: *mut c_void) {
        let _ = m;
    }

    pub unsafe fn mbarf(m: *mut c_void) {
        let _ = m;
    }

    // --- Window callbacks ---

    pub unsafe fn HelpResize(w: *mut c_void, r: ClipRect) {
        let _ = (w, r);
    }

    pub unsafe fn HelpClose(w: *mut c_void) {
        let _ = w;
    }

    pub unsafe fn HelpExpose(w: *mut c_void, r: ClipRect) {
        let _ = (w, r);
    }

    pub unsafe fn HelpMouseClick(w: *mut c_void, button: c_int, pt_x: c_int, pt_y: c_int) {
        let _ = (w, button, pt_x, pt_y);
    }

    pub unsafe fn HelpMouseMove(w: *mut c_void, button: c_int, pt_x: c_int, pt_y: c_int) {
        let _ = (w, button, pt_x, pt_y);
    }

    pub unsafe fn HelpMouseUp(w: *mut c_void, button: c_int, pt_x: c_int, pt_y: c_int) {
        let _ = (w, button, pt_x, pt_y);
    }

    pub unsafe fn CHelpKeyIn(w: *mut c_void, key: c_int) {
        let _ = (w, key);
    }

    pub unsafe fn NHelpKeyIn(w: *mut c_void, key: c_int) {
        let _ = (w, key);
    }

    pub unsafe fn devga_sbf(c: *mut c_void, pos: c_int) {
        let _ = (c, pos);
    }

    // --- SetColor / SetFont / SetLineStyle ---

    pub unsafe fn SetColor(color: c_int, gamma: c_double, xd: *mut gadesc) {
        if !xd.is_null() && (*xd).col != color {
            (*xd).col = color;
            (*xd).fgcolor = GArgb(color, gamma);
        }
    }

    pub unsafe fn SetFont(gc: *const c_void, rot: c_double, xd: *mut gadesc) {
        let _ = (gc, rot, xd);
    }

    pub unsafe fn SetLineStyle(gc: *const c_void, dd: *mut c_void) {
        let _ = (gc, dd);
    }

    // --- Device pixel dimensions ---

    pub unsafe fn pixelWidth(obj: *mut c_void) -> c_double {
        let _ = obj;
        1.0 / 96.0
    }

    pub unsafe fn pixelHeight(obj: *mut c_void) -> c_double {
        let _ = obj;
        1.0 / 96.0
    }

    // --- getClipRect helper ---

    pub unsafe fn getClipRect(xd: *mut gadesc) -> ClipRect {
        if xd.is_null() {
            return ClipRect::default();
        }
        (*xd).clip
    }

    // --- getregion helper ---

    pub unsafe fn getregion(xd: *mut gadesc) -> ClipRect {
        if xd.is_null() {
            return ClipRect::default();
        }
        ClipRect {
            x: 0,
            y: 0,
            width: (*xd).showWidth,
            height: (*xd).showHeight,
        }
    }

    // --- drawbits / timer helpers ---

    pub unsafe fn drawbits(xd: *mut gadesc) {
        let _ = xd;
    }

    pub unsafe fn GA_Timer(xd: *mut gadesc) {
        let _ = xd;
    }

    // --- GADeviceDriver ---

    pub unsafe fn GADeviceDriver(
        dd: *mut c_void,
        display: *const c_char,
        width: c_double,
        height: c_double,
        pointsize: c_double,
        recording: c_int,
        resize: c_int,
        bg: c_int,
        canvas: c_int,
        gamma: c_double,
        xpos: c_int,
        ypos: c_int,
        buffered: c_int,
        psenv: SEXP,
        restoreConsole: c_int,
        title: *const c_char,
        clickToConfirm: c_int,
        fillOddEven: c_int,
        family: *const c_char,
        quality: c_int,
        xpinch: c_double,
        ypinch: c_double,
    ) -> c_int {
        let _ = (
            dd,
            display,
            width,
            height,
            pointsize,
            recording,
            resize,
            bg,
            canvas,
            gamma,
            xpos,
            ypos,
            buffered,
            psenv,
            restoreConsole,
            title,
            clickToConfirm,
            fillOddEven,
            family,
            quality,
            xpinch,
            ypinch,
        );
        0 // FALSE
    }

    // --- deleteGraphMenus ---

    pub unsafe fn deleteGraphMenus(devnum: c_int) {
        let _ = devnum;
    }

    // --- donelocator ---

    pub unsafe fn donelocator(data: *mut c_void) {
        let _ = data;
    }

    // --- privategetpixel2 ---

    pub unsafe fn privategetpixel2(d: *mut c_void, i: c_int, j: c_int) -> c_uint {
        let _ = (d, i, j);
        0
    }

    // --- getKeyName ---

    pub unsafe fn getKeyName(key: c_int) -> c_int {
        let _ = key;
        0
    }

    // --- err_cannot_open ---

    pub unsafe fn err_cannot_open(fn_: *const c_char) {
        let _ = fn_;
    }

    // --- init_PS_PDF ---

    pub unsafe fn init_PS_PDF() {
        // no-op stub
    }

    // --- Load_Rcairo_Dll ---

    pub unsafe fn Load_Rcairo_Dll() -> c_int {
        0 // FALSE
    }
}

// ===========================================================================
// Non-Windows: provide all internal functions as stubs too
// (so the module compiles on all platforms)
// ===========================================================================

#[cfg(not(target_os = "windows"))]
mod win_impl {
    use super::*;

    pub unsafe fn GA_Activate(_dd: *mut c_void) {}
    pub unsafe fn GA_Circle(
        _x: c_double,
        _y: c_double,
        _radius: c_double,
        _gc: *const c_void,
        _dd: *mut c_void,
    ) {
    }
    pub unsafe fn GA_Clip(
        _x0: c_double,
        _x1: c_double,
        _y0: c_double,
        _y1: c_double,
        _dd: *mut c_void,
    ) {
    }
    pub unsafe fn GA_Close(_dd: *mut c_void) {}
    pub unsafe fn GA_Deactivate(_dd: *mut c_void) {}
    pub unsafe fn GA_eventHelper(_dd: *mut c_void, _code: c_int) {}
    pub unsafe fn GA_Locator(_x: *mut c_double, _y: *mut c_double, _dd: *mut c_void) -> c_int {
        0
    }
    pub unsafe fn GA_Line(
        _x1: c_double,
        _y1: c_double,
        _x2: c_double,
        _y2: c_double,
        _gc: *const c_void,
        _dd: *mut c_void,
    ) {
    }
    pub unsafe fn GA_MetricInfo(
        _c: c_int,
        _gc: *const c_void,
        _ascent: *mut c_double,
        _descent: *mut c_double,
        _width: *mut c_double,
        _dd: *mut c_void,
    ) {
    }
    pub unsafe fn GA_Mode(_mode: c_int, _dd: *mut c_void) {}
    pub unsafe fn GA_NewPage(_gc: *const c_void, _dd: *mut c_void) {}
    pub unsafe fn GA_Path(
        _x: *mut c_double,
        _y: *mut c_double,
        _npoly: c_int,
        _nper: *mut c_int,
        _winding: c_int,
        _gc: *const c_void,
        _dd: *mut c_void,
    ) {
    }
    pub unsafe fn GA_Polygon(
        _n: c_int,
        _x: *mut c_double,
        _y: *mut c_double,
        _gc: *const c_void,
        _dd: *mut c_void,
    ) {
    }
    pub unsafe fn GA_Polyline(
        _n: c_int,
        _x: *mut c_double,
        _y: *mut c_double,
        _gc: *const c_void,
        _dd: *mut c_void,
    ) {
    }
    pub unsafe fn GA_Rect(
        _x0: c_double,
        _y0: c_double,
        _x1: c_double,
        _y1: c_double,
        _gc: *const c_void,
        _dd: *mut c_void,
    ) {
    }
    pub unsafe fn GA_Size(
        _left: *mut c_double,
        _right: *mut c_double,
        _bottom: *mut c_double,
        _top: *mut c_double,
        _dd: *mut c_void,
    ) {
    }
    pub unsafe fn GA_Resize(_dd: *mut c_void) {}
    pub unsafe fn GA_Raster(
        _raster: *mut c_uint,
        _w: c_int,
        _h: c_int,
        _x: c_double,
        _y: c_double,
        _width: c_double,
        _height: c_double,
        _rot: c_double,
        _interpolate: c_int,
        _gc: *const c_void,
        _dd: *mut c_void,
    ) {
    }
    pub unsafe fn GA_Cap(_dd: *mut c_void) -> SEXP {
        R_NilValue()
    }
    pub unsafe fn GA_StrWidth(
        _str: *const c_char,
        _gc: *const c_void,
        _dd: *mut c_void,
    ) -> c_double {
        0.0
    }
    pub unsafe fn GA_Text(
        _x: c_double,
        _y: c_double,
        _str: *const c_char,
        _rot: c_double,
        _hadj: c_double,
        _gc: *const c_void,
        _dd: *mut c_void,
    ) {
    }
    pub unsafe fn GA_StrWidth_UTF8(
        _str: *const c_char,
        _gc: *const c_void,
        _dd: *mut c_void,
    ) -> c_double {
        0.0
    }
    pub unsafe fn GA_Text_UTF8(
        _x: c_double,
        _y: c_double,
        _str: *const c_char,
        _rot: c_double,
        _hadj: c_double,
        _gc: *const c_void,
        _dd: *mut c_void,
    ) {
    }
    pub unsafe fn GA_NewFrameConfirm(_dev: *mut c_void) -> c_int {
        1
    }
    pub unsafe fn GA_setPattern(_pattern: SEXP, _dd: *mut c_void) -> SEXP {
        R_NilValue()
    }
    pub unsafe fn GA_releasePattern(_ref: SEXP, _dd: *mut c_void) {}
    pub unsafe fn GA_setClipPath(_path: SEXP, _ref: SEXP, _dd: *mut c_void) -> SEXP {
        R_NilValue()
    }
    pub unsafe fn GA_releaseClipPath(_ref: SEXP, _dd: *mut c_void) {}
    pub unsafe fn GA_setMask(_path: SEXP, _ref: SEXP, _dd: *mut c_void) -> SEXP {
        R_NilValue()
    }
    pub unsafe fn GA_releaseMask(_ref: SEXP, _dd: *mut c_void) {}
    pub unsafe fn GA_holdflush(_dd: *mut c_void, _level: c_int) -> c_int {
        0
    }
    pub unsafe fn GA_onExit(_dd: *mut c_void) {}
    pub unsafe fn SaveAsPng(_dd: *mut c_void, _fn: *const c_char) {}
    pub unsafe fn SaveAsJpeg(_dd: *mut c_void, _quality: c_int, _fn: *const c_char) {}
    pub unsafe fn SaveAsBmp(_dd: *mut c_void, _fn: *const c_char) {}
    pub unsafe fn SaveAsTiff(_dd: *mut c_void, _fn: *const c_char) {}
    pub unsafe fn SaveAsBitmap(_dd: *mut c_void, _res: c_int) {}
    pub unsafe fn SaveAsWin(_dd: *mut c_void, _display: *const c_char, _restoreConsole: c_int) {}
    pub unsafe fn SaveAsPostscript(_dd: *mut c_void, _fn: *const c_char) {}
    pub unsafe fn SaveAsPDF(_dd: *mut c_void, _fn: *const c_char) {}
    pub unsafe fn doRaster(
        _raster: *mut c_uint,
        _x: c_int,
        _y: c_int,
        _w: c_int,
        _h: c_int,
        _rot: c_double,
        _dd: *mut c_void,
    ) {
    }
    pub unsafe fn flipRaster(
        _rasterImage: *mut c_uint,
        _imageWidth: c_int,
        _imageHeight: c_int,
        _invertX: c_int,
        _invertY: c_int,
        _flippedRaster: *mut c_uint,
    ) {
    }
    pub unsafe fn PrivateCopyDevice(_dd: *mut c_void, _ndd: *mut c_void, _name: *const c_char) {}
    pub unsafe fn NewPlotHistory(_n: c_int) -> SEXP {
        R_NilValue()
    }
    pub unsafe fn GrowthPlotHistory(_vdl: SEXP) -> SEXP {
        R_NilValue()
    }
    pub unsafe fn AddtoPlotHistory(_snapshot: SEXP, _replace: c_int) {}
    pub unsafe fn Replay(_dd: *mut c_void, _vdl: SEXP) {}
    pub unsafe fn menustop(_m: *mut c_void) {}
    pub unsafe fn menunextplot(_m: *mut c_void) {}
    pub unsafe fn menufilebitmap(_m: *mut c_void) {}
    pub unsafe fn menups(_m: *mut c_void) {}
    pub unsafe fn menupdf(_m: *mut c_void) {}
    pub unsafe fn menuwm(_m: *mut c_void) {}
    pub unsafe fn menuclpwm(_m: *mut c_void) {}
    pub unsafe fn menuclpbm(_m: *mut c_void) {}
    pub unsafe fn menustayontop(_m: *mut c_void) {}
    pub unsafe fn menuprint(_m: *mut c_void) {}
    pub unsafe fn menuclose(_m: *mut c_void) {}
    pub unsafe fn grpopupact(_m: *mut c_void) {}
    pub unsafe fn menurec(_m: *mut c_void) {}
    pub unsafe fn menuadd(_m: *mut c_void) {}
    pub unsafe fn menureplace(_m: *mut c_void) {}
    pub unsafe fn menunext(_m: *mut c_void) {}
    pub unsafe fn menuprev(_m: *mut c_void) {}
    pub unsafe fn menugrclear(_m: *mut c_void) {}
    pub unsafe fn menugvar(_m: *mut c_void) {}
    pub unsafe fn menusvar(_m: *mut c_void) {}
    pub unsafe fn menuconsole(_m: *mut c_void) {}
    pub unsafe fn menuR(_m: *mut c_void) {}
    pub unsafe fn menufit(_m: *mut c_void) {}
    pub unsafe fn menufix(_m: *mut c_void) {}
    pub unsafe fn mbarf(_m: *mut c_void) {}
    pub unsafe fn HelpResize(_w: *mut c_void, _r: ClipRect) {}
    pub unsafe fn HelpClose(_w: *mut c_void) {}
    pub unsafe fn HelpExpose(_w: *mut c_void, _r: ClipRect) {}
    pub unsafe fn HelpMouseClick(_w: *mut c_void, _button: c_int, _pt_x: c_int, _pt_y: c_int) {}
    pub unsafe fn HelpMouseMove(_w: *mut c_void, _button: c_int, _pt_x: c_int, _pt_y: c_int) {}
    pub unsafe fn HelpMouseUp(_w: *mut c_void, _button: c_int, _pt_x: c_int, _pt_y: c_int) {}
    pub unsafe fn CHelpKeyIn(_w: *mut c_void, _key: c_int) {}
    pub unsafe fn NHelpKeyIn(_w: *mut c_void, _key: c_int) {}
    pub unsafe fn devga_sbf(_c: *mut c_void, _pos: c_int) {}
    pub unsafe fn SetColor(_color: c_int, _gamma: c_double, _xd: *mut gadesc) {}
    pub unsafe fn SetFont(_gc: *const c_void, _rot: c_double, _xd: *mut gadesc) {}
    pub unsafe fn SetLineStyle(_gc: *const c_void, _dd: *mut c_void) {}
    pub unsafe fn pixelWidth(_obj: *mut c_void) -> c_double {
        1.0 / 96.0
    }
    pub unsafe fn pixelHeight(_obj: *mut c_void) -> c_double {
        1.0 / 96.0
    }
    pub unsafe fn getClipRect(_xd: *mut gadesc) -> ClipRect {
        ClipRect::default()
    }
    pub unsafe fn getregion(_xd: *mut gadesc) -> ClipRect {
        ClipRect::default()
    }
    pub unsafe fn drawbits(_xd: *mut gadesc) {}
    pub unsafe fn GA_Timer(_xd: *mut gadesc) {}
    pub unsafe fn GADeviceDriver(
        _dd: *mut c_void,
        _display: *const c_char,
        _width: c_double,
        _height: c_double,
        _pointsize: c_double,
        _recording: c_int,
        _resize: c_int,
        _bg: c_int,
        _canvas: c_int,
        _gamma: c_double,
        _xpos: c_int,
        _ypos: c_int,
        _buffered: c_int,
        _psenv: SEXP,
        _restoreConsole: c_int,
        _title: *const c_char,
        _clickToConfirm: c_int,
        _fillOddEven: c_int,
        _family: *const c_char,
        _quality: c_int,
        _xpinch: c_double,
        _ypinch: c_double,
    ) -> c_int {
        0
    }
    pub unsafe fn deleteGraphMenus(_devnum: c_int) {}
    pub unsafe fn donelocator(_data: *mut c_void) {}
    pub unsafe fn privategetpixel2(_d: *mut c_void, _i: c_int, _j: c_int) -> c_uint {
        0
    }
    pub unsafe fn getKeyName(_key: c_int) -> c_int {
        0
    }
    pub unsafe fn err_cannot_open(_fn: *const c_char) {}
    pub unsafe fn init_PS_PDF() {}
    pub unsafe fn Load_Rcairo_Dll() -> c_int {
        0
    }
}

// ===========================================================================
// Re-export the win_impl functions for use by other modules
// ===========================================================================

pub use win_impl::*;
