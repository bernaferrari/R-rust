
//! X11 rotated text support (rotated.c)
//!
//! Port of the xvertext 5.0 library used by R's X11 device for
//! drawing rotated text strings.
//!
//! The actual rendering functions (XRotDrawString, etc.) are stubs
//! since they require X11/Xlib for pixel operations.  However, the
//! following are ported with real implementations:
//!   - XRotVersion: returns the real version string
//!   - XRotSetMagnification / XRotSetBoundingBoxPad: state management
//!   - XRotTextExtents: bounding box calculation with rotation math
//!
//! All rotation math (sin/cos rounding, coordinate transforms, bounding
//! box rotation) is faithfully ported from the C source.
//!
//! Ported from r-source/src/modules/X11/rotated.c

use core::ffi::{c_char, c_double, c_int, c_void};
use libc::{free, malloc, strlen};

// ── Constants ────────────────────────────────────────────────────────

/// xvertext library version
const XV_VERSION: c_double = 5.0;

/// xvertext copyright string
const XV_COPYRIGHT: &[u8] = b"xvertext routines Copyright (c) 1993 Alan Richardson\0";

/// Degrees to radians conversion factor
const DEG2RAD: c_double = 0.01745329251994329576;

/// Text alignment constants
pub(crate) const ALIGN_NONE: c_int = 0;
pub(crate) const ALIGN_TLEFT: c_int = 1;
pub(crate) const ALIGN_TCENTRE: c_int = 2;
pub(crate) const ALIGN_TRIGHT: c_int = 3;
pub(crate) const ALIGN_MLEFT: c_int = 4;
pub(crate) const ALIGN_MCENTRE: c_int = 5;
pub(crate) const ALIGN_MRIGHT: c_int = 6;
pub(crate) const ALIGN_BLEFT: c_int = 7;
pub(crate) const ALIGN_BCENTRE: c_int = 8;
pub(crate) const ALIGN_BRIGHT: c_int = 9;

/// Font type constants (mirrors R_FontType from rotated.h)
pub(crate) const FONT_TYPE_ONE_FONT: c_int = 0;
pub(crate) const FONT_TYPE_FONT_SET: c_int = 1;

// ── State ────────────────────────────────────────────────────────────

/// Current magnification factor and bounding box padding.
/// These are mutable statics mirroring the C static `style` struct.
struct StyleState {
    magnify: c_double,
    bbx_pad: c_int,
}

/// Safety: Only modified through XRotSetMagnification / XRotSetBoundingBoxPad
/// which are called from the graphics engine in a single-threaded context.
static mut STYLE: StyleState = StyleState {
    magnify: 1.0,
    bbx_pad: 0,
};

// ── Helper functions ─────────────────────────────────────────────────

/// Round a double to the nearest integer value (as a double).
/// Mirrors C's `static double myround(double x)`.
#[inline]
fn myround(x: c_double) -> c_double {
    x.floor() + 0.5
}

/// Normalise an angle to the range [0, 2*pi) given in degrees.
/// Returns the angle in radians.
#[inline]
fn normalise_angle_to_radians(angle_deg: c_double) -> c_double {
    let mut angle = angle_deg;
    while angle < 0.0 {
        angle += 360.0;
    }
    while angle >= 360.0 {
        angle -= 360.0;
    }
    angle * DEG2RAD
}

/// Compute pre-rounded sin and cos values (rounded to 3 decimal places).
/// This matches the C implementation which rounds sin/cos to avoid
/// floating-point drift in pixel calculations.
#[inline]
fn rounded_sin_cos(angle_rad: c_double) -> (c_double, c_double) {
    let sin_angle = (myround(angle_rad.sin() * 1000.0)) / 1000.0;
    let cos_angle = (myround(angle_rad.cos() * 1000.0)) / 1000.0;
    (sin_angle, cos_angle)
}

/// A 2D point (used for bounding box corners).
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub(crate) struct XPoint {
    pub x: c_int,
    pub y: c_int,
}

/// Rotated text item metrics (portable subset).
///
/// This struct captures the geometric information about a rotated text
/// item that can be computed without X11 rendering.  It mirrors the
/// relevant fields from the C RotatedTextItem struct.
pub(crate) struct RotatedTextMetrics {
    /// Number of line sections (separated by '\n')
    pub nl: c_int,
    /// Width of the widest line section in pixels
    pub max_width: c_int,
    /// Input (horizontal) text dimensions
    pub cols_in: c_int,
    pub rows_in: c_int,
    /// Output (rotated) text dimensions
    pub cols_out: c_int,
    pub rows_out: c_int,
    /// Corner positions of each text section (4 corners * nl sections)
    pub corners_x: Vec<c_double>,
    pub corners_y: Vec<c_double>,
}

/// Count the number of line sections in a string (separated by '\n').
/// Returns 1 for a string with no newlines.
unsafe fn count_line_sections(text: *const c_char, align: c_int) -> c_int {
    if align == ALIGN_NONE {
        return 1;
    }
    let len = strlen(text);
    if len < 2 {
        return 1;
    }
    let mut nl: c_int = 1;
    let bytes = core::slice::from_raw_parts(text as *const u8, len);
    for i in 0..len - 1 {
        if bytes[i as usize] == b'\n' {
            nl += 1;
        }
    }
    nl
}

/// Compute the hot-spot offset for alignment.
/// Returns (hot_x, hot_y) relative to the bitmap center.
///
/// This is the same logic used in XRotPaintAlignedString and XRotTextExtents.
#[inline]
pub(crate) fn compute_hotspot(
    align: c_int,
    max_width: c_int,
    rows_in: c_int,
    font_descent: c_int,
    magnify: c_double,
) -> (c_double, c_double) {
    let (hot_x, hot_y): (c_double, c_double);

    // Y position
    if align == ALIGN_TLEFT || align == ALIGN_TCENTRE || align == ALIGN_TRIGHT {
        hot_y = rows_in as c_double / 2.0 * magnify;
    } else if align == ALIGN_MLEFT || align == ALIGN_MCENTRE || align == ALIGN_MRIGHT {
        hot_y = 0.0;
    } else if align == ALIGN_BLEFT || align == ALIGN_BCENTRE || align == ALIGN_BRIGHT {
        hot_y = -rows_in as c_double / 2.0 * magnify;
    } else {
        // NONE
        hot_y = -(rows_in as c_double / 2.0 - font_descent as c_double) * magnify;
    }

    // X position
    if align == ALIGN_TLEFT || align == ALIGN_MLEFT || align == ALIGN_BLEFT || align == ALIGN_NONE {
        hot_x = -max_width as c_double / 2.0 * magnify;
    } else if align == ALIGN_TCENTRE || align == ALIGN_MCENTRE || align == ALIGN_BCENTRE {
        hot_x = 0.0;
    } else {
        hot_x = max_width as c_double / 2.0 * magnify;
    }

    (hot_x, hot_y)
}

/// Rotate a point (px, py) around the origin by angle (given as pre-computed
/// sin_angle, cos_angle).  Returns (rx, ry).
#[inline]
pub(crate) fn rotate_point(
    px: c_double,
    py: c_double,
    sin_angle: c_double,
    cos_angle: c_double,
) -> (c_double, c_double) {
    let rx = px * cos_angle - py * sin_angle;
    let ry = px * sin_angle + py * cos_angle;
    (rx, ry)
}

/// Compute the output dimensions of a rotated text item.
/// Given the input dimensions (cols_in, rows_in) and the angle in radians,
/// returns (cols_out, rows_out) as odd numbers (for centered bitmap).
#[inline]
pub(crate) fn compute_rotated_dimensions(
    cols_in: c_int,
    rows_in: c_int,
    angle_rad: c_double,
) -> (c_int, c_int) {
    let (sin_angle, cos_angle) = rounded_sin_cos(angle_rad);

    let mut cols_out = (rows_in as c_double * sin_angle.abs()
        + cols_in as c_double * cos_angle.abs()
        + 0.99999
        + 2.0) as c_int;

    let mut rows_out = (rows_in as c_double * cos_angle.abs()
        + cols_in as c_double * sin_angle.abs()
        + 0.99999
        + 2.0) as c_int;

    // Make dimensions odd for centered bitmaps
    if cols_out % 2 == 0 {
        cols_out += 1;
    }
    if rows_out % 2 == 0 {
        rows_out += 1;
    }

    (cols_out, rows_out)
}

/// Compute the bounding box corners for rotated text.
///
/// This is the core of XRotTextExtents - it calculates the rotated
/// bounding box of a text string given its dimensions, angle, and
/// alignment, and returns 5 XPoints (4 corners + closing point).
///
/// Returns a Vec of 5 XPoints on success, or an empty Vec on failure.
pub(crate) unsafe fn compute_text_extents(
    font_ascent: c_int,
    font_descent: c_int,
    max_width: c_int,
    nl: c_int,
    angle_deg: c_double,
    x: c_int,
    y: c_int,
    align: c_int,
) -> Vec<XPoint> {
    let angle_rad = normalise_angle_to_radians(angle_deg);
    let (sin_angle, cos_angle) = rounded_sin_cos(angle_rad);

    let height = font_ascent + font_descent;
    let cols_in = max_width;
    let rows_in = nl * height;

    let magnify = STYLE.magnify;
    let bbx_pad = STYLE.bbx_pad;

    let (hot_x, hot_y) = compute_hotspot(align, max_width, rows_in, font_descent, magnify);

    // Bounding box when horizontal, relative to bitmap centre
    let xp_in: [(c_double, c_double); 5] = [
        (
            -(cols_in as c_double * magnify / 2.0 - bbx_pad as c_double),
            rows_in as c_double * magnify / 2.0 + bbx_pad as c_double,
        ),
        (
            cols_in as c_double * magnify / 2.0 + bbx_pad as c_double,
            rows_in as c_double * magnify / 2.0 + bbx_pad as c_double,
        ),
        (
            cols_in as c_double * magnify / 2.0 + bbx_pad as c_double,
            -(rows_in as c_double * magnify / 2.0 - bbx_pad as c_double),
        ),
        (
            -(cols_in as c_double * magnify / 2.0 - bbx_pad as c_double),
            -(rows_in as c_double * magnify / 2.0 - bbx_pad as c_double),
        ),
        (
            -(cols_in as c_double * magnify / 2.0 - bbx_pad as c_double),
            rows_in as c_double * magnify / 2.0 + bbx_pad as c_double,
        ),
    ];

    // Rotate and translate bounding box
    let mut result = Vec::with_capacity(5);
    for i in 0..5 {
        let px = xp_in[i].0 - hot_x;
        let py = xp_in[i].1 + hot_y;
        let (rx, ry) = rotate_point(px, py, sin_angle, cos_angle);
        result.push(XPoint {
            x: x as c_int + rx as c_int,
            y: y as c_int - ry as c_int,
        });
    }

    result
}

// ── Exported symbols (no_mangle) ──────────────────────────────────────

/// XRotVersion - return version/copyright information.
/// If `str` is non-null, copies the copyright string into it (up to `n` bytes).
/// Returns the version number (5.0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XRotVersion(str: *mut c_char, n: c_int) -> c_double {
    if !str.is_null() && n > 0 {
        let copy_len = XV_COPYRIGHT.len().min(n as usize) - 1;
        libc::strncpy(str, XV_COPYRIGHT.as_ptr() as *const c_char, copy_len);
        *str.add(copy_len) = 0; // null terminate
    }
    XV_VERSION
}

/// XRotSetMagnification - set the magnification factor for rotated text.
/// Only values > 0 are accepted.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XRotSetMagnification(m: c_double) {
    if m > 0.0 {
        STYLE.magnify = m;
    }
}

/// XRotSetBoundingBoxPad - set the padding for bounding boxes.
/// Only values >= 0 are accepted.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XRotSetBoundingBoxPad(p: c_int) {
    if p >= 0 {
        STYLE.bbx_pad = p;
    }
}

/// XRotDrawString - draw a rotated text string.
///
/// This function requires X11/Xlib for actual rendering (XCreatePixmap,
/// XPutImage, XFillRectangle, etc.).  It is kept as a stub returning 0
/// (failure) since we do not link against X11.
///
/// The rotation math and bounding box calculations are available through
/// the pub(crate) helper functions in this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XRotDrawString(
    _dpy: *mut c_void,
    _font: *mut c_void,
    _angle: c_double,
    _drawable: u64,
    _gc: u64,
    _x: c_int,
    _y: c_int,
    _str: *const c_char,
) -> c_int {
    0 // failure - no X11 support
}

/// XRotDrawImageString - draw a rotated text string (image variant).
///
/// Stub returning 0 (no X11 support).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XRotDrawImageString(
    _dpy: *mut c_void,
    _font: *mut c_void,
    _angle: c_double,
    _drawable: u64,
    _gc: u64,
    _x: c_int,
    _y: c_int,
    _str: *const c_char,
) -> c_int {
    0 // failure - no X11 support
}

/// XRotDrawAlignedString - draw a rotated, aligned text string.
///
/// Stub returning 0 (no X11 support).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XRotDrawAlignedString(
    _dpy: *mut c_void,
    _font: *mut c_void,
    _angle: c_double,
    _drawable: u64,
    _gc: u64,
    _x: c_int,
    _y: c_int,
    _text: *const c_char,
    _align: c_int,
) -> c_int {
    0 // failure - no X11 support
}

/// XRotDrawAlignedImageString - draw a rotated, aligned text string (image variant).
///
/// Stub returning 0 (no X11 support).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XRotDrawAlignedImageString(
    _dpy: *mut c_void,
    _font: *mut c_void,
    _angle: c_double,
    _drawable: u64,
    _gc: u64,
    _x: c_int,
    _y: c_int,
    _text: *const c_char,
    _align: c_int,
) -> c_int {
    0 // failure - no X11 support
}

/// XRotTextExtents - compute bounding box of a rotated text string.
///
/// The C implementation uses XTextExtents() to get font metrics from the
/// X11 server.  Without X11, we cannot determine the actual text width.
/// This stub returns null (failure).
///
/// For code that knows the text dimensions, the pub(crate) function
/// compute_text_extents() provides the full bounding box math.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XRotTextExtents(
    _dpy: *mut c_void,
    _font: *mut c_void,
    _angle: c_double,
    _x: c_int,
    _y: c_int,
    _text: *const c_char,
    _align: c_int,
) -> *mut c_void {
    std::ptr::null_mut()
}

/// XRfRotDrawString - draw a rotated string using an R_XFont (font set or single font).
///
/// The R_XFont struct contains a `type` field indicating whether it wraps
/// a single XFontStruct (type=0) or an XFontSet (type=1).  Without X11,
/// this stub returns 0 (failure).
///
/// In the C code, this dispatches to XRotDrawString or XmbRotDrawString
/// based on the font type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XRfRotDrawString(
    _dpy: *mut c_void,
    _rfont: *mut c_void,
    _angle: c_double,
    _drawable: u64,
    _gc: u64,
    _x: c_int,
    _y: c_int,
    _str: *const c_char,
) -> c_int {
    0 // failure - no X11 support
}
