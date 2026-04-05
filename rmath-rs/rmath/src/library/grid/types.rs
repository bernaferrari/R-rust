
//! Grid package shared types (equivalent to grid.h).
//!
//! Defines all constants, types, and stubs used across grid modules.

use std::ffi::c_void;
use std::os::raw::{c_char, c_double, c_int};

use crate::sexp::ffi::SEXP;

/* ==================== GSS (Grid System State) indices ==================== */

pub const GSS_DEVSIZE: c_int = 0;
pub const GSS_CURRLOC: c_int = 1;
pub const GSS_DL: c_int = 2;
pub const GSS_DLINDEX: c_int = 3;
pub const GSS_DLON: c_int = 4;
pub const GSS_GPAR: c_int = 5;
pub const GSS_GPSAVED: c_int = 6;
pub const GSS_VP: c_int = 7;
pub const GSS_GLOBALINDEX: c_int = 8;
pub const GSS_GRIDDEVICE: c_int = 9;
pub const GSS_PREVLOC: c_int = 10;
pub const GSS_ENGINEDLON: c_int = 11;
pub const GSS_CURRGROB: c_int = 12;
pub const GSS_ENGINERECORDING: c_int = 13;
/* GSS_ASK 14 unused in R >= 2.7.0 */
pub const GSS_SCALE: c_int = 15;
pub const GSS_RESOLVINGPATH: c_int = 16;
pub const GSS_GROUPS: c_int = 17;

/* ==================== VP (Viewport) structure indices ==================== */

pub const VP_X: c_int = 0;
pub const VP_Y: c_int = 1;
pub const VP_WIDTH: c_int = 2;
pub const VP_HEIGHT: c_int = 3;
pub const VP_JUST: c_int = 4;
pub const VP_GP: c_int = 5;
pub const VP_CLIP: c_int = 6;
pub const VP_XSCALE: c_int = 7;
pub const VP_YSCALE: c_int = 8;
pub const VP_ANGLE: c_int = 9;
pub const VP_LAYOUT: c_int = 10;
pub const VP_LPOSROW: c_int = 11;
pub const VP_LPOSCOL: c_int = 12;
pub const VP_VALIDJUST: c_int = 13;
pub const VP_VALIDLPOSROW: c_int = 14;
pub const VP_VALIDLPOSCOL: c_int = 15;
pub const VP_NAME: c_int = 16;
pub const VP_MASK: c_int = 31;

/* Additional structure of a pushedvp */
pub const PVP_PARENTGPAR: c_int = 17;
pub const PVP_GPAR: c_int = 18;
pub const PVP_TRANS: c_int = 19;
pub const PVP_WIDTHS: c_int = 20;
pub const PVP_HEIGHTS: c_int = 21;
pub const PVP_WIDTHCM: c_int = 22;
pub const PVP_HEIGHTCM: c_int = 23;
pub const PVP_ROTATION: c_int = 24;
pub const PVP_CLIPRECT: c_int = 25;
pub const PVP_PARENT: c_int = 26;
pub const PVP_CHILDREN: c_int = 27;
pub const PVP_DEVWIDTHCM: c_int = 28;
pub const PVP_DEVHEIGHTCM: c_int = 29;
pub const PVP_CLIPPATH: c_int = 30;
pub const PVP_MASK: c_int = 32;

/* ==================== Layout structure indices ==================== */

pub const LAYOUT_NROW: c_int = 0;
pub const LAYOUT_NCOL: c_int = 1;
pub const LAYOUT_WIDTHS: c_int = 2;
pub const LAYOUT_HEIGHTS: c_int = 3;
pub const LAYOUT_RESPECT: c_int = 4;
pub const LAYOUT_VRESPECT: c_int = 5;
pub const LAYOUT_MRESPECT: c_int = 6;
pub const LAYOUT_JUST: c_int = 7;
pub const LAYOUT_VJUST: c_int = 8;

/* ==================== GP (Graphics Parameters) indices ==================== */

pub const GP_FILL: c_int = 0;
pub const GP_COL: c_int = 1;
pub const GP_GAMMA: c_int = 2;
pub const GP_LTY: c_int = 3;
pub const GP_LWD: c_int = 4;
pub const GP_CEX: c_int = 5;
pub const GP_FONTSIZE: c_int = 6;
pub const GP_LINEHEIGHT: c_int = 7;
pub const GP_FONT: c_int = 8;
pub const GP_FONTFAMILY: c_int = 9;
pub const GP_ALPHA: c_int = 10;
pub const GP_LINEEND: c_int = 11;
pub const GP_LINEJOIN: c_int = 12;
pub const GP_LINEMITRE: c_int = 13;
pub const GP_LEX: c_int = 14;
pub const GP_FONTFACE: c_int = 15;

/* ==================== Arrow description indices ==================== */

pub const GRID_ARROWANGLE: c_int = 0;
pub const GRID_ARROWLENGTH: c_int = 1;
pub const GRID_ARROWENDS: c_int = 2;
pub const GRID_ARROWTYPE: c_int = 3;

/* ==================== Unit helpers ==================== */

/// uValue macro equivalent: get the numeric value of a unit.
#[inline]
pub unsafe fn uValue(x: SEXP) -> c_double {
    use crate::sexp::accessors::{REAL, VECTOR_ELT};
    *REAL(VECTOR_ELT(x, 0)).add(0)
}

/// uData macro equivalent: get the data component of a unit.
#[inline]
pub unsafe fn uData(x: SEXP) -> SEXP {
    use crate::sexp::accessors::VECTOR_ELT;
    VECTOR_ELT(x, 1)
}

/// uUnit macro equivalent: get the integer unit type of a unit.
#[inline]
pub unsafe fn uUnit(x: SEXP) -> c_int {
    use crate::sexp::accessors::{INTEGER, VECTOR_ELT};
    *INTEGER(VECTOR_ELT(x, 2)).add(0)
}

/* ==================== Type aliases ==================== */

/// 3x3 transformation matrix for 2D affine transforms.
pub type LTransform = [[f64; 3]; 3];

/// A location in homogeneous coordinates [x, y, 1].
pub type LLocation = [f64; 3];

/* ==================== Enums ==================== */

/// Arithmetic mode for null units.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LNullArithmeticMode {
    L_adding = 1,
    L_subtracting = 2,
    L_summing = 3,
    L_plain = 4,
    L_maximising = 5,
    L_minimising = 6,
    L_multiplying = 7,
}

/// Grid unit types. Order must match strings in unit.R.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LUnit {
    L_NPC = 0,
    L_CM = 1,
    L_INCHES = 2,
    L_LINES = 3,
    L_NATIVE = 4,
    L_NULL = 5,
    L_SNPC = 6,
    L_MM = 7,
    L_POINTS = 8,
    L_PICAS = 9,
    L_BIGPOINTS = 10,
    L_DIDA = 11,
    L_CICERO = 12,
    L_SCALEDPOINTS = 13,
    L_STRINGWIDTH = 14,
    L_STRINGHEIGHT = 15,
    L_STRINGASCENT = 16,
    L_STRINGDESCENT = 17,
    L_CHAR = 18,
    L_GROBX = 19,
    L_GROBY = 20,
    L_GROBWIDTH = 21,
    L_GROBHEIGHT = 22,
    L_GROBASCENT = 23,
    L_GROBDESCENT = 24,
    L_MYLINES = 103,
    L_MYCHAR = 104,
    L_MYSTRINGWIDTH = 105,
    L_MYSTRINGHEIGHT = 106,
    L_SUM = 201,
    L_MIN = 202,
    L_MAX = 203,
}

/// Justification values.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LJustification {
    L_LEFT = 0,
    L_RIGHT = 1,
    L_BOTTOM = 2,
    L_TOP = 3,
    L_CENTRE = 4,
    L_CENTER = 5,
}

/* ==================== Unit classification functions ==================== */

/// Check if a unit type is an absolute unit.
#[inline]
pub const fn isAbsolute(x: c_int) -> bool {
    (x > 1000)
        || ((x >= LUnit::L_MYLINES as c_int) && (x <= LUnit::L_MYSTRINGHEIGHT as c_int))
        || ((x < LUnit::L_GROBX as c_int)
            && (x > LUnit::L_NPC as c_int)
            && (x != LUnit::L_NATIVE as c_int)
            && (x != LUnit::L_SNPC as c_int))
}

/// Check if a unit type is an arithmetic unit.
#[inline]
pub const fn isArith(x: c_int) -> bool {
    (x >= LUnit::L_SUM as c_int) && (x <= LUnit::L_MAX as c_int)
}

/// Check if a unit type is a string-based unit.
#[inline]
pub const fn isStringUnit(x: c_int) -> bool {
    (x >= LUnit::L_STRINGWIDTH as c_int) && (x <= LUnit::L_STRINGDESCENT as c_int)
}

/// Check if a unit type is a grob-based unit.
#[inline]
pub const fn isGrobUnit(x: c_int) -> bool {
    (x >= LUnit::L_GROBX as c_int) && (x <= LUnit::L_GROBDESCENT as c_int)
}

/* ==================== Structs ==================== */

/// An arbitrarily-oriented rectangle.
/// The vertices are assumed to be in order going anticlockwise.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct LRect {
    pub x1: f64,
    pub x2: f64,
    pub x3: f64,
    pub x4: f64,
    pub y1: f64,
    pub y2: f64,
    pub y3: f64,
    pub y4: f64,
}

/// A description of the location of a viewport.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct LViewportLocation {
    pub x: SEXP,
    pub y: SEXP,
    pub width: SEXP,
    pub height: SEXP,
    pub hjust: f64,
    pub vjust: f64,
}

/// Components of a viewport which provide coordinate information for children.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct LViewportContext {
    pub xscalemin: f64,
    pub xscalemax: f64,
    pub yscalemin: f64,
    pub yscalemax: f64,
}

/* ==================== GE stubs ==================== */

/// Stub for pGEDevDesc (graphics engine device descriptor pointer).
/// Will be replaced when the GE is ported.
pub type pGEDevDesc = *mut c_void;

/// Stub for pGEcontext (graphics engine context pointer).
/// Will be replaced when the GE is ported.
pub type pGEcontext = *const c_void;

/// Stub for GEevent type.
pub type GEevent = c_int;

/* ==================== Global state ==================== */

/// Grid registration index (set by the graphics engine).
pub static mut gridRegisterIndex: c_int = 0;

/// Grid evaluation environment.
pub static mut R_gridEvalEnv: SEXP = std::ptr::null_mut();
