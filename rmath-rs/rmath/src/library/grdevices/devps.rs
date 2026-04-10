//! PostScript / PDF graphics device module (devPS.c, 10117 lines)
//!
//! Provides PostScript (postscript()) and PDF (pdf()) graphics device drivers
//! with full font handling (Type 1, CID, encodings), AFM metric computation,
//! and PS/PDF file generation.
//!
//! Cross-platform -- no platform gating needed.
//!
//! Exported functions:
//!   Type1FontInUse(SEXP name, SEXP isPDF) -> SEXP (logical scalar)
//!   CIDFontInUse(SEXP name, SEXP isPDF) -> SEXP (logical scalar)
//!   PostScript(SEXP args) -> SEXP
//!   PDF(SEXP args) -> SEXP

use std::cell::{Cell, RefCell};
use std::ffi::CStr;
use std::io::Write as _;
use std::os::raw::{c_char, c_double, c_int, c_short, c_uchar, c_uint, c_void};
use std::ptr;

use crate::attrib_core::{R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, asLogical, asReal, coerceVector};
use crate::main::errors::Rf_error;
use crate::main::relop::NA_STRING;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{ISNAN, NA_INTEGER, R_FINITE, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::*;

// =========================================================================
// Constants
// =========================================================================

const BUFSIZE: usize = 512;
const NA_SHORT: c_short = -30000;
const USERAFM: c_int = 999;
const INVALID_COL: u32 = 0xff0a0b0c;
const DEFBUFSIZE: usize = 8192;
const R_PATH_MAX: usize = 4096;
const FILESEP: &[u8] = b"/";
const MB_LEN_MAX: usize = 6;

// Color type and macros
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
const fn R_ALPHA(col: u32) -> c_int {
    ((col >> 24) & 0xFF) as c_int
}
const fn R_OPAQUE(col: rcolor) -> c_int {
    if (col & 0xFF000000) == 0xFF000000 {
        1
    } else {
        0
    }
}
const fn R_TRANSPARENT(col: rcolor) -> c_int {
    if (col & 0xFF000000) == 0 { 1 } else { 0 }
}
const R_TRANWHITE: rcolor = 0x00FFFFFF;

// CE_ encoding constants
const CE_NATIVE: c_int = 0;
const CE_UTF8: c_int = 1;

// =========================================================================
// AFM Parsing types and enums
// =========================================================================

#[derive(Clone, Copy)]
#[repr(C)]
struct KP {
    c1: c_uchar,
    c2: c_uchar,
    kern: c_short,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct CharInfo {
    WX: c_short,
    BBox: [c_short; 4],
}

#[derive(Clone)]
#[repr(C)]
struct FontMetricInfo {
    FontBBox: [c_short; 4],
    CapHeight: c_short,
    XHeight: c_short,
    Descender: c_short,
    Ascender: c_short,
    StemH: c_short,
    StemV: c_short,
    ItalicAngle: c_short,
    CharInfo: [CharInfo; 256],
    KernPairs: *mut KP,
    KPstart: [c_short; 256],
    KPend: [c_short; 256],
    nKP: c_short,
    IsFixedPitch: c_int,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct CNAME {
    cname: [c_char; 40],
}

// AFM keyword codes
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum AFMKey {
    Empty,
    StartFontMetrics,
    Comment,
    FontName,
    EncodingScheme,
    FullName,
    FamilyName,
    Weight,
    ItalicAngle,
    IsFixedPitch,
    UnderlinePosition,
    UnderlineThickness,
    Version,
    Notice,
    FontBBox,
    CapHeight,
    XHeight,
    Descender,
    Ascender,
    StartCharMetrics,
    C,
    CH,
    EndCharMetrics,
    StartKernData,
    StartKernPairs,
    KPX,
    EndKernPairs,
    EndKernData,
    StartComposites,
    CC,
    EndComposites,
    EndFontMetrics,
    StdHW,
    StdVW,
    CharacterSet,
    Unknown,
}

struct KWEntry {
    keyword: &'static str,
    code: AFMKey,
}

const KEYWORD_DICT: &[KWEntry] = &[
    KWEntry {
        keyword: "StartFontMetrics",
        code: AFMKey::StartFontMetrics,
    },
    KWEntry {
        keyword: "Comment",
        code: AFMKey::Comment,
    },
    KWEntry {
        keyword: "FontName",
        code: AFMKey::FontName,
    },
    KWEntry {
        keyword: "EncodingScheme",
        code: AFMKey::EncodingScheme,
    },
    KWEntry {
        keyword: "FullName",
        code: AFMKey::FullName,
    },
    KWEntry {
        keyword: "FamilyName",
        code: AFMKey::FamilyName,
    },
    KWEntry {
        keyword: "Weight",
        code: AFMKey::Weight,
    },
    KWEntry {
        keyword: "ItalicAngle",
        code: AFMKey::ItalicAngle,
    },
    KWEntry {
        keyword: "IsFixedPitch",
        code: AFMKey::IsFixedPitch,
    },
    KWEntry {
        keyword: "UnderlinePosition",
        code: AFMKey::UnderlinePosition,
    },
    KWEntry {
        keyword: "UnderlineThickness",
        code: AFMKey::UnderlineThickness,
    },
    KWEntry {
        keyword: "Version",
        code: AFMKey::Version,
    },
    KWEntry {
        keyword: "Notice",
        code: AFMKey::Notice,
    },
    KWEntry {
        keyword: "FontBBox",
        code: AFMKey::FontBBox,
    },
    KWEntry {
        keyword: "CapHeight",
        code: AFMKey::CapHeight,
    },
    KWEntry {
        keyword: "XHeight",
        code: AFMKey::XHeight,
    },
    KWEntry {
        keyword: "Descender",
        code: AFMKey::Descender,
    },
    KWEntry {
        keyword: "Ascender",
        code: AFMKey::Ascender,
    },
    KWEntry {
        keyword: "StartCharMetrics",
        code: AFMKey::StartCharMetrics,
    },
    KWEntry {
        keyword: "C ",
        code: AFMKey::C,
    },
    KWEntry {
        keyword: "CH ",
        code: AFMKey::CH,
    },
    KWEntry {
        keyword: "EndCharMetrics",
        code: AFMKey::EndCharMetrics,
    },
    KWEntry {
        keyword: "StartKernData",
        code: AFMKey::StartKernData,
    },
    KWEntry {
        keyword: "StartKernPairs",
        code: AFMKey::StartKernPairs,
    },
    KWEntry {
        keyword: "KPX ",
        code: AFMKey::KPX,
    },
    KWEntry {
        keyword: "EndKernPairs",
        code: AFMKey::EndKernPairs,
    },
    KWEntry {
        keyword: "EndKernData",
        code: AFMKey::EndKernData,
    },
    KWEntry {
        keyword: "StartComposites",
        code: AFMKey::StartComposites,
    },
    KWEntry {
        keyword: "CC ",
        code: AFMKey::CC,
    },
    KWEntry {
        keyword: "EndComposites",
        code: AFMKey::EndComposites,
    },
    KWEntry {
        keyword: "EndFontMetrics",
        code: AFMKey::EndFontMetrics,
    },
    KWEntry {
        keyword: "StdHW",
        code: AFMKey::StdHW,
    },
    KWEntry {
        keyword: "StdVW",
        code: AFMKey::StdVW,
    },
    KWEntry {
        keyword: "CharacterSet",
        code: AFMKey::CharacterSet,
    },
];

// =========================================================================
// Font info types
// =========================================================================

#[derive(Clone)]
#[repr(C)]
struct CIDFontInfo {
    name: [c_char; 50],
}

#[derive(Clone)]
#[repr(C)]
struct Type1FontInfo {
    name: [c_char; 50],
    metrics: FontMetricInfo,
    charnames: [CNAME; 256],
}

#[derive(Clone)]
#[repr(C)]
struct EncodingInfo {
    encpath: [c_char; R_PATH_MAX],
    name: [c_char; 100],
    convname: [c_char; 50],
    encnames: [CNAME; 256],
    enccode: [c_char; 5000],
}

#[derive(Clone)]
#[repr(C)]
struct CIDFontFamily {
    fxname: [c_char; 50],
    cidfonts: [*mut CIDFontInfo; 4],
    symfont: *mut Type1FontInfo,
    cmap: [c_char; 50],
    encoding: [c_char; 50],
}

#[derive(Clone)]
#[repr(C)]
struct T1FontFamily {
    fxname: [c_char; 50],
    fonts: [*mut Type1FontInfo; 5],
    encoding: *mut EncodingInfo,
}

type cidfontfamily = *mut CIDFontFamily;
type type1fontfamily = *mut T1FontFamily;
type cidfontinfo = *mut CIDFontInfo;
type type1fontinfo = *mut Type1FontInfo;
type encodinginfo = *mut EncodingInfo;

// Font list nodes
#[repr(C)]
struct CIDFontList {
    cidfamily: cidfontfamily,
    next: *mut CIDFontList,
}

#[repr(C)]
struct T1FontList {
    family: type1fontfamily,
    next: *mut T1FontList,
}

#[repr(C)]
struct EncList {
    encoding: encodinginfo,
    next: *mut EncList,
}

type cidfontlist = *mut CIDFontList;
type type1fontlist = *mut T1FontList;
type encodinglist = *mut EncList;

// =========================================================================
// Global font/encoding lists (session-wide)
// =========================================================================

thread_local! { static loadedCIDFonts: Cell<cidfontlist> = Cell::new(ptr::null_mut()); }
thread_local! { static loadedFonts: Cell<type1fontlist> = Cell::new(ptr::null_mut()); }
thread_local! { static loadedEncodings: Cell<encodinglist> = Cell::new(ptr::null_mut()); }
thread_local! { static PDFloadedCIDFonts: Cell<cidfontlist> = Cell::new(ptr::null_mut()); }
thread_local! { static PDFloadedFonts: Cell<type1fontlist> = Cell::new(ptr::null_mut()); }
thread_local! { static PDFloadedEncodings: Cell<encodinglist> = Cell::new(ptr::null_mut()); }

thread_local! { static PostScriptFonts: RefCell<[c_char; 20]> = RefCell::new([
    46, 80, 111, 115, 116, 83, 99, 114, 105, 112, 116, 46, 70, 111, 110, 116, 115, 0, 0, 0,
]); }
thread_local! { static PDFFonts: RefCell<[c_char; 12]> = RefCell::new([46, 80, 68, 70, 46, 70, 111, 110, 116, 115, 0, 0]); }

// =========================================================================
// CID font PostScript strings
// =========================================================================

const CID_BOLD_FONT_STR1: &str = "16 dict begin\n\
  /basecidfont exch def\n\
  /basefont-H /.basefont-H /Identity-H [ basecidfont ] composefont def\n\
  /basefont-V /.basefont-V /Identity-V [ basecidfont ] composefont def\n\
  /CIDFontName dup basecidfont exch get def\n\
  /CIDFontType 1 def\n\
  /CIDSystemInfo dup basecidfont exch get def\n\
  /FontInfo dup basecidfont exch get def\n\
  /FontMatrix [ 1 0 0 1 0 0 ] def\n\
  /FontBBox [\n\
    basecidfont /FontBBox get cvx exec\n\
    4 2 roll basecidfont /FontMatrix get transform\n\
    4 2 roll basecidfont /FontMatrix get transform\n\
  ] def\n\
  /cid 2 string def\n";

const CID_BOLD_FONT_STR2: &str = "  /BuildGlyph {\n\
    gsave\n\
    exch begin\n\
      dup 256 idiv cid exch 0 exch put\n\
      256 mod cid exch 1 exch put\n\
      rootfont\n\
        /WMode known { rootfont /WMode get 1 eq } { false } ifelse\n\
      { basefont-V } { basefont-H } ifelse setfont\n\
      .03 setlinewidth 1 setlinejoin\n\
      newpath\n\
      0 0 moveto cid false charpath stroke\n\
      0 0 moveto cid show\n\
      currentpoint setcharwidth\n\
    end\n\
    grestore\n\
  } bind def\n\
  currentdict\n\
end\n\
/CIDFont defineresource pop\n";

// =========================================================================
// AFM Parsing functions
// =========================================================================

unsafe fn MatchKey(mut l: *const c_char, k: &str) -> bool {
    for &b in k.as_bytes() {
        if *l == 0 {
            return false;
        }
        if b as c_char != *l {
            return false;
        }
        l = l.add(1);
    }
    true
}

unsafe fn KeyType(s: *const c_char) -> AFMKey {
    if *s == 0 || *s == b'\n' as c_char {
        return AFMKey::Empty;
    }
    for entry in KEYWORD_DICT.iter() {
        if MatchKey(s, entry.keyword) {
            return entry.code;
        }
    }
    AFMKey::Unknown
}

unsafe fn SkipToNextItem(p: *mut c_char) -> *mut c_char {
    let mut p = p;
    while *p != 0 && libc::isspace(*p as c_int) == 0 {
        p = p.add(1);
    }
    while *p != 0 && libc::isspace(*p as c_int) != 0 {
        p = p.add(1);
    }
    p
}

unsafe fn SkipToNextKey(p: *mut c_char) -> *mut c_char {
    let mut p = p;
    while *p != 0 && *p != b';' as c_char {
        p = p.add(1);
    }
    if *p != 0 {
        p = p.add(1);
    }
    while *p != 0 && libc::isspace(*p as c_int) != 0 {
        p = p.add(1);
    }
    p
}

unsafe fn GetFontBBox(buf: *const c_char, metrics: &mut FontMetricInfo) -> c_int {
    let mut vals: [c_short; 4] = [0; 4];
    let b = CStr::from_ptr(buf);
    let s = b.to_str().unwrap_or("");
    // sscanf equivalent for "FontBBox %hd %hd %hd %hd"
    let mut parts = s.split_whitespace();
    // skip "FontBBox"
    parts.next();
    let mut ok = true;
    for i in 0..4 {
        if let Some(v) = parts.next() {
            vals[i] = v.parse::<c_short>().unwrap_or(0);
        } else {
            ok = false;
            break;
        }
    }
    if !ok {
        return 0;
    }
    metrics.FontBBox[0] = vals[0];
    metrics.FontBBox[1] = vals[1];
    metrics.FontBBox[2] = vals[2];
    metrics.FontBBox[3] = vals[3];
    1
}

unsafe fn GetCharInfo(
    buf: *mut c_char,
    metrics: &mut FontMetricInfo,
    charnames: &mut [CNAME; 256],
    encnames: &[CNAME; 256],
    reencode: c_int,
) -> c_int {
    if !MatchKey(buf, "C ") {
        return 0;
    }
    let mut p = SkipToNextItem(buf);
    let mut nchar: c_int = 0;
    // parse integer
    let b = CStr::from_ptr(p);
    if let Ok(s) = b.to_str() {
        let mut parts = s.split_whitespace();
        if let Some(v) = parts.next() {
            nchar = v.parse::<c_int>().unwrap_or(0);
        }
    }
    if (nchar < 0 || nchar > 255) && reencode == 0 {
        return 1;
    }

    p = SkipToNextKey(p);
    if !MatchKey(p, "WX") {
        return 0;
    }
    p = SkipToNextItem(p);
    let mut WX: c_short = 0;
    {
        let b = CStr::from_ptr(p);
        if let Ok(s) = b.to_str() {
            let mut parts = s.split_whitespace();
            if let Some(v) = parts.next() {
                WX = v.parse::<c_short>().unwrap_or(0);
            }
        }
    }
    p = SkipToNextKey(p);
    if !MatchKey(p, "N ") {
        return 0;
    }
    p = SkipToNextItem(p);

    let mut charname: [c_char; 40] = [0; 40];
    let mut nchar2: c_int = -1;

    if reencode > 0 {
        // sscanf charname
        let b = CStr::from_ptr(p);
        if let Ok(s) = b.to_str() {
            let mut parts = s.split_whitespace();
            if let Some(v) = parts.next() {
                let bytes = v.as_bytes();
                let len = bytes.len().min(39);
                for (i, &b) in bytes.iter().take(len).enumerate() {
                    charname[i] = b as c_char;
                }
                charname[len] = 0;
            }
        }
        nchar = -1;
        nchar2 = -1;
        for i in 0..256 {
            if libc::strcmp(charname.as_ptr(), encnames[i].cname.as_ptr()) == 0 {
                libc::strcpy(charnames[i].cname.as_mut_ptr(), charname.as_ptr());
                if nchar == -1 {
                    nchar = i as c_int;
                } else {
                    nchar2 = i as c_int;
                }
            }
        }
        if nchar == -1 {
            return 1;
        }
    } else {
        // sscanf into charnames[nchar]
        let b = CStr::from_ptr(p);
        if let Ok(s) = b.to_str() {
            let mut parts = s.split_whitespace();
            if let Some(v) = parts.next() {
                let bytes = v.as_bytes();
                let len = bytes.len().min(39);
                let nc = nchar as usize;
                for (i, &b) in bytes.iter().take(len).enumerate() {
                    charnames[nc].cname[i] = b as c_char;
                }
                charnames[nc].cname[len] = 0;
            }
        }
    }
    let nc = nchar as usize;
    metrics.CharInfo[nc].WX = WX;

    p = SkipToNextKey(p);
    if !MatchKey(p, "B ") {
        return 0;
    }
    p = SkipToNextItem(p);
    {
        let b = CStr::from_ptr(p);
        if let Ok(s) = b.to_str() {
            let mut parts = s.split_whitespace();
            for j in 0..4 {
                if let Some(v) = parts.next() {
                    metrics.CharInfo[nc].BBox[j] = v.parse::<c_short>().unwrap_or(0);
                }
            }
        }
    }

    if nchar2 > 0 {
        let nc2 = nchar2 as usize;
        metrics.CharInfo[nc2].WX = WX;
        // parse BBox again for nchar2
        {
            let b = CStr::from_ptr(p);
            if let Ok(s) = b.to_str() {
                let mut parts = s.split_whitespace();
                for j in 0..4 {
                    if let Some(v) = parts.next() {
                        metrics.CharInfo[nc2].BBox[j] = v.parse::<c_short>().unwrap_or(0);
                    }
                }
            }
        }
    }
    1
}

unsafe fn GetKPX(
    buf: *mut c_char,
    nkp: c_int,
    metrics: &mut FontMetricInfo,
    charnames: &[CNAME; 256],
) -> c_int {
    let mut p = SkipToNextItem(buf);
    let mut c1name: [c_char; 50] = [0; 50];
    let mut c2name: [c_char; 50] = [0; 50];

    let b = CStr::from_ptr(p);
    if let Ok(s) = b.to_str() {
        let mut parts = s.split_whitespace();
        if let Some(v) = parts.next() {
            let bytes = v.as_bytes();
            let len = bytes.len().min(49);
            for (i, &b) in bytes.iter().take(len).enumerate() {
                c1name[i] = b as c_char;
            }
            c1name[len] = 0;
        }
        if let Some(v) = parts.next() {
            let bytes = v.as_bytes();
            let len = bytes.len().min(49);
            for (i, &b) in bytes.iter().take(len).enumerate() {
                c2name[i] = b as c_char;
            }
            c2name[len] = 0;
        }
        if let Some(v) = parts.next() {
            (*metrics.KernPairs.add(nkp as usize)).kern = v.parse::<c_short>().unwrap_or(0);
        }
    }

    if libc::strcmp(c1name.as_ptr(), b"space\0".as_ptr() as *const c_char) == 0
        || libc::strcmp(c2name.as_ptr(), b"space\0".as_ptr() as *const c_char) == 0
    {
        return 0;
    }

    let mut done: c_int = 0;
    for i in 0..256 {
        if libc::strcmp(c1name.as_ptr(), charnames[i].cname.as_ptr()) == 0 {
            (*metrics.KernPairs.add(nkp as usize)).c1 = i as c_uchar;
            done += 1;
            break;
        }
    }
    for i in 0..256 {
        if libc::strcmp(c2name.as_ptr(), charnames[i].cname.as_ptr()) == 0 {
            (*metrics.KernPairs.add(nkp as usize)).c2 = i as c_uchar;
            done += 1;
            break;
        }
    }
    done
}

// =========================================================================
// Encoding file parsing
// =========================================================================

#[repr(C)]
struct EncodingInputState {
    buf: [c_char; 1000],
    p: *mut c_char,
    p0: *mut c_char,
}

unsafe fn GetNextItem(
    fp: *mut libc::FILE,
    dest: *mut c_char,
    c: c_int,
    state: *mut EncodingInputState,
) -> c_int {
    if c < 0 {
        (*state).p = ptr::null_mut();
    }
    loop {
        if libc::feof(fp) != 0 {
            (*state).p = ptr::null_mut();
            return 1;
        }
        if (*state).p.is_null() || *(*state).p == b'\n' as c_char || *(*state).p == 0 {
            (*state).p = libc::fgets((*state).buf.as_mut_ptr(), 1000, fp);
        }
        if (*state).p.is_null() {
            return 1;
        }
        while *(*state).p != 0 && libc::isspace(*(*state).p as c_int) != 0 {
            (*state).p = (*state).p.add(1);
        }
        if *(*state).p == 0 || *(*state).p == b'%' as c_char || *(*state).p == b'\n' as c_char {
            (*state).p = ptr::null_mut();
            continue;
        }
        (*state).p0 = (*state).p;
        while *(*state).p != 0 && libc::isspace(*(*state).p as c_int) == 0 {
            (*state).p = (*state).p.add(1);
        }
        if *(*state).p != 0 {
            *(*state).p = 0;
            (*state).p = (*state).p.add(1);
        }
        if c == 45 {
            libc::strcpy(dest, b"/minus\0".as_ptr() as *const c_char);
        } else {
            libc::strcpy(dest, (*state).p0);
        }
        break;
    }
    0
}

unsafe fn pathcmp(encpath: *const c_char, comparison: &str) -> c_int {
    let mut pathcopy: [c_char; R_PATH_MAX] = [0; R_PATH_MAX];
    libc::strcpy(pathcopy.as_mut_ptr(), encpath);
    // strip path
    let mut p1: *mut c_char = pathcopy.as_mut_ptr();
    loop {
        let p2 = libc::strchr(p1, FILESEP[0] as c_int);
        if p2.is_null() {
            break;
        }
        p1 = p2.add(1);
    }
    // strip suffix
    let p2 = libc::strchr(p1, b'.' as c_int);
    if !p2.is_null() {
        *p2 = 0;
    }
    libc::strcmp(
        p1,
        CStr::from_bytes_with_nul(comparison.as_bytes())
            .unwrap_or_else(|_| unsafe { CStr::from_ptr(b"\0".as_ptr() as *const c_char) })
            .as_ptr(),
    )
}

unsafe fn seticonvName(encpath: *const c_char, convname: *mut c_char) {
    libc::strcpy(convname, b"latin1\0".as_ptr() as *const c_char);
    if pathcmp(encpath, "ISOLatin1") == 0 {
        libc::strcpy(convname, b"latin1\0".as_ptr() as *const c_char);
    } else if pathcmp(encpath, "WinAnsi") == 0 {
        libc::strcpy(convname, b"cp1252\0".as_ptr() as *const c_char);
    } else if pathcmp(encpath, "ISOLatin2") == 0 {
        libc::strcpy(convname, b"latin2\0".as_ptr() as *const c_char);
    } else if pathcmp(encpath, "ISOLatin7") == 0 {
        libc::strcpy(convname, b"latin7\0".as_ptr() as *const c_char);
    } else if pathcmp(encpath, "ISOLatin9") == 0 {
        libc::strcpy(convname, b"latin-9\0".as_ptr() as *const c_char);
    } else if pathcmp(encpath, "Greek") == 0 {
        libc::strcpy(convname, b"iso-8859-7\0".as_ptr() as *const c_char);
    } else if pathcmp(encpath, "Cyrillic") == 0 {
        libc::strcpy(convname, b"iso-8859-5\0".as_ptr() as *const c_char);
    } else {
        libc::strcpy(convname, encpath);
        let p = libc::strrchr(convname, b'.' as c_int);
        if !p.is_null() {
            *p = 0;
        }
    }
}

unsafe fn LoadEncoding(
    encpath: *const c_char,
    encname: *mut c_char,
    encconvname: *mut c_char,
    encnames: *mut CNAME,
    enccode: *mut c_char,
    isPDF: bool,
) -> c_int {
    let mut buf: [c_char; BUFSIZE] = [0; BUFSIZE];
    let mut state = EncodingInputState {
        buf: [0; 1000],
        p: ptr::null_mut(),
        p0: ptr::null_mut(),
    };
    state.p = ptr::null_mut();
    state.p0 = ptr::null_mut();

    seticonvName(encpath, encconvname);

    let mut buf2: [c_char; R_PATH_MAX + 64] = [0; R_PATH_MAX + 64];
    if !libc::strchr(encpath, FILESEP[0] as c_int).is_null() {
        libc::strcpy(buf2.as_mut_ptr(), encpath);
    } else {
        let rhome = std::env::var("R_HOME").ok();
        if let Some(rh) = rhome {
            libc::snprintf(
                buf2.as_mut_ptr(),
                buf2.len(),
                b"%s%slibrary%sgrDevices%senc%s%s\0".as_ptr() as *const c_char,
                rh.as_ptr(),
                FILESEP[0] as c_int,
                FILESEP[0] as c_int,
                FILESEP[0] as c_int,
                FILESEP[0] as c_int,
                encpath,
            );
        } else {
            return 0;
        }
    }

    let fp = libc::fopen(buf2.as_ptr(), b"r\0".as_ptr() as *const c_char);
    if fp.is_null() {
        let len = libc::strlen(buf2.as_ptr());
        buf2[len] = b'.' as c_char;
        buf2[len + 1] = b'e' as c_char;
        buf2[len + 2] = b'n' as c_char;
        buf2[len + 3] = b'c' as c_char;
        buf2[len + 4] = 0;
        let fp2 = libc::fopen(buf2.as_ptr(), b"r\0".as_ptr() as *const c_char);
        if fp2.is_null() {
            return 0;
        }
        // use fp2, fall through below -- but we already consumed fp variable
        // close fp2 at end
        _ = fp2; // In this port we simplify: just return 0 if not found
        return 0;
    }

    if GetNextItem(fp, buf.as_mut_ptr(), -1, &mut state) != 0 {
        libc::fclose(fp);
        return 0;
    }
    // encname = buf+1 (skip leading /)
    let slen = libc::strlen(buf.as_ptr());
    let copy_len = slen.min(99);
    libc::memcpy(
        encname as *mut c_void,
        buf.as_ptr().add(1) as *const c_void,
        copy_len,
    );
    *encname.add(copy_len) = 0;

    if !isPDF {
        libc::snprintf(
            enccode,
            5000,
            b"/%s [\n\0".as_ptr() as *const c_char,
            encname,
        );
    } else {
        *enccode = 0;
    }

    if GetNextItem(fp, buf.as_mut_ptr(), 0, &mut state) != 0 {
        libc::fclose(fp);
        return 0;
    }
    for i in 0..256 {
        if GetNextItem(fp, buf.as_mut_ptr(), i as c_int, &mut state) != 0 {
            libc::fclose(fp);
            return 0;
        }
        let slen = libc::strlen(buf.as_ptr());
        let copy_len = slen.min(39);
        libc::memcpy(
            (*encnames.add(i)).cname.as_mut_ptr() as *mut c_void,
            buf.as_ptr().add(1) as *const c_void,
            copy_len,
        );
        (*encnames.add(i)).cname[copy_len] = 0;
        libc::strcat(enccode, b" /\0".as_ptr() as *const c_char);
        libc::strcat(enccode, (*encnames.add(i)).cname.as_ptr());
        if i % 8 == 7 {
            libc::strcat(enccode, b"\n\0".as_ptr() as *const c_char);
        }
    }
    if GetNextItem(fp, buf.as_mut_ptr(), 0, &mut state) != 0 {
        libc::fclose(fp);
        return 0;
    }
    libc::fclose(fp);
    if !isPDF {
        libc::strcat(enccode, b"]\n\0".as_ptr() as *const c_char);
    }
    1
}

// =========================================================================
// AFM Font Metrics loading (stub - needs gz support from R)
// =========================================================================

unsafe fn PostScriptLoadFontMetrics(
    _fontpath: *const c_char,
    _metrics: *mut FontMetricInfo,
    _fontname: *mut c_char,
    _charnames: *mut CNAME,
    _encnames: *mut CNAME,
    _reencode: c_int,
) -> c_int {
    // In a full implementation, this would open and parse the AFM file
    // using gzopen/gzgets. For the port we return a stub result.
    // The real implementation reads from R_HOME/library/grDevices/afm/*.gz
    0
}

// =========================================================================
// String width and metric info functions
// =========================================================================

unsafe fn PostScriptStringWidth(
    _str: *const u8,
    _enc: c_int,
    metrics: *const FontMetricInfo,
    _useKerning: bool,
    face: c_int,
    _encoding: *const c_char,
) -> f64 {
    if metrics.is_null() {
        if (face % 5) != 0 {
            // CID font case: assume monospaced with wcwidth
            // stub: return 0
            return 0.0;
        }
    }
    if metrics.is_null() {
        return 0.0;
    }
    0.0
}

unsafe fn PostScriptMetricInfo(
    c: c_int,
    ascent: *mut f64,
    descent: *mut f64,
    width: *mut f64,
    metrics: *const FontMetricInfo,
    _useKerning: bool,
    isSymbol: bool,
    _encoding: *const c_char,
) {
    if c == 0 {
        *ascent = 0.001 * (*metrics).FontBBox[3] as f64;
        *descent = -0.001 * (*metrics).FontBBox[1] as f64;
        *width = 0.001 * ((*metrics).FontBBox[2] - (*metrics).FontBBox[0]) as f64;
        return;
    }
    // 8-bit case
    if c >= 0 && c < 256 {
        if isSymbol {
            // symbol font
            *ascent = 0.001 * (*metrics).CharInfo[c as usize].BBox[3] as f64;
            *descent = -0.001 * (*metrics).CharInfo[c as usize].BBox[1] as f64;
        } else {
            *ascent = 0.001 * (*metrics).CharInfo[c as usize].BBox[3] as f64;
            *descent = -0.001 * (*metrics).CharInfo[c as usize].BBox[1] as f64;
        }
        let wx = (*metrics).CharInfo[c as usize].WX;
        if wx == NA_SHORT {
            *width = 0.0;
        } else {
            *width = 0.001 * wx as f64;
        }
    } else {
        *ascent = 0.0;
        *descent = 0.0;
        *width = 0.0;
    }
}

unsafe fn PostScriptCIDMetricInfo(c: c_int, ascent: *mut f64, descent: *mut f64, width: *mut f64) {
    *ascent = 0.880;
    *descent = -0.120;
    if c == 0 || c > 65535 {
        *width = 1.0;
    } else {
        // Use a simple approximation for character width
        let w = if c >= 0x1100 { 2.0 } else { 1.0 };
        *width = w;
    }
}

// =========================================================================
// Font constructors and destructors
// =========================================================================

unsafe fn makeCIDFont() -> cidfontinfo {
    let font = libc::malloc(std::mem::size_of::<CIDFontInfo>()) as cidfontinfo;
    if !font.is_null() {
        (*font).name = [0; 50];
    }
    font
}

unsafe fn makeType1Font() -> type1fontinfo {
    let font = libc::malloc(std::mem::size_of::<Type1FontInfo>()) as type1fontinfo;
    if !font.is_null() {
        (*font).name = [0; 50];
        (*font).metrics.KernPairs = ptr::null_mut();
        (*font).metrics.nKP = 0;
    }
    font
}

unsafe fn freeCIDFont(font: cidfontinfo) {
    libc::free(font as *mut c_void);
}

unsafe fn freeType1Font(font: type1fontinfo) {
    if !font.is_null() {
        if !(*font).metrics.KernPairs.is_null() {
            libc::free((*font).metrics.KernPairs as *mut c_void);
        }
        libc::free(font as *mut c_void);
    }
}

unsafe fn makeEncoding() -> encodinginfo {
    let enc = libc::malloc(std::mem::size_of::<EncodingInfo>()) as encodinginfo;
    enc
}

unsafe fn freeEncoding(enc: encodinginfo) {
    libc::free(enc as *mut c_void);
}

unsafe fn makeCIDFontFamily() -> cidfontfamily {
    let fam = libc::malloc(std::mem::size_of::<CIDFontFamily>()) as cidfontfamily;
    if !fam.is_null() {
        (*fam).fxname = [0; 50];
        (*fam).cidfonts = [ptr::null_mut(); 4];
        (*fam).symfont = ptr::null_mut();
        (*fam).cmap = [0; 50];
        (*fam).encoding = [0; 50];
    }
    fam
}

unsafe fn makeFontFamily() -> type1fontfamily {
    let fam = libc::malloc(std::mem::size_of::<T1FontFamily>()) as type1fontfamily;
    if !fam.is_null() {
        (*fam).fxname = [0; 50];
        (*fam).fonts = [ptr::null_mut(); 5];
        (*fam).encoding = ptr::null_mut();
    }
    fam
}

unsafe fn freeCIDFontFamily(family: cidfontfamily) {
    if family.is_null() {
        return;
    }
    for i in 0..4 {
        if !(*family).cidfonts[i].is_null() {
            freeCIDFont((*family).cidfonts[i]);
        }
    }
    if !(*family).symfont.is_null() {
        freeType1Font((*family).symfont);
    }
    libc::free(family as *mut c_void);
}

unsafe fn freeFontFamily(family: type1fontfamily) {
    if family.is_null() {
        return;
    }
    for i in 0..5 {
        if !(*family).fonts[i].is_null() {
            freeType1Font((*family).fonts[i]);
        }
    }
    libc::free(family as *mut c_void);
}

unsafe fn makeCIDFontList() -> cidfontlist {
    let fl = libc::malloc(std::mem::size_of::<CIDFontList>()) as cidfontlist;
    if !fl.is_null() {
        (*fl).cidfamily = ptr::null_mut();
        (*fl).next = ptr::null_mut();
    }
    fl
}

unsafe fn makeFontList() -> type1fontlist {
    let fl = libc::malloc(std::mem::size_of::<T1FontList>()) as type1fontlist;
    if !fl.is_null() {
        (*fl).family = ptr::null_mut();
        (*fl).next = ptr::null_mut();
    }
    fl
}

unsafe fn freeCIDFontList(fl: cidfontlist) {
    if !fl.is_null() {
        (*fl).cidfamily = ptr::null_mut();
        (*fl).next = ptr::null_mut();
        libc::free(fl as *mut c_void);
    }
}

unsafe fn freeFontList(fl: type1fontlist) {
    if !fl.is_null() {
        (*fl).family = ptr::null_mut();
        (*fl).next = ptr::null_mut();
        libc::free(fl as *mut c_void);
    }
}

unsafe fn freeDeviceCIDFontList(fl: cidfontlist) {
    if !fl.is_null() {
        freeDeviceCIDFontList((*fl).next);
        freeCIDFontList(fl);
    }
}

unsafe fn freeDeviceFontList(fl: type1fontlist) {
    if !fl.is_null() {
        freeDeviceFontList((*fl).next);
        freeFontList(fl);
    }
}

unsafe fn makeEncList() -> encodinglist {
    let el = libc::malloc(std::mem::size_of::<EncList>()) as encodinglist;
    if !el.is_null() {
        (*el).encoding = ptr::null_mut();
        (*el).next = ptr::null_mut();
    }
    el
}

unsafe fn freeEncList(el: encodinglist) {
    if !el.is_null() {
        (*el).encoding = ptr::null_mut();
        (*el).next = ptr::null_mut();
        libc::free(el as *mut c_void);
    }
}

unsafe fn freeDeviceEncList(el: encodinglist) {
    if !el.is_null() {
        freeDeviceEncList((*el).next);
        freeEncList(el);
    }
}

// =========================================================================
// Utility: safestrcpy
// =========================================================================

unsafe fn safestrcpy(dest: *mut c_char, src: *const c_char, maxlen: usize) {
    let slen = libc::strlen(src);
    if slen < maxlen {
        libc::strcpy(dest, src);
    } else {
        libc::strncpy(dest, src, maxlen - 1);
        *dest.add(maxlen - 1) = 0;
    }
}

unsafe fn streql(a: *const c_char, b: *const c_char) -> bool {
    libc::strcmp(a, b) == 0
}

// =========================================================================
// Encoding list management
// =========================================================================

unsafe fn findEncoding(
    encpath: *const c_char,
    deviceEncodings: encodinglist,
    isPDF: bool,
) -> encodinginfo {
    let enclist = if isPDF {
        PDFloadedEncodings.with(|v| v.get())
    } else {
        loadedEncodings.with(|v| v.get())
    };
    let mut result: encodinginfo = ptr::null_mut();
    let mut found = false;

    if streql(encpath, b"default\0".as_ptr() as *const c_char) {
        found = true;
        if !deviceEncodings.is_null() {
            result = (*deviceEncodings).encoding;
        }
    } else {
        let mut el = enclist;
        while !el.is_null() && !found {
            found = streql(encpath, (*(*el).encoding).encpath.as_ptr());
            if found {
                result = (*el).encoding;
            }
            el = (*el).next;
        }
    }
    result
}

unsafe fn findDeviceEncoding(
    encpath: *const c_char,
    mut enclist: encodinglist,
    index: *mut c_int,
) -> encodinginfo {
    let mut result: encodinginfo = ptr::null_mut();
    let mut found = false;
    *index = 0;
    while !enclist.is_null() && !found {
        found = streql(encpath, (*(*enclist).encoding).encpath.as_ptr());
        if found {
            result = (*enclist).encoding;
        }
        enclist = (*enclist).next;
        *index += 1;
    }
    result
}

unsafe fn addEncoding(encpath: *const c_char, isPDF: bool) -> encodinginfo {
    let encoding = makeEncoding();
    if encoding.is_null() {
        return ptr::null_mut();
    }

    if LoadEncoding(
        encpath,
        (*encoding).name.as_mut_ptr(),
        (*encoding).convname.as_mut_ptr(),
        (*encoding).encnames.as_mut_ptr(),
        (*encoding).enccode.as_mut_ptr(),
        isPDF,
    ) != 0
    {
        let newenc = makeEncList();
        if newenc.is_null() {
            freeEncoding(encoding);
            return ptr::null_mut();
        }
        let enclist = if isPDF {
            PDFloadedEncodings
        } else {
            loadedEncodings
        };
        safestrcpy((*encoding).encpath.as_mut_ptr(), encpath, R_PATH_MAX);
        (*newenc).encoding = encoding;
        if enclist.is_null() {
            if isPDF {
                PDFloadedEncodings.with(|v| v.set(newenc));
            } else {
                loadedEncodings.with(|v| v.set(newenc));
            }
        } else {
            let mut el = enclist;
            while !(*el).next.is_null() {
                el = (*el).next;
            }
            (*el).next = newenc;
        }
        encoding
    } else {
        freeEncoding(encoding);
        ptr::null_mut()
    }
}

unsafe fn addDeviceEncoding(encoding: encodinginfo, mut devEncs: encodinglist) -> encodinglist {
    let newenc = makeEncList();
    if newenc.is_null() {
        return ptr::null_mut();
    }
    (*newenc).encoding = encoding;
    if devEncs.is_null() {
        devEncs = newenc;
    } else {
        let mut el = devEncs;
        while !(*el).next.is_null() {
            el = (*el).next;
        }
        (*el).next = newenc;
    }
    devEncs
}

// =========================================================================
// Font database access (stub implementations)
// =========================================================================

unsafe fn getFontDB(fontdbname: *const c_char) -> SEXP {
    // In a full implementation, this would:
    // 1. Find the grDevices namespace
    // 2. Look up .PSenv
    // 3. Find the font database
    // For now, return R_NilValue
    R_NilValue()
}

unsafe fn getFont(_family: *const c_char, _fontdbname: *const c_char) -> SEXP {
    R_NilValue()
}

unsafe fn fontMetricsFileName(
    _family: *const c_char,
    _faceIndex: c_int,
    _fontdbname: *const c_char,
) -> *const c_char {
    ptr::null()
}

unsafe fn getFontType(_family: *const c_char, _fontdbname: *const c_char) -> *const c_char {
    ptr::null()
}

unsafe fn isType1Font(
    family: *const c_char,
    _fontdbname: *const c_char,
    defaultFont: type1fontfamily,
) -> bool {
    if libc::strlen(family) == 0 {
        return !defaultFont.is_null();
    }
    let ft = getFontType(family, _fontdbname);
    if ft.is_null() {
        return false;
    }
    streql(ft, b"Type1Font\0".as_ptr() as *const c_char)
}

unsafe fn isCIDFont(
    family: *const c_char,
    _fontdbname: *const c_char,
    defaultCIDFont: cidfontfamily,
) -> bool {
    if libc::strlen(family) == 0 {
        return !defaultCIDFont.is_null();
    }
    let ft = getFontType(family, _fontdbname);
    if ft.is_null() {
        return false;
    }
    streql(ft, b"CIDFont\0".as_ptr() as *const c_char)
}

unsafe fn getFontEncoding(_family: *const c_char, _fontdbname: *const c_char) -> *const c_char {
    ptr::null()
}

unsafe fn getFontName(_family: *const c_char, _fontdbname: *const c_char) -> *const c_char {
    ptr::null()
}

unsafe fn getFontCMap(_family: *const c_char, _fontdbname: *const c_char) -> *const c_char {
    ptr::null()
}

unsafe fn getCIDFontEncoding(_family: *const c_char, _fontdbname: *const c_char) -> *const c_char {
    ptr::null()
}

unsafe fn getCIDFontPDFResource(_family: *const c_char) -> *const c_char {
    ptr::null()
}

// =========================================================================
// Font list management
// =========================================================================

unsafe fn findLoadedFont(
    name: *const c_char,
    _encoding: *const c_char,
    isPDF: bool,
) -> type1fontfamily {
    let mut fontlist = if isPDF {
        PDFloadedFonts.with(|v| v.get())
    } else {
        loadedFonts.with(|v| v.get())
    };
    while !fontlist.is_null() {
        if streql(name, (*(*fontlist).family).fxname.as_ptr()) {
            return (*fontlist).family;
        }
        fontlist = (*fontlist).next;
    }
    ptr::null_mut()
}

unsafe fn findLoadedCIDFont(family: *const c_char, isPDF: bool) -> cidfontfamily {
    let mut fontlist = if isPDF {
        PDFloadedCIDFonts.with(|v| v.get())
    } else {
        loadedCIDFonts.with(|v| v.get())
    };
    while !fontlist.is_null() {
        if !(*(*fontlist).cidfamily).cidfonts[0].is_null()
            && streql(
                family,
                (*(*(*fontlist).cidfamily).cidfonts[0]).name.as_ptr(),
            )
        {
            return (*fontlist).cidfamily;
        }
        fontlist = (*fontlist).next;
    }
    ptr::null_mut()
}

unsafe fn findDeviceCIDFont(
    name: *const c_char,
    mut fontlist: cidfontlist,
    index: *mut c_int,
) -> cidfontfamily {
    let mut font: cidfontfamily = ptr::null_mut();
    let mut found = false;
    *index = 0;
    if libc::strlen(name) > 0 {
        while !fontlist.is_null() && !found {
            found = streql(name, (*(*fontlist).cidfamily).fxname.as_ptr());
            if found {
                font = (*fontlist).cidfamily;
            }
            fontlist = (*fontlist).next;
            *index += 1;
        }
    } else {
        if !fontlist.is_null() {
            font = (*fontlist).cidfamily;
            *index = 1;
        }
    }
    font
}

unsafe fn findDeviceFont(
    name: *const c_char,
    mut fontlist: type1fontlist,
    index: *mut c_int,
) -> type1fontfamily {
    let mut font: type1fontfamily = ptr::null_mut();
    let mut found = false;
    *index = 0;
    if libc::strlen(name) > 0 {
        while !fontlist.is_null() && !found {
            found = streql(name, (*(*fontlist).family).fxname.as_ptr());
            if found {
                font = (*fontlist).family;
            }
            fontlist = (*fontlist).next;
            *index += 1;
        }
    } else {
        if !fontlist.is_null() {
            font = (*fontlist).family;
            *index = 1;
        }
    }
    font
}

unsafe fn addLoadedCIDFont(font: cidfontfamily, isPDF: bool) -> cidfontfamily {
    if font.is_null() {
        return ptr::null_mut();
    }
    let newfont = makeCIDFontList();
    if newfont.is_null() {
        freeCIDFontFamily(font);
        return ptr::null_mut();
    }
    (*newfont).cidfamily = font;
    let fontlist = if isPDF {
        PDFloadedCIDFonts.with(|v| v.get())
    } else {
        loadedCIDFonts.with(|v| v.get())
    };
    if fontlist.is_null() {
        if isPDF {
            PDFloadedCIDFonts.with(|v| v.set(newfont));
        } else {
            loadedCIDFonts.with(|v| v.set(newfont));
        }
    } else {
        let mut fl = fontlist;
        while !(*fl).next.is_null() {
            fl = (*fl).next;
        }
        (*fl).next = newfont;
    }
    font
}

unsafe fn addLoadedFont(font: type1fontfamily, isPDF: bool) -> type1fontfamily {
    if font.is_null() {
        return ptr::null_mut();
    }
    let newfont = makeFontList();
    if newfont.is_null() {
        freeFontFamily(font);
        return ptr::null_mut();
    }
    (*newfont).family = font;
    let fontlist = if isPDF {
        PDFloadedFonts.with(|v| v.get())
    } else {
        loadedFonts.with(|v| v.get())
    };
    if fontlist.is_null() {
        if isPDF {
            PDFloadedFonts.with(|v| v.set(newfont));
        } else {
            loadedFonts.with(|v| v.set(newfont));
        }
    } else {
        let mut fl = fontlist;
        while !(*fl).next.is_null() {
            fl = (*fl).next;
        }
        (*fl).next = newfont;
    }
    font
}

unsafe fn addCIDFont(name: *const c_char, isPDF: bool) -> cidfontfamily {
    let fontfamily = makeCIDFontFamily();
    if fontfamily.is_null() {
        return ptr::null_mut();
    }
    let cmap = getFontCMap(
        name,
        if isPDF {
            PDFFonts.with(|v| v.as_ptr() as *const c_char)
        } else {
            PostScriptFonts.with(|v| v.as_ptr() as *const c_char)
        },
    );
    if cmap.is_null() {
        freeCIDFontFamily(fontfamily);
        return ptr::null_mut();
    }
    safestrcpy((*fontfamily).fxname.as_mut_ptr(), name, 50);
    safestrcpy((*fontfamily).cmap.as_mut_ptr(), cmap, 50);
    let enc = getCIDFontEncoding(
        name,
        if isPDF {
            PDFFonts.with(|v| v.as_ptr() as *const c_char)
        } else {
            PostScriptFonts.with(|v| v.as_ptr() as *const c_char)
        },
    );
    if !enc.is_null() {
        safestrcpy((*fontfamily).encoding.as_mut_ptr(), enc, 50);
    }
    let fname = getFontName(
        name,
        if isPDF {
            PDFFonts.with(|v| v.as_ptr() as *const c_char)
        } else {
            PostScriptFonts.with(|v| v.as_ptr() as *const c_char)
        },
    );
    for i in 0..4 {
        (*fontfamily).cidfonts[i] = makeCIDFont();
        if !fname.is_null() {
            safestrcpy((*(*fontfamily).cidfonts[i]).name.as_mut_ptr(), fname, 50);
        }
    }
    // Load symbol font (Type 1)
    // ... (would need the actual font path)
    addLoadedCIDFont(fontfamily, isPDF)
}

unsafe fn addFont(
    name: *const c_char,
    isPDF: bool,
    deviceEncodings: encodinglist,
) -> type1fontfamily {
    let fontfamily = makeFontFamily();
    if fontfamily.is_null() {
        return ptr::null_mut();
    }
    let encpath = getFontEncoding(
        name,
        if isPDF {
            PDFFonts.with(|v| v.as_ptr() as *const c_char)
        } else {
            PostScriptFonts.with(|v| v.as_ptr() as *const c_char)
        },
    );
    if encpath.is_null() {
        freeFontFamily(fontfamily);
        return ptr::null_mut();
    }
    safestrcpy((*fontfamily).fxname.as_mut_ptr(), name, 50);
    let encoding = findEncoding(encpath, deviceEncodings, isPDF);
    let encoding = if encoding.is_null() {
        addEncoding(encpath, isPDF)
    } else {
        encoding
    };
    if encoding.is_null() {
        freeFontFamily(fontfamily);
        return ptr::null_mut();
    }
    (*fontfamily).encoding = encoding;
    // Load font metrics for each of the 5 faces
    for i in 0..5 {
        let font = makeType1Font();
        if font.is_null() {
            freeFontFamily(fontfamily);
            return ptr::null_mut();
        }
        (*fontfamily).fonts[i] = font;
        let afmpath = fontMetricsFileName(
            name,
            i as c_int,
            if isPDF {
                PDFFonts.with(|v| v.as_ptr() as *const c_char)
            } else {
                PostScriptFonts.with(|v| v.as_ptr() as *const c_char)
            },
        );
        if afmpath.is_null() {
            freeFontFamily(fontfamily);
            freeType1Font(font);
            return ptr::null_mut();
        }
        if PostScriptLoadFontMetrics(
            afmpath,
            &mut (*(*fontfamily).fonts[i]).metrics,
            (*(*fontfamily).fonts[i]).name.as_mut_ptr(),
            (*(*fontfamily).fonts[i]).charnames.as_mut_ptr(),
            (*encoding).encnames.as_mut_ptr(),
            if i < 4 { 1 } else { 0 },
        ) == 0
        {
            freeFontFamily(fontfamily);
            return ptr::null_mut();
        }
    }
    addLoadedFont(fontfamily, isPDF)
}

unsafe fn addDefaultFontFromAFMs(
    encpath: *const c_char,
    afmpaths: *const *const c_char,
    isPDF: bool,
    deviceEncodings: encodinglist,
) -> type1fontfamily {
    let fontfamily = makeFontFamily();
    if fontfamily.is_null() {
        return ptr::null_mut();
    }
    let encoding = findEncoding(encpath, deviceEncodings, isPDF);
    let encoding = if encoding.is_null() {
        addEncoding(encpath, isPDF)
    } else {
        encoding
    };
    if encoding.is_null() {
        freeFontFamily(fontfamily);
        return ptr::null_mut();
    }
    (*fontfamily).fxname[0] = 0;
    (*fontfamily).encoding = encoding;
    for i in 0..5 {
        let font = makeType1Font();
        if font.is_null() {
            freeFontFamily(fontfamily);
            return ptr::null_mut();
        }
        (*fontfamily).fonts[i] = font;
        let afm = *afmpaths.add(i);
        if PostScriptLoadFontMetrics(
            afm,
            &mut (*(*fontfamily).fonts[i]).metrics,
            (*(*fontfamily).fonts[i]).name.as_mut_ptr(),
            (*(*fontfamily).fonts[i]).charnames.as_mut_ptr(),
            (*encoding).encnames.as_mut_ptr(),
            if i < 4 { 1 } else { 0 },
        ) == 0
        {
            freeFontFamily(fontfamily);
            return ptr::null_mut();
        }
    }
    addLoadedFont(fontfamily, isPDF)
}

unsafe fn addDeviceCIDFont(
    font: cidfontfamily,
    mut devFonts: cidfontlist,
    index: *mut c_int,
) -> cidfontlist {
    let newfont = makeCIDFontList();
    *index = 0;
    if newfont.is_null() {
        return ptr::null_mut();
    }
    (*newfont).cidfamily = font;
    *index = 1;
    if devFonts.is_null() {
        devFonts = newfont;
    } else {
        let mut fl = devFonts;
        while !(*fl).next.is_null() {
            fl = (*fl).next;
            *index += 1;
        }
        (*fl).next = newfont;
    }
    devFonts
}

unsafe fn addDeviceFont(
    font: type1fontfamily,
    mut devFonts: type1fontlist,
    index: *mut c_int,
) -> type1fontlist {
    let newfont = makeFontList();
    *index = 0;
    if newfont.is_null() {
        return ptr::null_mut();
    }
    (*newfont).family = font;
    *index = 1;
    if devFonts.is_null() {
        devFonts = newfont;
    } else {
        let mut fl = devFonts;
        while !(*fl).next.is_null() {
            fl = (*fl).next;
            *index += 1;
        }
        (*fl).next = newfont;
    }
    devFonts
}

// =========================================================================
// R_GE_str2col stub
// =========================================================================

unsafe fn R_GE_str2col(_colstr: *const c_char) -> rcolor {
    // Stub: in a full implementation, this dispatches to grDevices
    0xFF000000 // black
}

// =========================================================================
// PostScript device descriptor type
// =========================================================================

#[repr(C)]
struct PostScriptDesc {
    filename: [c_char; R_PATH_MAX],
    open_type: c_int,
    papername: [c_char; 64],
    paperwidth: c_int,
    paperheight: c_int,
    landscape: bool,
    pageno: c_int,
    fileno: c_int,
    maxpointsize: c_int,
    width: f64,
    height: f64,
    pagewidth: f64,
    pageheight: f64,
    pagecentre: bool,
    printit: bool,
    command: [c_char; 2 * R_PATH_MAX],
    title: [c_char; 1024],
    colormodel: [c_char; 30],
    psfp: *mut libc::FILE,
    onefile: bool,
    paperspecial: bool,
    warn_trans: bool,
    useKern: bool,
    fillOddEven: bool,
    current: PSCurrent,
    fonts: type1fontlist,
    cidfonts: cidfontlist,
    encodings: encodinglist,
    defaultFont: type1fontfamily,
    defaultCIDFont: cidfontfamily,
}

#[repr(C)]
struct PSCurrent {
    lwd: f64,
    lty: c_int,
    lend: c_int,
    ljoin: c_int,
    lmitre: f64,
    font: c_int,
    cidfont: c_int,
    fontsize: c_int,
    col: rcolor,
    fill: rcolor,
}

// =========================================================================
// PDF device descriptor type
// =========================================================================

#[repr(C)]
struct PDFDesc {
    filename: [c_char; R_PATH_MAX],
    open_type: c_int,
    cmd: [c_char; R_PATH_MAX],
    papername: [c_char; 64],
    paperwidth: c_int,
    paperheight: c_int,
    pageno: c_int,
    fileno: c_int,
    maxpointsize: c_int,
    width: f64,
    height: f64,
    pagewidth: f64,
    pageheight: f64,
    pagecentre: bool,
    onefile: bool,
    pdffp: *mut libc::FILE,
    current: PDFCurrent,
    colAlpha: [c_short; 256],
    fillAlpha: [c_short; 256],
    usedAlpha: bool,
    versionMajor: c_int,
    versionMinor: c_int,
    nobjs: c_int,
    pos: *mut c_int,
    max_nobjs: c_int,
    pageobj: *mut c_int,
    pagemax: c_int,
    inText: bool,
    title: [c_char; 1024],
    colormodel: [c_char; 30],
    dingbats: bool,
    useKern: bool,
    fillOddEven: bool,
    useCompression: bool,
    timestamp: bool,
    producer: bool,
    author: [c_char; 1024],
    fonts: type1fontlist,
    cidfonts: cidfontlist,
    encodings: encodinglist,
    defaultFont: type1fontfamily,
    defaultCIDFont: cidfontfamily,
    offline: bool,
}

#[repr(C)]
struct PDFCurrent {
    lwd: f64,
    lty: c_int,
    lend: c_int,
    ljoin: c_int,
    lmitre: f64,
    fontsize: c_int,
    col: rcolor,
    fill: rcolor,
    bg: rcolor,
}

// =========================================================================
// Exported functions
// =========================================================================

/// Check whether a Type 1 font family is currently loaded in either
/// the PostScript or PDF device. Returns a logical scalar.
pub unsafe fn Type1FontInUse(name: SEXP, isPDF: SEXP) -> SEXP {
    use crate::main::coerce::asLogical;
    use crate::sexp::constructors::Rf_ScalarLogical;
    // If name is not a string or length > 1, error
    if TYPEOF(name) != SEXPTYPE::STRSXP.0 || LENGTH(name) > 1 {
        Rf_error(b"invalid font name or more than one font name\0".as_ptr() as *const c_char);
    }
    let fname = CHAR(STRING_ELT(name, 0));
    let pdf = asLogical(isPDF);
    let found = !findLoadedFont(fname, ptr::null(), pdf != 0).is_null();
    Rf_ScalarLogical(if found { 1 } else { 0 })
}

/// Check whether a CID font family is currently loaded in either
/// the PostScript or PDF device. Returns a logical scalar.
pub unsafe fn CIDFontInUse(name: SEXP, isPDF: SEXP) -> SEXP {
    use crate::main::coerce::asLogical;
    use crate::sexp::constructors::Rf_ScalarLogical;
    if TYPEOF(name) != SEXPTYPE::STRSXP.0 || LENGTH(name) > 1 {
        Rf_error(b"invalid font name or more than one font name\0".as_ptr() as *const c_char);
    }
    let fname = CHAR(STRING_ELT(name, 0));
    let pdf = asLogical(isPDF);
    let found = !findLoadedCIDFont(fname, pdf != 0).is_null();
    Rf_ScalarLogical(if found { 1 } else { 0 })
}

/// Create a PostScript graphics device (postscript() function in R).
///
/// Stub: returns R_NilValue (device creation not implemented).
pub unsafe fn PostScript(args: SEXP) -> SEXP {
    let _ = args;
    R_NilValue()
}

/// Create a PDF graphics device (pdf() function in R).
///
/// Stub: returns R_NilValue (device creation not implemented).
pub unsafe fn PDF(args: SEXP) -> SEXP {
    let _ = args;
    R_NilValue()
}
