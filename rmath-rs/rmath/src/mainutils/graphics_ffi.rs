#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::{CStr, CString, c_void};
use std::os::raw::{c_char, c_double, c_int, c_uint};

use crate::mainutils::errors::{Rf_error, Rf_warning};
use crate::sexp::ffi::{FALSE, NA_INTEGER, NA_REAL, Rboolean, SEXP, TRUE};
use crate::sexp::globals::R_NilValue;

const CE_NATIVE: c_int = 0;
const CE_UTF8: c_int = 1;
const CE_SYMBOL: c_int = 5;

const GE_DEVICE: c_int = 0;
const GE_NDC: c_int = 1;
const GE_INCHES: c_int = 2;
const GE_CM: c_int = 3;

const GE_GLYPHS_VERSION: c_int = 16;
const GE_GROUP_VERSION: c_int = 15;

const R_TRANWHITE: c_uint = 0x00FF_FFFF;

pub type pGEcontext = *const R_GE_gcontext;
pub type pDevDesc = *mut DevDesc;
pub type pGEDevDesc = *mut GEDevDesc;

// These C ABI function pointers are part of R's device descriptor contract.
// The Rust callers below stay Rust ABI; only device callback slots use extern "C".
type VoidFn = Option<unsafe extern "C" fn()>;
type ClipFn = Option<unsafe extern "C" fn(c_double, c_double, c_double, c_double, pDevDesc)>;
type CircleFn = Option<unsafe extern "C" fn(c_double, c_double, c_double, pGEcontext, pDevDesc)>;
type LineFn =
    Option<unsafe extern "C" fn(c_double, c_double, c_double, c_double, pGEcontext, pDevDesc)>;
type MetricInfoFn = Option<
    unsafe extern "C" fn(c_int, pGEcontext, *mut c_double, *mut c_double, *mut c_double, pDevDesc),
>;
type ModeFn = Option<unsafe extern "C" fn(c_int, pDevDesc)>;
type NewPageFn = Option<unsafe extern "C" fn(pGEcontext, pDevDesc)>;
type PolygonFn =
    Option<unsafe extern "C" fn(c_int, *mut c_double, *mut c_double, pGEcontext, pDevDesc)>;
type RectFn =
    Option<unsafe extern "C" fn(c_double, c_double, c_double, c_double, pGEcontext, pDevDesc)>;
type PathFn = Option<
    unsafe extern "C" fn(
        *mut c_double,
        *mut c_double,
        c_int,
        *mut c_int,
        Rboolean,
        pGEcontext,
        pDevDesc,
    ),
>;
type RasterFn = Option<
    unsafe extern "C" fn(
        *mut c_uint,
        c_int,
        c_int,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        Rboolean,
        pGEcontext,
        pDevDesc,
    ),
>;
type StrWidthFn = Option<unsafe extern "C" fn(*const c_char, pGEcontext, pDevDesc) -> c_double>;
type TextFn = Option<
    unsafe extern "C" fn(
        c_double,
        c_double,
        *const c_char,
        c_double,
        c_double,
        pGEcontext,
        pDevDesc,
    ),
>;
type ReleaseHook = Option<unsafe extern "C" fn(SEXP, pDevDesc)>;
type SetPatternHook = Option<unsafe extern "C" fn(SEXP, pDevDesc) -> SEXP>;
type SetPathHook = Option<unsafe extern "C" fn(SEXP, SEXP, pDevDesc) -> SEXP>;
type DefineGroupHook = Option<unsafe extern "C" fn(SEXP, c_int, SEXP, pDevDesc) -> SEXP>;
type UseGroupHook = Option<unsafe extern "C" fn(SEXP, SEXP, pDevDesc)>;
type StrokeFn = Option<unsafe extern "C" fn(SEXP, pGEcontext, pDevDesc)>;
type FillFn = Option<unsafe extern "C" fn(SEXP, c_int, pGEcontext, pDevDesc)>;
type GlyphFn = Option<
    unsafe extern "C" fn(
        c_int,
        *mut c_int,
        *mut c_double,
        *mut c_double,
        SEXP,
        c_double,
        c_int,
        c_double,
        pDevDesc,
    ),
>;

#[repr(C)]
#[derive(Clone, Copy)]
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

#[repr(C)]
pub struct GEDevDesc {
    pub dev: pDevDesc,
    pub displayListOn: Rboolean,
    pub displayList: SEXP,
    pub DLlastElt: SEXP,
    pub savedSnapshot: SEXP,
    pub dirty: Rboolean,
    pub recordGraphics: Rboolean,
}

#[repr(C)]
pub struct DevDesc {
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
    pub canClip: Rboolean,
    pub canChangeGamma: Rboolean,
    pub canHAdj: c_int,
    pub startps: c_double,
    pub startcol: c_int,
    pub startfill: c_int,
    pub startlty: c_int,
    pub startfont: c_int,
    pub startgamma: c_double,
    pub deviceSpecific: *mut c_void,
    pub displayListOn: Rboolean,
    pub canGenMouseDown: Rboolean,
    pub canGenMouseMove: Rboolean,
    pub canGenMouseUp: Rboolean,
    pub canGenKeybd: Rboolean,
    pub canGenIdle: Rboolean,
    pub gettingEvent: Rboolean,
    pub activate: VoidFn,
    pub circle: CircleFn,
    pub clip: ClipFn,
    pub close: Option<unsafe extern "C" fn(pDevDesc)>,
    pub deactivate: Option<unsafe extern "C" fn(pDevDesc)>,
    pub locator: Option<unsafe extern "C" fn(*mut c_double, *mut c_double, pDevDesc) -> Rboolean>,
    pub line: LineFn,
    pub metricInfo: MetricInfoFn,
    pub mode: ModeFn,
    pub newPage: NewPageFn,
    pub polygon: PolygonFn,
    pub polyline: PolygonFn,
    pub rect: RectFn,
    pub path: PathFn,
    pub raster: RasterFn,
    pub cap: Option<unsafe extern "C" fn(pDevDesc) -> SEXP>,
    pub size: Option<
        unsafe extern "C" fn(*mut c_double, *mut c_double, *mut c_double, *mut c_double, pDevDesc),
    >,
    pub strWidth: StrWidthFn,
    pub text: TextFn,
    pub onExit: Option<unsafe extern "C" fn(pDevDesc)>,
    pub getEvent: Option<unsafe extern "C" fn(SEXP, *const c_char) -> SEXP>,
    pub newFrameConfirm: Option<unsafe extern "C" fn(pDevDesc) -> Rboolean>,
    pub hasTextUTF8: Rboolean,
    pub textUTF8: TextFn,
    pub strWidthUTF8: StrWidthFn,
    pub wantSymbolUTF8: Rboolean,
    pub useRotatedTextInContour: Rboolean,
    pub eventEnv: SEXP,
    pub eventHelper: Option<unsafe extern "C" fn(pDevDesc, c_int)>,
    pub holdflush: Option<unsafe extern "C" fn(pDevDesc, c_int) -> c_int>,
    pub haveTransparency: c_int,
    pub haveTransparentBg: c_int,
    pub haveRaster: c_int,
    pub haveCapture: c_int,
    pub haveLocator: c_int,
    pub setPattern: SetPatternHook,
    pub releasePattern: ReleaseHook,
    pub setClipPath: SetPathHook,
    pub releaseClipPath: ReleaseHook,
    pub setMask: SetPathHook,
    pub releaseMask: ReleaseHook,
    pub deviceVersion: c_int,
    pub deviceClip: Rboolean,
    pub defineGroup: DefineGroupHook,
    pub useGroup: UseGroupHook,
    pub releaseGroup: ReleaseHook,
    pub stroke: StrokeFn,
    pub fill: FillFn,
    pub fillStroke: FillFn,
    pub capabilities: Option<unsafe extern "C" fn(SEXP) -> SEXP>,
    pub glyph: GlyphFn,
    pub reserved: [c_char; 64],
}

#[inline]
const fn red(col: c_uint) -> c_int {
    (col & 0xFF) as c_int
}

#[inline]
const fn green(col: c_uint) -> c_int {
    ((col >> 8) & 0xFF) as c_int
}

#[inline]
const fn blue(col: c_uint) -> c_int {
    ((col >> 16) & 0xFF) as c_int
}

#[inline]
const fn alpha(col: c_uint) -> c_int {
    ((col >> 24) & 0xFF) as c_int
}

#[inline]
const fn rgba(r: c_int, g: c_int, b: c_int, a: c_int) -> c_uint {
    (r as c_uint & 0xFF)
        | ((g as c_uint & 0xFF) << 8)
        | ((b as c_uint & 0xFF) << 16)
        | ((a as c_uint & 0xFF) << 24)
}

#[inline]
unsafe fn with_dev(dd: pGEDevDesc) -> Option<pDevDesc> {
    unsafe {
        if dd.is_null() {
            None
        } else {
            let dev = (*dd).dev;
            (!dev.is_null()).then_some(dev)
        }
    }
}

#[inline]
fn cstring_message(message: String) -> CString {
    CString::new(message).expect("formatted graphics message contains no NUL")
}

#[inline]
unsafe fn ge_width_for_line(
    line: *const c_char,
    gc: pGEcontext,
    dd: pGEDevDesc,
    utf8: bool,
) -> c_double {
    unsafe {
        let Some(dev) = with_dev(dd) else {
            return 0.0;
        };
        if utf8 {
            if let Some(width) = (*dev).strWidthUTF8 {
                return width(line, gc, dev);
            }
        }
        if let Some(width) = (*dev).strWidth {
            return width(line, gc, dev);
        }
        0.0
    }
}

unsafe fn ge_max_line_width(
    str_: *const c_char,
    gc: pGEcontext,
    dd: pGEDevDesc,
    utf8: bool,
) -> c_double {
    unsafe {
        if str_.is_null() || *str_ == 0 {
            return 0.0;
        }

        let bytes = CStr::from_ptr(str_).to_bytes();
        let mut width = 0.0;
        for line in bytes.split(|b| *b == b'\n') {
            let mut buf = Vec::with_capacity(line.len() + 1);
            buf.extend_from_slice(line);
            buf.push(0);
            let w = ge_width_for_line(buf.as_ptr().cast::<c_char>(), gc, dd, utf8);
            if w > width {
                width = w;
            }
        }
        width
    }
}

#[inline]
unsafe fn warning_message(message: String) {
    unsafe {
        let msg = cstring_message(message);
        Rf_warning(msg.as_ptr());
    }
}

#[inline]
unsafe fn error_message(message: String) -> ! {
    unsafe {
        let msg = cstring_message(message);
        Rf_error(msg.as_ptr());
        unreachable!("Rf_error does not return");
    }
}

#[inline]
unsafe fn ge_device_version(dd: pGEDevDesc) -> c_int {
    unsafe { with_dev(dd).map_or(0, |dev| (*dev).deviceVersion) }
}

pub(crate) unsafe fn rmath_grid_release_pattern(dd: pGEDevDesc, ref_: SEXP) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(release) = (*dev).releasePattern {
                release(ref_, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_grid_release_clip_path(dd: pGEDevDesc, ref_: SEXP) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(release) = (*dev).releaseClipPath {
                release(ref_, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_grid_release_mask(dd: pGEDevDesc, ref_: SEXP) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(release) = (*dev).releaseMask {
                release(ref_, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_grid_release_group(dd: pGEDevDesc, ref_: SEXP) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(release) = (*dev).releaseGroup {
                release(ref_, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_grid_release_definitions(dd: pGEDevDesc, clear_groups: c_int) {
    unsafe {
        rmath_grid_release_pattern(dd, R_NilValue());
        rmath_grid_release_clip_path(dd, R_NilValue());
        rmath_grid_release_mask(dd, R_NilValue());

        if clear_groups != 0 && ge_device_version(dd) > GE_GROUP_VERSION {
            rmath_grid_release_group(dd, R_NilValue());
        }
    }
}

pub(crate) unsafe fn rmath_ge_set_clip(
    x1: c_double,
    y1: c_double,
    x2: c_double,
    y2: c_double,
    dd: pGEDevDesc,
) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(clip) = (*dev).clip {
                clip(x1, x2, y1, y2, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_line(
    x1: c_double,
    y1: c_double,
    x2: c_double,
    y2: c_double,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(line) = (*dev).line {
                line(x1, y1, x2, y2, gc, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_polyline(
    n: c_int,
    x: *mut c_double,
    y: *mut c_double,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(polyline) = (*dev).polyline {
                polyline(n, x, y, gc, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_polygon(
    n: c_int,
    x: *mut c_double,
    y: *mut c_double,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(polygon) = (*dev).polygon {
                polygon(n, x, y, gc, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_circle(
    x: c_double,
    y: c_double,
    radius: c_double,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(circle) = (*dev).circle {
                circle(x, y, radius, gc, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_rect(
    x0: c_double,
    y0: c_double,
    x1: c_double,
    y1: c_double,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(rect) = (*dev).rect {
                rect(x0, y0, x1, y1, gc, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_path(
    x: *mut c_double,
    y: *mut c_double,
    npoly: c_int,
    nper: *mut c_int,
    winding: c_int,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(path) = (*dev).path {
                path(
                    x,
                    y,
                    npoly,
                    nper,
                    if winding != 0 { TRUE } else { FALSE },
                    gc,
                    dev,
                );
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_raster(
    raster: *mut c_uint,
    w: c_int,
    h: c_int,
    x: c_double,
    y: c_double,
    width: c_double,
    height: c_double,
    angle: c_double,
    interpolate: c_int,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(draw_raster) = (*dev).raster {
                draw_raster(
                    raster,
                    w,
                    h,
                    x,
                    y,
                    width,
                    height,
                    angle,
                    if interpolate != 0 { TRUE } else { FALSE },
                    gc,
                    dev,
                );
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_text(
    x: c_double,
    y: c_double,
    str_: *const c_char,
    rot: c_double,
    hadj: c_double,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(text) = (*dev).text {
                text(x, y, str_, rot, hadj, gc, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_text_with_encoding(
    x: c_double,
    y: c_double,
    str_: *const c_char,
    enc: c_int,
    rot: c_double,
    hadj: c_double,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if (*dev).hasTextUTF8 == TRUE && (*dev).textUTF8.is_some() && enc != CE_NATIVE {
                if let Some(text) = (*dev).textUTF8 {
                    text(x, y, str_, rot, hadj, gc, dev);
                }
                return;
            }
            if let Some(text) = (*dev).text {
                text(x, y, str_, rot, hadj, gc, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_mode(mode: c_int, dd: pGEDevDesc) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(mode_fn) = (*dev).mode {
                mode_fn(mode, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_new_page(gc: pGEcontext, dd: pGEDevDesc) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(new_page) = (*dev).newPage {
                new_page(gc, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_stroke(path: SEXP, gc: pGEcontext, dd: pGEDevDesc) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(stroke) = (*dev).stroke {
                stroke(path, gc, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_fill(path: SEXP, rule: c_int, gc: pGEcontext, dd: pGEDevDesc) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(fill) = (*dev).fill {
                fill(path, rule, gc, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_fill_stroke(path: SEXP, rule: c_int, gc: pGEcontext, dd: pGEDevDesc) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if let Some(fill_stroke) = (*dev).fillStroke {
                fill_stroke(path, rule, gc, dev);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_device_dirty(dd: pGEDevDesc) -> c_int {
    unsafe { if dd.is_null() { FALSE } else { (*dd).dirty } }
}

pub(crate) unsafe fn rmath_ge_mark_dirty(dd: pGEDevDesc) {
    unsafe {
        if !dd.is_null() {
            (*dd).dirty = TRUE;
        }
    }
}

pub(crate) unsafe fn rmath_ge_mark_clean(dd: pGEDevDesc) {
    unsafe {
        if !dd.is_null() {
            (*dd).dirty = FALSE;
        }
    }
}

pub(crate) unsafe fn rmath_ge_recording(dd: pGEDevDesc) -> c_int {
    unsafe {
        if dd.is_null() {
            FALSE
        } else {
            (*dd).recordGraphics
        }
    }
}

pub(crate) unsafe fn rmath_ge_set_recording(dd: pGEDevDesc, value: c_int) {
    unsafe {
        if !dd.is_null() {
            (*dd).recordGraphics = if value != 0 { TRUE } else { FALSE };
        }
    }
}

pub(crate) unsafe fn rmath_ge_device_left(dd: pGEDevDesc) -> c_double {
    unsafe { with_dev(dd).map_or(0.0, |dev| (*dev).left) }
}

pub(crate) unsafe fn rmath_ge_device_right(dd: pGEDevDesc) -> c_double {
    unsafe { with_dev(dd).map_or(0.0, |dev| (*dev).right) }
}

pub(crate) unsafe fn rmath_ge_device_bottom(dd: pGEDevDesc) -> c_double {
    unsafe { with_dev(dd).map_or(0.0, |dev| (*dev).bottom) }
}

pub(crate) unsafe fn rmath_ge_device_top(dd: pGEDevDesc) -> c_double {
    unsafe { with_dev(dd).map_or(0.0, |dev| (*dev).top) }
}

pub(crate) unsafe fn rmath_ge_device_ipr_x(dd: pGEDevDesc) -> c_double {
    unsafe { with_dev(dd).map_or(0.0, |dev| (*dev).ipr[0]) }
}

pub(crate) unsafe fn rmath_ge_device_ipr_y(dd: pGEDevDesc) -> c_double {
    unsafe { with_dev(dd).map_or(0.0, |dev| (*dev).ipr[1]) }
}

pub(crate) unsafe fn rmath_ge_device_cra_y(dd: pGEDevDesc) -> c_double {
    unsafe { with_dev(dd).map_or(0.0, |dev| (*dev).cra[1]) }
}

pub(crate) unsafe fn rmath_ge_device_startps(dd: pGEDevDesc) -> c_double {
    unsafe { with_dev(dd).map_or(1.0, |dev| (*dev).startps) }
}

pub(crate) unsafe fn rmath_ge_device_has_text_utf8(dd: pGEDevDesc) -> c_int {
    unsafe { with_dev(dd).map_or(FALSE, |dev| (*dev).hasTextUTF8) }
}

pub(crate) unsafe fn rmath_ge_device_want_symbol_utf8(dd: pGEDevDesc) -> c_int {
    unsafe { with_dev(dd).map_or(FALSE, |dev| (*dev).wantSymbolUTF8) }
}

pub(crate) unsafe fn rmath_ge_device_version(dd: pGEDevDesc) -> c_int {
    unsafe { ge_device_version(dd) }
}

pub(crate) unsafe fn rmath_ge_gc_fontface(gc: pGEcontext) -> c_int {
    unsafe { if gc.is_null() { 0 } else { (*gc).fontface } }
}

pub(crate) unsafe fn rmath_ge_gc_cex(gc: pGEcontext) -> c_double {
    unsafe { if gc.is_null() { 1.0 } else { (*gc).cex } }
}

pub(crate) unsafe fn rmath_ge_gc_ps(gc: pGEcontext) -> c_double {
    unsafe { if gc.is_null() { 12.0 } else { (*gc).ps } }
}

pub(crate) unsafe fn rmath_ge_gc_lineheight(gc: pGEcontext) -> c_double {
    unsafe { if gc.is_null() { 1.0 } else { (*gc).lineheight } }
}

pub(crate) unsafe fn rmath_ge_gc_fontfamily(gc: pGEcontext) -> *const c_char {
    unsafe {
        if gc.is_null() {
            std::ptr::null()
        } else {
            (*gc).fontfamily.as_ptr()
        }
    }
}

pub(crate) unsafe fn rmath_ge_from_device_x(
    value: c_double,
    to: c_int,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        let Some(dev) = with_dev(dd) else {
            return value;
        };
        let mut result = value;
        match to {
            GE_DEVICE => {}
            GE_NDC => {
                result = (result - (*dev).left) / ((*dev).right - (*dev).left);
            }
            GE_INCHES => {
                result = (result - (*dev).left) / ((*dev).right - (*dev).left)
                    * ((*dev).right - (*dev).left).abs()
                    * (*dev).ipr[0];
            }
            GE_CM => {
                result = (result - (*dev).left) / ((*dev).right - (*dev).left)
                    * ((*dev).right - (*dev).left).abs()
                    * (*dev).ipr[0]
                    * 2.54;
            }
            _ => {}
        }
        result
    }
}

pub(crate) unsafe fn rmath_ge_to_device_x(
    value: c_double,
    from: c_int,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        let Some(dev) = with_dev(dd) else {
            return value;
        };
        let mut result = value;
        match from {
            GE_CM => {
                result /= 2.54;
                result = (result / (*dev).ipr[0]) / ((*dev).right - (*dev).left).abs();
                result = (*dev).left + result * ((*dev).right - (*dev).left);
            }
            GE_INCHES => {
                result = (result / (*dev).ipr[0]) / ((*dev).right - (*dev).left).abs();
                result = (*dev).left + result * ((*dev).right - (*dev).left);
            }
            GE_NDC => {
                result = (*dev).left + result * ((*dev).right - (*dev).left);
            }
            _ => {}
        }
        result
    }
}

pub(crate) unsafe fn rmath_ge_from_device_y(
    value: c_double,
    to: c_int,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        let Some(dev) = with_dev(dd) else {
            return value;
        };
        let mut result = value;
        match to {
            GE_DEVICE => {}
            GE_NDC => {
                result = (result - (*dev).bottom) / ((*dev).top - (*dev).bottom);
            }
            GE_INCHES => {
                result = (result - (*dev).bottom) / ((*dev).top - (*dev).bottom)
                    * ((*dev).top - (*dev).bottom).abs()
                    * (*dev).ipr[1];
            }
            GE_CM => {
                result = (result - (*dev).bottom) / ((*dev).top - (*dev).bottom)
                    * ((*dev).top - (*dev).bottom).abs()
                    * (*dev).ipr[1]
                    * 2.54;
            }
            _ => {}
        }
        result
    }
}

pub(crate) unsafe fn rmath_ge_to_device_y(
    value: c_double,
    from: c_int,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        let Some(dev) = with_dev(dd) else {
            return value;
        };
        let mut result = value;
        match from {
            GE_CM => {
                result /= 2.54;
                result = (result / (*dev).ipr[1]) / ((*dev).top - (*dev).bottom).abs();
                result = (*dev).bottom + result * ((*dev).top - (*dev).bottom);
            }
            GE_INCHES => {
                result = (result / (*dev).ipr[1]) / ((*dev).top - (*dev).bottom).abs();
                result = (*dev).bottom + result * ((*dev).top - (*dev).bottom);
            }
            GE_NDC => {
                result = (*dev).bottom + result * ((*dev).top - (*dev).bottom);
            }
            _ => {}
        }
        result
    }
}

pub(crate) unsafe fn rmath_ge_from_device_width(
    value: c_double,
    to: c_int,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        let Some(dev) = with_dev(dd) else {
            return value;
        };
        let mut result = value;
        match to {
            GE_DEVICE => {}
            GE_NDC => result /= (*dev).right - (*dev).left,
            GE_INCHES => result *= (*dev).ipr[0],
            GE_CM => result *= (*dev).ipr[0] * 2.54,
            _ => {}
        }
        result
    }
}

pub(crate) unsafe fn rmath_ge_to_device_width(
    value: c_double,
    from: c_int,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        let Some(dev) = with_dev(dd) else {
            return value;
        };
        let mut result = value;
        match from {
            GE_CM => {
                result /= 2.54;
                result = (result / (*dev).ipr[0]) / ((*dev).right - (*dev).left).abs();
                result *= (*dev).right - (*dev).left;
            }
            GE_INCHES => {
                result = (result / (*dev).ipr[0]) / ((*dev).right - (*dev).left).abs();
                result *= (*dev).right - (*dev).left;
            }
            GE_NDC => result *= (*dev).right - (*dev).left,
            _ => {}
        }
        result
    }
}

pub(crate) unsafe fn rmath_ge_from_device_height(
    value: c_double,
    to: c_int,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        let Some(dev) = with_dev(dd) else {
            return value;
        };
        let mut result = value;
        match to {
            GE_DEVICE => {}
            GE_NDC => result /= (*dev).top - (*dev).bottom,
            GE_INCHES => result *= (*dev).ipr[1],
            GE_CM => result *= (*dev).ipr[1] * 2.54,
            _ => {}
        }
        result
    }
}

pub(crate) unsafe fn rmath_ge_to_device_height(
    value: c_double,
    from: c_int,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        let Some(dev) = with_dev(dd) else {
            return value;
        };
        let mut result = value;
        match from {
            GE_CM => {
                result /= 2.54;
                result = (result / (*dev).ipr[1]) / ((*dev).top - (*dev).bottom).abs();
                result *= (*dev).top - (*dev).bottom;
            }
            GE_INCHES => {
                result = (result / (*dev).ipr[1]) / ((*dev).top - (*dev).bottom).abs();
                result *= (*dev).top - (*dev).bottom;
            }
            GE_NDC => result *= (*dev).top - (*dev).bottom,
            _ => {}
        }
        result
    }
}

pub(crate) unsafe fn rmath_ge_metric_info(
    c: c_int,
    gc: pGEcontext,
    ascent: *mut c_double,
    descent: *mut c_double,
    width: *mut c_double,
    dd: pGEDevDesc,
) {
    unsafe {
        if !ascent.is_null() {
            *ascent = 0.0;
        }
        if !descent.is_null() {
            *descent = 0.0;
        }
        if !width.is_null() {
            *width = 0.0;
        }
        let Some(dev) = with_dev(dd) else {
            return;
        };
        if let Some(metric_info) = (*dev).metricInfo {
            metric_info(c, gc, ascent, descent, width, dev);
        }
    }
}

pub(crate) unsafe fn rmath_ge_str_width(
    str_: *const c_char,
    _enc: c_int,
    gc: pGEcontext,
    dd: pGEDevDesc,
) -> c_double {
    unsafe { ge_max_line_width(str_, gc, dd, false) }
}

pub(crate) unsafe fn rmath_ge_str_width_utf8(
    str_: *const c_char,
    gc: pGEcontext,
    dd: pGEDevDesc,
) -> c_double {
    unsafe { ge_max_line_width(str_, gc, dd, true) }
}

pub(crate) unsafe fn rmath_ge_str_height(
    str_: *const c_char,
    _enc: c_int,
    gc: pGEcontext,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        if str_.is_null() || *str_ == 0 {
            return 0.0;
        }
        let bytes = CStr::from_ptr(str_).to_bytes();
        let n = bytes.iter().filter(|b| **b == b'\n').count() as c_int;

        let mut asc = 0.0;
        let mut dsc = 0.0;
        let mut wid = 0.0;
        rmath_ge_metric_info('M' as c_int, gc, &mut asc, &mut dsc, &mut wid, dd);

        let mut lineheight = 1.0;
        if !gc.is_null() {
            if let Some(dev) = with_dev(dd) {
                if (*dev).startps != 0.0 {
                    lineheight =
                        (*gc).lineheight * (*gc).cex * (*dev).cra[1] * (*gc).ps / (*dev).startps;
                }
            }
        }
        if asc == 0.0 && dsc == 0.0 && wid == 0.0 {
            asc = lineheight;
        }
        n as c_double * lineheight + asc
    }
}

pub(crate) unsafe fn rmath_ge_str_metric(
    str_: *const c_char,
    enc: c_int,
    gc: pGEcontext,
    ascent: *mut c_double,
    descent: *mut c_double,
    width: *mut c_double,
    dd: pGEDevDesc,
) {
    unsafe {
        if !ascent.is_null() {
            *ascent = 0.0;
        }
        if !descent.is_null() {
            *descent = 0.0;
        }
        if !width.is_null() {
            *width = 0.0;
        }
        if str_.is_null() || *str_ == 0 {
            return;
        }

        let mut asc = 0.0;
        let mut dsc = 0.0;
        let mut wid = 0.0;
        rmath_ge_metric_info('M' as c_int, gc, &mut asc, &mut dsc, &mut wid, dd);
        let lineheight = rmath_ge_str_height(str_, enc, gc, dd);

        if !ascent.is_null() {
            *ascent = asc + (lineheight - asc);
        }
        if !descent.is_null() {
            *descent = dsc;
        }
        if !width.is_null() {
            *width = rmath_ge_str_width(str_, enc, gc, dd);
        }
    }
}

pub(crate) unsafe fn rmath_ge_symbol(
    x: c_double,
    y: c_double,
    pch: c_int,
    size: c_double,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    unsafe {
        let maxchar = if !gc.is_null() && (*gc).fontface != 5 {
            127
        } else {
            255
        };
        let mut use_gc = gc;
        let mut mutable_gc = if gc.is_null() {
            R_GE_gcontext {
                col: 0,
                fill: 0,
                gamma: 0.0,
                lwd: 0.0,
                lty: 0,
                lend: 0,
                ljoin: 0,
                lmitre: 0.0,
                cex: 1.0,
                ps: 12.0,
                lineheight: 1.0,
                fontface: 1,
                fontfamily: [0; 201],
                patternFill: R_NilValue(),
            }
        } else {
            *gc
        };

        if pch == NA_INTEGER {
            return;
        } else if pch < 0 {
            if !gc.is_null() && (*gc).fontface == 5 {
                error_message("use of negative pch with symbol font is invalid".to_string());
            }
            let ch = char::from_u32((-pch) as u32).unwrap_or('\u{FFFD}');
            let mut utf8 = [0_u8; 4];
            let encoded = ch.encode_utf8(&mut utf8);
            let str_ =
                CString::new(encoded.as_bytes()).expect("single UTF-8 codepoint contains no NUL");
            rmath_ge_text_with_encoding(x, y, str_.as_ptr(), CE_UTF8, 0.0, NA_REAL, gc, dd);
        } else if (' ' as c_int) <= pch && pch <= maxchar {
            if pch == '.' as c_int {
                if !gc.is_null() {
                    mutable_gc.fill = mutable_gc.col;
                    mutable_gc.col = R_TRANWHITE as c_int;
                    use_gc = &mutable_gc;
                }
                let mut xc = size * rmath_ge_to_device_width(0.005, GE_INCHES, dd).abs();
                let mut yc = size * rmath_ge_to_device_height(0.005, GE_INCHES, dd).abs();
                if size > 0.0 && xc < 0.5 {
                    xc = 0.5;
                }
                if size > 0.0 && yc < 0.5 {
                    yc = 0.5;
                }
                rmath_ge_rect(x - xc, y - yc, x + xc, y + yc, use_gc, dd);
            } else {
                let str_ = [pch as u8, 0];
                rmath_ge_text_with_encoding(
                    x,
                    y,
                    str_.as_ptr().cast::<c_char>(),
                    if !gc.is_null() && (*gc).fontface == 5 {
                        CE_SYMBOL
                    } else {
                        CE_NATIVE
                    },
                    0.0,
                    NA_REAL,
                    gc,
                    dd,
                );
            }
        } else if pch > maxchar {
            warning_message(format!("pch value '{}' is invalid in this locale", pch));
        } else {
            let gstr0 = rmath_ge_from_device_width(size, GE_INCHES, dd);
            let r;
            let mut xc;
            let yc;
            let mut xx = [0.0; 4];
            let mut yy = [0.0; 4];
            match pch {
                0 => {
                    xc = rmath_ge_to_device_width(0.375 * gstr0, GE_INCHES, dd);
                    yc = rmath_ge_to_device_height(0.375 * gstr0, GE_INCHES, dd);
                    if !gc.is_null() {
                        mutable_gc.fill = R_TRANWHITE as c_int;
                        use_gc = &mutable_gc;
                    }
                    rmath_ge_rect(x - xc, y - yc, x + xc, y + yc, use_gc, dd);
                }
                1 => {
                    xc = 0.375 * size;
                    if !gc.is_null() {
                        mutable_gc.fill = R_TRANWHITE as c_int;
                        use_gc = &mutable_gc;
                    }
                    rmath_ge_circle(x, y, xc, use_gc, dd);
                }
                2 => {
                    xc = 0.375 * gstr0;
                    r = rmath_ge_to_device_height(1.5551203015562142 * xc, GE_INCHES, dd);
                    yc = rmath_ge_to_device_height(0.7775601507781071 * xc, GE_INCHES, dd);
                    xc = rmath_ge_to_device_width(1.3467736870885984 * xc, GE_INCHES, dd);
                    xx[0] = x;
                    yy[0] = y + r;
                    xx[1] = x + xc;
                    yy[1] = y - yc;
                    xx[2] = x - xc;
                    yy[2] = y - yc;
                    if !gc.is_null() {
                        mutable_gc.fill = R_TRANWHITE as c_int;
                        use_gc = &mutable_gc;
                    }
                    rmath_ge_polygon(3, xx.as_mut_ptr(), yy.as_mut_ptr(), use_gc, dd);
                }
                3 => {
                    xc = rmath_ge_to_device_width(
                        std::f64::consts::SQRT_2 * 0.375 * gstr0,
                        GE_INCHES,
                        dd,
                    );
                    yc = rmath_ge_to_device_height(
                        std::f64::consts::SQRT_2 * 0.375 * gstr0,
                        GE_INCHES,
                        dd,
                    );
                    rmath_ge_line(x - xc, y, x + xc, y, gc, dd);
                    rmath_ge_line(x, y - yc, x, y + yc, gc, dd);
                }
                4 => {
                    xc = rmath_ge_to_device_width(0.375 * gstr0, GE_INCHES, dd);
                    yc = rmath_ge_to_device_height(0.375 * gstr0, GE_INCHES, dd);
                    rmath_ge_line(x - xc, y - yc, x + xc, y + yc, gc, dd);
                    rmath_ge_line(x - xc, y + yc, x + xc, y - yc, gc, dd);
                }
                5 => {
                    xc = rmath_ge_to_device_width(
                        std::f64::consts::SQRT_2 * 0.375 * gstr0,
                        GE_INCHES,
                        dd,
                    );
                    yc = rmath_ge_to_device_height(
                        std::f64::consts::SQRT_2 * 0.375 * gstr0,
                        GE_INCHES,
                        dd,
                    );
                    xx[0] = x - xc;
                    yy[0] = y;
                    xx[1] = x;
                    yy[1] = y + yc;
                    xx[2] = x + xc;
                    yy[2] = y;
                    xx[3] = x;
                    yy[3] = y - yc;
                    if !gc.is_null() {
                        mutable_gc.fill = R_TRANWHITE as c_int;
                        use_gc = &mutable_gc;
                    }
                    rmath_ge_polygon(4, xx.as_mut_ptr(), yy.as_mut_ptr(), use_gc, dd);
                }
                6 => {
                    xc = 0.375 * gstr0;
                    r = rmath_ge_to_device_height(1.5551203015562142 * xc, GE_INCHES, dd);
                    yc = rmath_ge_to_device_height(0.7775601507781071 * xc, GE_INCHES, dd);
                    xc = rmath_ge_to_device_width(1.3467736870885984 * xc, GE_INCHES, dd);
                    xx[0] = x;
                    yy[0] = y - r;
                    xx[1] = x + xc;
                    yy[1] = y + yc;
                    xx[2] = x - xc;
                    yy[2] = y + yc;
                    if !gc.is_null() {
                        mutable_gc.fill = R_TRANWHITE as c_int;
                        use_gc = &mutable_gc;
                    }
                    rmath_ge_polygon(3, xx.as_mut_ptr(), yy.as_mut_ptr(), use_gc, dd);
                }
                7 => {
                    xc = 0.375 * gstr0;
                    yc = 0.375 * gstr0;
                    xx[0] = x;
                    yy[0] = y + yc;
                    xx[1] = x + xc;
                    yy[1] = y;
                    xx[2] = x;
                    yy[2] = y - yc;
                    xx[3] = x - xc;
                    yy[3] = y;
                    if !gc.is_null() {
                        mutable_gc.fill = R_TRANWHITE as c_int;
                        use_gc = &mutable_gc;
                    }
                    rmath_ge_polygon(4, xx.as_mut_ptr(), yy.as_mut_ptr(), use_gc, dd);
                }
                8 => {
                    xc = rmath_ge_to_device_width(
                        std::f64::consts::SQRT_2 * 0.375 * gstr0,
                        GE_INCHES,
                        dd,
                    );
                    yc = rmath_ge_to_device_height(
                        std::f64::consts::SQRT_2 * 0.375 * gstr0,
                        GE_INCHES,
                        dd,
                    );
                    rmath_ge_line(x - xc, y, x + xc, y, gc, dd);
                    rmath_ge_line(x, y - yc, x, y + yc, gc, dd);
                    rmath_ge_line(x - xc, y - yc, x + xc, y + yc, gc, dd);
                    rmath_ge_line(x - xc, y + yc, x + xc, y - yc, gc, dd);
                }
                9 => {
                    xc = rmath_ge_to_device_width(
                        std::f64::consts::SQRT_2 * 0.375 * gstr0,
                        GE_INCHES,
                        dd,
                    );
                    yc = rmath_ge_to_device_height(
                        std::f64::consts::SQRT_2 * 0.375 * gstr0,
                        GE_INCHES,
                        dd,
                    );
                    xx[0] = x - xc;
                    yy[0] = y - yc;
                    xx[1] = x + xc;
                    yy[1] = y - yc;
                    xx[2] = x + xc;
                    yy[2] = y + yc;
                    xx[3] = x - xc;
                    yy[3] = y + yc;
                    if !gc.is_null() {
                        mutable_gc.fill = R_TRANWHITE as c_int;
                        use_gc = &mutable_gc;
                    }
                    rmath_ge_polygon(4, xx.as_mut_ptr(), yy.as_mut_ptr(), use_gc, dd);
                }
                10 => rmath_ge_text_with_encoding(
                    x,
                    y,
                    b"+\0".as_ptr().cast::<c_char>(),
                    CE_NATIVE,
                    0.0,
                    NA_REAL,
                    gc,
                    dd,
                ),
                11 => rmath_ge_text_with_encoding(
                    x,
                    y,
                    b"x\0".as_ptr().cast::<c_char>(),
                    CE_NATIVE,
                    0.0,
                    NA_REAL,
                    gc,
                    dd,
                ),
                12 => rmath_ge_line(x, y - size / 2.0, x, y + size / 2.0, gc, dd),
                13 => rmath_ge_line(x - size / 2.0, y, x + size / 2.0, y, gc, dd),
                14 => rmath_ge_line(
                    x - size / 2.0,
                    y - size / 2.0,
                    x + size / 2.0,
                    y + size / 2.0,
                    gc,
                    dd,
                ),
                15 => rmath_ge_line(
                    x - size / 2.0,
                    y + size / 2.0,
                    x + size / 2.0,
                    y - size / 2.0,
                    gc,
                    dd,
                ),
                _ => {}
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_raster_scale(
    sraster: *const c_uint,
    sw: c_int,
    sh: c_int,
    draster: *mut c_uint,
    dw: c_int,
    dh: c_int,
) {
    unsafe {
        for i in 0..dh {
            for j in 0..dw {
                let sy = i * sh / dh;
                let sx = j * sw / dw;
                let mut pixel = 0;
                if sx >= 0 && sx < sw && sy >= 0 && sy < sh {
                    pixel = *sraster.add((sy * sw + sx) as usize);
                }
                *draster.add((i * dw + j) as usize) = pixel;
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_raster_interpolate(
    sraster: *const c_uint,
    sw: c_int,
    sh: c_int,
    draster: *mut c_uint,
    dw: c_int,
    dh: c_int,
) {
    unsafe {
        let scx = (16.0 * sw as c_double) / dw as c_double;
        let scy = (16.0 * sh as c_double) / dh as c_double;
        let wm2 = sw - 2;
        let hm2 = sh - 2;

        for i in 0..dh {
            let ypm = (scy * i as c_double - 8.0).max(0.0) as c_int;
            let yp = ypm >> 4;
            let yf = ypm & 0x0f;
            let dline = draster.add((i * dw) as usize);
            let sline = sraster.add((yp * sw) as usize);
            for j in 0..dw {
                let xpm = (scx * j as c_double - 8.0).max(0.0) as c_int;
                let xp = xpm >> 4;
                let xf = xpm & 0x0f;
                let pixels1 = *sline.add(xp as usize);
                let (pixels2, pixels3, pixels4) = if xp > wm2 || yp > hm2 {
                    if yp > hm2 && xp <= wm2 {
                        let p2 = *sline.add((xp + 1) as usize);
                        (p2, pixels1, p2)
                    } else if xp > wm2 && yp <= hm2 {
                        let p3 = *sline.add((sw + xp) as usize);
                        (pixels1, p3, p3)
                    } else {
                        (pixels1, pixels1, pixels1)
                    }
                } else {
                    (
                        *sline.add((xp + 1) as usize),
                        *sline.add((sw + xp) as usize),
                        *sline.add((sw + xp + 1) as usize),
                    )
                };

                let area00 = (16 - xf) * (16 - yf);
                let area10 = xf * (16 - yf);
                let area01 = (16 - xf) * yf;
                let area11 = xf * yf;

                let pixel = (((area00 * red(pixels1)
                    + area10 * red(pixels2)
                    + area01 * red(pixels3)
                    + area11 * red(pixels4)
                    + 128)
                    >> 8)
                    & 0x0000_00ff_u32 as c_int) as c_uint
                    | ((area00 * green(pixels1)
                        + area10 * green(pixels2)
                        + area01 * green(pixels3)
                        + area11 * green(pixels4)
                        + 128)
                        & 0x0000_ff00_u32 as c_int) as c_uint
                    | ((((area00 * blue(pixels1)
                        + area10 * blue(pixels2)
                        + area01 * blue(pixels3)
                        + area11 * blue(pixels4)
                        + 128)
                        << 8)
                        & 0x00ff_0000_u32 as c_int) as c_uint)
                    | ((((area00 * alpha(pixels1)
                        + area10 * alpha(pixels2)
                        + area01 * alpha(pixels3)
                        + area11 * alpha(pixels4)
                        + 128)
                        << 16)
                        & 0xff00_0000_u32 as c_int) as c_uint);

                *dline.add(j as usize) = pixel;
            }
        }
    }
}

fn raster_rotated_size(w: c_int, h: c_int, angle: c_double) -> (c_int, c_int) {
    let diag = ((w * w + h * h) as c_double).sqrt();
    let theta = (h as c_double).atan2(w as c_double);
    let trx1 = diag * (theta + angle).cos();
    let trx2 = diag * (theta - angle).cos();
    let try1 = diag * (theta + angle).sin();
    let try2 = diag * (angle - theta).sin();

    let mut rotated_width = trx1.abs().max(trx2.abs()).round() as c_int;
    let mut rotated_height = try1.abs().max(try2.abs()).round() as c_int;
    if rotated_width < w {
        rotated_width = w;
    }
    if rotated_height < h {
        rotated_height = h;
    }

    (rotated_width, rotated_height)
}

pub(crate) unsafe fn rmath_ge_raster_rotated_size(
    w: c_int,
    h: c_int,
    angle: c_double,
    wnew: *mut c_int,
    hnew: *mut c_int,
) {
    let (rotated_width, rotated_height) = raster_rotated_size(w, h, angle);
    if !wnew.is_null() {
        unsafe { *wnew = rotated_width };
    }
    if !hnew.is_null() {
        unsafe { *hnew = rotated_height };
    }
}

fn raster_rotated_offset(
    w: c_int,
    h: c_int,
    angle: c_double,
    botleft: bool,
) -> (c_double, c_double) {
    let hypot = 0.5 * ((w * w + h * h) as c_double).sqrt();
    let (theta, dw, dh) = if botleft {
        let theta = std::f64::consts::PI + (h as c_double).atan2(w as c_double);
        let dw = hypot * (theta + angle).cos();
        let dh = hypot * (theta + angle).sin();
        (theta, dw, dh)
    } else {
        let theta = -std::f64::consts::PI - (h as c_double).atan2(w as c_double);
        let dw = hypot * (theta + angle).cos();
        let dh = hypot * (theta + angle).sin();
        (theta, dw, dh)
    };
    let _ = theta;
    if botleft {
        (dw + w as c_double / 2.0, dh + h as c_double / 2.0)
    } else {
        (dw + w as c_double / 2.0, dh - h as c_double / 2.0)
    }
}

pub(crate) unsafe fn rmath_ge_raster_rotated_offset(
    w: c_int,
    h: c_int,
    angle: c_double,
    botleft: c_int,
    xoff: *mut c_double,
    yoff: *mut c_double,
) {
    let (x_offset, y_offset) = raster_rotated_offset(w, h, angle, botleft != 0);
    if !xoff.is_null() {
        unsafe { *xoff = x_offset };
    }
    if !yoff.is_null() {
        unsafe { *yoff = y_offset };
    }
}

pub(crate) unsafe fn rmath_ge_raster_resize_for_rotation(
    sraster: *const c_uint,
    w: c_int,
    h: c_int,
    newRaster: *mut c_uint,
    wnew: c_int,
    hnew: c_int,
    gc: pGEcontext,
) {
    unsafe {
        let xoff = (wnew - w) / 2;
        let yoff = (hnew - h) / 2;
        let fill = if gc.is_null() {
            0
        } else {
            (*gc).fill as c_uint
        };
        for i in 0..hnew {
            for j in 0..wnew {
                *newRaster.add((i * wnew + j) as usize) = fill;
            }
        }
        for i in 0..h {
            for j in 0..w {
                let inew = i + yoff;
                let jnew = j + xoff;
                *newRaster.add((inew * wnew + jnew) as usize) = *sraster.add((i * w + j) as usize);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_raster_rotate(
    sraster: *const c_uint,
    w: c_int,
    h: c_int,
    angle: c_double,
    draster: *mut c_uint,
    gc: pGEcontext,
    smoothAlpha: c_int,
) {
    unsafe {
        let angle = -angle;
        let xcen = w / 2;
        let wm2 = w - 2;
        let ycen = h / 2;
        let hm2 = h - 2;
        let sina = 16.0 * angle.sin();
        let cosa = 16.0 * angle.cos();
        let fill = if gc.is_null() {
            0
        } else {
            (*gc).fill as c_uint
        };

        for i in 0..h {
            let ydif = ycen - i;
            let dline = draster.add((i * w) as usize);
            for j in 0..w {
                let xdif = xcen - j;
                let xpm = (-xdif as c_double * cosa - ydif as c_double * sina) as c_int;
                let ypm = (-ydif as c_double * cosa + xdif as c_double * sina) as c_int;
                let xp = xcen + (xpm >> 4);
                let yp = ycen + (ypm >> 4);
                let xf = xpm & 0x0f;
                let yf = ypm & 0x0f;
                if xp < 0 || yp < 0 || xp > wm2 || yp > hm2 {
                    *dline.add(j as usize) = fill;
                    continue;
                }
                let sline = sraster.add((yp * w) as usize);
                let word00 = *sline.add(xp as usize);
                let word10 = *sline.add((xp + 1) as usize);
                let word01 = *sline.add((w + xp) as usize);
                let word11 = *sline.add((w + xp + 1) as usize);

                let rval = ((16 - xf) * (16 - yf) * red(word00)
                    + xf * (16 - yf) * red(word10)
                    + (16 - xf) * yf * red(word01)
                    + xf * yf * red(word11)
                    + 128)
                    / 256;
                let gval = ((16 - xf) * (16 - yf) * green(word00)
                    + xf * (16 - yf) * green(word10)
                    + (16 - xf) * yf * green(word01)
                    + xf * yf * green(word11)
                    + 128)
                    / 256;
                let bval = ((16 - xf) * (16 - yf) * blue(word00)
                    + xf * (16 - yf) * blue(word10)
                    + (16 - xf) * yf * blue(word01)
                    + xf * yf * blue(word11)
                    + 128)
                    / 256;
                let aval = if smoothAlpha != 0 {
                    ((16 - xf) * (16 - yf) * alpha(word00)
                        + xf * (16 - yf) * alpha(word10)
                        + (16 - xf) * yf * alpha(word01)
                        + xf * yf * alpha(word11)
                        + 128)
                        / 256
                } else {
                    alpha(word00)
                        .max(alpha(word10))
                        .max(alpha(word01))
                        .max(alpha(word11))
                };
                *dline.add(j as usize) = rgba(rval, gval, bval, aval);
            }
        }
    }
}

pub(crate) unsafe fn rmath_ge_glyph(
    n: c_int,
    glyphs: *mut c_int,
    x: *mut c_double,
    y: *mut c_double,
    font: SEXP,
    size: c_double,
    colour: c_int,
    rot: c_double,
    dd: pGEDevDesc,
) {
    unsafe {
        if let Some(dev) = with_dev(dd) {
            if (*dev).deviceVersion >= GE_GLYPHS_VERSION {
                if let Some(glyph) = (*dev).glyph {
                    glyph(n, glyphs, x, y, font, size, colour, rot, dev);
                }
            }
        }
    }
}
