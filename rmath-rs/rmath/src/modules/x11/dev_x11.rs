//! X11 graphics device driver (devX11.c)
//!
//! Port of R's X11 device driver. Provides the X11() graphics device,
//! bitmap save routines (png, jpeg, tiff, bmp), and the X11 module
//! registration entry point.
//!
//! All functions are stubs returning safe defaults since we do not
//! link against X11/Xlib/Xt.  However, the type definitions and
//! constants are ported with real values so that code referencing
//! them can compile correctly.
//!
//! Ported from r-source/src/modules/X11/devX11.c and devX11.h

use crate::main::errors::Rf_error_unimplemented;
use crate::sexp::ffi::SEXP;
use crate::sexp::instance::with_required_current_instance;
use core::ffi::{c_char, c_double, c_int, c_uint, c_void};

// ── Constants ────────────────────────────────────────────────────────

/// Millimeters per inch (for coordinate conversion)
pub(crate) const MM_PER_INCH: c_double = 25.4;

/// Special colour used for transparent background on PNG exports.
/// Must be grey since it is used as both RGB and BGR.
pub(crate) const PNG_TRANS: u32 = 0xfefefe;

/// Symbol font face index (used in R's Hershey vector fonts)
pub(crate) const SYMBOL_FONTFACE: c_int = 5;

/// Bell volume for locator mode (-100 to 100)
pub(crate) const X_BELL_VOLUME: c_int = 0;

/// Maximum path length for filenames
pub(crate) const R_PATH_MAX: usize = 4096;

// ── Enums ────────────────────────────────────────────────────────────

/// X11 colour model types.
///
/// These mirror the X_COLORTYPE enum from devX11.h and determine how
/// colours are allocated on the X11 display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) enum XColorType {
    Monochrome = 0,
    Grayscale = 1,
    PseudoColor1 = 2,
    PseudoColor2 = 3,
    TrueColor = 4,
}

/// X11 graphics device type.
///
/// These mirror the X_GTYPE enum from devX11.h and determine whether
/// the device renders to a window, pixmap, or file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) enum XGType {
    Window = 0,
    XImage = 1,
    Png = 2,
    Jpeg = 3,
    Tiff = 4,
    PngDirect = 5,
    Svg = 6,
    Pdf = 7,
    Ps = 8,
    Bmp = 9,
}

/// R line end style (mirrors R_GE_lineend from GraphicsEngine.h)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) enum RGELineEnd {
    Round = 0,
    Butt = 1,
    Square = 2,
}

/// R line join style (mirrors R_GE_linejoin from GraphicsEngine.h)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) enum RGELineJoin {
    Round = 0,
    Miter = 1,
    Bevel = 2,
}

// ── Opaque pointer types ─────────────────────────────────────────────

/// Opaque pointer used where pDevDesc / pGEDevDesc appear.
pub(crate) type pDevDesc = *mut c_void;

fn unsupported(name: &str) -> ! {
    Rf_error_unimplemented(name);
    unreachable!("Rf_error_unimplemented returned")
}

// ── X11Desc structure ────────────────────────────────────────────────
//
// This mirrors the C X11Desc struct from devX11.h.
// It is provided as a Rust-native type so that code which
// references these fields can compile.  All X11-specific
// pointer fields (Window, GC, etc.) are represented as
// opaque pointers since we don't have X11 headers.

/// X11 device descriptor structure.
///
/// This contains both generic graphics parameters (local copies for
/// change detection) and X11-specific device parameters.
///
/// Ported from r-source/src/modules/X11/devX11.h X11Desc.
#[repr(C)]
pub(crate) struct X11Desc {
    // ── Graphics Parameters (local copies for change detection) ──
    /// Line type
    pub lty: c_int,
    /// Line width
    pub lwd: c_double,
    /// Line end style
    pub lend: RGELineEnd,
    /// Line join style
    pub ljoin: RGELineJoin,
    /// Line width scaling factor (multiple of 1/96")
    pub lwdscale: c_double,

    /// Current drawing colour
    pub col: c_int,
    /// Current fill colour
    pub fill: c_int,
    /// Background colour
    pub bg: c_int,
    /// Canvas colour
    pub canvas: c_int,
    /// Typeface index (1-5)
    pub fontface: c_int,
    /// Font size in points
    pub fontsize: c_int,
    /// Point size as double
    pub pointsize: c_double,
    /// Initial font family name
    pub basefontfamily: [c_char; 500],

    // ── X11 Driver Specific Parameters ──
    /// Window width in pixels
    pub windowWidth: c_int,
    /// Window height in pixels
    pub windowHeight: c_int,
    /// Window resized flag
    pub resize: c_int,
    /// Graphics window handle (opaque X11 Window)
    pub window: *mut c_void,
    /// Graphics context handle (opaque X11 GC)
    pub wgc: *mut c_void,
    /// Clipping rectangle
    pub clip: XRectangle,

    /// Font handle (opaque R_XFont*)
    pub font: *mut c_void,
    /// Current font family name
    pub fontfamily: [c_char; 500],
    /// Symbol font family name
    pub symbolfamily: [c_char; 500],
    /// Use PUA (Private Use Area) flag
    pub usePUA: c_int,

    /// Device type (window or bitmap)
    pub dtype: XGType,
    /// Page counter for bitmap devices
    pub npages: c_int,
    /// File pointer for bitmap devices
    pub fp: *mut c_void,
    /// Filename for bitmap devices
    pub filename: [c_char; R_PATH_MAX],
    /// JPEG quality / TIFF compression level
    pub quality: c_int,

    /// Whether events are handled externally
    pub handleOwnEvents: c_int,
    /// Resolution in DPI for bitmap devices
    pub res_dpi: c_int,
    /// Whether we've warned about translucent colours
    pub warn_trans: c_int,
    /// Window title
    pub title: [c_char; 101],
    /// One-file mode flag
    pub onefile: c_int,

    /// Font scaling factor
    pub fontscale: c_double,
    /// Hold/flush level for buffering
    pub holdlevel: c_int,
}

/// X11 rectangle structure (mirrors XRectangle from X11/Xlib.h).
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub(crate) struct XRectangle {
    pub x: c_int,
    pub y: c_int,
    pub width: c_uint,
    pub height: c_uint,
}

impl Default for X11Desc {
    fn default() -> Self {
        X11Desc {
            lty: 0,
            lwd: 1.0,
            lend: RGELineEnd::Round,
            ljoin: RGELineJoin::Round,
            lwdscale: 1.0,
            col: 0,
            fill: -1,
            bg: 0xFFFFFF,
            canvas: 0xFFFFFF,
            fontface: 1,
            fontsize: 12,
            pointsize: 12.0,
            basefontfamily: [0; 500],
            windowWidth: 0,
            windowHeight: 0,
            resize: 0,
            window: std::ptr::null_mut(),
            wgc: std::ptr::null_mut(),
            clip: XRectangle::default(),
            font: std::ptr::null_mut(),
            fontfamily: [0; 500],
            symbolfamily: [0; 500],
            usePUA: 0,
            dtype: XGType::Window,
            npages: 0,
            fp: std::ptr::null_mut(),
            filename: [0; R_PATH_MAX],
            quality: 75,
            handleOwnEvents: 0,
            res_dpi: 72,
            warn_trans: 0,
            title: [0; 101],
            onefile: 0,
            fontscale: 1.0,
            holdlevel: 0,
        }
    }
}

// ── Color palette tables ─────────────────────────────────────────────
//
// The C code stores up to 512 entries in RPalette[] and XPalette[].
// We define the same capacity as Rust arrays.

/// Maximum palette size for X11 colour cube
pub(crate) const MAX_PALETTE_SIZE: usize = 512;

/// RGB colour palette entry (R's internal representation, 0-255 per channel).
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub(crate) struct RPaletteEntry {
    pub red: c_int,
    pub green: c_int,
    pub blue: c_int,
}

/// X colour palette entry (X11 XColor fields, 0-65535 per channel).
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub(crate) struct XPaletteEntry {
    pub red: u16,
    pub green: u16,
    pub blue: u16,
    pub pixel: u64,
    pub flags: u8,
}

/// PseudoColor RGB level table.
///
/// These are the standard colour cube configurations tried by R's
/// X11 driver, in order of decreasing quality.  Each entry is
/// (nr, ng, nb) levels for red, green, blue channels.
pub(crate) const RGB_LEVELS: &[(c_int, c_int, c_int)] = &[
    (8, 8, 4),
    (6, 7, 6),
    (6, 6, 6),
    (6, 6, 5),
    (6, 6, 4),
    (5, 5, 5),
    (5, 5, 4),
    (4, 4, 4),
    (4, 4, 3),
    (3, 3, 3),
    (2, 2, 2),
];

/// Gamma correction values (default 1.0 = no correction)
pub(crate) const DEFAULT_RED_GAMMA: c_double = 1.0;
pub(crate) const DEFAULT_GREEN_GAMMA: c_double = 1.0;
pub(crate) const DEFAULT_BLUE_GAMMA: c_double = 1.0;

// ── Colour conversion helpers (platform-independent) ─────────────────
//
// These functions are pure algorithms that don't need X11.

/// Convert RGB to luminance using the ITU-R BT.601 coefficients.
/// Returns a value in the range 0-255.
#[inline]
pub(crate) fn rgb_to_luminance(r: c_int, g: c_int, b: c_int) -> c_int {
    (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64) as c_int
}

/// Convert a luminance value to monochrome pixel (threshold at 127).
/// Returns true for white, false for black.
#[inline]
pub(crate) fn luminance_to_mono(r: c_int, g: c_int, b: c_int) -> bool {
    rgb_to_luminance(r, g, b) > 127
}

/// Apply gamma correction to a channel value.
/// `value` is in 0.0..1.0, `gamma` is the correction exponent.
/// Returns a value in 0..65535 (X11 colour range).
#[inline]
pub(crate) fn gamma_correct(value: f64, gamma: f64) -> u16 {
    (value.powf(gamma) * 65535.0) as u16
}

/// Compute the squared Euclidean distance between two RGB colours.
/// Used for finding the nearest colour in a palette.
#[inline]
pub(crate) fn colour_distance_sq(
    r1: c_int,
    g1: c_int,
    b1: c_int,
    r2: c_int,
    g2: c_int,
    b2: c_int,
) -> u32 {
    let dr = (r1 - r2) as i64;
    let dg = (g1 - g2) as i64;
    let db = (b1 - b2) as i64;
    (dr * dr + dg * dg + db * db) as u32
}

/// Extract colour channels from a packed 24-bit colour value.
/// Supports both BGR and RGB byte orders.
#[inline]
pub(crate) fn unpack_colour(col: u32, bgr: bool) -> (u8, u8, u8) {
    if bgr {
        (
            (col & 0xFF) as u8,
            ((col >> 8) & 0xFF) as u8,
            ((col >> 16) & 0xFF) as u8,
        )
    } else {
        (
            ((col >> 16) & 0xFF) as u8,
            ((col >> 8) & 0xFF) as u8,
            (col & 0xFF) as u8,
        )
    }
}

/// Find the nearest colour in a palette using squared Euclidean distance.
/// Returns the index of the nearest palette entry.
pub(crate) fn find_nearest_palette_colour(
    r: c_int,
    g: c_int,
    b: c_int,
    palette: &[RPaletteEntry],
    palette_size: usize,
) -> usize {
    let mut best_idx = 0;
    let mut best_dist = u32::MAX;
    for i in 0..palette_size {
        let d = colour_distance_sq(r, g, b, palette[i].red, palette[i].green, palette[i].blue);
        if d < best_dist {
            best_dist = d;
            best_idx = i;
        }
    }
    best_idx
}

/// Compute the device pixel dimensions from a physical size and resolution.
///
/// `width_mm` and `height_mm` are in millimeters.
/// `res_dpi` is the resolution in dots per inch.
/// Returns (width_px, height_px) in pixels.
#[inline]
pub(crate) fn physical_to_pixel_size(
    width_mm: c_double,
    height_mm: c_double,
    res_dpi: c_double,
) -> (c_int, c_int) {
    let width_in = width_mm / MM_PER_INCH;
    let height_in = height_mm / MM_PER_INCH;
    let width_px = (width_in * res_dpi).round() as c_int;
    let height_px = (height_in * res_dpi).round() as c_int;
    (width_px, height_px)
}

// ── Runtime state ────────────────────────────────────────────────────

pub(crate) struct X11RuntimeState {
    display_color_model: XColorType,
    max_cube_size: c_int,
    display_res_dpi: c_int,
    num_x11_devices: c_int,
    red_gamma: c_double,
    green_gamma: c_double,
    blue_gamma: c_double,
    pub(crate) rotated_magnify: c_double,
    pub(crate) rotated_bbx_pad: c_int,
}

impl Default for X11RuntimeState {
    fn default() -> Self {
        Self {
            display_color_model: XColorType::TrueColor,
            max_cube_size: 256,
            display_res_dpi: 72,
            num_x11_devices: 0,
            red_gamma: DEFAULT_RED_GAMMA,
            green_gamma: DEFAULT_GREEN_GAMMA,
            blue_gamma: DEFAULT_BLUE_GAMMA,
            rotated_magnify: 1.0,
            rotated_bbx_pad: 0,
        }
    }
}

fn with_x11_state<R>(f: impl FnOnce(&mut X11RuntimeState) -> R) -> R {
    with_required_current_instance(|instance| f(&mut instance.x11_state))
}

// ── Exported symbols (no_mangle) ──────────────────────────────────────

/// X11DeviceDriver - entry point called by the graphics engine when
/// the user creates an X11 device. Stub returns FALSE (failure).
///
/// In a real implementation, this function:
/// 1. Opens the X11 display (if not already open)
/// 2. Sets up the colour model (mono, grey, pseudo, truecolor)
/// 3. Creates an X11 window or pixmap
/// 4. Allocates and initialises an X11Desc struct
/// 5. Sets up all device driver callbacks in the pDevDesc
pub unsafe fn X11DeviceDriver(
    _dd: pDevDesc,
    _display: *const c_char,
    _width: c_double,
    _height: c_double,
    _ps: c_double,
    _gamma: c_double,
    _colormodel: c_int,
    _maxcube: c_int,
    _bgcolor: c_int,
    _canvascolor: c_int,
    _sfonts: SEXP,
    _res: c_int,
    _xpos: c_int,
    _ypos: c_int,
    _title: *const c_char,
    _useCairo: c_int,
    _antialias: c_int,
    _family: *const c_char,
    _symbolfamily: *const c_char,
    _usePUA: c_int,
) -> c_int {
    0 // FALSE - no X11 support
}

/// Rf_allocX11DeviceDesc - allocate an X11 device descriptor.
///
/// In a real implementation, this allocates a zeroed X11Desc struct
/// and initialises its fields to sensible defaults.
/// Stub returns a null pointer.
pub unsafe fn Rf_allocX11DeviceDesc(ps: c_double) -> *mut c_void {
    let _ = ps; // would set default pointsize
    std::ptr::null_mut()
}

/// Rf_setX11DeviceData - attach X11-specific data to a device.
///
/// In a real implementation, this sets up gamma correction,
/// colour model, and links the X11Desc to the pDevDesc.
/// Stub returns 0 (failure).
pub unsafe fn Rf_setX11DeviceData(_dd: pDevDesc, gamma_fac: c_double, _xd: *mut c_void) -> c_int {
    // Apply gamma correction even in stub mode
    if gamma_fac > 0.0 {
        with_x11_state(|state| {
            state.red_gamma = gamma_fac;
            state.green_gamma = gamma_fac;
            state.blue_gamma = gamma_fac;
        });
    }
    0
}

/// Rf_getX11Display - return the current X11 Display pointer.
/// Stub returns null.
pub unsafe fn Rf_getX11Display() -> *mut c_void {
    std::ptr::null_mut()
}

/// Rf_setX11Display - open / configure the X11 display.
///
/// In a real implementation, this:
/// 1. Opens the X11 display connection
/// 2. Sets up gamma correction
/// 3. Configures the colour model
/// 4. Installs event handlers
/// Stub returns 0 (failure).
pub unsafe fn Rf_setX11Display(
    _dpy: *mut c_void,
    gamma_fac: c_double,
    colormodel: c_int,
    maxcube: c_int,
    _setHandlers: c_int,
) -> c_int {
    // Apply settings even in stub mode
    if gamma_fac > 0.0 {
        with_x11_state(|state| {
            state.red_gamma = gamma_fac;
            state.green_gamma = gamma_fac;
            state.blue_gamma = gamma_fac;
        });
    }

    // Set colour model
    with_x11_state(|state| {
        state.display_color_model = match colormodel {
            0 => XColorType::Monochrome,
            1 => XColorType::Grayscale,
            2 => XColorType::PseudoColor1,
            3 => XColorType::PseudoColor2,
            _ => XColorType::TrueColor,
        };
        state.max_cube_size = maxcube;
    });
    0
}

/// R_init_R_X11 - module initialisation entry point called by R's
/// dynamic loader.  Populates the R_X11Routines dispatch table.
///
/// In a real implementation, this fills in a table of function
/// pointers for the X11 module (data entry, device creation, etc.).
pub unsafe fn R_init_R_X11(_info: *mut c_void) {
    // no-op stub - would populate R_X11Routines
}

/// in_R_pngVersion - return the libpng version string.
/// Empty string since libpng is not linked.
pub unsafe fn in_R_pngVersion() -> *const c_char {
    b"\0".as_ptr() as *const c_char
}

/// in_R_jpegVersion - return the libjpeg version string.
/// Empty string since libjpeg is not linked.
pub unsafe fn in_R_jpegVersion() -> *const c_char {
    b"\0".as_ptr() as *const c_char
}

/// in_R_tiffVersion - return the libtiff version string.
/// Empty string since libtiff is not linked.
pub unsafe fn in_R_tiffVersion() -> *const c_char {
    b"\0".as_ptr() as *const c_char
}

/// in_R_GetX11Image - retrieve an X11 image from device number d.
///
/// In a real implementation, this extracts the pixel data from the
/// X11 pixmap associated with device number `d`.
/// Stub returns FALSE.
pub unsafe fn in_R_GetX11Image(
    _d: c_int,
    _pximage: *mut c_void,
    _pwidth: *mut c_int,
    _pheight: *mut c_int,
) -> c_int {
    0 // FALSE
}

/// in_RX11_dataentry - X11 data editor entry point (dataentry.c).
///
/// In a real implementation, this opens a spreadsheet-style
/// data editor window using X11/Xt widgets.
/// Stub reports unsupported explicitly.
pub unsafe fn in_RX11_dataentry(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsupported("X11 dataentry")
}

/// in_R_X11_dataviewer - X11 data viewer entry point.
///
/// In a real implementation, this opens a read-only data viewer.
/// Stub reports unsupported explicitly.
pub unsafe fn in_R_X11_dataviewer(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsupported("X11 dataviewer")
}

// ── Module-private stubs (symbols already exported elsewhere) ─────────

/// R_setX11Routines - already exported in unix/x11.rs.
/// Provided here as a module-private stub for completeness.
unsafe fn _R_setX11Routines(_routines: *mut c_void) -> *mut c_void {
    std::ptr::null_mut()
}

/// R_setdeRoutines - set the data-entry routine dispatch table.
/// Stub returns null.
pub unsafe fn R_setdeRoutines(_routines: *mut c_void) -> *mut c_void {
    std::ptr::null_mut()
}

/// R_SaveAsPng - save device content as PNG.
/// NOTE: #[unsafe(no_mangle)] is in library/grdevices/winbitmap.rs;
/// this is a module-private stub only.
unsafe fn _R_SaveAsPng(
    _d: *mut c_void,
    _width: c_int,
    _height: c_int,
    _gp: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int) -> u32>,
    _bgr: c_int,
    _fp: *mut c_void,
    _transparent: u32,
    _res: c_int,
) -> c_int {
    0
}

/// R_SaveAsJpeg - save device content as JPEG.
/// Module-private stub (no_mangle in winbitmap.rs).
unsafe fn _R_SaveAsJpeg(
    _d: *mut c_void,
    _width: c_int,
    _height: c_int,
    _gp: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int) -> u32>,
    _bgr: c_int,
    _quality: c_int,
    _outfile: *mut c_void,
    _res: c_int,
) -> c_int {
    0
}

/// R_SaveAsTIFF - save device content as TIFF.
/// Module-private stub (no_mangle in winbitmap.rs).
unsafe fn _R_SaveAsTIFF(
    _d: *mut c_void,
    _width: c_int,
    _height: c_int,
    _gp: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int) -> u32>,
    _bgr: c_int,
    _outfile: *const c_char,
    _res: c_int,
    _compression: c_int,
) -> c_int {
    0
}

/// R_SaveAsBmp - save device content as BMP.
/// Module-private stub (no_mangle in winbitmap.rs).
/// Delegates to the real implementation in rbitmap.rs.
unsafe fn _R_SaveAsBmp(
    d: *mut c_void,
    width: c_int,
    height: c_int,
    gp: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int) -> u32>,
    bgr: c_int,
    fp: *mut c_void,
    res: c_int,
) -> c_int {
    unsafe { super::rbitmap::save_as_bmp(d, width, height, gp, bgr, fp, res) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::instance::{RInstance, replace_current_instance};

    #[test]
    fn x11_runtime_state_is_session_local() {
        let mut first = RInstance::new();
        let mut second = RInstance::new();

        unsafe {
            let previous = replace_current_instance(Some(&mut first as *mut RInstance));
            Rf_setX11Display(std::ptr::null_mut(), 2.0, 1, 64, 0);
            replace_current_instance(previous);

            let previous = replace_current_instance(Some(&mut second as *mut RInstance));
            assert_eq!(
                with_x11_state(|state| state.display_color_model),
                XColorType::TrueColor
            );
            assert_eq!(with_x11_state(|state| state.max_cube_size), 256);
            Rf_setX11Display(std::ptr::null_mut(), 3.0, 3, 32, 0);
            replace_current_instance(previous);
        }

        assert_eq!(first.x11_state.display_color_model, XColorType::Grayscale);
        assert_eq!(first.x11_state.max_cube_size, 64);
        assert_eq!(first.x11_state.red_gamma, 2.0);
        assert_eq!(
            second.x11_state.display_color_model,
            XColorType::PseudoColor2
        );
        assert_eq!(second.x11_state.max_cube_size, 32);
        assert_eq!(second.x11_state.red_gamma, 3.0);
    }

    #[test]
    fn dataentry_reports_unsupported() {
        let _session = crate::sexp::session::RSession::new();
        let err = std::panic::catch_unwind(|| unsafe {
            in_RX11_dataentry(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        })
        .expect_err("expected RError");
        let r_error = err
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("expected RError");
        assert!(
            r_error
                .message
                .contains("function 'X11 dataentry' is not yet implemented")
        );
    }

    #[test]
    fn dataviewer_reports_unsupported() {
        let _session = crate::sexp::session::RSession::new();
        let err = std::panic::catch_unwind(|| unsafe {
            in_R_X11_dataviewer(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        })
        .expect_err("expected RError");
        let r_error = err
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("expected RError");
        assert!(
            r_error
                .message
                .contains("function 'X11 dataviewer' is not yet implemented")
        );
    }
}
