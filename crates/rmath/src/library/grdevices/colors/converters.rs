//! colors — extracted verbatim from colors.rs.

use super::*;

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

pub(crate) const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

pub(crate) unsafe fn streql(a: *const c_char, b: *const c_char) -> c_int {
    unsafe {
        if a.is_null() || b.is_null() {
            return 0;
        }
        if std::ffi::CStr::from_ptr(a).to_bytes() == std::ffi::CStr::from_ptr(b).to_bytes() {
            1
        } else {
            0
        }
    }
}

pub(crate) unsafe fn isMatrix(x: SEXP) -> bool {
    unsafe {
        let dim = getAttrib(x, R_DimSymbol());
        Rf_isNull(dim) == 0 && LENGTH(dim) == 2
    }
}

pub(crate) unsafe fn RGB2rgb_func(r: u32, g: u32, b: u32) -> *const c_char {
    unsafe {
        with_color_state(|state| {
            let c = &mut state.col_buf;
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
}

pub(crate) unsafe fn RGBA2rgb_func(r: u32, g: u32, b: u32, a: u32) -> *const c_char {
    unsafe {
        with_color_state(|state| {
            let c = &mut state.col_buf;
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
}

pub(crate) unsafe fn incol2name_buf_opaque(col: rcolor) -> *const c_char {
    unsafe {
        with_color_state(|state| {
            let c = &mut state.col_buf;
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
}

pub(crate) unsafe fn incol2name_buf_trans(col: rcolor) -> *const c_char {
    unsafe {
        with_color_state(|state| {
            let c = &mut state.col_buf;
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
}

pub(crate) unsafe fn ScaleColor(x: c_double) -> u32 {
    unsafe {
        if ISNAN(x) {
            Rf_error(b"color intensity NA, not in [0,1]\0".as_ptr() as *const c_char);
        }
        if !R_FINITE(x) || x < 0.0 || x > 1.0 {
            Rf_error(b"color intensity not in [0,1]\0".as_ptr() as *const c_char);
        }
        (255.0 * x + 0.5) as u32
    }
}

pub(crate) unsafe fn CheckColor(x: c_int) -> u32 {
    unsafe {
        if x == NA_INTEGER {
            Rf_error(b"color intensity NA, not in 0:255\0".as_ptr() as *const c_char);
        }
        if x < 0 || x > 255 {
            Rf_error(b"color intensity not in 0:255\0".as_ptr() as *const c_char);
        }
        x as u32
    }
}

pub(crate) unsafe fn ScaleAlpha(x: c_double) -> u32 {
    unsafe {
        if ISNAN(x) {
            Rf_error(b"alpha level NA, not in [0,1]\0".as_ptr() as *const c_char);
        }
        if !R_FINITE(x) || x < 0.0 || x > 1.0 {
            Rf_error(b"alpha level not in [0,1]\0".as_ptr() as *const c_char);
        }
        (255.0 * x + 0.5) as u32
    }
}

pub(crate) unsafe fn CheckAlpha(x: c_int) -> u32 {
    unsafe {
        if x == NA_INTEGER {
            Rf_error(b"alpha level NA, not in 0:255\0".as_ptr() as *const c_char);
        }
        if x < 0 || x > 255 {
            Rf_error(b"alpha level not in 0:255\0".as_ptr() as *const c_char);
        }
        x as u32
    }
}

// ---------------------------------------------------------------------------
// HSV <-> RGB conversion
// ---------------------------------------------------------------------------

pub(crate) unsafe fn hsv2rgb(
    h: c_double,
    s: c_double,
    v: c_double,
    r: &mut c_double,
    g: &mut c_double,
    b: &mut c_double,
) {
    unsafe {
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
}

pub(crate) unsafe fn rgb2hsv(
    r: c_double,
    g: c_double,
    b: c_double,
    h: &mut c_double,
    s: &mut c_double,
    v: &mut c_double,
) {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// HCL -> RGB conversion
// ---------------------------------------------------------------------------

pub(crate) unsafe fn gtrans(u: c_double) -> c_double {
    unsafe {
        if u > 0.00304 {
            1.055 * u.powf(1.0 / GAMMA) - 0.055
        } else {
            12.92 * u
        }
    }
}

pub(crate) unsafe fn FixupColor(r: &mut c_int, g: &mut c_int, b: &mut c_int) -> c_int {
    unsafe {
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
}

pub(crate) unsafe fn hcl2rgb(
    h: c_double,
    c: c_double,
    l: c_double,
    rv: &mut c_double,
    gv: &mut c_double,
    bv: &mut c_double,
) {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// String matching
// ---------------------------------------------------------------------------

/// Palette names are ASCII on every platform. Avoid locale-dependent libc
/// calls (and its undefined behavior for negative signed character bytes).
#[inline]
pub(crate) fn c_tolower(c: c_int) -> c_int {
    if (0..128).contains(&c) {
        (c as u8).to_ascii_lowercase() as c_int
    } else {
        c
    }
}

pub(crate) unsafe fn StrMatch(s: *const c_char, t: *const c_char) -> c_int {
    unsafe {
        let mut si = 0usize;
        let mut ti = 0usize;
        loop {
            let sc = *s.add(si);
            let tc = *t.add(ti);
            if sc == 0 && tc == 0 {
                return 1;
            }
            if sc == b' ' as core::ffi::c_char {
                si += 1;
                continue;
            }
            if tc == b' ' as core::ffi::c_char {
                ti += 1;
                continue;
            }
            if c_tolower(sc as c_int) != c_tolower(tc as c_int) {
                return 0;
            }
            si += 1;
            ti += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Hex digit conversion
// ---------------------------------------------------------------------------

pub(crate) unsafe fn hexdigit(d: c_int) -> u32 {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// Color specification parsing
// ---------------------------------------------------------------------------

pub(crate) unsafe fn rgb2col(rgb: *const c_char) -> rcolor {
    unsafe {
        if *rgb != b'#' as core::ffi::c_char {
            Rf_error(b"invalid RGB specification\0".as_ptr() as *const c_char);
        }
        let len = std::ffi::CStr::from_ptr(rgb).to_bytes().len();
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
}

// ---------------------------------------------------------------------------
// Color name/str lookup functions
// ---------------------------------------------------------------------------

pub(crate) unsafe fn name2col(nm: *const c_char) -> rcolor {
    unsafe {
        let nm_bytes = std::ffi::CStr::from_ptr(nm).to_bytes();
        if nm_bytes == b"NA".as_slice() || nm_bytes == b"transparent".as_slice() {
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
        let name = std::ffi::CStr::from_ptr(nm).to_string_lossy();
        crate::sexp::context::r_error(format!("invalid color name '{name}'"))
    }
}

pub(crate) unsafe fn str2col(s: *const c_char, bg: rcolor) -> rcolor {
    unsafe {
        if *s == b'#' as core::ffi::c_char {
            return rgb2col(s);
        }
        if (*s as c_int) >= b'0' as c_int && (*s as c_int) <= b'9' as c_int {
            let token = std::ffi::CStr::from_ptr(s).to_bytes();
            let indx = super::numeric::palette_index(token)
                .unwrap_or_else(|| crate::sexp::context::r_error("invalid color specification"));
            if indx == 0 {
                return bg;
            }
            let ps = with_color_state(|state| state.palette_size) as usize;
            let idx = (indx - 1) % ps;
            with_color_state(|state| state.palette[idx])
        } else {
            name2col(s)
        }
    }
}

// ---------------------------------------------------------------------------
// Internal engine functions (exported via function pointers)
// ---------------------------------------------------------------------------

/// Internal to external color representation.
pub unsafe extern "C" fn incol2name(col: c_uint) -> *const c_char {
    unsafe {
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
}

pub unsafe extern "C" fn inR_GE_str2col(s: *const c_char) -> c_uint {
    unsafe {
        if streql(s, b"0\0".as_ptr() as *const c_char) != 0 {
            Rf_error(b"invalid color specification\0".as_ptr() as *const c_char);
        }
        str2col(s, R_TRANWHITE)
    }
}

/// Convert a sexp element to an R color desc.
pub unsafe fn inRGBpar3(x: SEXP, i: c_int, bg: rcolor) -> rcolor {
    unsafe {
        let t = TYPEOF(x);
        let indx: c_int;
        match t {
            tt if tt == SEXPTYPE::STRSXP => {
                // Keep the original runtime and selected child alive across an
                // ALTREP provider. NA_STRING has no byte payload in this engine.
                let fail = |error: crate::sexp::object::SexpError| -> ! {
                    crate::sexp::context::r_error(error.to_string())
                };
                let owner =
                    crate::sexp::owner::OwnerToken::current().unwrap_or_else(|error| fail(error));
                let _pin = owner.pin().unwrap_or_else(|error| fail(error));
                let parent = owner.sexp(x).unwrap_or_else(|error| fail(error));
                let character = parent
                    .try_string_elt(i as R_xlen_t)
                    .unwrap_or_else(|error| fail(error));
                owner.require_active().unwrap_or_else(|error| fail(error));
                if character.is_na_string() {
                    return R_TRANWHITE;
                }
                let text = character
                    .try_as_string()
                    .unwrap_or_else(|error| fail(error));
                let text = std::ffi::CString::new(text).unwrap_or_else(|_| {
                    crate::sexp::context::r_error("invalid color specification")
                });
                return str2col(text.as_ptr(), bg);
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
        let ps = with_color_state(|state| state.palette_size) as usize;
        let idx = (indx as usize).wrapping_sub(1) % ps;
        with_color_state(|state| state.palette[idx])
    }
}
