#![allow(unsafe_op_in_unsafe_fn)]
#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's `src/library/grDevices/src/devPicTeX.c`.
//!
//! PicTeX graphics device for R. Generates LaTeX/PicTeX code for plotting.
//!
//! The device writes LaTeX picture environment commands to a FILE*.
//! Since it only generates text output (no external libraries needed),
//! ALL drawing functions have real implementations that write LaTeX.
//!
//! Exported functions:
//!   PicTeX(SEXP args) -> SEXP

use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;

use crate::main::coerce::{asLogical, asReal};
use crate::main::colors::R_GE_str2col;
use crate::main::devices::{
    GEaddDevice2f, GEcreateDD, GEcreateDevDesc, GEfreeDD, R_CheckDeviceAvailable,
};
use crate::main::engine::{R_GE_definitions, pDevDesc, pGEcontext};
use crate::main::errors::Rf_error;
use crate::main::relop::NA_STRING;
use crate::main::sysutils::{R_ExpandFileName, R_fopen, translateCharFP};
use crate::main::util_main::asChar;
use crate::sexp::accessors::{CAR, CDR, CHAR};
use crate::sexp::ffi::{NA_LOGICAL, SEXP};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory_ext::{vmaxget, vmaxset};

/* ==================== Constants ==================== */

const DOTSperIN: c_double = 72.27;

#[inline(always)]
fn in2dots(x: c_double) -> c_double {
    DOTSperIN * x
}

/* ==================== File writing helpers ==================== */

/// Helper to write a formatted string to a FILE*.
/// Uses Rust's format! and then writes via libc::fputs.
/// Returns the number of bytes written, or -1 on error.
#[inline]
unsafe fn fprintf(fp: *mut libc::FILE, fmt: std::fmt::Arguments<'_>) -> c_int {
    let s = fmt.to_string();
    let bytes = s.as_bytes();
    let n = bytes.len();
    if libc::fputs(s.as_ptr() as *const c_char, fp) == libc::EOF {
        return -1;
    }
    n as c_int
}

/// Helper to write a single char to a FILE*.
#[inline]
unsafe fn fputc_ch(c: u8, fp: *mut libc::FILE) -> c_int {
    libc::fputc(c as c_int, fp)
}

/* ==================== Device-specific descriptor ==================== */

/// PicTeX device-specific information.
///
/// In the C code this is heap-allocated and attached to DevDesc->deviceSpecific.
#[repr(C)]
struct picTeXDesc {
    texfp: *mut libc::FILE,
    filename: [c_char; 128],
    pageno: c_int,
    landscape: c_int,
    width: c_double,
    height: c_double,
    pagewidth: c_double,
    pageheight: c_double,
    xlast: c_double,
    ylast: c_double,
    clipleft: c_double,
    clipright: c_double,
    cliptop: c_double,
    clipbottom: c_double,
    clippedx0: c_double,
    clippedy0: c_double,
    clippedx1: c_double,
    clippedy1: c_double,
    lty: c_int,
    col: u32,
    fill: u32,
    fontsize: c_int,
    fontface: c_int,
    debug: bool,
}

/* ==================== Font tables ==================== */

/// Character width tables for 4 font faces (128 entries each).
/// Indexed as charwidth[fontface-1][char_code].
static CHARWIDTH: [[c_double; 128]; 4] = [
    [
        0.5416690, 0.8333360, 0.7777810, 0.6111145, 0.6666690, 0.7083380, 0.7222240, 0.7777810,
        0.7222240, 0.7777810, 0.7222240, 0.5833360, 0.5361130, 0.5361130, 0.8138910, 0.8138910,
        0.2388900, 0.2666680, 0.5000020, 0.5000020, 0.5000020, 0.5000020, 0.5000020, 0.6666700,
        0.4444460, 0.4805580, 0.7222240, 0.7777810, 0.5000020, 0.8611145, 0.9722260, 0.7777810,
        0.2388900, 0.3194460, 0.5000020, 0.8333360, 0.5000020, 0.8333360, 0.7583360, 0.2777790,
        0.3888900, 0.3888900, 0.5000020, 0.7777810, 0.2777790, 0.3333340, 0.2777790, 0.5000020,
        0.5000020, 0.5000020, 0.5000020, 0.5000020, 0.5000020, 0.5000020, 0.5000020, 0.5000020,
        0.5000020, 0.5000020, 0.2777790, 0.2777790, 0.3194460, 0.7777810, 0.4722240, 0.4722240,
        0.6666690, 0.6666700, 0.6666700, 0.6388910, 0.7222260, 0.5972240, 0.5694475, 0.6666690,
        0.7083380, 0.2777810, 0.4722240, 0.6944480, 0.5416690, 0.8750050, 0.7083380, 0.7361130,
        0.6388910, 0.7361130, 0.6458360, 0.5555570, 0.6805570, 0.6875050, 0.6666700, 0.9444480,
        0.6666700, 0.6666700, 0.6111130, 0.2888900, 0.5000020, 0.2888900, 0.5000020, 0.2777790,
        0.2777790, 0.4805570, 0.5166680, 0.4444460, 0.5166680, 0.4444460, 0.3055570, 0.5000020,
        0.5166680, 0.2388900, 0.2666680, 0.4888920, 0.2388900, 0.7944470, 0.5166680, 0.5000020,
        0.5166680, 0.5166680, 0.3416690, 0.3833340, 0.3611120, 0.5166680, 0.4611130, 0.6833360,
        0.4611130, 0.4611130, 0.4347230, 0.5000020, 1.0000030, 0.5000020, 0.5000020, 0.5000020,
    ],
    [
        0.5805590, 0.9166720, 0.8555600, 0.6722260, 0.7333370, 0.7944490, 0.7944490, 0.8555600,
        0.7944490, 0.8555600, 0.7944490, 0.6416700, 0.5861150, 0.5861150, 0.8916720, 0.8916720,
        0.2555570, 0.2861130, 0.5500030, 0.5500030, 0.5500030, 0.5500030, 0.5500030, 0.7333370,
        0.4888920, 0.5652800, 0.7944490, 0.8555600, 0.5500030, 0.9472275, 1.0694500, 0.8555600,
        0.2555570, 0.3666690, 0.5583360, 0.9166720, 0.5500030, 1.0291190, 0.8305610, 0.3055570,
        0.4277800, 0.4277800, 0.5500030, 0.8555600, 0.3055570, 0.3666690, 0.3055570, 0.5500030,
        0.5500030, 0.5500030, 0.5500030, 0.5500030, 0.5500030, 0.5500030, 0.5500030, 0.5500030,
        0.5500030, 0.5500030, 0.3055570, 0.3055570, 0.3666690, 0.8555600, 0.5194470, 0.5194470,
        0.7333370, 0.7333370, 0.7333370, 0.7027820, 0.7944490, 0.6416700, 0.6111145, 0.7333370,
        0.7944490, 0.3305570, 0.5194470, 0.7638930, 0.5805590, 0.9777830, 0.7944490, 0.7944490,
        0.7027820, 0.7944490, 0.7027820, 0.6111145, 0.7333370, 0.7638930, 0.7333370, 1.0388950,
        0.7333370, 0.7333370, 0.6722260, 0.3430580, 0.5583360, 0.3430580, 0.5500030, 0.3055570,
        0.3055570, 0.5250030, 0.5611140, 0.4888920, 0.5611140, 0.5111140, 0.3361130, 0.5500030,
        0.5611140, 0.2555570, 0.2861130, 0.5305590, 0.2555570, 0.8666720, 0.5611140, 0.5500030,
        0.5611140, 0.5611140, 0.3722250, 0.4216690, 0.4041690, 0.5611140, 0.5000030, 0.7444490,
        0.5000030, 0.5000030, 0.4763920, 0.5500030, 1.1000060, 0.5500030, 0.5500030, 0.5500030,
    ],
    [
        0.5416690, 0.8333360, 0.7777810, 0.6111145, 0.6666690, 0.7083380, 0.7222240, 0.7777810,
        0.7222240, 0.7777810, 0.7222240, 0.5833360, 0.5361130, 0.5361130, 0.8138910, 0.8138910,
        0.2388900, 0.2666680, 0.5000020, 0.5000020, 0.5000020, 0.5000020, 0.5000020, 0.7375210,
        0.4444460, 0.4805580, 0.7222240, 0.7777810, 0.5000020, 0.8611145, 0.9722260, 0.7777810,
        0.2388900, 0.3194460, 0.5000020, 0.8333360, 0.5000020, 0.8333360, 0.7583360, 0.2777790,
        0.3888900, 0.3888900, 0.5000020, 0.7777810, 0.2777790, 0.3333340, 0.2777790, 0.5000020,
        0.5000020, 0.5000020, 0.5000020, 0.5000020, 0.5000020, 0.5000020, 0.5000020, 0.5000020,
        0.5000020, 0.5000020, 0.2777790, 0.2777790, 0.3194460, 0.7777810, 0.4722240, 0.4722240,
        0.6666690, 0.6666700, 0.6666700, 0.6388910, 0.7222260, 0.5972240, 0.5694475, 0.6666690,
        0.7083380, 0.2777810, 0.4722240, 0.6944480, 0.5416690, 0.8750050, 0.7083380, 0.7361130,
        0.6388910, 0.7361130, 0.6458360, 0.5555570, 0.6805570, 0.6875050, 0.6666700, 0.9444480,
        0.6666700, 0.6666700, 0.6111130, 0.2888900, 0.5000020, 0.2888900, 0.5000020, 0.2777790,
        0.2777790, 0.4805570, 0.5166680, 0.4444460, 0.5166680, 0.4444460, 0.3055570, 0.5000020,
        0.5166680, 0.2388900, 0.2666680, 0.4888920, 0.2388900, 0.7944470, 0.5166680, 0.5000020,
        0.5166680, 0.5166680, 0.3416690, 0.3833340, 0.3611120, 0.5166680, 0.4611130, 0.6833360,
        0.4611130, 0.4611130, 0.4347230, 0.5000020, 1.0000030, 0.5000020, 0.5000020, 0.5000020,
    ],
    [
        0.5805590, 0.9166720, 0.8555600, 0.6722260, 0.7333370, 0.7944490, 0.7944490, 0.8555600,
        0.7944490, 0.8555600, 0.7944490, 0.6416700, 0.5861150, 0.5861150, 0.8916720, 0.8916720,
        0.2555570, 0.2861130, 0.5500030, 0.5500030, 0.5500030, 0.5500030, 0.5500030, 0.8002530,
        0.4888920, 0.5652800, 0.7944490, 0.8555600, 0.5500030, 0.9472275, 1.0694500, 0.8555600,
        0.2555570, 0.3666690, 0.5583360, 0.9166720, 0.5500030, 1.0291190, 0.8305610, 0.3055570,
        0.4277800, 0.4277800, 0.5500030, 0.8555600, 0.3055570, 0.3666690, 0.3055570, 0.5500030,
        0.5500030, 0.5500030, 0.5500030, 0.5500030, 0.5500030, 0.5500030, 0.5500030, 0.5500030,
        0.5500030, 0.5500030, 0.3055570, 0.3055570, 0.3666690, 0.8555600, 0.5194470, 0.5194470,
        0.7333370, 0.7333370, 0.7333370, 0.7027820, 0.7944490, 0.6416700, 0.6111145, 0.7333370,
        0.7944490, 0.3305570, 0.5194470, 0.7638930, 0.5805590, 0.9777830, 0.7944490, 0.7944490,
        0.7027820, 0.7944490, 0.7027820, 0.6111145, 0.7333370, 0.7638930, 0.7333370, 1.0388950,
        0.7333370, 0.7333370, 0.6722260, 0.3430580, 0.5583360, 0.3430580, 0.5500030, 0.3055570,
        0.3055570, 0.5250030, 0.5611140, 0.4888920, 0.5611140, 0.5111140, 0.3361130, 0.5500030,
        0.5611140, 0.2555570, 0.2861130, 0.5305590, 0.2555570, 0.8666720, 0.5611140, 0.5500030,
        0.5611140, 0.5611140, 0.3722250, 0.4216690, 0.4041690, 0.5611140, 0.5000030, 0.7444490,
        0.5000030, 0.5000030, 0.4763920, 0.5500030, 1.1000060, 0.5500030, 0.5500030, 0.5500030,
    ],
];

/// Font names for PicTeX (Computer Modern Sans Serif family).
static FONTNAME: [&str; 4] = ["cmss10", "cmssbx10", "cmssi10", "cmssxi10"];

/* ==================== Internal functions ==================== */

/// SetLinetype - set the dash pattern for lines.
///
/// Writes `\setdashpattern <...>` or `\setsolid` commands to the TeX file.
unsafe fn SetLinetype(mut newlty: c_int, newlwd: c_double, dd: pDevDesc) {
    let ptd = (*dd).deviceSpecific as *mut picTeXDesc;

    (*ptd).lty = newlty;
    if (*ptd).lty != 0 {
        fprintf((*ptd).texfp, format_args!("\\setdashpattern <"));
        let mut i = 0;
        while i < 8 && (newlty & 15) != 0 {
            let lwd = (newlwd as c_int) * (newlty & 15);
            fprintf((*ptd).texfp, format_args!("{}pt", lwd));
            let templty = newlty >> 4;
            if (i + 1) < 8 && (templty & 15) != 0 {
                fprintf((*ptd).texfp, format_args!(", "));
            }
            newlty = newlty >> 4;
            i += 1;
        }
        fprintf((*ptd).texfp, format_args!(">\n"));
    } else {
        fprintf((*ptd).texfp, format_args!("\\setsolid\n"));
    }
}

/// SetFont - select a font face and size.
///
/// Writes `\font\picfont <name> at <size>pt\picfont` to the TeX file
/// if the font or size has changed.
unsafe fn SetFont(face: c_int, size: c_int, ptd: *mut picTeXDesc) {
    let mut lface = face;
    let mut lsize = size;
    if lface < 1 || lface > 4 {
        lface = 1;
    }
    if lsize < 1 || lsize > 24 {
        lsize = 10;
    }
    if lsize != (*ptd).fontsize || lface != (*ptd).fontface {
        fprintf(
            (*ptd).texfp,
            format_args!(
                "\\font\\picfont {} at {}pt\\picfont\n",
                FONTNAME[(lface - 1) as usize],
                lsize,
            ),
        );
        (*ptd).fontsize = lsize;
        (*ptd).fontface = lface;
    }
}

/// PicTeX_ClipLine - clip a line segment to the current clip region.
///
/// Full Cohen-Sutherland-style clipping implementation, faithful to C source.
unsafe fn PicTeX_ClipLine(
    x0: c_double,
    y0: c_double,
    x1: c_double,
    y1: c_double,
    ptd: *mut picTeXDesc,
) {
    (*ptd).clippedx0 = x0;
    (*ptd).clippedx1 = x1;
    (*ptd).clippedy0 = y0;
    (*ptd).clippedy1 = y1;

    // Trivial reject: entirely outside on any side
    if ((*ptd).clippedx0 < (*ptd).clipleft && (*ptd).clippedx1 < (*ptd).clipleft)
        || ((*ptd).clippedx0 > (*ptd).clipright && (*ptd).clippedx1 > (*ptd).clipright)
        || ((*ptd).clippedy0 < (*ptd).clipbottom && (*ptd).clippedy1 < (*ptd).clipbottom)
        || ((*ptd).clippedy0 > (*ptd).cliptop && (*ptd).clippedy1 > (*ptd).cliptop)
    {
        // Collapse to a zero-length segment
        (*ptd).clippedx0 = (*ptd).clippedx1;
        (*ptd).clippedy0 = (*ptd).clippedy1;
        return;
    }

    /* Clipping Left */
    if (*ptd).clippedx1 >= (*ptd).clipleft && (*ptd).clippedx0 < (*ptd).clipleft {
        (*ptd).clippedy0 = ((*ptd).clippedy1 - (*ptd).clippedy0)
            / ((*ptd).clippedx1 - (*ptd).clippedx0)
            * ((*ptd).clipleft - (*ptd).clippedx0)
            + (*ptd).clippedy0;
        (*ptd).clippedx0 = (*ptd).clipleft;
    }
    if (*ptd).clippedx1 <= (*ptd).clipleft && (*ptd).clippedx0 > (*ptd).clipleft {
        (*ptd).clippedy1 = ((*ptd).clippedy1 - (*ptd).clippedy0)
            / ((*ptd).clippedx1 - (*ptd).clippedx0)
            * ((*ptd).clipleft - (*ptd).clippedx0)
            + (*ptd).clippedy0;
        (*ptd).clippedx1 = (*ptd).clipleft;
    }

    /* Clipping Right */
    if (*ptd).clippedx1 >= (*ptd).clipright && (*ptd).clippedx0 < (*ptd).clipright {
        (*ptd).clippedy1 = ((*ptd).clippedy1 - (*ptd).clippedy0)
            / ((*ptd).clippedx1 - (*ptd).clippedx0)
            * ((*ptd).clipright - (*ptd).clippedx0)
            + (*ptd).clippedy0;
        (*ptd).clippedx1 = (*ptd).clipright;
    }
    if (*ptd).clippedx1 <= (*ptd).clipright && (*ptd).clippedx0 > (*ptd).clipright {
        (*ptd).clippedy0 = ((*ptd).clippedy1 - (*ptd).clippedy0)
            / ((*ptd).clippedx1 - (*ptd).clippedx0)
            * ((*ptd).clipright - (*ptd).clippedx0)
            + (*ptd).clippedy0;
        (*ptd).clippedx0 = (*ptd).clipright;
    }

    /* Clipping Bottom */
    if (*ptd).clippedy1 >= (*ptd).clipbottom && (*ptd).clippedy0 < (*ptd).clipbottom {
        (*ptd).clippedx0 = ((*ptd).clippedx1 - (*ptd).clippedx0)
            / ((*ptd).clippedy1 - (*ptd).clippedy0)
            * ((*ptd).clipbottom - (*ptd).clippedy0)
            + (*ptd).clippedx0;
        (*ptd).clippedy0 = (*ptd).clipbottom;
    }
    if (*ptd).clippedy1 <= (*ptd).clipbottom && (*ptd).clippedy0 > (*ptd).clipbottom {
        (*ptd).clippedx1 = ((*ptd).clippedx1 - (*ptd).clippedx0)
            / ((*ptd).clippedy1 - (*ptd).clippedy0)
            * ((*ptd).clipbottom - (*ptd).clippedy0)
            + (*ptd).clippedx0;
        (*ptd).clippedy1 = (*ptd).clipbottom;
    }

    /* Clipping Top */
    if (*ptd).clippedy1 >= (*ptd).cliptop && (*ptd).clippedy0 < (*ptd).cliptop {
        (*ptd).clippedx1 = ((*ptd).clippedx1 - (*ptd).clippedx0)
            / ((*ptd).clippedy1 - (*ptd).clippedy0)
            * ((*ptd).cliptop - (*ptd).clippedy0)
            + (*ptd).clippedx0;
        (*ptd).clippedy1 = (*ptd).cliptop;
    }
    if (*ptd).clippedy1 <= (*ptd).cliptop && (*ptd).clippedy0 > (*ptd).cliptop {
        (*ptd).clippedx0 = ((*ptd).clippedx1 - (*ptd).clippedx0)
            / ((*ptd).clippedy1 - (*ptd).clippedy0)
            * ((*ptd).cliptop - (*ptd).clippedy0)
            + (*ptd).clippedx0;
        (*ptd).clippedy0 = (*ptd).cliptop;
    }
}

/// textext - escape special TeX characters in a string and write to file.
///
/// Wraps the string in braces and escapes: $ % { } ^
/// Faithful to the C source (only those 5 special chars are escaped).
unsafe fn textext(str: *const c_char, ptd: *mut picTeXDesc) {
    fputc_ch(b'{', (*ptd).texfp);
    if !str.is_null() {
        let mut p = str;
        while *p != 0 {
            match *p as u8 {
                b'$' => {
                    fprintf((*ptd).texfp, format_args!("\\$"));
                }
                b'%' => {
                    fprintf((*ptd).texfp, format_args!("\\%%"));
                }
                b'{' => {
                    fprintf((*ptd).texfp, format_args!("\\{{"));
                }
                b'}' => {
                    fprintf((*ptd).texfp, format_args!("\\}}"));
                }
                b'^' => {
                    fprintf((*ptd).texfp, format_args!("\\^{{}}"));
                }
                c => {
                    fputc_ch(c, (*ptd).texfp);
                }
            }
            p = p.add(1);
        }
    }
    fprintf((*ptd).texfp, format_args!("}} "));
}

/* ==================== Device driver actions ==================== */

/// PicTeX_Circle - draw a circle using `\circulararc` command.
unsafe extern "C" fn PicTeX_Circle(
    x: c_double,
    y: c_double,
    r: c_double,
    _gc: pGEcontext,
    dd: pDevDesc,
) {
    let ptd = (*dd).deviceSpecific as *mut picTeXDesc;
    fprintf(
        (*ptd).texfp,
        format_args!(
            "\\circulararc 360 degrees from {:.2} {:.2} center at {:.2} {:.2}\n",
            x,
            (y + r),
            x,
            y,
        ),
    );
}

/// PicTeX_Clip - set the clip region.
unsafe extern "C" fn PicTeX_Clip(
    x0: c_double,
    x1: c_double,
    y0: c_double,
    y1: c_double,
    dd: pDevDesc,
) {
    let ptd = (*dd).deviceSpecific as *mut picTeXDesc;
    if (*ptd).debug {
        fprintf(
            (*ptd).texfp,
            format_args!(
                "% Setting Clip Region to {:.2} {:.2} {:.2} {:.2}\n",
                x0, y0, x1, y1,
            ),
        );
    }
    (*ptd).clipleft = x0;
    (*ptd).clipright = x1;
    (*ptd).clipbottom = y0;
    (*ptd).cliptop = y1;
}

/// PicTeX_Close - close the device: write closing LaTeX and free resources.
unsafe extern "C" fn PicTeX_Close(dd: pDevDesc) {
    let ptd = (*dd).deviceSpecific as *mut picTeXDesc;
    fprintf((*ptd).texfp, format_args!("\\endpicture\n}}\n"));
    libc::fclose((*ptd).texfp);
    libc::free(ptd as *mut c_void);
}

/// PicTeX_Line - draw a line segment with clipping.
unsafe extern "C" fn PicTeX_Line(
    x1: c_double,
    y1: c_double,
    x2: c_double,
    y2: c_double,
    gc: pGEcontext,
    dd: pDevDesc,
) {
    if x1 != x2 || y1 != y2 {
        let gc_ref = if gc.is_null() { None } else { Some(&*gc) };
        let lty = gc_ref.map(|g| g.lty).unwrap_or(0);
        let lwd = gc_ref.map(|g| g.lwd).unwrap_or(1.0);
        SetLinetype(lty, lwd, dd);
        let ptd = (*dd).deviceSpecific as *mut picTeXDesc;
        if (*ptd).debug {
            fprintf(
                (*ptd).texfp,
                format_args!(
                    "% Drawing line from {:.2}, {:.2} to {:.2}, {:.2}\n",
                    x1, y1, x2, y2,
                ),
            );
        }
        PicTeX_ClipLine(x1, y1, x2, y2, ptd);
        if (*ptd).debug {
            fprintf(
                (*ptd).texfp,
                format_args!(
                    "% Drawing clipped line from {:.2}, {:.2} to {:.2}, {:.2}\n",
                    (*ptd).clippedx0,
                    (*ptd).clippedy0,
                    (*ptd).clippedx1,
                    (*ptd).clippedy1,
                ),
            );
        }
        fprintf(
            (*ptd).texfp,
            format_args!(
                "\\plot {:.2} {:.2} {:.2} {:.2} /\n",
                (*ptd).clippedx0,
                (*ptd).clippedy0,
                (*ptd).clippedx1,
                (*ptd).clippedy1,
            ),
        );
    }
}

/// PicTeX_MetricInfo - get font metric information for a character.
///
/// Returns 0,0,0 as in the C source (metric info not available).
unsafe extern "C" fn PicTeX_MetricInfo(
    _c: c_int,
    _gc: pGEcontext,
    ascent: *mut c_double,
    descent: *mut c_double,
    width: *mut c_double,
    _dd: pDevDesc,
) {
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

/// PicTeX_NewPage - start a new page.
unsafe extern "C" fn PicTeX_NewPage(_gc: pGEcontext, dd: pDevDesc) {
    let ptd = (*dd).deviceSpecific as *mut picTeXDesc;

    if (*ptd).pageno != 0 {
        fprintf((*ptd).texfp, format_args!("\\endpicture\n}}\n\n\n"));
        fprintf((*ptd).texfp, format_args!("\\hbox{{\\beginpicture\n"));
        fprintf(
            (*ptd).texfp,
            format_args!("\\setcoordinatesystem units <1pt,1pt>\n"),
        );
        fprintf(
            (*ptd).texfp,
            format_args!(
                "\\setplotarea x from 0 to {:.2}, y from 0 to {:.2}\n",
                in2dots((*ptd).width),
                in2dots((*ptd).height),
            ),
        );
        fprintf((*ptd).texfp, format_args!("\\setlinear\n"));
        fprintf(
            (*ptd).texfp,
            format_args!("\\font\\picfont cmss10\\picfont\n"),
        );
    }
    (*ptd).pageno += 1;

    // Reset font to force SetFont to write the font command
    let face = (*ptd).fontface;
    let size = (*ptd).fontsize;
    (*ptd).fontface = 0;
    (*ptd).fontsize = 0;
    SetFont(face, size, ptd);
}

/// PicTeX_Polygon - draw a filled/stroked polygon.
unsafe extern "C" fn PicTeX_Polygon(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    gc: pGEcontext,
    dd: pDevDesc,
) {
    let ptd = (*dd).deviceSpecific as *mut picTeXDesc;
    {
        let gc_ref = if gc.is_null() { None } else { Some(&*gc) };
        let lty = gc_ref.map(|g| g.lty).unwrap_or(0);
        let lwd = gc_ref.map(|g| g.lwd).unwrap_or(1.0);
        SetLinetype(lty, lwd, dd);
    }

    if n < 2 || x.is_null() || y.is_null() {
        return;
    }

    let mut x1 = *x.add(0);
    let mut y1 = *y.add(0);

    for i in 1..n as usize {
        let x2 = *x.add(i);
        let y2 = *y.add(i);
        PicTeX_ClipLine(x1, y1, x2, y2, ptd);
        fprintf(
            (*ptd).texfp,
            format_args!(
                "\\plot {:.2} {:.2} {:.2} {:.2} /\n",
                (*ptd).clippedx0,
                (*ptd).clippedy0,
                (*ptd).clippedx1,
                (*ptd).clippedy1,
            ),
        );
        x1 = x2;
        y1 = y2;
    }

    // Close the polygon
    let x2 = *x.add(0);
    let y2 = *y.add(0);
    PicTeX_ClipLine(x1, y1, x2, y2, ptd);
    fprintf(
        (*ptd).texfp,
        format_args!(
            "\\plot {:.2} {:.2} {:.2} {:.2} /\n",
            (*ptd).clippedx0,
            (*ptd).clippedy0,
            (*ptd).clippedx1,
            (*ptd).clippedy1,
        ),
    );
}

/// PicTeX_Polyline - draw a polyline (series of connected line segments).
unsafe extern "C" fn PicTeX_Polyline(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    gc: pGEcontext,
    dd: pDevDesc,
) {
    let ptd = (*dd).deviceSpecific as *mut picTeXDesc;
    {
        let gc_ref = if gc.is_null() { None } else { Some(&*gc) };
        let lty = gc_ref.map(|g| g.lty).unwrap_or(0);
        let lwd = gc_ref.map(|g| g.lwd).unwrap_or(1.0);
        SetLinetype(lty, lwd, dd);
    }

    if n < 2 || x.is_null() || y.is_null() {
        return;
    }

    let mut x1 = *x.add(0);
    let mut y1 = *y.add(0);

    for i in 1..n as usize {
        let x2 = *x.add(i);
        let y2 = *y.add(i);
        PicTeX_ClipLine(x1, y1, x2, y2, ptd);
        fprintf(
            (*ptd).texfp,
            format_args!(
                "\\plot {:.2} {:.2} {:.2} {:.2} /\n",
                (*ptd).clippedx0,
                (*ptd).clippedy0,
                (*ptd).clippedx1,
                (*ptd).clippedy1,
            ),
        );
        x1 = x2;
        y1 = y2;
    }
}

/// PicTeX_Rect - draw a rectangle (delegates to PicTeX_Polygon).
unsafe extern "C" fn PicTeX_Rect(
    x0: c_double,
    y0: c_double,
    x1: c_double,
    y1: c_double,
    gc: pGEcontext,
    dd: pDevDesc,
) {
    let x = [x0, x0, x1, x1];
    let y = [y0, y1, y1, y0];
    PicTeX_Polygon(4, x.as_ptr(), y.as_ptr(), gc, dd);
}

/// PicTeX_Size - return the device size (left, right, bottom, top).
unsafe extern "C" fn PicTeX_Size(
    left: *mut c_double,
    right: *mut c_double,
    bottom: *mut c_double,
    top: *mut c_double,
    dd: pDevDesc,
) {
    if !left.is_null() {
        *left = (*dd).left;
    }
    if !right.is_null() {
        *right = (*dd).right;
    }
    if !bottom.is_null() {
        *bottom = (*dd).bottom;
    }
    if !top.is_null() {
        *top = (*dd).top;
    }
}

/// PicTeX_StrWidth - compute the string width in rasters.
///
/// Sums character widths from the CHARWIDTH table for each byte in the string,
/// scaled by the current font size.
unsafe extern "C" fn PicTeX_StrWidth(str: *const c_char, gc: pGEcontext, dd: pDevDesc) -> c_double {
    let ptd = (*dd).deviceSpecific as *mut picTeXDesc;

    // Compute font size from gc, matching C: size = (int)(gc->cex * gc->ps + 0.5)
    let size = if !gc.is_null() {
        ((*gc).cex * (*gc).ps + 0.5) as c_int
    } else {
        10
    };
    let face = if !gc.is_null() { (*gc).fontface } else { 1 };
    SetFont(face, size, ptd);

    let mut sum: c_double = 0.0;

    if !str.is_null() {
        let mut p = str;
        while *p != 0 {
            let ch = *p as u8;
            if ch < 128 {
                sum += CHARWIDTH[((*ptd).fontface - 1).max(0).min(3) as usize][ch as usize];
            } else {
                // For non-ASCII chars, use a rough width estimate
                sum += 0.5;
            }
            p = p.add(1);
        }
    }

    sum * (*ptd).fontsize as c_double
}

/// PicTeX_Text - draw text at a position.
///
/// Writes `\put` commands with optional `\rotatebox` for rotated text.
/// Escapes special TeX characters via textext().
unsafe extern "C" fn PicTeX_Text(
    x: c_double,
    y: c_double,
    str: *const c_char,
    rot: c_double,
    hadj: c_double,
    gc: pGEcontext,
    dd: pDevDesc,
) {
    let ptd = (*dd).deviceSpecific as *mut picTeXDesc;
    let xoff: c_double = 0.0;
    let yoff: c_double = 0.0;

    // Compute font size from gc, matching C: size = (int)(gc->cex * gc->ps + 0.5)
    let size = if !gc.is_null() {
        ((*gc).cex * (*gc).ps + 0.5) as c_int
    } else {
        10
    };
    let face = if !gc.is_null() { (*gc).fontface } else { 1 };
    SetFont(face, size, ptd);

    if (*ptd).debug {
        let sw = PicTeX_StrWidth(str, gc, dd);
        fprintf(
            (*ptd).texfp,
            format_args!(
                "% Writing string of length {:.2}, at {:.2} {:.2}, xc = {:.2} yc = {:.2}\n",
                sw, x, y, 0.0, 0.0,
            ),
        );
    }

    if rot == 90.0 {
        fprintf(
            (*ptd).texfp,
            format_args!("\\put {{\\rotatebox{{{}}}", rot as c_int),
        );
        textext(str, ptd);
        fprintf(
            (*ptd).texfp,
            format_args!("}} [rB] <{:.2}pt,{:.2}pt>", xoff, yoff),
        );
    } else {
        fprintf((*ptd).texfp, format_args!("\\put "));
        textext(str, ptd);
        fprintf(
            (*ptd).texfp,
            format_args!("[lB] <{:.2}pt,{:.2}pt>", xoff, yoff),
        );
    }
    fprintf((*ptd).texfp, format_args!(" at {:.2} {:.2}\n", x, y));
    let _ = hadj;
}

/// PicTeX_setPattern - set a fill pattern.
/// Returns R_NilValue (patterns not supported).
unsafe extern "C" fn PicTeX_setPattern(_pattern: SEXP, _dd: pDevDesc) -> SEXP {
    R_NilValue()
}

/// PicTeX_releasePattern - release a fill pattern reference.
/// No-op.
unsafe extern "C" fn PicTeX_releasePattern(_ref: SEXP, _dd: pDevDesc) {}

/// PicTeX_setClipPath - set a clipping path.
/// Returns R_NilValue (clip paths not supported).
unsafe extern "C" fn PicTeX_setClipPath(_path: SEXP, _ref: SEXP, _dd: pDevDesc) -> SEXP {
    R_NilValue()
}

/// PicTeX_releaseClipPath - release a clipping path reference.
/// No-op.
unsafe extern "C" fn PicTeX_releaseClipPath(_ref: SEXP, _dd: pDevDesc) {}

/// PicTeX_setMask - set a mask.
/// Returns R_NilValue (masks not supported).
unsafe extern "C" fn PicTeX_setMask(_path: SEXP, _ref: SEXP, _dd: pDevDesc) -> SEXP {
    R_NilValue()
}

/// PicTeX_releaseMask - release a mask reference.
/// No-op.
unsafe extern "C" fn PicTeX_releaseMask(_ref: SEXP, _dd: pDevDesc) {}

/* ==================== Device driver initialization ==================== */

/// PicTeXDeviceDriver - initialize the PicTeX device driver.
///
/// Allocates a `picTeXDesc`, opens the output file, writes the PicTeX
/// header commands (beginpicture, coordinate system, plot area, etc.),
/// and fills in all the DevDesc callback fields.
///
/// Returns true on success, false on failure.
unsafe fn PicTeXDeviceDriver(
    dd: pDevDesc,
    filename: *const c_char,
    bg: *const c_char,
    fg: *const c_char,
    width: c_double,
    height: c_double,
    debug: bool,
) -> bool {
    // Allocate device-specific structure
    let ptd: *mut picTeXDesc = libc::malloc(std::mem::size_of::<picTeXDesc>()) as *mut picTeXDesc;
    if ptd.is_null() {
        return false;
    }

    // Initialize to zero
    ptr::write_bytes(ptd, 0, 1);

    // Open the output file
    let expanded = R_ExpandFileName(filename);
    let fp = R_fopen(expanded, b"w\0".as_ptr() as *const c_char);
    if fp.is_null() {
        libc::free(ptd as *mut c_void);
        return false;
    }

    (*ptd).texfp = fp;

    // Copy filename (safely truncate to 127 chars + null)
    if !filename.is_null() {
        let mut i = 0usize;
        while i < 127 && *filename.add(i) != 0 {
            (*ptd).filename[i] = *filename.add(i);
            i += 1;
        }
        (*ptd).filename[i] = 0;
    }

    // Set initial device colors
    (*dd).startfill = R_GE_str2col(bg) as c_int;
    (*dd).startcol = R_GE_str2col(fg) as c_int;
    (*dd).startps = 10.0;
    (*dd).startlty = 0;
    (*dd).startfont = 1;
    (*dd).startgamma = 1.0;

    // Set device callbacks
    (*dd).close = Some(PicTeX_Close);
    (*dd).clip = Some(PicTeX_Clip);
    (*dd).size = Some(PicTeX_Size);
    (*dd).newPage = Some(PicTeX_NewPage);
    (*dd).line = Some(PicTeX_Line);
    (*dd).text = Some(PicTeX_Text);
    (*dd).strWidth = Some(PicTeX_StrWidth);
    (*dd).rect = Some(PicTeX_Rect);
    (*dd).circle = Some(PicTeX_Circle);
    (*dd).polygon = Some(PicTeX_Polygon);
    (*dd).polyline = Some(PicTeX_Polyline);
    (*dd).metricInfo = Some(PicTeX_MetricInfo);
    (*dd).hasTextUTF8 = 0;
    (*dd).useRotatedTextInContour = 0;
    (*dd).setPattern = Some(PicTeX_setPattern);
    (*dd).releasePattern = Some(PicTeX_releasePattern);
    (*dd).setClipPath = Some(PicTeX_setClipPath);
    (*dd).releaseClipPath = Some(PicTeX_releaseClipPath);
    (*dd).setMask = Some(PicTeX_setMask);
    (*dd).releaseMask = Some(PicTeX_releaseMask);

    // Screen Dimensions in Pixels (dots)
    (*dd).left = 0.0;
    (*dd).right = in2dots(width);
    (*dd).bottom = 0.0;
    (*dd).top = in2dots(height);
    (*dd).clipLeft = (*dd).left;
    (*dd).clipRight = (*dd).right;
    (*dd).clipBottom = (*dd).bottom;
    (*dd).clipTop = (*dd).top;

    // Store dimensions
    (*ptd).width = width;
    (*ptd).height = height;

    // PicTeX_Open: write the LaTeX picture environment header
    (*ptd).fontsize = 0;
    (*ptd).fontface = 0;
    (*ptd).debug = false;
    fprintf((*ptd).texfp, format_args!("\\hbox{{\\beginpicture\n"));
    fprintf(
        (*ptd).texfp,
        format_args!("\\setcoordinatesystem units <1pt,1pt>\n"),
    );
    fprintf(
        (*ptd).texfp,
        format_args!(
            "\\setplotarea x from 0 to {:.2}, y from 0 to {:.2}\n",
            in2dots((*ptd).width),
            in2dots((*ptd).height),
        ),
    );
    fprintf((*ptd).texfp, format_args!("\\setlinear\n"));
    fprintf(
        (*ptd).texfp,
        format_args!("\\font\\picfont cmss10\\picfont\n"),
    );
    SetFont(1, 10, ptd);
    (*ptd).pageno += 1;

    // Base Pointsize / Nominal Character Sizes in Pixels
    (*dd).cra[0] = 9.0;
    (*dd).cra[1] = 12.0;

    // Character Addressing Offsets
    (*dd).xCharOffset = 0.0;
    (*dd).yCharOffset = 0.0;
    (*dd).yLineBias = 0.0;

    // Inches per Raster Unit (printer points: 72.27 dots per inch)
    (*dd).ipr[0] = 1.0 / DOTSperIN;
    (*dd).ipr[1] = 1.0 / DOTSperIN;

    // Device capabilities
    (*dd).canClip = 1; // TRUE
    (*dd).canHAdj = 0;
    (*dd).canChangeGamma = 0; // FALSE

    (*ptd).lty = 1;
    (*ptd).pageno = 0;
    (*ptd).debug = debug;

    (*dd).haveTransparency = 1;
    (*dd).haveTransparentBg = 2;

    (*dd).deviceSpecific = ptd as *mut c_void;
    (*dd).displayListOn = 0; // FALSE
    (*dd).deviceVersion = R_GE_definitions;

    true
}

/* ==================== R entry point ==================== */

/// PicTeX() - R entry point for creating a PicTeX graphics device.
///
/// Parameters (from the R call):
///   file    - output filename
///   bg      - background color string
///   fg      - foreground color string
///   width   - width in inches
///   height  - height in inches
///   debug   - logical: if TRUE, write TeX comments into output
#[unsafe(no_mangle)]
pub unsafe extern "C" fn PicTeX(args: SEXP) -> SEXP {
    let mut args = args;

    let vmax = vmaxget();

    // Skip entry point name
    args = CDR(args);

    // Get filename
    let tmp = asChar(CAR(args) as *const c_void);
    if tmp as SEXP == NA_STRING() {
        Rf_error(b"invalid 'file' parameter in pictex\0".as_ptr() as *const c_char);
        unreachable!();
    }
    let file = translateCharFP(tmp as SEXP);
    args = CDR(args);

    let bg = CHAR(asChar(CAR(args) as *const c_void) as SEXP);
    args = CDR(args);

    let fg = CHAR(asChar(CAR(args) as *const c_void) as SEXP);
    args = CDR(args);

    let width = asReal(CAR(args));
    args = CDR(args);

    let height = asReal(CAR(args));
    args = CDR(args);

    let mut debug = asLogical(CAR(args));
    if debug == NA_LOGICAL {
        debug = 0;
    }

    R_CheckDeviceAvailable();

    let dev = GEcreateDD();
    if dev.is_null() {
        Rf_error(b"unable to start pictex() device\0".as_ptr() as *const c_char);
        unreachable!();
    }

    if !PicTeXDeviceDriver(dev, file, bg, fg, width, height, debug != 0) {
        GEfreeDD(dev);
        Rf_error(b"unable to start pictex() device\0".as_ptr() as *const c_char);
        unreachable!();
    }

    let dd = GEcreateDevDesc(dev);
    GEaddDevice2f(dd, b"pictex\0".as_ptr() as *const c_char, file);

    vmaxset(vmax);
    R_NilValue()
}
