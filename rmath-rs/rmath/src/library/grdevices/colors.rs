
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/colors.c
 *
 *  Color specification, conversion, and palette management.
 *  This should be regarded as part of the graphics engine.
 */

use std::cell::Cell;
use std::os::raw::{c_char, c_double, c_int, c_uint};

use crate::attrib_core::{R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, asLogical, asReal, coerceVector};
use crate::main::errors::Rf_error;
use crate::main::relop::NA_STRING;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{ISNAN, NA_INTEGER, R_FINITE, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const WHITE_X: c_double = 95.047;
const WHITE_Y: c_double = 100.000;
const WHITE_Z: c_double = 108.883;
const WHITE_u: c_double = 0.1978398;
const WHITE_v: c_double = 0.4683363;
const GAMMA: c_double = 2.4;
const DEG2RAD: c_double = std::f64::consts::PI / 180.0;
const MAX_PALETTE_SIZE: usize = 1024;
const NA_LOGICAL: c_int = NA_INTEGER;

// ---------------------------------------------------------------------------
// Color type and macros
// ---------------------------------------------------------------------------

type rcolor = u32;

const fn R_RED(col: rcolor) -> c_int {
    (col & 0xFF) as c_int
}
const fn R_GREEN(col: rcolor) -> c_int {
    ((col >> 8) & 0xFF) as c_int
}
const fn R_BLUE(col: rcolor) -> c_int {
    ((col >> 16) & 0xFF) as c_int
}
const fn R_ALPHA(col: rcolor) -> c_int {
    ((col >> 24) & 0xFF) as c_int
}

const fn R_RGB(r: c_int, g: c_int, b: c_int) -> rcolor {
    (r as u32 & 0xFF) | ((g as u32 & 0xFF) << 8) | ((b as u32 & 0xFF) << 16) | (0xFF_u32 << 24)
}

const fn R_RGBA(r: c_int, g: c_int, b: c_int, a: c_int) -> rcolor {
    (r as u32 & 0xFF)
        | ((g as u32 & 0xFF) << 8)
        | ((b as u32 & 0xFF) << 16)
        | ((a as u32 & 0xFF) << 24)
}

const fn R_OPAQUE(col: rcolor) -> c_int {
    ((col & 0xFF000000) == 0xFF000000) as c_int
}
const fn R_TRANSPARENT(col: rcolor) -> c_int {
    ((col & 0xFF000000) == 0) as c_int
}
const R_TRANWHITE: rcolor = 0x00FFFFFF;

// ---------------------------------------------------------------------------
// Default palette
// ---------------------------------------------------------------------------

const DEFAULT_PALETTE: [rcolor; 8] = [
    0xff000000, 0xff6b53df, 0xff4fd061, 0xffe69722, 0xffe5e228, 0xffbc0bcd, 0xff10c7f5, 0xff9e9e9e,
];

// ---------------------------------------------------------------------------
// Static palette state
// ---------------------------------------------------------------------------

const PALETTE_INIT: [rcolor; 32] = [
    0xff000000, 0xff6b53df, 0xff4fd061, 0xffe69722, 0xffe5e228, 0xffbc0bcd, 0xff10c7f5, 0xff9e9e9e,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

thread_local! {
    static PALETTE_SIZE: Cell<c_int> = Cell::new(8);
    static PALETTE: Cell<[rcolor; MAX_PALETTE_SIZE]> = Cell::new({
        let mut arr = [0u32; MAX_PALETTE_SIZE];
        let mut i = 0;
        while i < PALETTE_INIT.len() {
            arr[i] = PALETTE_INIT[i];
            i += 1;
        }
        arr
    });
    static PALETTE0: Cell<[rcolor; MAX_PALETTE_SIZE]> = Cell::new([0; MAX_PALETTE_SIZE]);
}

// ---------------------------------------------------------------------------
// Thread-local return buffers
// ---------------------------------------------------------------------------

thread_local! {
    static COL_BUF: std::cell::RefCell<[u8; 10]> = std::cell::RefCell::new([0u8; 10]);
}

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

unsafe fn streql(a: *const c_char, b: *const c_char) -> c_int {
    if a.is_null() || b.is_null() {
        return 0;
    }
    if libc::strcmp(a, b) == 0 { 1 } else { 0 }
}

unsafe fn isMatrix(x: SEXP) -> bool {
    let dim = getAttrib(x, R_DimSymbol());
    Rf_isNull(dim) == 0 && LENGTH(dim) == 2
}

unsafe fn RGB2rgb_func(r: u32, g: u32, b: u32) -> *const c_char {
    COL_BUF.with(|buf| {
        let mut c = buf.borrow_mut();
        c[0] = b'#';
        c[1] = HEX_DIGITS[((r >> 4) & 0x0F) as usize];
        c[2] = HEX_DIGITS[(r & 0x0F) as usize];
        c[3] = HEX_DIGITS[((g >> 4) & 0x0F) as usize];
        c[4] = HEX_DIGITS[(g & 0x0F) as usize];
        c[5] = HEX_DIGITS[((b >> 4) & 0x0F) as usize];
        c[6] = HEX_DIGITS[(b & 0x0F) as usize];
        c[7] = 0;
        c.as_ptr() as *const c_char
    })
}

unsafe fn RGBA2rgb_func(r: u32, g: u32, b: u32, a: u32) -> *const c_char {
    COL_BUF.with(|buf| {
        let mut c = buf.borrow_mut();
        c[0] = b'#';
        c[1] = HEX_DIGITS[((r >> 4) & 0x0F) as usize];
        c[2] = HEX_DIGITS[(r & 0x0F) as usize];
        c[3] = HEX_DIGITS[((g >> 4) & 0x0F) as usize];
        c[4] = HEX_DIGITS[(g & 0x0F) as usize];
        c[5] = HEX_DIGITS[((b >> 4) & 0x0F) as usize];
        c[6] = HEX_DIGITS[(b & 0x0F) as usize];
        c[7] = HEX_DIGITS[((a >> 4) & 0x0F) as usize];
        c[8] = HEX_DIGITS[(a & 0x0F) as usize];
        c[9] = 0;
        c.as_ptr() as *const c_char
    })
}

unsafe fn incol2name_buf_opaque(col: rcolor) -> *const c_char {
    COL_BUF.with(|buf| {
        let mut c = buf.borrow_mut();
        c[0] = b'#';
        c[1] = HEX_DIGITS[((col >> 4) & 0x0F) as usize];
        c[2] = HEX_DIGITS[(col & 0x0F) as usize];
        c[3] = HEX_DIGITS[((col >> 12) & 0x0F) as usize];
        c[4] = HEX_DIGITS[((col >> 8) & 0x0F) as usize];
        c[5] = HEX_DIGITS[((col >> 20) & 0x0F) as usize];
        c[6] = HEX_DIGITS[((col >> 16) & 0x0F) as usize];
        c[7] = 0;
        c.as_ptr() as *const c_char
    })
}

unsafe fn incol2name_buf_trans(col: rcolor) -> *const c_char {
    COL_BUF.with(|buf| {
        let mut c = buf.borrow_mut();
        c[0] = b'#';
        c[1] = HEX_DIGITS[((col >> 4) & 0x0F) as usize];
        c[2] = HEX_DIGITS[(col & 0x0F) as usize];
        c[3] = HEX_DIGITS[((col >> 12) & 0x0F) as usize];
        c[4] = HEX_DIGITS[((col >> 8) & 0x0F) as usize];
        c[5] = HEX_DIGITS[((col >> 20) & 0x0F) as usize];
        c[6] = HEX_DIGITS[((col >> 16) & 0x0F) as usize];
        c[7] = HEX_DIGITS[((col >> 28) & 0x0F) as usize];
        c[8] = HEX_DIGITS[((col >> 24) & 0x0F) as usize];
        c[9] = 0;
        c.as_ptr() as *const c_char
    })
}

unsafe fn ScaleColor(x: c_double) -> u32 {
    if ISNAN(x) {
        Rf_error(b"color intensity NA, not in [0,1]\0".as_ptr() as *const c_char);
    }
    if !R_FINITE(x) || x < 0.0 || x > 1.0 {
        Rf_error(b"color intensity not in [0,1]\0".as_ptr() as *const c_char);
    }
    (255.0 * x + 0.5) as u32
}

unsafe fn CheckColor(x: c_int) -> u32 {
    if x == NA_INTEGER {
        Rf_error(b"color intensity NA, not in 0:255\0".as_ptr() as *const c_char);
    }
    if x < 0 || x > 255 {
        Rf_error(b"color intensity not in 0:255\0".as_ptr() as *const c_char);
    }
    x as u32
}

unsafe fn ScaleAlpha(x: c_double) -> u32 {
    if ISNAN(x) {
        Rf_error(b"alpha level NA, not in [0,1]\0".as_ptr() as *const c_char);
    }
    if !R_FINITE(x) || x < 0.0 || x > 1.0 {
        Rf_error(b"alpha level not in [0,1]\0".as_ptr() as *const c_char);
    }
    (255.0 * x + 0.5) as u32
}

unsafe fn CheckAlpha(x: c_int) -> u32 {
    if x == NA_INTEGER {
        Rf_error(b"alpha level NA, not in 0:255\0".as_ptr() as *const c_char);
    }
    if x < 0 || x > 255 {
        Rf_error(b"alpha level not in 0:255\0".as_ptr() as *const c_char);
    }
    x as u32
}

// ---------------------------------------------------------------------------
// HSV <-> RGB conversion
// ---------------------------------------------------------------------------

unsafe fn hsv2rgb(
    h: c_double,
    s: c_double,
    v: c_double,
    r: &mut c_double,
    g: &mut c_double,
    b: &mut c_double,
) {
    if !R_FINITE(h) || !R_FINITE(s) || !R_FINITE(v) {
        Rf_error(b"inputs must be finite\0".as_ptr() as *const c_char);
    }
    let (f, t_val) = libm::modf(h * 6.0);
    let i = (t_val as c_int) % 6;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match i {
        0 => {
            *r = v;
            *g = t;
            *b = p;
        }
        1 => {
            *r = q;
            *g = v;
            *b = p;
        }
        2 => {
            *r = p;
            *g = v;
            *b = t;
        }
        3 => {
            *r = p;
            *g = q;
            *b = v;
        }
        4 => {
            *r = t;
            *g = p;
            *b = v;
        }
        5 => {
            *r = v;
            *g = p;
            *b = q;
        }
        _ => {
            Rf_error(b"bad hsv to rgb color conversion\0".as_ptr() as *const c_char);
        }
    }
}

unsafe fn rgb2hsv(
    r: c_double,
    g: c_double,
    b: c_double,
    h: &mut c_double,
    s: &mut c_double,
    v: &mut c_double,
) {
    let mut min = r;
    let mut max = r;
    let mut r_max = true;
    let mut _b_max = false;

    if min > g {
        if b < g {
            min = b;
        } else {
            min = g;
            if b > r {
                max = b;
                _b_max = true;
                r_max = false;
            }
        }
    } else {
        if b > g {
            max = b;
            _b_max = true;
            r_max = false;
        } else {
            max = g;
            r_max = false;
            if b < r {
                min = b;
            }
        }
    }

    *v = max;
    let delta = max - min;
    if max == 0.0 || delta == 0.0 {
        *s = 0.0;
        *h = 0.0;
        return;
    }
    *s = delta / max;

    if r_max {
        *h = (g - b) / delta;
    } else if _b_max {
        *h = 4.0 + (r - g) / delta;
    } else {
        *h = 2.0 + (b - r) / delta;
    }
    *h /= 6.0;
    if *h < 0.0 {
        *h += 1.0;
    }
}

// ---------------------------------------------------------------------------
// HCL -> RGB conversion
// ---------------------------------------------------------------------------

unsafe fn gtrans(u: c_double) -> c_double {
    if u > 0.00304 {
        1.055 * u.powf(1.0 / GAMMA) - 0.055
    } else {
        12.92 * u
    }
}

unsafe fn FixupColor(r: &mut c_int, g: &mut c_int, b: &mut c_int) -> c_int {
    let mut fix = 0;
    if *r < 0 {
        *r = 0;
        fix = 1;
    } else if *r > 255 {
        *r = 255;
        fix = 1;
    }
    if *g < 0 {
        *g = 0;
        fix = 1;
    } else if *g > 255 {
        *g = 255;
        fix = 1;
    }
    if *b < 0 {
        *b = 0;
        fix = 1;
    } else if *b > 255 {
        *b = 255;
        fix = 1;
    }
    fix
}

unsafe fn hcl2rgb(
    h: c_double,
    c: c_double,
    l: c_double,
    rv: &mut c_double,
    gv: &mut c_double,
    bv: &mut c_double,
) {
    if l <= 0.0 {
        *rv = 0.0;
        *gv = 0.0;
        *bv = 0.0;
        return;
    }

    let rad = DEG2RAD * h;
    let _lu = l;
    let uu = c * rad.cos();
    let vv = c * rad.sin();

    let (x, y, z);
    if l <= 0.0 && uu == 0.0 && vv == 0.0 {
        x = 0.0;
        y = 0.0;
        z = 0.0;
    } else {
        y = WHITE_Y
            * if l > 7.999592 {
                ((l + 16.0) / 116.0).powi(3)
            } else {
                l / 903.3
            };
        let u2 = uu / (13.0 * l) + WHITE_u;
        let v2 = vv / (13.0 * l) + WHITE_v;
        x = 9.0 * y * u2 / (4.0 * v2);
        z = -x / 3.0 - 5.0 * y + 3.0 * y / v2;
    }

    *rv = gtrans((3.240479 * x - 1.537150 * y - 0.498535 * z) / WHITE_Y);
    *gv = gtrans((-0.969256 * x + 1.875992 * y + 0.041556 * z) / WHITE_Y);
    *bv = gtrans((0.055648 * x - 0.204043 * y + 1.057311 * z) / WHITE_Y);
}

// ---------------------------------------------------------------------------
// String matching
// ---------------------------------------------------------------------------

unsafe fn StrMatch(s: *const c_char, t: *const c_char) -> c_int {
    let mut si = 0usize;
    let mut ti = 0usize;
    loop {
        let sc = *s.add(si);
        let tc = *t.add(ti);
        if sc == 0 && tc == 0 {
            return 1;
        }
        if sc == b' ' as libc::c_char {
            si += 1;
            continue;
        }
        if tc == b' ' as libc::c_char {
            ti += 1;
            continue;
        }
        if libc::tolower(sc as c_int) != libc::tolower(tc as c_int) {
            return 0;
        }
        si += 1;
        ti += 1;
    }
}

// ---------------------------------------------------------------------------
// Hex digit conversion
// ---------------------------------------------------------------------------

unsafe fn hexdigit(d: c_int) -> u32 {
    if d >= b'0' as c_int && d <= b'9' as c_int {
        return (d - b'0' as c_int) as u32;
    }
    if d >= b'A' as c_int && d <= b'F' as c_int {
        return (10 + d - b'A' as c_int) as u32;
    }
    if d >= b'a' as c_int && d <= b'f' as c_int {
        return (10 + d - b'a' as c_int) as u32;
    }
    Rf_error(b"invalid hex digit in 'color' or 'lty'\0".as_ptr() as *const c_char);
    0
}

// ---------------------------------------------------------------------------
// Color specification parsing
// ---------------------------------------------------------------------------

unsafe fn rgb2col(rgb: *const c_char) -> rcolor {
    if *rgb != b'#' as libc::c_char {
        Rf_error(b"invalid RGB specification\0".as_ptr() as *const c_char);
    }
    let len = libc::strlen(rgb);
    let mut r: u32 = 0;
    let mut g: u32 = 0;
    let mut b: u32 = 0;
    let mut a: u32 = 0;

    // Parse r, g, b and optionally a based on string length
    match len {
        9 => {
            a = 16 * hexdigit(*rgb.add(7) as c_int) + hexdigit(*rgb.add(8) as c_int);
            r = 16 * hexdigit(*rgb.add(1) as c_int) + hexdigit(*rgb.add(2) as c_int);
            g = 16 * hexdigit(*rgb.add(3) as c_int) + hexdigit(*rgb.add(4) as c_int);
            b = 16 * hexdigit(*rgb.add(5) as c_int) + hexdigit(*rgb.add(6) as c_int);
            R_RGBA(r as c_int, g as c_int, b as c_int, a as c_int)
        }
        7 => {
            r = 16 * hexdigit(*rgb.add(1) as c_int) + hexdigit(*rgb.add(2) as c_int);
            g = 16 * hexdigit(*rgb.add(3) as c_int) + hexdigit(*rgb.add(4) as c_int);
            b = 16 * hexdigit(*rgb.add(5) as c_int) + hexdigit(*rgb.add(6) as c_int);
            R_RGB(r as c_int, g as c_int, b as c_int)
        }
        5 => {
            a = 17 * hexdigit(*rgb.add(4) as c_int);
            r = 17 * hexdigit(*rgb.add(1) as c_int);
            g = 17 * hexdigit(*rgb.add(2) as c_int);
            b = 17 * hexdigit(*rgb.add(3) as c_int);
            R_RGBA(r as c_int, g as c_int, b as c_int, a as c_int)
        }
        4 => {
            r = 17 * hexdigit(*rgb.add(1) as c_int);
            g = 17 * hexdigit(*rgb.add(2) as c_int);
            b = 17 * hexdigit(*rgb.add(3) as c_int);
            R_RGB(r as c_int, g as c_int, b as c_int)
        }
        _ => {
            Rf_error(b"invalid RGB specification\0".as_ptr() as *const c_char);
            0
        }
    }
}

// ---------------------------------------------------------------------------
// Color name database
// ---------------------------------------------------------------------------

struct ColorDataBaseEntry {
    name: &'static [u8],
    code: rcolor,
}

static COLOR_DATA_BASE: &[ColorDataBaseEntry] = &COLOR_DATA;

// The actual database is defined as a const array below
const COLOR_DATA: [ColorDataBaseEntry; 657] = [
    ColorDataBaseEntry {
        name: b"white\0",
        code: 0xffffffff,
    },
    ColorDataBaseEntry {
        name: b"aliceblue\0",
        code: 0xfffff8f0,
    },
    ColorDataBaseEntry {
        name: b"antiquewhite\0",
        code: 0xffd7ebfa,
    },
    ColorDataBaseEntry {
        name: b"antiquewhite1\0",
        code: 0xffdbefff,
    },
    ColorDataBaseEntry {
        name: b"antiquewhite2\0",
        code: 0xffccdfee,
    },
    ColorDataBaseEntry {
        name: b"antiquewhite3\0",
        code: 0xffb0c0cd,
    },
    ColorDataBaseEntry {
        name: b"antiquewhite4\0",
        code: 0xff78838b,
    },
    ColorDataBaseEntry {
        name: b"aquamarine\0",
        code: 0xffd4ff7f,
    },
    ColorDataBaseEntry {
        name: b"aquamarine1\0",
        code: 0xffd4ff7f,
    },
    ColorDataBaseEntry {
        name: b"aquamarine2\0",
        code: 0xffc6ee76,
    },
    ColorDataBaseEntry {
        name: b"aquamarine3\0",
        code: 0xffaacd66,
    },
    ColorDataBaseEntry {
        name: b"aquamarine4\0",
        code: 0xff748b45,
    },
    ColorDataBaseEntry {
        name: b"azure\0",
        code: 0xfffffff0,
    },
    ColorDataBaseEntry {
        name: b"azure1\0",
        code: 0xfffffff0,
    },
    ColorDataBaseEntry {
        name: b"azure2\0",
        code: 0xffeeeee0,
    },
    ColorDataBaseEntry {
        name: b"azure3\0",
        code: 0xffcdcdc1,
    },
    ColorDataBaseEntry {
        name: b"azure4\0",
        code: 0xff8b8b83,
    },
    ColorDataBaseEntry {
        name: b"beige\0",
        code: 0xffdcf5f5,
    },
    ColorDataBaseEntry {
        name: b"bisque\0",
        code: 0xffc4e4ff,
    },
    ColorDataBaseEntry {
        name: b"bisque1\0",
        code: 0xffc4e4ff,
    },
    ColorDataBaseEntry {
        name: b"bisque2\0",
        code: 0xffb7d5ee,
    },
    ColorDataBaseEntry {
        name: b"bisque3\0",
        code: 0xff9eb7cd,
    },
    ColorDataBaseEntry {
        name: b"bisque4\0",
        code: 0xff6b7d8b,
    },
    ColorDataBaseEntry {
        name: b"black\0",
        code: 0xff000000,
    },
    ColorDataBaseEntry {
        name: b"blanchedalmond\0",
        code: 0xffcdebff,
    },
    ColorDataBaseEntry {
        name: b"blue\0",
        code: 0xffff0000,
    },
    ColorDataBaseEntry {
        name: b"blue1\0",
        code: 0xffff0000,
    },
    ColorDataBaseEntry {
        name: b"blue2\0",
        code: 0xffee0000,
    },
    ColorDataBaseEntry {
        name: b"blue3\0",
        code: 0xffcd0000,
    },
    ColorDataBaseEntry {
        name: b"blue4\0",
        code: 0xff8b0000,
    },
    ColorDataBaseEntry {
        name: b"blueviolet\0",
        code: 0xffe22b8a,
    },
    ColorDataBaseEntry {
        name: b"brown\0",
        code: 0xff2a2aa5,
    },
    ColorDataBaseEntry {
        name: b"brown1\0",
        code: 0xff4040ff,
    },
    ColorDataBaseEntry {
        name: b"brown2\0",
        code: 0xff3b3bee,
    },
    ColorDataBaseEntry {
        name: b"brown3\0",
        code: 0xff3333cd,
    },
    ColorDataBaseEntry {
        name: b"brown4\0",
        code: 0xff23238b,
    },
    ColorDataBaseEntry {
        name: b"burlywood\0",
        code: 0xff87b8de,
    },
    ColorDataBaseEntry {
        name: b"burlywood1\0",
        code: 0xff9bd3ff,
    },
    ColorDataBaseEntry {
        name: b"burlywood2\0",
        code: 0xff91c5ee,
    },
    ColorDataBaseEntry {
        name: b"burlywood3\0",
        code: 0xff7daacd,
    },
    ColorDataBaseEntry {
        name: b"burlywood4\0",
        code: 0xff55738b,
    },
    ColorDataBaseEntry {
        name: b"cadetblue\0",
        code: 0xffa09e5f,
    },
    ColorDataBaseEntry {
        name: b"cadetblue1\0",
        code: 0xfffff598,
    },
    ColorDataBaseEntry {
        name: b"cadetblue2\0",
        code: 0xffeee58e,
    },
    ColorDataBaseEntry {
        name: b"cadetblue3\0",
        code: 0xffcdc57a,
    },
    ColorDataBaseEntry {
        name: b"cadetblue4\0",
        code: 0xff8b8653,
    },
    ColorDataBaseEntry {
        name: b"chartreuse\0",
        code: 0xff00ff7f,
    },
    ColorDataBaseEntry {
        name: b"chartreuse1\0",
        code: 0xff00ff7f,
    },
    ColorDataBaseEntry {
        name: b"chartreuse2\0",
        code: 0xff00ee76,
    },
    ColorDataBaseEntry {
        name: b"chartreuse3\0",
        code: 0xff00cd66,
    },
    ColorDataBaseEntry {
        name: b"chartreuse4\0",
        code: 0xff008b45,
    },
    ColorDataBaseEntry {
        name: b"chocolate\0",
        code: 0xff1e69d2,
    },
    ColorDataBaseEntry {
        name: b"chocolate1\0",
        code: 0xff247fff,
    },
    ColorDataBaseEntry {
        name: b"chocolate2\0",
        code: 0xff2176ee,
    },
    ColorDataBaseEntry {
        name: b"chocolate3\0",
        code: 0xff1d66cd,
    },
    ColorDataBaseEntry {
        name: b"chocolate4\0",
        code: 0xff13458b,
    },
    ColorDataBaseEntry {
        name: b"coral\0",
        code: 0xff507fff,
    },
    ColorDataBaseEntry {
        name: b"coral1\0",
        code: 0xff5672ff,
    },
    ColorDataBaseEntry {
        name: b"coral2\0",
        code: 0xff506aee,
    },
    ColorDataBaseEntry {
        name: b"coral3\0",
        code: 0xff455bcd,
    },
    ColorDataBaseEntry {
        name: b"coral4\0",
        code: 0xff2f3e8b,
    },
    ColorDataBaseEntry {
        name: b"cornflowerblue\0",
        code: 0xffed9564,
    },
    ColorDataBaseEntry {
        name: b"cornsilk\0",
        code: 0xffdcf8ff,
    },
    ColorDataBaseEntry {
        name: b"cornsilk1\0",
        code: 0xffdcf8ff,
    },
    ColorDataBaseEntry {
        name: b"cornsilk2\0",
        code: 0xffcde8ee,
    },
    ColorDataBaseEntry {
        name: b"cornsilk3\0",
        code: 0xffb1c8cd,
    },
    ColorDataBaseEntry {
        name: b"cornsilk4\0",
        code: 0xff78888b,
    },
    ColorDataBaseEntry {
        name: b"cyan\0",
        code: 0xffffff00,
    },
    ColorDataBaseEntry {
        name: b"cyan1\0",
        code: 0xffffff00,
    },
    ColorDataBaseEntry {
        name: b"cyan2\0",
        code: 0xffeeee00,
    },
    ColorDataBaseEntry {
        name: b"cyan3\0",
        code: 0xffcdcd00,
    },
    ColorDataBaseEntry {
        name: b"cyan4\0",
        code: 0xff8b8b00,
    },
    ColorDataBaseEntry {
        name: b"darkblue\0",
        code: 0xff8b0000,
    },
    ColorDataBaseEntry {
        name: b"darkcyan\0",
        code: 0xff8b8b00,
    },
    ColorDataBaseEntry {
        name: b"darkgoldenrod\0",
        code: 0xff0b86b8,
    },
    ColorDataBaseEntry {
        name: b"darkgoldenrod1\0",
        code: 0xff0fb9ff,
    },
    ColorDataBaseEntry {
        name: b"darkgoldenrod2\0",
        code: 0xff0eadee,
    },
    ColorDataBaseEntry {
        name: b"darkgoldenrod3\0",
        code: 0xff0c95cd,
    },
    ColorDataBaseEntry {
        name: b"darkgoldenrod4\0",
        code: 0xff08658b,
    },
    ColorDataBaseEntry {
        name: b"darkgray\0",
        code: 0xffa9a9a9,
    },
    ColorDataBaseEntry {
        name: b"darkgreen\0",
        code: 0xff006400,
    },
    ColorDataBaseEntry {
        name: b"darkgrey\0",
        code: 0xffa9a9a9,
    },
    ColorDataBaseEntry {
        name: b"darkkhaki\0",
        code: 0xff6bb7bd,
    },
    ColorDataBaseEntry {
        name: b"darkmagenta\0",
        code: 0xff8b008b,
    },
    ColorDataBaseEntry {
        name: b"darkolivegreen\0",
        code: 0xff2f6b55,
    },
    ColorDataBaseEntry {
        name: b"darkolivegreen1\0",
        code: 0xff70ffca,
    },
    ColorDataBaseEntry {
        name: b"darkolivegreen2\0",
        code: 0xff68eebc,
    },
    ColorDataBaseEntry {
        name: b"darkolivegreen3\0",
        code: 0xff5acda2,
    },
    ColorDataBaseEntry {
        name: b"darkolivegreen4\0",
        code: 0xff3d8b6e,
    },
    ColorDataBaseEntry {
        name: b"darkorange\0",
        code: 0xff008cff,
    },
    ColorDataBaseEntry {
        name: b"darkorange1\0",
        code: 0xff007fff,
    },
    ColorDataBaseEntry {
        name: b"darkorange2\0",
        code: 0xff0076ee,
    },
    ColorDataBaseEntry {
        name: b"darkorange3\0",
        code: 0xff0066cd,
    },
    ColorDataBaseEntry {
        name: b"darkorange4\0",
        code: 0xff00458b,
    },
    ColorDataBaseEntry {
        name: b"darkorchid\0",
        code: 0xffcc3299,
    },
    ColorDataBaseEntry {
        name: b"darkorchid1\0",
        code: 0xffff3ebf,
    },
    ColorDataBaseEntry {
        name: b"darkorchid2\0",
        code: 0xffee3ab2,
    },
    ColorDataBaseEntry {
        name: b"darkorchid3\0",
        code: 0xffcd329a,
    },
    ColorDataBaseEntry {
        name: b"darkorchid4\0",
        code: 0xff8b2268,
    },
    ColorDataBaseEntry {
        name: b"darkred\0",
        code: 0xff00008b,
    },
    ColorDataBaseEntry {
        name: b"darksalmon\0",
        code: 0xff7a96e9,
    },
    ColorDataBaseEntry {
        name: b"darkseagreen\0",
        code: 0xff8fbc8f,
    },
    ColorDataBaseEntry {
        name: b"darkseagreen1\0",
        code: 0xffc1ffc1,
    },
    ColorDataBaseEntry {
        name: b"darkseagreen2\0",
        code: 0xffb4eeb4,
    },
    ColorDataBaseEntry {
        name: b"darkseagreen3\0",
        code: 0xff9bcd9b,
    },
    ColorDataBaseEntry {
        name: b"darkseagreen4\0",
        code: 0xff698b69,
    },
    ColorDataBaseEntry {
        name: b"darkslateblue\0",
        code: 0xff8b3d48,
    },
    ColorDataBaseEntry {
        name: b"darkslategray\0",
        code: 0xff4f4f2f,
    },
    ColorDataBaseEntry {
        name: b"darkslategray1\0",
        code: 0xffffff97,
    },
    ColorDataBaseEntry {
        name: b"darkslategray2\0",
        code: 0xffeeee8d,
    },
    ColorDataBaseEntry {
        name: b"darkslategray3\0",
        code: 0xffcdcd79,
    },
    ColorDataBaseEntry {
        name: b"darkslategray4\0",
        code: 0xff8b8b52,
    },
    ColorDataBaseEntry {
        name: b"darkslategrey\0",
        code: 0xff4f4f2f,
    },
    ColorDataBaseEntry {
        name: b"darkturquoise\0",
        code: 0xffd1ce00,
    },
    ColorDataBaseEntry {
        name: b"darkviolet\0",
        code: 0xffd30094,
    },
    ColorDataBaseEntry {
        name: b"deeppink\0",
        code: 0xff9314ff,
    },
    ColorDataBaseEntry {
        name: b"deeppink1\0",
        code: 0xff9314ff,
    },
    ColorDataBaseEntry {
        name: b"deeppink2\0",
        code: 0xff8912ee,
    },
    ColorDataBaseEntry {
        name: b"deeppink3\0",
        code: 0xff7610cd,
    },
    ColorDataBaseEntry {
        name: b"deeppink4\0",
        code: 0xff500a8b,
    },
    ColorDataBaseEntry {
        name: b"deepskyblue\0",
        code: 0xffffbf00,
    },
    ColorDataBaseEntry {
        name: b"deepskyblue1\0",
        code: 0xffffbf00,
    },
    ColorDataBaseEntry {
        name: b"deepskyblue2\0",
        code: 0xffeeb200,
    },
    ColorDataBaseEntry {
        name: b"deepskyblue3\0",
        code: 0xffcd9a00,
    },
    ColorDataBaseEntry {
        name: b"deepskyblue4\0",
        code: 0xff8b6800,
    },
    ColorDataBaseEntry {
        name: b"dimgray\0",
        code: 0xff696969,
    },
    ColorDataBaseEntry {
        name: b"dimgrey\0",
        code: 0xff696969,
    },
    ColorDataBaseEntry {
        name: b"dodgerblue\0",
        code: 0xffff901e,
    },
    ColorDataBaseEntry {
        name: b"dodgerblue1\0",
        code: 0xffff901e,
    },
    ColorDataBaseEntry {
        name: b"dodgerblue2\0",
        code: 0xffee861c,
    },
    ColorDataBaseEntry {
        name: b"dodgerblue3\0",
        code: 0xffcd7418,
    },
    ColorDataBaseEntry {
        name: b"dodgerblue4\0",
        code: 0xff8b4e10,
    },
    ColorDataBaseEntry {
        name: b"firebrick\0",
        code: 0xff2222b2,
    },
    ColorDataBaseEntry {
        name: b"firebrick1\0",
        code: 0xff3030ff,
    },
    ColorDataBaseEntry {
        name: b"firebrick2\0",
        code: 0xff2c2cee,
    },
    ColorDataBaseEntry {
        name: b"firebrick3\0",
        code: 0xff2626cd,
    },
    ColorDataBaseEntry {
        name: b"firebrick4\0",
        code: 0xff1a1a8b,
    },
    ColorDataBaseEntry {
        name: b"floralwhite\0",
        code: 0xfff0faff,
    },
    ColorDataBaseEntry {
        name: b"forestgreen\0",
        code: 0xff228b22,
    },
    ColorDataBaseEntry {
        name: b"gainsboro\0",
        code: 0xffdcdcdc,
    },
    ColorDataBaseEntry {
        name: b"ghostwhite\0",
        code: 0xfffff8f8,
    },
    ColorDataBaseEntry {
        name: b"gold\0",
        code: 0xff00d7ff,
    },
    ColorDataBaseEntry {
        name: b"gold1\0",
        code: 0xff00d7ff,
    },
    ColorDataBaseEntry {
        name: b"gold2\0",
        code: 0xff00c9ee,
    },
    ColorDataBaseEntry {
        name: b"gold3\0",
        code: 0xff00adcd,
    },
    ColorDataBaseEntry {
        name: b"gold4\0",
        code: 0xff00758b,
    },
    ColorDataBaseEntry {
        name: b"goldenrod\0",
        code: 0xff20a5da,
    },
    ColorDataBaseEntry {
        name: b"goldenrod1\0",
        code: 0xff25c1ff,
    },
    ColorDataBaseEntry {
        name: b"goldenrod2\0",
        code: 0xff22b4ee,
    },
    ColorDataBaseEntry {
        name: b"goldenrod3\0",
        code: 0xff1d9bcd,
    },
    ColorDataBaseEntry {
        name: b"goldenrod4\0",
        code: 0xff14698b,
    },
    ColorDataBaseEntry {
        name: b"gray\0",
        code: 0xffbebebe,
    },
    ColorDataBaseEntry {
        name: b"gray0\0",
        code: 0xff000000,
    },
    ColorDataBaseEntry {
        name: b"gray1\0",
        code: 0xff030303,
    },
    ColorDataBaseEntry {
        name: b"gray2\0",
        code: 0xff050505,
    },
    ColorDataBaseEntry {
        name: b"gray3\0",
        code: 0xff080808,
    },
    ColorDataBaseEntry {
        name: b"gray4\0",
        code: 0xff0a0a0a,
    },
    ColorDataBaseEntry {
        name: b"gray5\0",
        code: 0xff0d0d0d,
    },
    ColorDataBaseEntry {
        name: b"gray6\0",
        code: 0xff0f0f0f,
    },
    ColorDataBaseEntry {
        name: b"gray7\0",
        code: 0xff121212,
    },
    ColorDataBaseEntry {
        name: b"gray8\0",
        code: 0xff141414,
    },
    ColorDataBaseEntry {
        name: b"gray9\0",
        code: 0xff171717,
    },
    ColorDataBaseEntry {
        name: b"gray10\0",
        code: 0xff1a1a1a,
    },
    ColorDataBaseEntry {
        name: b"gray11\0",
        code: 0xff1c1c1c,
    },
    ColorDataBaseEntry {
        name: b"gray12\0",
        code: 0xff1f1f1f,
    },
    ColorDataBaseEntry {
        name: b"gray13\0",
        code: 0xff212121,
    },
    ColorDataBaseEntry {
        name: b"gray14\0",
        code: 0xff242424,
    },
    ColorDataBaseEntry {
        name: b"gray15\0",
        code: 0xff262626,
    },
    ColorDataBaseEntry {
        name: b"gray16\0",
        code: 0xff292929,
    },
    ColorDataBaseEntry {
        name: b"gray17\0",
        code: 0xff2b2b2b,
    },
    ColorDataBaseEntry {
        name: b"gray18\0",
        code: 0xff2e2e2e,
    },
    ColorDataBaseEntry {
        name: b"gray19\0",
        code: 0xff303030,
    },
    ColorDataBaseEntry {
        name: b"gray20\0",
        code: 0xff333333,
    },
    ColorDataBaseEntry {
        name: b"gray21\0",
        code: 0xff363636,
    },
    ColorDataBaseEntry {
        name: b"gray22\0",
        code: 0xff383838,
    },
    ColorDataBaseEntry {
        name: b"gray23\0",
        code: 0xff3b3b3b,
    },
    ColorDataBaseEntry {
        name: b"gray24\0",
        code: 0xff3d3d3d,
    },
    ColorDataBaseEntry {
        name: b"gray25\0",
        code: 0xff404040,
    },
    ColorDataBaseEntry {
        name: b"gray26\0",
        code: 0xff424242,
    },
    ColorDataBaseEntry {
        name: b"gray27\0",
        code: 0xff454545,
    },
    ColorDataBaseEntry {
        name: b"gray28\0",
        code: 0xff474747,
    },
    ColorDataBaseEntry {
        name: b"gray29\0",
        code: 0xff4a4a4a,
    },
    ColorDataBaseEntry {
        name: b"gray30\0",
        code: 0xff4d4d4d,
    },
    ColorDataBaseEntry {
        name: b"gray31\0",
        code: 0xff4f4f4f,
    },
    ColorDataBaseEntry {
        name: b"gray32\0",
        code: 0xff525252,
    },
    ColorDataBaseEntry {
        name: b"gray33\0",
        code: 0xff545454,
    },
    ColorDataBaseEntry {
        name: b"gray34\0",
        code: 0xff575757,
    },
    ColorDataBaseEntry {
        name: b"gray35\0",
        code: 0xff595959,
    },
    ColorDataBaseEntry {
        name: b"gray36\0",
        code: 0xff5c5c5c,
    },
    ColorDataBaseEntry {
        name: b"gray37\0",
        code: 0xff5e5e5e,
    },
    ColorDataBaseEntry {
        name: b"gray38\0",
        code: 0xff616161,
    },
    ColorDataBaseEntry {
        name: b"gray39\0",
        code: 0xff636363,
    },
    ColorDataBaseEntry {
        name: b"gray40\0",
        code: 0xff666666,
    },
    ColorDataBaseEntry {
        name: b"gray41\0",
        code: 0xff696969,
    },
    ColorDataBaseEntry {
        name: b"gray42\0",
        code: 0xff6b6b6b,
    },
    ColorDataBaseEntry {
        name: b"gray43\0",
        code: 0xff6e6e6e,
    },
    ColorDataBaseEntry {
        name: b"gray44\0",
        code: 0xff707070,
    },
    ColorDataBaseEntry {
        name: b"gray45\0",
        code: 0xff737373,
    },
    ColorDataBaseEntry {
        name: b"gray46\0",
        code: 0xff757575,
    },
    ColorDataBaseEntry {
        name: b"gray47\0",
        code: 0xff787878,
    },
    ColorDataBaseEntry {
        name: b"gray48\0",
        code: 0xff7a7a7a,
    },
    ColorDataBaseEntry {
        name: b"gray49\0",
        code: 0xff7d7d7d,
    },
    ColorDataBaseEntry {
        name: b"gray50\0",
        code: 0xff7f7f7f,
    },
    ColorDataBaseEntry {
        name: b"gray51\0",
        code: 0xff828282,
    },
    ColorDataBaseEntry {
        name: b"gray52\0",
        code: 0xff858585,
    },
    ColorDataBaseEntry {
        name: b"gray53\0",
        code: 0xff878787,
    },
    ColorDataBaseEntry {
        name: b"gray54\0",
        code: 0xff8a8a8a,
    },
    ColorDataBaseEntry {
        name: b"gray55\0",
        code: 0xff8c8c8c,
    },
    ColorDataBaseEntry {
        name: b"gray56\0",
        code: 0xff8f8f8f,
    },
    ColorDataBaseEntry {
        name: b"gray57\0",
        code: 0xff919191,
    },
    ColorDataBaseEntry {
        name: b"gray58\0",
        code: 0xff949494,
    },
    ColorDataBaseEntry {
        name: b"gray59\0",
        code: 0xff969696,
    },
    ColorDataBaseEntry {
        name: b"gray60\0",
        code: 0xff999999,
    },
    ColorDataBaseEntry {
        name: b"gray61\0",
        code: 0xff9c9c9c,
    },
    ColorDataBaseEntry {
        name: b"gray62\0",
        code: 0xff9e9e9e,
    },
    ColorDataBaseEntry {
        name: b"gray63\0",
        code: 0xffa1a1a1,
    },
    ColorDataBaseEntry {
        name: b"gray64\0",
        code: 0xffa3a3a3,
    },
    ColorDataBaseEntry {
        name: b"gray65\0",
        code: 0xffa6a6a6,
    },
    ColorDataBaseEntry {
        name: b"gray66\0",
        code: 0xffa8a8a8,
    },
    ColorDataBaseEntry {
        name: b"gray67\0",
        code: 0xffababab,
    },
    ColorDataBaseEntry {
        name: b"gray68\0",
        code: 0xffadadad,
    },
    ColorDataBaseEntry {
        name: b"gray69\0",
        code: 0xffb0b0b0,
    },
    ColorDataBaseEntry {
        name: b"gray70\0",
        code: 0xffb3b3b3,
    },
    ColorDataBaseEntry {
        name: b"gray71\0",
        code: 0xffb5b5b5,
    },
    ColorDataBaseEntry {
        name: b"gray72\0",
        code: 0xffb8b8b8,
    },
    ColorDataBaseEntry {
        name: b"gray73\0",
        code: 0xffbababa,
    },
    ColorDataBaseEntry {
        name: b"gray74\0",
        code: 0xffbdbdbd,
    },
    ColorDataBaseEntry {
        name: b"gray75\0",
        code: 0xffbfbfbf,
    },
    ColorDataBaseEntry {
        name: b"gray76\0",
        code: 0xffc2c2c2,
    },
    ColorDataBaseEntry {
        name: b"gray77\0",
        code: 0xffc4c4c4,
    },
    ColorDataBaseEntry {
        name: b"gray78\0",
        code: 0xffc7c7c7,
    },
    ColorDataBaseEntry {
        name: b"gray79\0",
        code: 0xffc9c9c9,
    },
    ColorDataBaseEntry {
        name: b"gray80\0",
        code: 0xffcccccc,
    },
    ColorDataBaseEntry {
        name: b"gray81\0",
        code: 0xffcfcfcf,
    },
    ColorDataBaseEntry {
        name: b"gray82\0",
        code: 0xffd1d1d1,
    },
    ColorDataBaseEntry {
        name: b"gray83\0",
        code: 0xffd4d4d4,
    },
    ColorDataBaseEntry {
        name: b"gray84\0",
        code: 0xffd6d6d6,
    },
    ColorDataBaseEntry {
        name: b"gray85\0",
        code: 0xffd9d9d9,
    },
    ColorDataBaseEntry {
        name: b"gray86\0",
        code: 0xffdbdbdb,
    },
    ColorDataBaseEntry {
        name: b"gray87\0",
        code: 0xffdedede,
    },
    ColorDataBaseEntry {
        name: b"gray88\0",
        code: 0xffe0e0e0,
    },
    ColorDataBaseEntry {
        name: b"gray89\0",
        code: 0xffe3e3e3,
    },
    ColorDataBaseEntry {
        name: b"gray90\0",
        code: 0xffe5e5e5,
    },
    ColorDataBaseEntry {
        name: b"gray91\0",
        code: 0xffe8e8e8,
    },
    ColorDataBaseEntry {
        name: b"gray92\0",
        code: 0xffebebeb,
    },
    ColorDataBaseEntry {
        name: b"gray93\0",
        code: 0xffededed,
    },
    ColorDataBaseEntry {
        name: b"gray94\0",
        code: 0xfff0f0f0,
    },
    ColorDataBaseEntry {
        name: b"gray95\0",
        code: 0xfff2f2f2,
    },
    ColorDataBaseEntry {
        name: b"gray96\0",
        code: 0xfff5f5f5,
    },
    ColorDataBaseEntry {
        name: b"gray97\0",
        code: 0xfff7f7f7,
    },
    ColorDataBaseEntry {
        name: b"gray98\0",
        code: 0xfffafafa,
    },
    ColorDataBaseEntry {
        name: b"gray99\0",
        code: 0xfffcfcfc,
    },
    ColorDataBaseEntry {
        name: b"gray100\0",
        code: 0xffffffff,
    },
    ColorDataBaseEntry {
        name: b"green\0",
        code: 0xff00ff00,
    },
    ColorDataBaseEntry {
        name: b"green1\0",
        code: 0xff00ff00,
    },
    ColorDataBaseEntry {
        name: b"green2\0",
        code: 0xff00ee00,
    },
    ColorDataBaseEntry {
        name: b"green3\0",
        code: 0xff00cd00,
    },
    ColorDataBaseEntry {
        name: b"green4\0",
        code: 0xff008b00,
    },
    ColorDataBaseEntry {
        name: b"greenyellow\0",
        code: 0xff2fffad,
    },
    ColorDataBaseEntry {
        name: b"grey\0",
        code: 0xffbebebe,
    },
    ColorDataBaseEntry {
        name: b"grey0\0",
        code: 0xff000000,
    },
    ColorDataBaseEntry {
        name: b"grey1\0",
        code: 0xff030303,
    },
    ColorDataBaseEntry {
        name: b"grey2\0",
        code: 0xff050505,
    },
    ColorDataBaseEntry {
        name: b"grey3\0",
        code: 0xff080808,
    },
    ColorDataBaseEntry {
        name: b"grey4\0",
        code: 0xff0a0a0a,
    },
    ColorDataBaseEntry {
        name: b"grey5\0",
        code: 0xff0d0d0d,
    },
    ColorDataBaseEntry {
        name: b"grey6\0",
        code: 0xff0f0f0f,
    },
    ColorDataBaseEntry {
        name: b"grey7\0",
        code: 0xff121212,
    },
    ColorDataBaseEntry {
        name: b"grey8\0",
        code: 0xff141414,
    },
    ColorDataBaseEntry {
        name: b"grey9\0",
        code: 0xff171717,
    },
    ColorDataBaseEntry {
        name: b"grey10\0",
        code: 0xff1a1a1a,
    },
    ColorDataBaseEntry {
        name: b"grey11\0",
        code: 0xff1c1c1c,
    },
    ColorDataBaseEntry {
        name: b"grey12\0",
        code: 0xff1f1f1f,
    },
    ColorDataBaseEntry {
        name: b"grey13\0",
        code: 0xff212121,
    },
    ColorDataBaseEntry {
        name: b"grey14\0",
        code: 0xff242424,
    },
    ColorDataBaseEntry {
        name: b"grey15\0",
        code: 0xff262626,
    },
    ColorDataBaseEntry {
        name: b"grey16\0",
        code: 0xff292929,
    },
    ColorDataBaseEntry {
        name: b"grey17\0",
        code: 0xff2b2b2b,
    },
    ColorDataBaseEntry {
        name: b"grey18\0",
        code: 0xff2e2e2e,
    },
    ColorDataBaseEntry {
        name: b"grey19\0",
        code: 0xff303030,
    },
    ColorDataBaseEntry {
        name: b"grey20\0",
        code: 0xff333333,
    },
    ColorDataBaseEntry {
        name: b"grey21\0",
        code: 0xff363636,
    },
    ColorDataBaseEntry {
        name: b"grey22\0",
        code: 0xff383838,
    },
    ColorDataBaseEntry {
        name: b"grey23\0",
        code: 0xff3b3b3b,
    },
    ColorDataBaseEntry {
        name: b"grey24\0",
        code: 0xff3d3d3d,
    },
    ColorDataBaseEntry {
        name: b"grey25\0",
        code: 0xff404040,
    },
    ColorDataBaseEntry {
        name: b"grey26\0",
        code: 0xff424242,
    },
    ColorDataBaseEntry {
        name: b"grey27\0",
        code: 0xff454545,
    },
    ColorDataBaseEntry {
        name: b"grey28\0",
        code: 0xff474747,
    },
    ColorDataBaseEntry {
        name: b"grey29\0",
        code: 0xff4a4a4a,
    },
    ColorDataBaseEntry {
        name: b"grey30\0",
        code: 0xff4d4d4d,
    },
    ColorDataBaseEntry {
        name: b"grey31\0",
        code: 0xff4f4f4f,
    },
    ColorDataBaseEntry {
        name: b"grey32\0",
        code: 0xff525252,
    },
    ColorDataBaseEntry {
        name: b"grey33\0",
        code: 0xff545454,
    },
    ColorDataBaseEntry {
        name: b"grey34\0",
        code: 0xff575757,
    },
    ColorDataBaseEntry {
        name: b"grey35\0",
        code: 0xff595959,
    },
    ColorDataBaseEntry {
        name: b"grey36\0",
        code: 0xff5c5c5c,
    },
    ColorDataBaseEntry {
        name: b"grey37\0",
        code: 0xff5e5e5e,
    },
    ColorDataBaseEntry {
        name: b"grey38\0",
        code: 0xff616161,
    },
    ColorDataBaseEntry {
        name: b"grey39\0",
        code: 0xff636363,
    },
    ColorDataBaseEntry {
        name: b"grey40\0",
        code: 0xff666666,
    },
    ColorDataBaseEntry {
        name: b"grey41\0",
        code: 0xff696969,
    },
    ColorDataBaseEntry {
        name: b"grey42\0",
        code: 0xff6b6b6b,
    },
    ColorDataBaseEntry {
        name: b"grey43\0",
        code: 0xff6e6e6e,
    },
    ColorDataBaseEntry {
        name: b"grey44\0",
        code: 0xff707070,
    },
    ColorDataBaseEntry {
        name: b"grey45\0",
        code: 0xff737373,
    },
    ColorDataBaseEntry {
        name: b"grey46\0",
        code: 0xff757575,
    },
    ColorDataBaseEntry {
        name: b"grey47\0",
        code: 0xff787878,
    },
    ColorDataBaseEntry {
        name: b"grey48\0",
        code: 0xff7a7a7a,
    },
    ColorDataBaseEntry {
        name: b"grey49\0",
        code: 0xff7d7d7d,
    },
    ColorDataBaseEntry {
        name: b"grey50\0",
        code: 0xff7f7f7f,
    },
    ColorDataBaseEntry {
        name: b"grey51\0",
        code: 0xff828282,
    },
    ColorDataBaseEntry {
        name: b"grey52\0",
        code: 0xff858585,
    },
    ColorDataBaseEntry {
        name: b"grey53\0",
        code: 0xff878787,
    },
    ColorDataBaseEntry {
        name: b"grey54\0",
        code: 0xff8a8a8a,
    },
    ColorDataBaseEntry {
        name: b"grey55\0",
        code: 0xff8c8c8c,
    },
    ColorDataBaseEntry {
        name: b"grey56\0",
        code: 0xff8f8f8f,
    },
    ColorDataBaseEntry {
        name: b"grey57\0",
        code: 0xff919191,
    },
    ColorDataBaseEntry {
        name: b"grey58\0",
        code: 0xff949494,
    },
    ColorDataBaseEntry {
        name: b"grey59\0",
        code: 0xff969696,
    },
    ColorDataBaseEntry {
        name: b"grey60\0",
        code: 0xff999999,
    },
    ColorDataBaseEntry {
        name: b"grey61\0",
        code: 0xff9c9c9c,
    },
    ColorDataBaseEntry {
        name: b"grey62\0",
        code: 0xff9e9e9e,
    },
    ColorDataBaseEntry {
        name: b"grey63\0",
        code: 0xffa1a1a1,
    },
    ColorDataBaseEntry {
        name: b"grey64\0",
        code: 0xffa3a3a3,
    },
    ColorDataBaseEntry {
        name: b"grey65\0",
        code: 0xffa6a6a6,
    },
    ColorDataBaseEntry {
        name: b"grey66\0",
        code: 0xffa8a8a8,
    },
    ColorDataBaseEntry {
        name: b"grey67\0",
        code: 0xffababab,
    },
    ColorDataBaseEntry {
        name: b"grey68\0",
        code: 0xffadadad,
    },
    ColorDataBaseEntry {
        name: b"grey69\0",
        code: 0xffb0b0b0,
    },
    ColorDataBaseEntry {
        name: b"grey70\0",
        code: 0xffb3b3b3,
    },
    ColorDataBaseEntry {
        name: b"grey71\0",
        code: 0xffb5b5b5,
    },
    ColorDataBaseEntry {
        name: b"grey72\0",
        code: 0xffb8b8b8,
    },
    ColorDataBaseEntry {
        name: b"grey73\0",
        code: 0xffbababa,
    },
    ColorDataBaseEntry {
        name: b"grey74\0",
        code: 0xffbdbdbd,
    },
    ColorDataBaseEntry {
        name: b"grey75\0",
        code: 0xffbfbfbf,
    },
    ColorDataBaseEntry {
        name: b"grey76\0",
        code: 0xffc2c2c2,
    },
    ColorDataBaseEntry {
        name: b"grey77\0",
        code: 0xffc4c4c4,
    },
    ColorDataBaseEntry {
        name: b"grey78\0",
        code: 0xffc7c7c7,
    },
    ColorDataBaseEntry {
        name: b"grey79\0",
        code: 0xffc9c9c9,
    },
    ColorDataBaseEntry {
        name: b"grey80\0",
        code: 0xffcccccc,
    },
    ColorDataBaseEntry {
        name: b"grey81\0",
        code: 0xffcfcfcf,
    },
    ColorDataBaseEntry {
        name: b"grey82\0",
        code: 0xffd1d1d1,
    },
    ColorDataBaseEntry {
        name: b"grey83\0",
        code: 0xffd4d4d4,
    },
    ColorDataBaseEntry {
        name: b"grey84\0",
        code: 0xffd6d6d6,
    },
    ColorDataBaseEntry {
        name: b"grey85\0",
        code: 0xffd9d9d9,
    },
    ColorDataBaseEntry {
        name: b"grey86\0",
        code: 0xffdbdbdb,
    },
    ColorDataBaseEntry {
        name: b"grey87\0",
        code: 0xffdedede,
    },
    ColorDataBaseEntry {
        name: b"grey88\0",
        code: 0xffe0e0e0,
    },
    ColorDataBaseEntry {
        name: b"grey89\0",
        code: 0xffe3e3e3,
    },
    ColorDataBaseEntry {
        name: b"grey90\0",
        code: 0xffe5e5e5,
    },
    ColorDataBaseEntry {
        name: b"grey91\0",
        code: 0xffe8e8e8,
    },
    ColorDataBaseEntry {
        name: b"grey92\0",
        code: 0xffebebeb,
    },
    ColorDataBaseEntry {
        name: b"grey93\0",
        code: 0xffededed,
    },
    ColorDataBaseEntry {
        name: b"grey94\0",
        code: 0xfff0f0f0,
    },
    ColorDataBaseEntry {
        name: b"grey95\0",
        code: 0xfff2f2f2,
    },
    ColorDataBaseEntry {
        name: b"grey96\0",
        code: 0xfff5f5f5,
    },
    ColorDataBaseEntry {
        name: b"grey97\0",
        code: 0xfff7f7f7,
    },
    ColorDataBaseEntry {
        name: b"grey98\0",
        code: 0xfffafafa,
    },
    ColorDataBaseEntry {
        name: b"grey99\0",
        code: 0xfffcfcfc,
    },
    ColorDataBaseEntry {
        name: b"grey100\0",
        code: 0xffffffff,
    },
    ColorDataBaseEntry {
        name: b"honeydew\0",
        code: 0xfff0fff0,
    },
    ColorDataBaseEntry {
        name: b"honeydew1\0",
        code: 0xfff0fff0,
    },
    ColorDataBaseEntry {
        name: b"honeydew2\0",
        code: 0xffe0eee0,
    },
    ColorDataBaseEntry {
        name: b"honeydew3\0",
        code: 0xffc1cdc1,
    },
    ColorDataBaseEntry {
        name: b"honeydew4\0",
        code: 0xff838b83,
    },
    ColorDataBaseEntry {
        name: b"hotpink\0",
        code: 0xffb469ff,
    },
    ColorDataBaseEntry {
        name: b"hotpink1\0",
        code: 0xffb46eff,
    },
    ColorDataBaseEntry {
        name: b"hotpink2\0",
        code: 0xffa76aee,
    },
    ColorDataBaseEntry {
        name: b"hotpink3\0",
        code: 0xff9060cd,
    },
    ColorDataBaseEntry {
        name: b"hotpink4\0",
        code: 0xff623a8b,
    },
    ColorDataBaseEntry {
        name: b"indianred\0",
        code: 0xff5c5ccd,
    },
    ColorDataBaseEntry {
        name: b"indianred1\0",
        code: 0xff6a6aff,
    },
    ColorDataBaseEntry {
        name: b"indianred2\0",
        code: 0xff6363ee,
    },
    ColorDataBaseEntry {
        name: b"indianred3\0",
        code: 0xff5555cd,
    },
    ColorDataBaseEntry {
        name: b"indianred4\0",
        code: 0xff3a3a8b,
    },
    ColorDataBaseEntry {
        name: b"ivory\0",
        code: 0xfff0ffff,
    },
    ColorDataBaseEntry {
        name: b"ivory1\0",
        code: 0xfff0ffff,
    },
    ColorDataBaseEntry {
        name: b"ivory2\0",
        code: 0xffe0eeee,
    },
    ColorDataBaseEntry {
        name: b"ivory3\0",
        code: 0xffc1cdcd,
    },
    ColorDataBaseEntry {
        name: b"ivory4\0",
        code: 0xff838b8b,
    },
    ColorDataBaseEntry {
        name: b"khaki\0",
        code: 0xff8ce6f0,
    },
    ColorDataBaseEntry {
        name: b"khaki1\0",
        code: 0xff8ff6ff,
    },
    ColorDataBaseEntry {
        name: b"khaki2\0",
        code: 0xff85e6ee,
    },
    ColorDataBaseEntry {
        name: b"khaki3\0",
        code: 0xff73c6cd,
    },
    ColorDataBaseEntry {
        name: b"khaki4\0",
        code: 0xff4e868b,
    },
    ColorDataBaseEntry {
        name: b"lavender\0",
        code: 0xfffae6e6,
    },
    ColorDataBaseEntry {
        name: b"lavenderblush\0",
        code: 0xfff5f0ff,
    },
    ColorDataBaseEntry {
        name: b"lavenderblush1\0",
        code: 0xfff5f0ff,
    },
    ColorDataBaseEntry {
        name: b"lavenderblush2\0",
        code: 0xffe5e0ee,
    },
    ColorDataBaseEntry {
        name: b"lavenderblush3\0",
        code: 0xffc5c1cd,
    },
    ColorDataBaseEntry {
        name: b"lavenderblush4\0",
        code: 0xff86838b,
    },
    ColorDataBaseEntry {
        name: b"lawngreen\0",
        code: 0xff00fc7c,
    },
    ColorDataBaseEntry {
        name: b"lemonchiffon\0",
        code: 0xffcdfaff,
    },
    ColorDataBaseEntry {
        name: b"lemonchiffon1\0",
        code: 0xffcdfaff,
    },
    ColorDataBaseEntry {
        name: b"lemonchiffon2\0",
        code: 0xffbfe9ee,
    },
    ColorDataBaseEntry {
        name: b"lemonchiffon3\0",
        code: 0xffa5c9cd,
    },
    ColorDataBaseEntry {
        name: b"lemonchiffon4\0",
        code: 0xff70898b,
    },
    ColorDataBaseEntry {
        name: b"lightblue\0",
        code: 0xffe6d8ad,
    },
    ColorDataBaseEntry {
        name: b"lightblue1\0",
        code: 0xffffefbf,
    },
    ColorDataBaseEntry {
        name: b"lightblue2\0",
        code: 0xffeedfb2,
    },
    ColorDataBaseEntry {
        name: b"lightblue3\0",
        code: 0xffcdc09a,
    },
    ColorDataBaseEntry {
        name: b"lightblue4\0",
        code: 0xff8b8368,
    },
    ColorDataBaseEntry {
        name: b"lightcoral\0",
        code: 0xff8080f0,
    },
    ColorDataBaseEntry {
        name: b"lightcyan\0",
        code: 0xffffffe0,
    },
    ColorDataBaseEntry {
        name: b"lightcyan1\0",
        code: 0xffffffe0,
    },
    ColorDataBaseEntry {
        name: b"lightcyan2\0",
        code: 0xffeeeed1,
    },
    ColorDataBaseEntry {
        name: b"lightcyan3\0",
        code: 0xffcdcdb4,
    },
    ColorDataBaseEntry {
        name: b"lightcyan4\0",
        code: 0xff8b8b7a,
    },
    ColorDataBaseEntry {
        name: b"lightgoldenrod\0",
        code: 0xff82ddee,
    },
    ColorDataBaseEntry {
        name: b"lightgoldenrod1\0",
        code: 0xff8becff,
    },
    ColorDataBaseEntry {
        name: b"lightgoldenrod2\0",
        code: 0xff82dcee,
    },
    ColorDataBaseEntry {
        name: b"lightgoldenrod3\0",
        code: 0xff70becd,
    },
    ColorDataBaseEntry {
        name: b"lightgoldenrod4\0",
        code: 0xff4c818b,
    },
    ColorDataBaseEntry {
        name: b"lightgoldenrodyellow\0",
        code: 0xffd2fafa,
    },
    ColorDataBaseEntry {
        name: b"lightgray\0",
        code: 0xffd3d3d3,
    },
    ColorDataBaseEntry {
        name: b"lightgreen\0",
        code: 0xff90ee90,
    },
    ColorDataBaseEntry {
        name: b"lightgrey\0",
        code: 0xffd3d3d3,
    },
    ColorDataBaseEntry {
        name: b"lightpink\0",
        code: 0xffc1b6ff,
    },
    ColorDataBaseEntry {
        name: b"lightpink1\0",
        code: 0xffb9aeff,
    },
    ColorDataBaseEntry {
        name: b"lightpink2\0",
        code: 0xffada2ee,
    },
    ColorDataBaseEntry {
        name: b"lightpink3\0",
        code: 0xff958ccd,
    },
    ColorDataBaseEntry {
        name: b"lightpink4\0",
        code: 0xff655f8b,
    },
    ColorDataBaseEntry {
        name: b"lightsalmon\0",
        code: 0xff7aa0ff,
    },
    ColorDataBaseEntry {
        name: b"lightsalmon1\0",
        code: 0xff7aa0ff,
    },
    ColorDataBaseEntry {
        name: b"lightsalmon2\0",
        code: 0xff7295ee,
    },
    ColorDataBaseEntry {
        name: b"lightsalmon3\0",
        code: 0xff6281cd,
    },
    ColorDataBaseEntry {
        name: b"lightsalmon4\0",
        code: 0xff42578b,
    },
    ColorDataBaseEntry {
        name: b"lightseagreen\0",
        code: 0xffaab220,
    },
    ColorDataBaseEntry {
        name: b"lightskyblue\0",
        code: 0xffface87,
    },
    ColorDataBaseEntry {
        name: b"lightskyblue1\0",
        code: 0xffffe2b0,
    },
    ColorDataBaseEntry {
        name: b"lightskyblue2\0",
        code: 0xffeed3a4,
    },
    ColorDataBaseEntry {
        name: b"lightskyblue3\0",
        code: 0xffcdb68d,
    },
    ColorDataBaseEntry {
        name: b"lightskyblue4\0",
        code: 0xff8b7b60,
    },
    ColorDataBaseEntry {
        name: b"lightslateblue\0",
        code: 0xffff7084,
    },
    ColorDataBaseEntry {
        name: b"lightslategray\0",
        code: 0xff998877,
    },
    ColorDataBaseEntry {
        name: b"lightslategrey\0",
        code: 0xff998877,
    },
    ColorDataBaseEntry {
        name: b"lightsteelblue\0",
        code: 0xffdec4b0,
    },
    ColorDataBaseEntry {
        name: b"lightsteelblue1\0",
        code: 0xffffe1ca,
    },
    ColorDataBaseEntry {
        name: b"lightsteelblue2\0",
        code: 0xffeed2bc,
    },
    ColorDataBaseEntry {
        name: b"lightsteelblue3\0",
        code: 0xffcdb5a2,
    },
    ColorDataBaseEntry {
        name: b"lightsteelblue4\0",
        code: 0xff8b7b6e,
    },
    ColorDataBaseEntry {
        name: b"lightyellow\0",
        code: 0xffe0ffff,
    },
    ColorDataBaseEntry {
        name: b"lightyellow1\0",
        code: 0xffe0ffff,
    },
    ColorDataBaseEntry {
        name: b"lightyellow2\0",
        code: 0xffd1eeee,
    },
    ColorDataBaseEntry {
        name: b"lightyellow3\0",
        code: 0xffb4cdcd,
    },
    ColorDataBaseEntry {
        name: b"lightyellow4\0",
        code: 0xff7a8b8b,
    },
    ColorDataBaseEntry {
        name: b"limegreen\0",
        code: 0xff32cd32,
    },
    ColorDataBaseEntry {
        name: b"linen\0",
        code: 0xffe6f0fa,
    },
    ColorDataBaseEntry {
        name: b"magenta\0",
        code: 0xffff00ff,
    },
    ColorDataBaseEntry {
        name: b"magenta1\0",
        code: 0xffff00ff,
    },
    ColorDataBaseEntry {
        name: b"magenta2\0",
        code: 0xffee00ee,
    },
    ColorDataBaseEntry {
        name: b"magenta3\0",
        code: 0xffcd00cd,
    },
    ColorDataBaseEntry {
        name: b"magenta4\0",
        code: 0xff8b008b,
    },
    ColorDataBaseEntry {
        name: b"maroon\0",
        code: 0xff6030b0,
    },
    ColorDataBaseEntry {
        name: b"maroon1\0",
        code: 0xffb334ff,
    },
    ColorDataBaseEntry {
        name: b"maroon2\0",
        code: 0xffa730ee,
    },
    ColorDataBaseEntry {
        name: b"maroon3\0",
        code: 0xff9029cd,
    },
    ColorDataBaseEntry {
        name: b"maroon4\0",
        code: 0xff621c8b,
    },
    ColorDataBaseEntry {
        name: b"mediumaquamarine\0",
        code: 0xffaacd66,
    },
    ColorDataBaseEntry {
        name: b"mediumblue\0",
        code: 0xffcd0000,
    },
    ColorDataBaseEntry {
        name: b"mediumorchid\0",
        code: 0xffd355ba,
    },
    ColorDataBaseEntry {
        name: b"mediumorchid1\0",
        code: 0xffff66e0,
    },
    ColorDataBaseEntry {
        name: b"mediumorchid2\0",
        code: 0xffee5fd1,
    },
    ColorDataBaseEntry {
        name: b"mediumorchid3\0",
        code: 0xffcd52b4,
    },
    ColorDataBaseEntry {
        name: b"mediumorchid4\0",
        code: 0xff8b377a,
    },
    ColorDataBaseEntry {
        name: b"mediumpurple\0",
        code: 0xffdb7093,
    },
    ColorDataBaseEntry {
        name: b"mediumpurple1\0",
        code: 0xffff82ab,
    },
    ColorDataBaseEntry {
        name: b"mediumpurple2\0",
        code: 0xffee799f,
    },
    ColorDataBaseEntry {
        name: b"mediumpurple3\0",
        code: 0xffcd6889,
    },
    ColorDataBaseEntry {
        name: b"mediumpurple4\0",
        code: 0xff8b475d,
    },
    ColorDataBaseEntry {
        name: b"mediumseagreen\0",
        code: 0xff71b33c,
    },
    ColorDataBaseEntry {
        name: b"mediumslateblue\0",
        code: 0xffee687b,
    },
    ColorDataBaseEntry {
        name: b"mediumspringgreen\0",
        code: 0xff9afa00,
    },
    ColorDataBaseEntry {
        name: b"mediumturquoise\0",
        code: 0xffccd148,
    },
    ColorDataBaseEntry {
        name: b"mediumvioletred\0",
        code: 0xff8515c7,
    },
    ColorDataBaseEntry {
        name: b"midnightblue\0",
        code: 0xff701919,
    },
    ColorDataBaseEntry {
        name: b"mintcream\0",
        code: 0xfffafff5,
    },
    ColorDataBaseEntry {
        name: b"mistyrose\0",
        code: 0xffe1e4ff,
    },
    ColorDataBaseEntry {
        name: b"mistyrose1\0",
        code: 0xffe1e4ff,
    },
    ColorDataBaseEntry {
        name: b"mistyrose2\0",
        code: 0xffd2d5ee,
    },
    ColorDataBaseEntry {
        name: b"mistyrose3\0",
        code: 0xffb5b7cd,
    },
    ColorDataBaseEntry {
        name: b"mistyrose4\0",
        code: 0xff7b7d8b,
    },
    ColorDataBaseEntry {
        name: b"moccasin\0",
        code: 0xffb5e4ff,
    },
    ColorDataBaseEntry {
        name: b"navajowhite\0",
        code: 0xffaddeff,
    },
    ColorDataBaseEntry {
        name: b"navajowhite1\0",
        code: 0xffaddeff,
    },
    ColorDataBaseEntry {
        name: b"navajowhite2\0",
        code: 0xffa1cfee,
    },
    ColorDataBaseEntry {
        name: b"navajowhite3\0",
        code: 0xff8bb3cd,
    },
    ColorDataBaseEntry {
        name: b"navajowhite4\0",
        code: 0xff5e798b,
    },
    ColorDataBaseEntry {
        name: b"navy\0",
        code: 0xff800000,
    },
    ColorDataBaseEntry {
        name: b"navyblue\0",
        code: 0xff800000,
    },
    ColorDataBaseEntry {
        name: b"oldlace\0",
        code: 0xffe6f5fd,
    },
    ColorDataBaseEntry {
        name: b"olivedrab\0",
        code: 0xff238e6b,
    },
    ColorDataBaseEntry {
        name: b"olivedrab1\0",
        code: 0xff3effc0,
    },
    ColorDataBaseEntry {
        name: b"olivedrab2\0",
        code: 0xff3aeeb3,
    },
    ColorDataBaseEntry {
        name: b"olivedrab3\0",
        code: 0xff32cd9a,
    },
    ColorDataBaseEntry {
        name: b"olivedrab4\0",
        code: 0xff228b69,
    },
    ColorDataBaseEntry {
        name: b"orange\0",
        code: 0xff00a5ff,
    },
    ColorDataBaseEntry {
        name: b"orange1\0",
        code: 0xff00a5ff,
    },
    ColorDataBaseEntry {
        name: b"orange2\0",
        code: 0xff009aee,
    },
    ColorDataBaseEntry {
        name: b"orange3\0",
        code: 0xff0085cd,
    },
    ColorDataBaseEntry {
        name: b"orange4\0",
        code: 0xff005a8b,
    },
    ColorDataBaseEntry {
        name: b"orangered\0",
        code: 0xff0045ff,
    },
    ColorDataBaseEntry {
        name: b"orangered1\0",
        code: 0xff0045ff,
    },
    ColorDataBaseEntry {
        name: b"orangered2\0",
        code: 0xff0040ee,
    },
    ColorDataBaseEntry {
        name: b"orangered3\0",
        code: 0xff0037cd,
    },
    ColorDataBaseEntry {
        name: b"orangered4\0",
        code: 0xff00258b,
    },
    ColorDataBaseEntry {
        name: b"orchid\0",
        code: 0xffd670da,
    },
    ColorDataBaseEntry {
        name: b"orchid1\0",
        code: 0xfffa83ff,
    },
    ColorDataBaseEntry {
        name: b"orchid2\0",
        code: 0xffe97aee,
    },
    ColorDataBaseEntry {
        name: b"orchid3\0",
        code: 0xffc969cd,
    },
    ColorDataBaseEntry {
        name: b"orchid4\0",
        code: 0xff89478b,
    },
    ColorDataBaseEntry {
        name: b"palegoldenrod\0",
        code: 0xffaae8ee,
    },
    ColorDataBaseEntry {
        name: b"palegreen\0",
        code: 0xff98fb98,
    },
    ColorDataBaseEntry {
        name: b"palegreen1\0",
        code: 0xff9aff9a,
    },
    ColorDataBaseEntry {
        name: b"palegreen2\0",
        code: 0xff90ee90,
    },
    ColorDataBaseEntry {
        name: b"palegreen3\0",
        code: 0xff7ccd7c,
    },
    ColorDataBaseEntry {
        name: b"palegreen4\0",
        code: 0xff548b54,
    },
    ColorDataBaseEntry {
        name: b"paleturquoise\0",
        code: 0xffeeeeaf,
    },
    ColorDataBaseEntry {
        name: b"paleturquoise1\0",
        code: 0xffffffbb,
    },
    ColorDataBaseEntry {
        name: b"paleturquoise2\0",
        code: 0xffeeeeae,
    },
    ColorDataBaseEntry {
        name: b"paleturquoise3\0",
        code: 0xffcdcd96,
    },
    ColorDataBaseEntry {
        name: b"paleturquoise4\0",
        code: 0xff8b8b66,
    },
    ColorDataBaseEntry {
        name: b"palevioletred\0",
        code: 0xff9370db,
    },
    ColorDataBaseEntry {
        name: b"palevioletred1\0",
        code: 0xffab82ff,
    },
    ColorDataBaseEntry {
        name: b"palevioletred2\0",
        code: 0xff9f79ee,
    },
    ColorDataBaseEntry {
        name: b"palevioletred3\0",
        code: 0xff8968cd,
    },
    ColorDataBaseEntry {
        name: b"palevioletred4\0",
        code: 0xff5d478b,
    },
    ColorDataBaseEntry {
        name: b"papayawhip\0",
        code: 0xffd5efff,
    },
    ColorDataBaseEntry {
        name: b"peachpuff\0",
        code: 0xffb9daff,
    },
    ColorDataBaseEntry {
        name: b"peachpuff1\0",
        code: 0xffb9daff,
    },
    ColorDataBaseEntry {
        name: b"peachpuff2\0",
        code: 0xffadcbee,
    },
    ColorDataBaseEntry {
        name: b"peachpuff3\0",
        code: 0xff95afcd,
    },
    ColorDataBaseEntry {
        name: b"peachpuff4\0",
        code: 0xff65778b,
    },
    ColorDataBaseEntry {
        name: b"peru\0",
        code: 0xff3f85cd,
    },
    ColorDataBaseEntry {
        name: b"pink\0",
        code: 0xffcbc0ff,
    },
    ColorDataBaseEntry {
        name: b"pink1\0",
        code: 0xffc5b5ff,
    },
    ColorDataBaseEntry {
        name: b"pink2\0",
        code: 0xffb8a9ee,
    },
    ColorDataBaseEntry {
        name: b"pink3\0",
        code: 0xff9e91cd,
    },
    ColorDataBaseEntry {
        name: b"pink4\0",
        code: 0xff6c638b,
    },
    ColorDataBaseEntry {
        name: b"plum\0",
        code: 0xffdda0dd,
    },
    ColorDataBaseEntry {
        name: b"plum1\0",
        code: 0xffffbbff,
    },
    ColorDataBaseEntry {
        name: b"plum2\0",
        code: 0xffeeaeee,
    },
    ColorDataBaseEntry {
        name: b"plum3\0",
        code: 0xffcd96cd,
    },
    ColorDataBaseEntry {
        name: b"plum4\0",
        code: 0xff8b668b,
    },
    ColorDataBaseEntry {
        name: b"powderblue\0",
        code: 0xffe6e0b0,
    },
    ColorDataBaseEntry {
        name: b"purple\0",
        code: 0xfff020a0,
    },
    ColorDataBaseEntry {
        name: b"purple1\0",
        code: 0xffff309b,
    },
    ColorDataBaseEntry {
        name: b"purple2\0",
        code: 0xffee2c91,
    },
    ColorDataBaseEntry {
        name: b"purple3\0",
        code: 0xffcd267d,
    },
    ColorDataBaseEntry {
        name: b"purple4\0",
        code: 0xff8b1a55,
    },
    ColorDataBaseEntry {
        name: b"red\0",
        code: 0xff0000ff,
    },
    ColorDataBaseEntry {
        name: b"red1\0",
        code: 0xff0000ff,
    },
    ColorDataBaseEntry {
        name: b"red2\0",
        code: 0xff0000ee,
    },
    ColorDataBaseEntry {
        name: b"red3\0",
        code: 0xff0000cd,
    },
    ColorDataBaseEntry {
        name: b"red4\0",
        code: 0xff00008b,
    },
    ColorDataBaseEntry {
        name: b"rosybrown\0",
        code: 0xff8f8fbc,
    },
    ColorDataBaseEntry {
        name: b"rosybrown1\0",
        code: 0xffc1c1ff,
    },
    ColorDataBaseEntry {
        name: b"rosybrown2\0",
        code: 0xffb4b4ee,
    },
    ColorDataBaseEntry {
        name: b"rosybrown3\0",
        code: 0xff9b9bcd,
    },
    ColorDataBaseEntry {
        name: b"rosybrown4\0",
        code: 0xff69698b,
    },
    ColorDataBaseEntry {
        name: b"royalblue\0",
        code: 0xffe16941,
    },
    ColorDataBaseEntry {
        name: b"royalblue1\0",
        code: 0xffff7648,
    },
    ColorDataBaseEntry {
        name: b"royalblue2\0",
        code: 0xffee6e43,
    },
    ColorDataBaseEntry {
        name: b"royalblue3\0",
        code: 0xffcd5f3a,
    },
    ColorDataBaseEntry {
        name: b"royalblue4\0",
        code: 0xff8b4027,
    },
    ColorDataBaseEntry {
        name: b"saddlebrown\0",
        code: 0xff13458b,
    },
    ColorDataBaseEntry {
        name: b"salmon\0",
        code: 0xff7280fa,
    },
    ColorDataBaseEntry {
        name: b"salmon1\0",
        code: 0xff698cff,
    },
    ColorDataBaseEntry {
        name: b"salmon2\0",
        code: 0xff6282ee,
    },
    ColorDataBaseEntry {
        name: b"salmon3\0",
        code: 0xff5470cd,
    },
    ColorDataBaseEntry {
        name: b"salmon4\0",
        code: 0xff394c8b,
    },
    ColorDataBaseEntry {
        name: b"sandybrown\0",
        code: 0xff60a4f4,
    },
    ColorDataBaseEntry {
        name: b"seagreen\0",
        code: 0xff578b2e,
    },
    ColorDataBaseEntry {
        name: b"seagreen1\0",
        code: 0xff9fff54,
    },
    ColorDataBaseEntry {
        name: b"seagreen2\0",
        code: 0xff94ee4e,
    },
    ColorDataBaseEntry {
        name: b"seagreen3\0",
        code: 0xff80cd43,
    },
    ColorDataBaseEntry {
        name: b"seagreen4\0",
        code: 0xff578b2e,
    },
    ColorDataBaseEntry {
        name: b"seashell\0",
        code: 0xffeef5ff,
    },
    ColorDataBaseEntry {
        name: b"seashell1\0",
        code: 0xffeef5ff,
    },
    ColorDataBaseEntry {
        name: b"seashell2\0",
        code: 0xffdee5ee,
    },
    ColorDataBaseEntry {
        name: b"seashell3\0",
        code: 0xffbfc5cd,
    },
    ColorDataBaseEntry {
        name: b"seashell4\0",
        code: 0xff82868b,
    },
    ColorDataBaseEntry {
        name: b"sienna\0",
        code: 0xff2d52a0,
    },
    ColorDataBaseEntry {
        name: b"sienna1\0",
        code: 0xff4782ff,
    },
    ColorDataBaseEntry {
        name: b"sienna2\0",
        code: 0xff4279ee,
    },
    ColorDataBaseEntry {
        name: b"sienna3\0",
        code: 0xff3968cd,
    },
    ColorDataBaseEntry {
        name: b"sienna4\0",
        code: 0xff26478b,
    },
    ColorDataBaseEntry {
        name: b"skyblue\0",
        code: 0xffebce87,
    },
    ColorDataBaseEntry {
        name: b"skyblue1\0",
        code: 0xffffce87,
    },
    ColorDataBaseEntry {
        name: b"skyblue2\0",
        code: 0xffeec07e,
    },
    ColorDataBaseEntry {
        name: b"skyblue3\0",
        code: 0xffcda66c,
    },
    ColorDataBaseEntry {
        name: b"skyblue4\0",
        code: 0xff8b704a,
    },
    ColorDataBaseEntry {
        name: b"slateblue\0",
        code: 0xffcd5a6a,
    },
    ColorDataBaseEntry {
        name: b"slateblue1\0",
        code: 0xffff6f83,
    },
    ColorDataBaseEntry {
        name: b"slateblue2\0",
        code: 0xffee677a,
    },
    ColorDataBaseEntry {
        name: b"slateblue3\0",
        code: 0xffcd5969,
    },
    ColorDataBaseEntry {
        name: b"slateblue4\0",
        code: 0xff8b3c47,
    },
    ColorDataBaseEntry {
        name: b"slategray\0",
        code: 0xff908070,
    },
    ColorDataBaseEntry {
        name: b"slategray1\0",
        code: 0xffffe2c6,
    },
    ColorDataBaseEntry {
        name: b"slategray2\0",
        code: 0xffeed3b9,
    },
    ColorDataBaseEntry {
        name: b"slategray3\0",
        code: 0xffcdb69f,
    },
    ColorDataBaseEntry {
        name: b"slategray4\0",
        code: 0xff8b7b6c,
    },
    ColorDataBaseEntry {
        name: b"slategrey\0",
        code: 0xff908070,
    },
    ColorDataBaseEntry {
        name: b"snow\0",
        code: 0xfffafaff,
    },
    ColorDataBaseEntry {
        name: b"snow1\0",
        code: 0xfffafaff,
    },
    ColorDataBaseEntry {
        name: b"snow2\0",
        code: 0xffe9e9ee,
    },
    ColorDataBaseEntry {
        name: b"snow3\0",
        code: 0xffc9c9cd,
    },
    ColorDataBaseEntry {
        name: b"snow4\0",
        code: 0xff89898b,
    },
    ColorDataBaseEntry {
        name: b"springgreen\0",
        code: 0xff7fff00,
    },
    ColorDataBaseEntry {
        name: b"springgreen1\0",
        code: 0xff7fff00,
    },
    ColorDataBaseEntry {
        name: b"springgreen2\0",
        code: 0xff76ee00,
    },
    ColorDataBaseEntry {
        name: b"springgreen3\0",
        code: 0xff66cd00,
    },
    ColorDataBaseEntry {
        name: b"springgreen4\0",
        code: 0xff458b00,
    },
    ColorDataBaseEntry {
        name: b"steelblue\0",
        code: 0xffb48246,
    },
    ColorDataBaseEntry {
        name: b"steelblue1\0",
        code: 0xffffb863,
    },
    ColorDataBaseEntry {
        name: b"steelblue2\0",
        code: 0xffeeac5c,
    },
    ColorDataBaseEntry {
        name: b"steelblue3\0",
        code: 0xffcd944f,
    },
    ColorDataBaseEntry {
        name: b"steelblue4\0",
        code: 0xff8b6436,
    },
    ColorDataBaseEntry {
        name: b"tan\0",
        code: 0xff8cb4d2,
    },
    ColorDataBaseEntry {
        name: b"tan1\0",
        code: 0xff4fa5ff,
    },
    ColorDataBaseEntry {
        name: b"tan2\0",
        code: 0xff499aee,
    },
    ColorDataBaseEntry {
        name: b"tan3\0",
        code: 0xff3f85cd,
    },
    ColorDataBaseEntry {
        name: b"tan4\0",
        code: 0xff2b5a8b,
    },
    ColorDataBaseEntry {
        name: b"thistle\0",
        code: 0xffd8bfd8,
    },
    ColorDataBaseEntry {
        name: b"thistle1\0",
        code: 0xffffe1ff,
    },
    ColorDataBaseEntry {
        name: b"thistle2\0",
        code: 0xffeed2ee,
    },
    ColorDataBaseEntry {
        name: b"thistle3\0",
        code: 0xffcdb5cd,
    },
    ColorDataBaseEntry {
        name: b"thistle4\0",
        code: 0xff8b7b8b,
    },
    ColorDataBaseEntry {
        name: b"tomato\0",
        code: 0xff4763ff,
    },
    ColorDataBaseEntry {
        name: b"tomato1\0",
        code: 0xff4763ff,
    },
    ColorDataBaseEntry {
        name: b"tomato2\0",
        code: 0xff425cee,
    },
    ColorDataBaseEntry {
        name: b"tomato3\0",
        code: 0xff394fcd,
    },
    ColorDataBaseEntry {
        name: b"tomato4\0",
        code: 0xff26368b,
    },
    ColorDataBaseEntry {
        name: b"turquoise\0",
        code: 0xffd0e040,
    },
    ColorDataBaseEntry {
        name: b"turquoise1\0",
        code: 0xfffff500,
    },
    ColorDataBaseEntry {
        name: b"turquoise2\0",
        code: 0xffeee500,
    },
    ColorDataBaseEntry {
        name: b"turquoise3\0",
        code: 0xffcdc500,
    },
    ColorDataBaseEntry {
        name: b"turquoise4\0",
        code: 0xff8b8600,
    },
    ColorDataBaseEntry {
        name: b"violet\0",
        code: 0xffee82ee,
    },
    ColorDataBaseEntry {
        name: b"violetred\0",
        code: 0xff9020d0,
    },
    ColorDataBaseEntry {
        name: b"violetred1\0",
        code: 0xff963eff,
    },
    ColorDataBaseEntry {
        name: b"violetred2\0",
        code: 0xff8c3aee,
    },
    ColorDataBaseEntry {
        name: b"violetred3\0",
        code: 0xff7832cd,
    },
    ColorDataBaseEntry {
        name: b"violetred4\0",
        code: 0xff52228b,
    },
    ColorDataBaseEntry {
        name: b"wheat\0",
        code: 0xffb3def5,
    },
    ColorDataBaseEntry {
        name: b"wheat1\0",
        code: 0xffbae7ff,
    },
    ColorDataBaseEntry {
        name: b"wheat2\0",
        code: 0xffaed8ee,
    },
    ColorDataBaseEntry {
        name: b"wheat3\0",
        code: 0xff96bacd,
    },
    ColorDataBaseEntry {
        name: b"wheat4\0",
        code: 0xff667e8b,
    },
    ColorDataBaseEntry {
        name: b"whitesmoke\0",
        code: 0xfff5f5f5,
    },
    ColorDataBaseEntry {
        name: b"yellow\0",
        code: 0xff00ffff,
    },
    ColorDataBaseEntry {
        name: b"yellow1\0",
        code: 0xff00ffff,
    },
    ColorDataBaseEntry {
        name: b"yellow2\0",
        code: 0xff00eeee,
    },
    ColorDataBaseEntry {
        name: b"yellow3\0",
        code: 0xff00cdcd,
    },
    ColorDataBaseEntry {
        name: b"yellow4\0",
        code: 0xff008b8b,
    },
    ColorDataBaseEntry {
        name: b"yellowgreen\0",
        code: 0xff32cd9a,
    },
];

// ---------------------------------------------------------------------------
// Color name/str lookup functions
// ---------------------------------------------------------------------------

unsafe fn name2col(nm: *const c_char) -> rcolor {
    if libc::strcmp(nm, b"NA\0".as_ptr() as *const c_char) == 0
        || libc::strcmp(nm, b"transparent\0".as_ptr() as *const c_char) == 0
    {
        return R_TRANWHITE;
    }
    for entry in COLOR_DATA_BASE.iter() {
        if entry.name.is_empty() {
            break;
        }
        if StrMatch(entry.name.as_ptr() as *const c_char, nm) != 0 {
            return entry.code;
        }
    }
    Rf_error(b"invalid color name\0".as_ptr() as *const c_char);
    0
}

unsafe fn str2col(s: *const c_char, bg: rcolor) -> rcolor {
    if *s == b'#' as libc::c_char {
        return rgb2col(s);
    }
    if (*s as c_int) >= b'0' as c_int && (*s as c_int) <= b'9' as c_int {
        let mut ptr: *mut c_char = std::ptr::null_mut();
        let indx = libc::strtod(s, &mut ptr) as c_int;
        if !ptr.is_null() && *ptr != 0 {
            Rf_error(b"invalid color specification\0".as_ptr() as *const c_char);
        }
        if indx == 0 {
            return bg;
        }
        let ps = PALETTE_SIZE.with(|v| v.get()) as usize;
        let idx = (indx as usize).wrapping_sub(1) % ps;
        PALETTE.with(|v| v.get()[idx])
    } else {
        name2col(s)
    }
}

// ---------------------------------------------------------------------------
// Internal engine functions (exported via function pointers)
// ---------------------------------------------------------------------------

/// Internal to external color representation.
pub unsafe extern "C" fn incol2name(col: c_uint) -> *const c_char {
    if R_OPAQUE(col) != 0 {
        for entry in COLOR_DATA_BASE.iter() {
            if entry.name.is_empty() {
                break;
            }
            if col == entry.code {
                return entry.name.as_ptr() as *const c_char;
            }
        }
        incol2name_buf_opaque(col)
    } else if R_TRANSPARENT(col) != 0 {
        b"transparent\0".as_ptr() as *const c_char
    } else {
        incol2name_buf_trans(col)
    }
}

pub unsafe extern "C" fn inR_GE_str2col(s: *const c_char) -> c_uint {
    if streql(s, b"0\0".as_ptr() as *const c_char) != 0 {
        Rf_error(b"invalid color specification\0".as_ptr() as *const c_char);
    }
    str2col(s, R_TRANWHITE)
}

/// Convert a sexp element to an R color desc.
pub unsafe fn inRGBpar3(x: SEXP, i: c_int, bg: rcolor) -> rcolor {
    let t = TYPEOF(x);
    let indx: c_int;
    match t {
        tt if tt == SEXPTYPE::STRSXP => {
            // STRSXP
            return str2col(CHAR(STRING_ELT(x, i as R_xlen_t)), bg);
        }
        tt if tt == SEXPTYPE::LGLSXP => {
            // LGLSXP
            indx = *LOGICAL(x).add(i as usize);
            if indx == NA_LOGICAL {
                return R_TRANWHITE;
            }
        }
        tt if tt == SEXPTYPE::INTSXP => {
            // INTSXP
            indx = *INTEGER(x).add(i as usize);
            if indx == NA_INTEGER {
                return R_TRANWHITE;
            }
        }
        tt if tt == SEXPTYPE::REALSXP => {
            // REALSXP
            if !R_FINITE(*REAL(x).add(i as usize)) {
                return R_TRANWHITE;
            }
            indx = *REAL(x).add(i as usize) as c_int;
        }
        _ => {
            return bg;
        }
    }
    if indx < 0 {
        Rf_error(b"numerical color values must be >= 0\0".as_ptr() as *const c_char);
    }
    if indx == 0 {
        return bg;
    }
    let ps = PALETTE_SIZE.with(|v| v.get()) as usize;
    let idx = (indx as usize).wrapping_sub(1) % ps;
    PALETTE.with(|v| v.get()[idx])
}

/// Save/restore palette (NOT #[unsafe(no_mangle)] — main/colors.rs already exports it)
unsafe extern "C" fn savePalette_impl(save: c_int) {
    let ps = PALETTE_SIZE.with(|v| v.get()) as usize;
    if save != 0 {
        let mut i = 0usize;
        while i < ps {
            PALETTE0.with(|v0| {
                let mut arr = v0.get();
                arr[i] = PALETTE.with(|v| v.get()[i]);
                v0.set(arr);
            });
            i += 1;
        }
    } else {
        let mut i = 0usize;
        while i < ps {
            PALETTE.with(|v| {
                let mut arr = v.get();
                arr[i] = PALETTE0.with(|v0| v0.get()[i]);
                v.set(arr);
            });
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// initPalette — register function pointers
// ---------------------------------------------------------------------------

/// Wrapper for inRGBpar3 with void pointer signature (for Rg_set_col_ptrs).
unsafe extern "C" fn inRGBpar3_dispatch(
    x: *mut std::os::raw::c_void,
    i: c_int,
    bg: c_uint,
) -> c_uint {
    inRGBpar3(x as SEXP, i, bg)
}

#[unsafe(no_mangle)]
pub unsafe fn initPalette() {
    crate::main::colors::Rg_set_col_ptrs(
        Some(inRGBpar3_dispatch),
        Some(incol2name),
        Some(inR_GE_str2col),
        Some(savePalette_impl),
    );
}

// ---------------------------------------------------------------------------
// SEXP-callable color functions
// ---------------------------------------------------------------------------

pub unsafe fn do_hsv(h: SEXP, s: SEXP, v: SEXP, a: SEXP) -> SEXP {
    let mut r: c_double = 0.0;
    let mut g: c_double = 0.0;
    let mut b: c_double = 0.0;

    let h = Rf_protect(coerceVector(h, SEXPTYPE::REALSXP.into()));
    let s = Rf_protect(coerceVector(s, SEXPTYPE::REALSXP.into()));
    let v = Rf_protect(coerceVector(v, SEXPTYPE::REALSXP.into()));
    let a = if Rf_isNull(a) != 0 {
        a
    } else {
        Rf_protect(coerceVector(a, SEXPTYPE::REALSXP.into()))
    };

    let nh = XLENGTH(h) as usize;
    let ns = XLENGTH(s) as usize;
    let nv = XLENGTH(v) as usize;
    let na = if Rf_isNull(a) != 0 {
        1usize
    } else {
        XLENGTH(a) as usize
    };

    if nh == 0 || ns == 0 || nv == 0 || na == 0 {
        Rf_unprotect(4);
        return Rf_allocVector(SEXPTYPE::STRSXP, 0);
    }

    let mut max = nh;
    if max < ns {
        max = ns;
    }
    if max < nv {
        max = nv;
    }
    if max < na {
        max = na;
    }
    let c = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, max as c_int));

    if Rf_isNull(a) != 0 {
        let mut i = 0usize;
        while i < max {
            let hh = *REAL(h).add(i % nh);
            let ss = *REAL(s).add(i % ns);
            let vv = *REAL(v).add(i % nv);
            if hh < 0.0 || hh > 1.0 || ss < 0.0 || ss > 1.0 || vv < 0.0 || vv > 1.0 {
                Rf_error(b"invalid hsv color\0".as_ptr() as *const c_char);
            }
            hsv2rgb(hh, ss, vv, &mut r, &mut g, &mut b);
            SET_STRING_ELT(
                c,
                i as R_xlen_t,
                Rf_mkChar(RGB2rgb_func(ScaleColor(r), ScaleColor(g), ScaleColor(b))),
            );
            i += 1;
        }
    } else {
        let mut i = 0usize;
        while i < max {
            let hh = *REAL(h).add(i % nh);
            let ss = *REAL(s).add(i % ns);
            let vv = *REAL(v).add(i % nv);
            let aa = *REAL(a).add(i % na);
            if hh < 0.0
                || hh > 1.0
                || ss < 0.0
                || ss > 1.0
                || vv < 0.0
                || vv > 1.0
                || aa < 0.0
                || aa > 1.0
            {
                Rf_error(b"invalid hsv color\0".as_ptr() as *const c_char);
            }
            hsv2rgb(hh, ss, vv, &mut r, &mut g, &mut b);
            SET_STRING_ELT(
                c,
                i as R_xlen_t,
                Rf_mkChar(RGBA2rgb_func(
                    ScaleColor(r),
                    ScaleColor(g),
                    ScaleColor(b),
                    ScaleAlpha(aa),
                )),
            );
            i += 1;
        }
    }
    Rf_unprotect(5);
    c
}

pub unsafe fn do_hcl(h: SEXP, c: SEXP, l: SEXP, a: SEXP, sfixup: SEXP) -> SEXP {
    let fixup = asLogical(sfixup);
    let h = Rf_protect(coerceVector(h, SEXPTYPE::REALSXP.into()));
    let c = Rf_protect(coerceVector(c, SEXPTYPE::REALSXP.into()));
    let l = Rf_protect(coerceVector(l, SEXPTYPE::REALSXP.into()));
    let a = if Rf_isNull(a) != 0 {
        a
    } else {
        Rf_protect(coerceVector(a, SEXPTYPE::REALSXP.into()))
    };

    let nh = XLENGTH(h) as usize;
    let nc = XLENGTH(c) as usize;
    let nl = XLENGTH(l) as usize;
    let na = if Rf_isNull(a) != 0 {
        1usize
    } else {
        XLENGTH(a) as usize
    };

    if nh == 0 || nc == 0 || nl == 0 || na == 0 {
        Rf_unprotect(4);
        return Rf_allocVector(SEXPTYPE::STRSXP, 0);
    }

    let mut max = nh;
    if max < nc {
        max = nc;
    }
    if max < nl {
        max = nl;
    }
    if max < na {
        max = na;
    }
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, max as c_int));

    if Rf_isNull(a) != 0 {
        let mut i = 0usize;
        while i < max {
            let H = *REAL(h).add(i % nh);
            let C = *REAL(c).add(i % nc);
            let L = *REAL(l).add(i % nl);
            if R_FINITE(H) && R_FINITE(C) && R_FINITE(L) {
                if L < 0.0 || L > WHITE_Y || C < 0.0 {
                    Rf_error(b"invalid hcl color\0".as_ptr() as *const c_char);
                }
                let mut rv: c_double = 0.0;
                let mut gv: c_double = 0.0;
                let mut bv: c_double = 0.0;
                hcl2rgb(H, C, L, &mut rv, &mut gv, &mut bv);
                let mut ir = (255.0 * rv + 0.5) as c_int;
                let mut ig = (255.0 * gv + 0.5) as c_int;
                let mut ib = (255.0 * bv + 0.5) as c_int;
                if FixupColor(&mut ir, &mut ig, &mut ib) != 0 && fixup == 0 {
                    SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
                } else {
                    SET_STRING_ELT(
                        ans,
                        i as R_xlen_t,
                        Rf_mkChar(RGB2rgb_func(ir as u32, ig as u32, ib as u32)),
                    );
                }
            } else {
                SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
            }
            i += 1;
        }
    } else {
        let mut i = 0usize;
        while i < max {
            let H = *REAL(h).add(i % nh);
            let C = *REAL(c).add(i % nc);
            let L = *REAL(l).add(i % nl);
            let mut A = *REAL(a).add(i % na);
            if !R_FINITE(A) {
                A = 1.0;
            }
            if R_FINITE(H) && R_FINITE(C) && R_FINITE(L) {
                if L < 0.0 || L > WHITE_Y || C < 0.0 || A < 0.0 || A > 1.0 {
                    Rf_error(b"invalid hcl color\0".as_ptr() as *const c_char);
                }
                let mut rv: c_double = 0.0;
                let mut gv: c_double = 0.0;
                let mut bv: c_double = 0.0;
                hcl2rgb(H, C, L, &mut rv, &mut gv, &mut bv);
                let mut ir = (255.0 * rv + 0.5) as c_int;
                let mut ig = (255.0 * gv + 0.5) as c_int;
                let mut ib = (255.0 * bv + 0.5) as c_int;
                if FixupColor(&mut ir, &mut ig, &mut ib) != 0 && fixup == 0 {
                    SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
                } else {
                    SET_STRING_ELT(
                        ans,
                        i as R_xlen_t,
                        Rf_mkChar(RGBA2rgb_func(
                            ir as u32,
                            ig as u32,
                            ib as u32,
                            ScaleAlpha(A),
                        )),
                    );
                }
            } else {
                SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
            }
            i += 1;
        }
    }
    Rf_unprotect(5);
    ans
}

pub unsafe fn do_rgb(r: SEXP, g: SEXP, b: SEXP, a: SEXP, mcv: SEXP, nam: SEXP) -> SEXP {
    let mV = asReal(mcv);
    if !R_FINITE(mV) || mV == 0.0 {
        Rf_error(b"invalid value of 'maxColorValue'\0".as_ptr() as *const c_char);
    }

    let (r, g, b, a) = if mV == 255.0 {
        (
            Rf_protect(coerceVector(r, SEXPTYPE::INTSXP.into())),
            Rf_protect(coerceVector(g, SEXPTYPE::INTSXP.into())),
            Rf_protect(coerceVector(b, SEXPTYPE::INTSXP.into())),
            if Rf_isNull(a) != 0 {
                a
            } else {
                Rf_protect(coerceVector(a, SEXPTYPE::INTSXP.into()))
            },
        )
    } else {
        (
            Rf_protect(coerceVector(r, SEXPTYPE::REALSXP.into())),
            Rf_protect(coerceVector(g, SEXPTYPE::REALSXP.into())),
            Rf_protect(coerceVector(b, SEXPTYPE::REALSXP.into())),
            if Rf_isNull(a) != 0 {
                a
            } else {
                Rf_protect(coerceVector(a, SEXPTYPE::REALSXP.into()))
            },
        )
    };

    let nr = XLENGTH(r) as usize;
    let ng = XLENGTH(g) as usize;
    let nb = XLENGTH(b) as usize;
    let na = if Rf_isNull(a) != 0 {
        1usize
    } else {
        XLENGTH(a) as usize
    };

    if nr == 0 || ng == 0 || nb == 0 || na == 0 {
        Rf_unprotect(4);
        return Rf_allocVector(SEXPTYPE::STRSXP, 0);
    }

    let mut l_max = nr;
    if l_max < ng {
        l_max = ng;
    }
    if l_max < nb {
        l_max = nb;
    }
    if l_max < na {
        l_max = na;
    }

    let nam = Rf_protect(coerceVector(nam, SEXPTYPE::STRSXP.into()));
    if LENGTH(nam) != 0 && LENGTH(nam) != l_max as c_int {
        Rf_error(b"invalid 'names' vector\0".as_ptr() as *const c_char);
    }
    let c = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, l_max as c_int));

    if mV == 255.0 {
        if Rf_isNull(a) != 0 {
            let mut i = 0usize;
            while i < l_max {
                let sr = CheckColor(*INTEGER(r).add(i % nr));
                let sg = CheckColor(*INTEGER(g).add(i % ng));
                let sb = CheckColor(*INTEGER(b).add(i % nb));
                SET_STRING_ELT(c, i as R_xlen_t, Rf_mkChar(RGB2rgb_func(sr, sg, sb)));
                i += 1;
            }
        } else {
            let mut i = 0usize;
            while i < l_max {
                let sr = CheckColor(*INTEGER(r).add(i % nr));
                let sg = CheckColor(*INTEGER(g).add(i % ng));
                let sb = CheckColor(*INTEGER(b).add(i % nb));
                let sa = CheckAlpha(*INTEGER(a).add(i % na));
                SET_STRING_ELT(c, i as R_xlen_t, Rf_mkChar(RGBA2rgb_func(sr, sg, sb, sa)));
                i += 1;
            }
        }
    } else if mV == 1.0 {
        if Rf_isNull(a) != 0 {
            let mut i = 0usize;
            while i < l_max {
                let sr = ScaleColor(*REAL(r).add(i % nr));
                let sg = ScaleColor(*REAL(g).add(i % ng));
                let sb = ScaleColor(*REAL(b).add(i % nb));
                SET_STRING_ELT(c, i as R_xlen_t, Rf_mkChar(RGB2rgb_func(sr, sg, sb)));
                i += 1;
            }
        } else {
            let mut i = 0usize;
            while i < l_max {
                let sr = ScaleColor(*REAL(r).add(i % nr));
                let sg = ScaleColor(*REAL(g).add(i % ng));
                let sb = ScaleColor(*REAL(b).add(i % nb));
                let sa = ScaleAlpha(*REAL(a).add(i % na));
                SET_STRING_ELT(c, i as R_xlen_t, Rf_mkChar(RGBA2rgb_func(sr, sg, sb, sa)));
                i += 1;
            }
        }
    } else {
        if Rf_isNull(a) != 0 {
            let mut i = 0usize;
            while i < l_max {
                let sr = ScaleColor(*REAL(r).add(i % nr) / mV);
                let sg = ScaleColor(*REAL(g).add(i % ng) / mV);
                let sb = ScaleColor(*REAL(b).add(i % nb) / mV);
                SET_STRING_ELT(c, i as R_xlen_t, Rf_mkChar(RGB2rgb_func(sr, sg, sb)));
                i += 1;
            }
        } else {
            let mut i = 0usize;
            while i < l_max {
                let sr = ScaleColor(*REAL(r).add(i % nr) / mV);
                let sg = ScaleColor(*REAL(g).add(i % ng) / mV);
                let sb = ScaleColor(*REAL(b).add(i % nb) / mV);
                let sa = ScaleAlpha(*REAL(a).add(i % na) / mV);
                SET_STRING_ELT(c, i as R_xlen_t, Rf_mkChar(RGBA2rgb_func(sr, sg, sb, sa)));
                i += 1;
            }
        }
    }

    if LENGTH(nam) != 0 {
        setAttrib(c, R_NamesSymbol(), nam);
    }
    Rf_unprotect(6);
    c
}

pub unsafe fn do_gray(lev: SEXP, a: SEXP) -> SEXP {
    let lev = Rf_protect(coerceVector(lev, SEXPTYPE::REALSXP.into()));
    let nlev = LENGTH(lev) as usize;
    let ans = Rf_allocVector(SEXPTYPE::STRSXP, nlev as c_int);
    if nlev == 0 {
        Rf_unprotect(1);
        return ans;
    }
    Rf_protect(ans);
    let a = if Rf_isNull(a) != 0 {
        a
    } else {
        Rf_protect(coerceVector(a, SEXPTYPE::REALSXP.into()))
    };

    if Rf_isNull(a) != 0 {
        let mut i = 0usize;
        while i < nlev {
            let level = *REAL(lev).add(i);
            if ISNAN(level) || level < 0.0 || level > 1.0 {
                Rf_error(b"invalid gray level, must be in [0,1].\0".as_ptr() as *const c_char);
            }
            let ilevel = (255.0 * level + 0.5) as c_int;
            SET_STRING_ELT(
                ans,
                i as R_xlen_t,
                Rf_mkChar(RGB2rgb_func(ilevel as u32, ilevel as u32, ilevel as u32)),
            );
            i += 1;
        }
        Rf_unprotect(2);
    } else {
        let na = LENGTH(a) as usize;
        let max = if nlev > na { nlev } else { na };
        let mut i = 0usize;
        while i < max {
            let level = *REAL(lev).add(i % nlev);
            if ISNAN(level) || level < 0.0 || level > 1.0 {
                Rf_error(b"invalid gray level, must be in [0,1].\0".as_ptr() as *const c_char);
            }
            let ilevel = (255.0 * level + 0.5) as c_int;
            let aa = *REAL(a).add(i % na);
            SET_STRING_ELT(
                ans,
                i as R_xlen_t,
                Rf_mkChar(RGBA2rgb_func(
                    ilevel as u32,
                    ilevel as u32,
                    ilevel as u32,
                    ScaleAlpha(aa),
                )),
            );
            i += 1;
        }
        Rf_unprotect(3);
    }
    ans
}

pub unsafe fn do_RGB2hsv(rgb: SEXP) -> SEXP {
    use crate::main::array::allocMatrix;

    let rgb = Rf_protect(coerceVector(rgb, SEXPTYPE::REALSXP.into()));
    if !isMatrix(rgb) {
        Rf_error(b"rgb is not a matrix (internally)\0".as_ptr() as *const c_char);
    }
    let dd = getAttrib(rgb, R_DimSymbol());
    if *INTEGER(dd).add(0) != 3 {
        Rf_error(b"rgb must have 3 rows (internally)\0".as_ptr() as *const c_char);
    }
    let n = *INTEGER(dd).add(1) as usize;

    let ans = Rf_protect(allocMatrix(SEXPTYPE::REALSXP.into(), 3, n as c_int));
    let dmns = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 2));
    let names = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, 3));
    SET_STRING_ELT(names, 0, Rf_mkChar(b"h\0".as_ptr() as *const c_char));
    SET_STRING_ELT(names, 1, Rf_mkChar(b"s\0".as_ptr() as *const c_char));
    SET_STRING_ELT(names, 2, Rf_mkChar(b"v\0".as_ptr() as *const c_char));
    SET_VECTOR_ELT(dmns, 0, names);

    if Rf_isNull(getAttrib(rgb, R_DimNamesSymbol())) == 0 {
        let dd2 = getAttrib(rgb, R_DimNamesSymbol());
        if Rf_isNull(dd2) == 0 {
            let col_names = VECTOR_ELT(dd2, 1);
            if Rf_isNull(col_names) == 0 {
                SET_VECTOR_ELT(dmns, 1, col_names);
            }
        }
    }
    setAttrib(ans, R_DimNamesSymbol(), dmns);
    Rf_unprotect(2);

    let mut i = 0usize;
    let mut i3 = 0usize;
    while i < n {
        rgb2hsv(
            *REAL(rgb).add(i3),
            *REAL(rgb).add(i3 + 1),
            *REAL(rgb).add(i3 + 2),
            &mut *REAL(ans).add(i3),
            &mut *REAL(ans).add(i3 + 1),
            &mut *REAL(ans).add(i3 + 2),
        );
        i += 1;
        i3 += 3;
    }
    Rf_unprotect(2);
    ans
}

pub unsafe fn do_col2rgb(colors: SEXP, alpha: SEXP) -> SEXP {
    use crate::main::array::allocMatrix;

    let alph = asLogical(alpha);
    if alph == NA_LOGICAL {
        Rf_error(b"invalid 'alpha' value\0".as_ptr() as *const c_char);
    }

    let t = TYPEOF(colors);
    let colors = match t {
        tt if tt == SEXPTYPE::INTSXP || tt == SEXPTYPE::STRSXP => colors, // INTSXP or STRSXP
        tt if tt == SEXPTYPE::REALSXP => Rf_protect(coerceVector(colors, SEXPTYPE::INTSXP.into())),
        _ => Rf_protect(coerceVector(colors, SEXPTYPE::STRSXP.into())),
    };
    Rf_protect(colors);

    let n = LENGTH(colors) as usize;
    let ans = Rf_protect(allocMatrix(SEXPTYPE::INTSXP.into(), 3 + alph, n as c_int));
    let dmns = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 2));
    let names = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, 3 + alph));
    SET_STRING_ELT(names, 0, Rf_mkChar(b"red\0".as_ptr() as *const c_char));
    SET_STRING_ELT(names, 1, Rf_mkChar(b"green\0".as_ptr() as *const c_char));
    SET_STRING_ELT(names, 2, Rf_mkChar(b"blue\0".as_ptr() as *const c_char));
    if alph != 0 {
        SET_STRING_ELT(names, 3, Rf_mkChar(b"alpha\0".as_ptr() as *const c_char));
    }
    SET_VECTOR_ELT(dmns, 0, names);

    let col_names = getAttrib(colors, R_NamesSymbol());
    if Rf_isNull(col_names) == 0 {
        SET_VECTOR_ELT(dmns, 1, col_names);
    }
    setAttrib(ans, R_DimNamesSymbol(), dmns);

    let mut j = 0usize;
    let mut i = 0usize;
    while i < n {
        let icol = inRGBpar3(colors, i as c_int, R_TRANWHITE);
        *INTEGER(ans).add(j) = R_RED(icol);
        j += 1;
        *INTEGER(ans).add(j) = R_GREEN(icol);
        j += 1;
        *INTEGER(ans).add(j) = R_BLUE(icol);
        j += 1;
        if alph != 0 {
            *INTEGER(ans).add(j) = R_ALPHA(icol);
            j += 1;
        }
        i += 1;
    }
    Rf_unprotect(4);
    ans
}

pub unsafe fn do_palette(val: SEXP) -> SEXP {
    if Rf_isString(val) == 0 {
        Rf_error(b"invalid argument type\0".as_ptr() as *const c_char);
    }

    // Record current palette
    let ps = PALETTE_SIZE.with(|v| v.get()) as usize;
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, ps as c_int));
    let mut i = 0usize;
    while i < ps {
        SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(incol2name(PALETTE.with(|v| v.get()[i]))));
        i += 1;
    }

    let n = LENGTH(val) as usize;
    if n == 1 {
        let s = CHAR(STRING_ELT(val, 0));
        if StrMatch(b"default\0".as_ptr() as *const c_char, s) != 0 {
            let mut i = 0usize;
            while i < 8 {
                PALETTE.with(|v| {
                    let mut arr = v.get();
                    arr[i] = DEFAULT_PALETTE[i];
                    v.set(arr);
                });
                i += 1;
            }
            PALETTE_SIZE.with(|v| v.set(8));
        } else {
            Rf_error(b"unknown palette (need >= 2 colors)\0".as_ptr() as *const c_char);
        }
    } else if n > 1 {
        if n > MAX_PALETTE_SIZE {
            Rf_error(b"maximum number of colors is 1024\0".as_ptr() as *const c_char);
        }
        let mut color_buf: [rcolor; MAX_PALETTE_SIZE] = [0; MAX_PALETTE_SIZE];
        let mut i = 0usize;
        while i < n {
            let s = CHAR(STRING_ELT(val, i as R_xlen_t));
            color_buf[i] = if *s == b'#' as libc::c_char {
                rgb2col(s)
            } else {
                name2col(s)
            };
            i += 1;
        }
        let mut i = 0usize;
        while i < n {
            PALETTE.with(|v| {
                let mut arr = v.get();
                arr[i] = color_buf[i];
                v.set(arr);
            });
            i += 1;
        }
        PALETTE_SIZE.with(|v| v.set(n as c_int));
    }

    Rf_unprotect(1);
    ans
}

pub unsafe fn do_palette2(val: SEXP) -> SEXP {
    let ps = PALETTE_SIZE.with(|v| v.get()) as usize;
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP, ps as c_int));
    let ians = INTEGER(ans);
    let mut i = 0usize;
    while i < ps {
        *ians.add(i) = PALETTE.with(|v| v.get()[i]) as c_int;
        i += 1;
    }

    let n = LENGTH(val) as usize;
    if n > 0 {
        if TYPEOF(val) != SEXPTYPE::INTSXP {
            Rf_error(b"requires INTSXP argument\0".as_ptr() as *const c_char);
        }
        if n > MAX_PALETTE_SIZE {
            Rf_error(b"maximum number of colors is 1024\0".as_ptr() as *const c_char);
        }
        let mut i = 0usize;
        while i < n {
            PALETTE.with(|v| {
                let mut arr = v.get();
                arr[i] = *INTEGER(val).add(i) as rcolor;
                v.set(arr);
            });
            i += 1;
        }
        PALETTE_SIZE.with(|v| v.set(n as c_int));
    }
    Rf_unprotect(1);
    ans
}

pub unsafe fn do_colors() -> SEXP {
    // Count entries
    let mut n = 0usize;
    for entry in COLOR_DATA_BASE.iter() {
        if entry.name.is_empty() {
            break;
        }
        n += 1;
    }
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, n as c_int));
    let mut i = 0usize;
    for entry in COLOR_DATA_BASE.iter() {
        if entry.name.is_empty() {
            break;
        }
        SET_STRING_ELT(
            ans,
            i as R_xlen_t,
            Rf_mkChar(entry.name.as_ptr() as *const c_char),
        );
        i += 1;
    }
    Rf_unprotect(1);
    ans
}
