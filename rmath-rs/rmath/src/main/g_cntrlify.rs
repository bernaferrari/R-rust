#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/main/g_cntrlify.c
 *
 *  PAUL MURRELL
 *  This is from the GNU plotutils libplot-2.3 distribution
 *  All references to HAVE_PROTOS removed
 *  All references to "plotter" replaced with references to "GEDevDesc"
 *
 *  <UTF8-FIXME> This assumes single-byte encoding
 *
 *  _controlify() converts a "label" (i.e. a character string), which may
 *  contain troff-like escape sequences, into a string of unsigned shorts.
 */

use std::os::raw::{c_int, c_uchar};

use crate::main::engine::pGEDevDesc;
use crate::main::g_fontdb::{
    _hershey_font_info, _hershey_typeface_info, HERSHEY_CYRILLIC, HERSHEY_EUC,
    HERSHEY_GOTHIC_GERMAN, HERSHEY_SERIF, HERSHEY_SERIF_ITALIC, UNDE,
};
use crate::main::g_her_glyph::{NUM_OCCIDENTAL_HERSHEY_GLYPHS, NUM_ORIENTAL_HERSHEY_GLYPHS};

// ---------------------------------------------------------------------------
// Constants from g_control.h
// ---------------------------------------------------------------------------

/// Control codes (order must agree with _control_tbl in g_cntrlify.h)
pub const C_BEGIN_SUPERSCRIPT: c_int = 0;
pub const C_END_SUPERSCRIPT: c_int = 1;
pub const C_BEGIN_SUBSCRIPT: c_int = 2;
pub const C_END_SUBSCRIPT: c_int = 3;
pub const C_PUSH_LOCATION: c_int = 4;
pub const C_POP_LOCATION: c_int = 5;
pub const C_RIGHT_ONE_EM: c_int = 6;
pub const C_RIGHT_HALF_EM: c_int = 7;
pub const C_RIGHT_QUARTER_EM: c_int = 8;
pub const C_RIGHT_SIXTH_EM: c_int = 9;
pub const C_RIGHT_EIGHTH_EM: c_int = 10;
pub const C_RIGHT_TWELFTH_EM: c_int = 11;
pub const C_LEFT_ONE_EM: c_int = 12;
pub const C_LEFT_HALF_EM: c_int = 13;
pub const C_LEFT_QUARTER_EM: c_int = 14;
pub const C_LEFT_SIXTH_EM: c_int = 15;
pub const C_LEFT_EIGHTH_EM: c_int = 16;
pub const C_LEFT_TWELFTH_EM: c_int = 17;

pub const C_RIGHT_RADICAL_SHIFT: c_int = 254;
pub const C_LEFT_RADICAL_SHIFT: c_int = 255;
pub const PS_RADICAL_WIDTH: f64 = 0.515;
pub const PCL_RADICAL_WIDTH: f64 = 0.080;
pub const RADICALEX: c_int = 96;

/// Flags in each unsigned short in a `controlified' text string (mutually exclusive)
pub const CONTROL_CODE: u16 = 0x8000;
pub const RAW_HERSHEY_GLYPH: u16 = 0x4000;
pub const RAW_ORIENTAL_HERSHEY_GLYPH: u16 = 0x2000;

pub const ONE_BYTE: u16 = 0xff;
pub const FONT_SHIFT: c_int = 8;
pub const FONT_SPEC: u16 = (ONE_BYTE as u16) << (FONT_SHIFT as u16);
pub const GLYPH_SPEC: u16 = 0x1fff;

// ---------------------------------------------------------------------------
// Constants from g_cntrlify.h (not imported from elsewhere)
// ---------------------------------------------------------------------------

pub const FINAL_LOWERCASE_S: u16 = 0o230;
pub const VECTOR_SYMBOL_FONT_UNDERSCORE: u16 = 0o237;

// ---------------------------------------------------------------------------
// Constants from g_jis.h
// ---------------------------------------------------------------------------

pub const BEGINNING_OF_KANJI: c_int = 0x3000;

// ---------------------------------------------------------------------------
// Data structures from g_cntrlify.h
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Escape {
    pub byte: u8,
    pub string: *const u8,  // const char *
    pub ps_name: *const u8, // const char *
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Raiseinfo {
    pub from: u8,
    pub to: u8,
    pub underscored: c_int,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Deligature {
    pub from: u8,
    pub to: *const u8, // const char *
    pub except_font: c_int,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct DeligatureEscape {
    pub from: *const u8, // const char *
    pub to: *const u8,   // const char *
    pub except_font: c_int,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Ligature {
    pub font: c_int,
    pub from: *const u8, // const char *
    pub byte: u8,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct kanjipair {
    pub jis: c_int,
    pub nelson: c_int,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct jis_entry {
    pub jis: c_int,
    pub font: c_int,
    pub charnum: u16,
}

// ---------------------------------------------------------------------------
// Static data tables (from g_cntrlify.h)
// ---------------------------------------------------------------------------

const _control_tbl: [&[u8]; 18] = [
    b"sp", // \sp = start superscript
    b"ep", // \ep = end superscript
    b"sb", // \sb = start subscript
    b"eb", // \eb = end subscript
    b"mk", // \mk = mark location
    b"rt", // \rt = return
    b"r1", // \r1 = shift right by 1 em
    b"r2", // \r2 = shift right by em/2
    b"r4", // \r4 = shift right by em/4
    b"r6", // \r6 = shift right by em/6
    b"r8", // \r8 = shift right by em/8
    b"r^", // \r^ = shift right by em/12
    b"l1", // \l1 = shift left by 1 em
    b"l2", // \l2 = shift left by em/2
    b"l4", // \l4 = shift left by em/4
    b"l6", // \l6 = shift left by em/6
    b"l8", // \l8 = shift left by em/8
    b"l^", // \l^ = shift left by em/12
];

const NUM_CONTROLS: usize = 18;

/// ISO-Latin-1 escape table
static _iso_escape_tbl: [(u8, &[u8], &[u8]); 95] = [
    (161, b"r!", b"exclamdown"),
    (162, b"ct", b"cent"),
    (163, b"Po", b"sterling"),
    (164, b"Cs", b"currency"),
    (165, b"Ye", b"yen"),
    (166, b"bb", b"brokenbar"),
    (167, b"sc", b"section"),
    (168, b"ad", b"dieresis"),
    (169, b"co", b"copyright"),
    (170, b"Of", b"ordfeminine"),
    (171, b"Fo", b"guillemotleft"),
    (172, b"no", b"logicalnot"),
    (173, b"hy", b"hyphen"),
    (174, b"rg", b"registered"),
    (175, b"a-", b"macron"),
    (176, b"de", b"degree"),
    (177, b"+-", b"plusminus"),
    (178, b"S2", b"twosuperior"),
    (179, b"S3", b"threesuperior"),
    (180, b"aa", b"acute"),
    (181, b"*m", b"mu"),
    (182, b"ps", b"paragraph"),
    (183, b"md", b"periodcentered"),
    (184, b"ac", b"cedilla"),
    (185, b"S1", b"onesuperior"),
    (186, b"Om", b"ordmasculine"),
    (187, b"Fc", b"guillemotright"),
    (188, b"14", b"onequarter"),
    (189, b"12", b"onehalf"),
    (190, b"34", b"threequarters"),
    (191, b"r?", b"questiondown"),
    (192, b"`A", b"Agrave"),
    (193, b"'A", b"Aacute"),
    (194, b"^A", b"Acircumflex"),
    (195, b"~A", b"Atilde"),
    (196, b":A", b"Adieresis"),
    (197, b"oA", b"Aring"),
    (198, b"AE", b"AE"),
    (199, b",C", b"Ccedilla"),
    (200, b"`E", b"Egrave"),
    (201, b"'E", b"Eacute"),
    (202, b"^E", b"Ecircumflex"),
    (203, b":E", b"Edieresis"),
    (204, b"`I", b"Igrave"),
    (205, b"'I", b"Iacute"),
    (206, b"^I", b"Icircumflex"),
    (207, b":I", b"Idieresis"),
    (208, b"-D", b"Eth"),
    (209, b"~N", b"Ntilde"),
    (210, b"`O", b"Ograve"),
    (211, b"'O", b"Oacute"),
    (212, b"^O", b"Ocircumflex"),
    (213, b"~O", b"Otilde"),
    (214, b":O", b"Odieresis"),
    (215, b"mu", b"multiply"),
    (216, b"/O", b"Oslash"),
    (217, b"`U", b"Ugrave"),
    (218, b"'U", b"Uacute"),
    (219, b"^U", b"Ucircumflex"),
    (220, b":U", b"Udieresis"),
    (221, b"'Y", b"Yacute"),
    (222, b"TP", b"Thorn"),
    (223, b"ss", b"germandbls"),
    (224, b"`a", b"agrave"),
    (225, b"'a", b"aacute"),
    (226, b"^a", b"acircumflex"),
    (227, b"~a", b"atilde"),
    (228, b":a", b"adieresis"),
    (229, b"oa", b"aring"),
    (230, b"ae", b"ae"),
    (231, b",c", b"ccedilla"),
    (232, b"`e", b"egrave"),
    (233, b"'e", b"eacute"),
    (234, b"^e", b"ecircumflex"),
    (235, b":e", b"edieresis"),
    (236, b"`i", b"igrave"),
    (237, b"'i", b"iacute"),
    (238, b"^i", b"icircumflex"),
    (239, b":i", b"idieresis"),
    (240, b"Sd", b"eth"),
    (241, b"~n", b"ntilde"),
    (242, b"`o", b"ograve"),
    (243, b"'o", b"oacute"),
    (244, b"^o", b"ocircumflex"),
    (245, b"~o", b"otilde"),
    (246, b":o", b"odieresis"),
    (247, b"di", b"divide"),
    (248, b"/o", b"oslash"),
    (249, b"`u", b"ugrave"),
    (250, b"'u", b"uacute"),
    (251, b"^u", b"ucircumflex"),
    (252, b":u", b"udieresis"),
    (253, b"'y", b"yacute"),
    (254, b"Tp", b"thorn"),
    (255, b":y", b"ydieresis"),
];

const NUM_ISO_ESCAPES: usize = 95;

/// Symbol escape table (161 entries)
static _symbol_escape_tbl: [(u8, &[u8], &[u8]); 161] = [
    (0o042, b"fa", b"universal"),
    (0o044, b"te", b"existential"),
    (0o047, b"st", b"suchthat"),
    (0o052, b"**", b"asteriskmath"),
    (0o100, b"=~", b"congruent"),
    (0o101, b"*A", b"Alpha"),
    (0o102, b"*B", b"Beta"),
    (0o103, b"*X", b"Chi"),
    (0o104, b"*D", b"Delta"),
    (0o105, b"*E", b"Epsilon"),
    (0o106, b"*F", b"Phi"),
    (0o107, b"*G", b"Gamma"),
    (0o110, b"*Y", b"Eta"),
    (0o111, b"*I", b"Iota"),
    (0o112, b"+h", b"theta1"),
    (0o113, b"*K", b"kappa"),
    (0o114, b"*L", b"Lambda"),
    (0o115, b"*M", b"Mu"),
    (0o116, b"*N", b"Nu"),
    (0o117, b"*O", b"Omicron"),
    (0o120, b"*P", b"Pi"),
    (0o121, b"*H", b"Theta"),
    (0o122, b"*R", b"Rho"),
    (0o123, b"*S", b"Sigma"),
    (0o124, b"*T", b"Tau"),
    (0o125, b"*U", b"Upsilon"),
    (0o126, b"ts", b"sigma1"),
    (0o127, b"*W", b"Omega"),
    (0o130, b"*C", b"Xi"),
    (0o131, b"*Q", b"Psi"),
    (0o132, b"*Z", b"Zeta"),
    (0o134, b"tf", b"therefore"),
    (0o136, b"pp", b"perpendicular"),
    (0o137, b"ul", b"underline"),
    (0o140, b"rx", b"radicalex"),
    (0o141, b"*a", b"alpha"),
    (0o142, b"*b", b"beta"),
    (0o143, b"*x", b"chi"),
    (0o144, b"*d", b"delta"),
    (0o145, b"*e", b"epsilon"),
    (0o146, b"*f", b"phi"),
    (0o147, b"*g", b"gamma"),
    (0o150, b"*y", b"eta"),
    (0o151, b"*i", b"iota"),
    (0o152, b"+f", b"phi1"),
    (0o153, b"*k", b"kappa"),
    (0o154, b"*l", b"lambda"),
    (0o155, b"*m", b"mu"),
    (0o156, b"*n", b"nu"),
    (0o157, b"*o", b"omicron"),
    (0o160, b"*p", b"pi"),
    (0o161, b"*h", b"theta"),
    (0o162, b"*r", b"rho"),
    (0o163, b"*s", b"sigma"),
    (0o164, b"*t", b"tau"),
    (0o165, b"*u", b"upsilon"),
    (0o166, b"+p", b"omega1"),
    (0o167, b"*w", b"omega"),
    (0o170, b"*c", b"xi"),
    (0o171, b"*q", b"psi"),
    (0o172, b"*z", b"zeta"),
    (0o176, b"ap", b"similar"),
    (0o241, b"+U", b"Upsilon1"),
    (0o242, b"fm", b"minute"),
    (0o243, b"<=", b"lessequal"),
    (0o244, b"f/", b"fraction"),
    (0o245, b"if", b"infinity"),
    (0o246, b"Fn", b"florin"),
    (0o247, b"CL", b"club"),
    (0o250, b"DI", b"diamond"),
    (0o251, b"HE", b"heart"),
    (0o252, b"SP", b"spade"),
    (0o253, b"<>", b"arrowboth"),
    (0o254, b"<-", b"arrowleft"),
    (0o255, b"ua", b"arrowup"),
    (0o256, b"->", b"arrowright"),
    (0o257, b"da", b"arrowdown"),
    (0o260, b"de", b"degree"),
    (0o261, b"+-", b"plusminus"),
    (0o262, b"sd", b"second"),
    (0o263, b">=", b"greaterequal"),
    (0o264, b"mu", b"multiply"),
    (0o265, b"pt", b"proportional"),
    (0o266, b"pd", b"partialdiff"),
    (0o267, b"bu", b"bullet"),
    (0o270, b"di", b"divide"),
    (0o271, b"!=", b"notequal"),
    (0o272, b"==", b"equivalence"),
    (0o273, b"~~", b"approxequal"),
    (0o274, b"..", b"ellipsis"),
    (0o275, b"NO_ABBREV", b"arrowvertex"),
    (0o276, b"an", b"arrowhorizex"),
    (0o277, b"CR", b"carriagereturn"),
    (0o300, b"Ah", b"aleph"),
    (0o301, b"Im", b"Ifraktur"),
    (0o302, b"Re", b"Rfraktur"),
    (0o303, b"wp", b"weierstrass"),
    (0o304, b"c*", b"circlemultiply"),
    (0o305, b"c+", b"circleplus"),
    (0o306, b"es", b"emptyset"),
    (0o307, b"ca", b"cap"),
    (0o310, b"cu", b"cup"),
    (0o311, b"SS", b"superset"),
    (0o312, b"ip", b"reflexsuperset"),
    (0o313, b"n<", b"notsubset"),
    (0o314, b"SB", b"subset"),
    (0o315, b"ib", b"reflexsubset"),
    (0o316, b"mo", b"element"),
    (0o317, b"nm", b"notelement"),
    (0o320, b"/_", b"angle"),
    (0o321, b"gr", b"nabla"),
    (0o322, b"rg", b"registerserif"),
    (0o323, b"co", b"copyrightserif"),
    (0o324, b"tm", b"trademarkserif"),
    (0o325, b"PR", b"product"),
    (0o326, b"sr", b"radical"),
    (0o327, b"md", b"dotmath"),
    (0o330, b"no", b"logicalnot"),
    (0o331, b"AN", b"logicaland"),
    (0o332, b"OR", b"logicalor"),
    (0o333, b"hA", b"arrowdblboth"),
    (0o334, b"lA", b"arrowdblleft"),
    (0o335, b"uA", b"arrowdblup"),
    (0o336, b"rA", b"arrowdblright"),
    (0o337, b"dA", b"arrowdbldown"),
    (0o340, b"lz", b"lozenge"),
    (0o341, b"la", b"angleleft"),
    (0o342, b"RG", b"registersans"),
    (0o343, b"CO", b"copyrightsans"),
    (0o344, b"TM", b"trademarksans"),
    (0o345, b"SU", b"summation"),
    (0o346, b"NO_ABBREV", b"parenlefttp"),
    (0o347, b"NO_ABBREV", b"parenleftex"),
    (0o350, b"NO_ABBREV", b"parenleftbt"),
    (0o351, b"lc", b"bracketlefttp"),
    (0o352, b"NO_ABBREV", b"bracketleftex"),
    (0o353, b"lf", b"bracketleftbt"),
    (0o354, b"lt", b"bracelefttp"),
    (0o355, b"lk", b"braceleftmid"),
    (0o356, b"lb", b"braceleftbt"),
    (0o357, b"bv", b"braceex"),
    (0o360, b"eu", b"euro"),
    (0o361, b"ra", b"angleright"),
    (0o362, b"is", b"integral"),
    (0o363, b"NO_ABBREV", b"integraltp"),
    (0o364, b"NO_ABBREV", b"integralex"),
    (0o365, b"NO_ABBREV", b"integralbt"),
    (0o366, b"NO_ABBREV", b"parenrighttp"),
    (0o367, b"NO_ABBREV", b"parenrightex"),
    (0o370, b"NO_ABBREV", b"parenrightbt"),
    (0o371, b"rc", b"bracketrighttp"),
    (0o372, b"NO_ABBREV", b"bracketrightex"),
    (0o373, b"rf", b"bracketrightbt"),
    (0o374, b"RT", b"bracerighttp"),
    (0o375, b"rk", b"bracerightmid"),
    (0o376, b"rb", b"bracerightbt"),
    // traditional UGS aliases
    (0o100, b"~=", b"congruent"),
    (0o242, b"pr", b"minute"),
    (0o245, b"in", b"infinity"),
    (0o271, b"n=", b"notequal"),
    (0o321, b"dl", b"nabla"),
];

const NUM_SYMBOL_ESCAPES: usize = 161;

/// Special escape table (40 entries)
static _special_escape_tbl: [(u8, &[u8], &[u8]); 40] = [
    (0o01, b"AR", b"aries"),
    (0o02, b"TA", b"taurus"),
    (0o03, b"GE", b"gemini"),
    (0o04, b"CA", b"cancer"),
    (0o05, b"LE", b"leo"),
    (0o06, b"VI", b"virgo"),
    (0o07, b"LI", b"libra"),
    (0o010, b"SC", b"scorpio"),
    (0o011, b"SG", b"sagittarius"),
    (0o012, b"CP", b"capricornus"),
    (0o013, b"AQ", b"aquarius"),
    (0o014, b"PI", b"pisces"),
    (0o204, b"~-", b"modifiedcongruent"),
    (0o205, b"hb", b"hbar"),
    (0o206, b"IB", b"interbang"),
    (0o207, b"Lb", b"lambdabar"),
    (0o210, b"UD", b"undefined"),
    (0o211, b"SO", b"sun"),
    (0o212, b"ME", b"mercury"),
    (0o213, b"VE", b"venus"),
    (0o214, b"EA", b"earth"),
    (0o215, b"MA", b"mars"),
    (0o216, b"JU", b"jupiter"),
    (0o217, b"SA", b"saturn"),
    (0o220, b"UR", b"uranus"),
    (0o221, b"NE", b"neptune"),
    (0o222, b"PL", b"pluto"),
    (0o223, b"LU", b"moon"),
    (0o224, b"CT", b"comet"),
    (0o225, b"ST", b"star"),
    (0o226, b"AS", b"ascendingnode"),
    (0o227, b"DE", b"descendingnode"),
    (0o230, b"s-", b"s1"),
    (0o231, b"dg", b"dagger"),
    (0o232, b"dd", b"daggerdbl"),
    (0o233, b"li", b"line integral"),
    (0o234, b"-+", b"minusplus"),
    (0o235, b"||", b"parallel"),
    (0o236, b"rn", b"overscore"),
    (0o237, b"ul", b"underscore"),
];

const NUM_SPECIAL_ESCAPES: usize = 40;

/// Raised character table
static _raised_char_tbl: [(u8, u8, c_int); 5] = [
    (170, 97, 1),  // ordfeminine mapped to 'a'
    (178, 50, 0),  // twosuperior mapped to '2'
    (179, 51, 0),  // threesuperior mapped to '3'
    (185, 49, 0),  // onesuperior mapped to '1'
    (186, 111, 1), // ordmasculine mapped to 'o'
];

const NUM_RAISED_CHARS: usize = 5;

/// Single-character deligature table
static _deligature_char_tbl: [(u8, &[u8], c_int); 3] = [
    (198, b"AE", 999),
    (230, b"ae", 999),
    (223, b"ss", HERSHEY_GOTHIC_GERMAN),
];

const NUM_DELIGATURED_CHARS: usize = 3;

/// Deligature escape table
static _deligature_escape_tbl: [(&[u8], &[u8], c_int); 3] = [
    (b"AE", b"AE", 999),
    (b"ae", b"ae", 999),
    (b"ss", b"ss", HERSHEY_GOTHIC_GERMAN),
];

const NUM_DELIGATURED_ESCAPES: usize = 3;

/// Ligature table
static _ligature_tbl: [(c_int, &[u8], u8); 22] = [
    (HERSHEY_SERIF, b"ffi", 0o203),
    (HERSHEY_SERIF, b"ffl", 0o204),
    (HERSHEY_SERIF, b"ff", 0o200),
    (HERSHEY_SERIF, b"fi", 0o201),
    (HERSHEY_SERIF, b"fl", 0o202),
    (HERSHEY_SERIF_ITALIC, b"ffi", 0o203),
    (HERSHEY_SERIF_ITALIC, b"ffl", 0o204),
    (HERSHEY_SERIF_ITALIC, b"ff", 0o200),
    (HERSHEY_SERIF_ITALIC, b"fi", 0o201),
    (HERSHEY_SERIF_ITALIC, b"fl", 0o202),
    (HERSHEY_GOTHIC_GERMAN, b"ch", 0o206),
    (HERSHEY_GOTHIC_GERMAN, b"tz", 0o207),
    (HERSHEY_CYRILLIC, b"ffi", 0o203),
    (HERSHEY_CYRILLIC, b"ffl", 0o204),
    (HERSHEY_CYRILLIC, b"ff", 0o200),
    (HERSHEY_CYRILLIC, b"fi", 0o201),
    (HERSHEY_CYRILLIC, b"fl", 0o202),
    (HERSHEY_EUC, b"ffi", 0o203),
    (HERSHEY_EUC, b"ffl", 0o204),
    (HERSHEY_EUC, b"ff", 0o200),
    (HERSHEY_EUC, b"fi", 0o201),
    (HERSHEY_EUC, b"fl", 0o202),
];

const NUM_LIGATURES: usize = 22;

// ---------------------------------------------------------------------------
// External symbols (from g_her_glyph.c) - stubs
// ---------------------------------------------------------------------------

// TODO: These are defined in g_her_glyph.c which is not yet ported.
// When it is, replace these stubs with actual extern declarations.

/// _builtin_kanji_glyphs array from g_jis.h
/// TODO: Replace with actual extern when g_her_glyph.rs is ported
#[unsafe(no_mangle)]
pub static _builtin_kanji_glyphs: [kanjipair; 1] = [kanjipair { jis: 0, nelson: 0 }];

/// _builtin_jis_chars array from g_jis.h
/// TODO: Replace with actual extern when g_her_glyph.rs is ported
#[unsafe(no_mangle)]
pub static _builtin_jis_chars: [jis_entry; 1] = [jis_entry {
    jis: 0,
    font: 0,
    charnum: 0,
}];

// ---------------------------------------------------------------------------
// Helper macros
// ---------------------------------------------------------------------------

/// GOOD_JIS_INDEX macro
#[inline(always)]
fn GOOD_JIS_INDEX(row: c_int, col: c_int) -> bool {
    row > 0x20 && row < 0x7f && col > 0x20 && col < 0x7f
}

/// Case-insensitive comparison for filenames
#[inline]
fn strcmpcasenosensitive_internal(fileName1: &[u8], fileName2: &[u8]) -> c_int {
    let mut i1 = 0;
    let mut i2 = 0;
    loop {
        let c1 = if i1 < fileName1.len() {
            fileName1[i1]
        } else {
            0
        };
        let c2 = if i2 < fileName2.len() {
            fileName2[i2]
        } else {
            0
        };
        let c1 = if c1 >= b'a' && c1 <= b'z' {
            c1 - 0x20
        } else {
            c1
        };
        let c2 = if c2 >= b'a' && c2 <= b'z' {
            c2 - 0x20
        } else {
            c2
        };
        if c1 == 0 {
            return if c2 == 0 { 0 } else { -1 };
        }
        if c2 == 0 {
            return 1;
        }
        if c1 < c2 {
            return -1;
        }
        if c1 > c2 {
            return 1;
        }
        i1 += 1;
        i2 += 1;
    }
}

/// Helper to compare a byte slice to a null-terminated C string
#[inline]
fn byte_eq_cstr(bytes: &[u8], s: &[u8]) -> bool {
    // bytes comes from the escape sequence (2 chars)
    if bytes.len() < s.len() {
        return false;
    }
    &bytes[..s.len()] == s
}

/// Helper: does the byte slice starting at src match the given pattern?
#[inline]
fn strncmp_bytes(src: &[u8], pat: &[u8]) -> bool {
    if src.len() < pat.len() {
        return false;
    }
    src[..pat.len()] == *pat
}

// ---------------------------------------------------------------------------
// R_alloc stub (for the actual function, see sexp::memory_ext)
// ---------------------------------------------------------------------------

unsafe extern "C" {
    fn R_alloc(size: usize, nelem: usize) -> *mut std::ffi::c_void;
}

// ---------------------------------------------------------------------------
// _controlify: converts a label string into annotated unsigned shorts
// ---------------------------------------------------------------------------

pub unsafe fn _controlify(
    dd: pGEDevDesc,
    src: *const c_uchar,
    typeface: c_int,
    fontindex: c_int,
) -> *mut u16 {
    unsafe {
        let dest: *mut u16;
        let c: c_uchar;
        let d: c_uchar;

        // Allocate destination buffer: string length can grow by factor of 6
        let src_len = libc::strlen(src as *const i8);
        dest = R_alloc(6 * src_len + 1, std::mem::size_of::<u16>()) as *mut u16;

        // Get font info from typeface tables
        let raw_fontnum =
            (*_hershey_typeface_info.as_ptr().add(typeface as usize)).fonts[fontindex as usize];
        let raw_symbol_fontnum = (*_hershey_typeface_info.as_ptr().add(typeface as usize)).fonts[0];

        let fontword = ((raw_fontnum as u16) << FONT_SHIFT) as u16;
        let symbol_fontword = ((raw_symbol_fontnum as u16) << FONT_SHIFT) as u16;

        let mut j: usize = 0;
        let mut src_idx: usize = 0;

        while src_idx < src_len {
            let src_ptr = src.add(src_idx);

            // EUC two-byte character check
            if raw_fontnum == HERSHEY_EUC
                && (*src_ptr & 0x80) != 0
                && (src_idx + 1 < src_len)
                && (*(src_ptr.add(1)) & 0x80) != 0
            {
                let jis_row = *src_ptr & !(0x80);
                let jis_col = *src_ptr.add(1) & !(0x80);

                if GOOD_JIS_INDEX(jis_row as c_int, jis_col as c_int) {
                    let jis_glyphindex = 256 * jis_row as c_int + jis_col as c_int;

                    if jis_glyphindex >= BEGINNING_OF_KANJI {
                        // Kanji range - look up in kanji table
                        let mut kanji = _builtin_kanji_glyphs.as_ptr();
                        let mut matched = false;

                        while (*kanji).jis != 0 {
                            if jis_glyphindex == (*kanji).jis {
                                matched = true;
                                break;
                            }
                            kanji = kanji.add(1);
                        }
                        if matched {
                            *dest.add(j) = RAW_ORIENTAL_HERSHEY_GLYPH | ((*kanji).nelson as u16);
                            j += 1;
                            src_idx += 2;
                            continue;
                        } else {
                            // Kanji we don't have
                            *dest.add(j) = RAW_HERSHEY_GLYPH | (UNDE as u16);
                            j += 1;
                            src_idx += 2;
                            continue;
                        }
                    } else {
                        // Not in Kanji range, look in char table
                        let mut char_mapping = _builtin_jis_chars.as_ptr();
                        let mut matched = false;

                        while (*char_mapping).jis != 0 {
                            if jis_glyphindex == (*char_mapping).jis {
                                matched = true;
                                break;
                            }
                            char_mapping = char_mapping.add(1);
                        }
                        if matched {
                            let fontnum = (*char_mapping).font;
                            let charnum = (*char_mapping).charnum;

                            if charnum & RAW_HERSHEY_GLYPH != 0 {
                                *dest.add(j) = RAW_HERSHEY_GLYPH | charnum;
                            } else {
                                *dest.add(j) = (((fontnum as u16) << FONT_SHIFT) | charnum) as u16;
                            }
                            j += 1;
                            src_idx += 2;
                            continue;
                        } else {
                            *dest.add(j) = RAW_HERSHEY_GLYPH | (UNDE as u16);
                            j += 1;
                            src_idx += 2;
                            continue;
                        }
                    }
                } else {
                    // JIS index is OOB
                    src_idx += 2;
                    continue;
                }
            }

            // Ligature matching (Hershey fonts)
            {
                let mut matched = false;
                let mut lig_i: usize = 0;

                for i in 0..NUM_LIGATURES {
                    if _ligature_tbl[i].0 == raw_fontnum {
                        let remaining = src_len - src_idx;
                        if remaining >= _ligature_tbl[i].1.len()
                            && strncmp_bytes(
                                std::slice::from_raw_parts(src_ptr, remaining),
                                _ligature_tbl[i].1,
                            )
                        {
                            matched = true;
                            lig_i = i;
                            break;
                        }
                    }
                }

                if matched {
                    *dest.add(j) = fontword | (_ligature_tbl[lig_i].2 as u16);
                    j += 1;
                    src_idx += _ligature_tbl[lig_i].1.len();
                    continue;
                }
            }

            let c = *src_ptr;
            src_idx += 1;

            if c != b'\\' {
                // Ordinary character, may pass through
                if (*_hershey_font_info.as_ptr().add(raw_fontnum as usize)).iso8859_1 {
                    let mut matched = false;
                    let mut rch_i: usize = 0;

                    // Check if this is a 'raised' ISO-Latin-1 character
                    for i in 0..NUM_RAISED_CHARS {
                        if c == _raised_char_tbl[i].0 {
                            matched = true;
                            rch_i = i;
                            break;
                        }
                    }
                    if matched {
                        *dest.add(j) = CONTROL_CODE | (C_BEGIN_SUPERSCRIPT as u16);
                        j += 1;
                        if _raised_char_tbl[rch_i].2 != 0 {
                            // also underline
                            *dest.add(j) = CONTROL_CODE | (C_PUSH_LOCATION as u16);
                            j += 1;
                            *dest.add(j) = fontword | (_raised_char_tbl[rch_i].1 as u16);
                            j += 1;
                            *dest.add(j) = CONTROL_CODE | (C_POP_LOCATION as u16);
                            j += 1;
                            *dest.add(j) = symbol_fontword | VECTOR_SYMBOL_FONT_UNDERSCORE;
                            j += 1;
                        } else {
                            *dest.add(j) = fontword | (_raised_char_tbl[rch_i].1 as u16);
                            j += 1;
                        }
                        *dest.add(j) = CONTROL_CODE | (C_END_SUPERSCRIPT as u16);
                        j += 1;
                        continue;
                    }

                    // Check if this char should be deligatured
                    let mut dlig_matched = false;
                    let mut dlig_i: usize = 0;
                    for i in 0..NUM_DELIGATURED_CHARS {
                        if c == _deligature_char_tbl[i].0 {
                            dlig_matched = true;
                            dlig_i = i;
                            break;
                        }
                    }
                    if dlig_matched {
                        if _deligature_char_tbl[dlig_i].2 != raw_fontnum {
                            *dest.add(j) = fontword | (_deligature_char_tbl[dlig_i].1[0] as u16);
                            j += 1;
                            *dest.add(j) = fontword | (_deligature_char_tbl[dlig_i].1[1] as u16);
                            j += 1;
                            continue;
                        }
                    }
                }

                // Didn't do anything special, pass character through
                *dest.add(j) = fontword | (c as u16);
                j += 1;
                continue;
            } else {
                // Character is a backslash
                if src_idx >= src_len {
                    // ASCII NUL
                    *dest.add(j) = fontword | (b'\\' as u16);
                    j += 1;
                    break;
                }

                let c = *src.add(src_idx);
                src_idx += 1;

                if c == b'\\' {
                    *dest.add(j) = fontword | (b'\\' as u16);
                    j += 1;
                    *dest.add(j) = fontword | (b'\\' as u16);
                    j += 1;
                    continue;
                }

                if src_idx >= src_len {
                    *dest.add(j) = fontword | (b'\\' as u16);
                    j += 1;
                    *dest.add(j) = fontword | (c as u16);
                    j += 1;
                    break;
                }

                let d = *src.add(src_idx);
                src_idx += 1;

                // esc[0] = c, esc[1] = d
                let esc: &[u8] = &[c, d];

                // Check for raw Hershey glyph escape \#H0001
                if esc[0] == b'#'
                    && esc[1] == b'H'
                    && src_idx + 3 < src_len
                    && (*src.add(src_idx)).is_ascii_digit()
                    && (*src.add(src_idx + 1)).is_ascii_digit()
                    && (*src.add(src_idx + 2)).is_ascii_digit()
                    && (*src.add(src_idx + 3)).is_ascii_digit()
                {
                    let glyphindex = (*src.add(src_idx + 3) - b'0') as c_int
                        + 10 * (*src.add(src_idx + 2) - b'0') as c_int
                        + 100 * (*src.add(src_idx + 1) - b'0') as c_int
                        + 1000 * (*src.add(src_idx) - b'0') as c_int;
                    if (glyphindex as usize) < NUM_OCCIDENTAL_HERSHEY_GLYPHS {
                        *dest.add(j) = RAW_HERSHEY_GLYPH | (glyphindex as u16);
                        j += 1;
                        src_idx += 4;
                        continue;
                    }
                }

                // Check for raw Japanese Hershey glyph \#N0001
                if esc[0] == b'#'
                    && esc[1] == b'N'
                    && src_idx + 3 < src_len
                    && (*src.add(src_idx)).is_ascii_digit()
                    && (*src.add(src_idx + 1)).is_ascii_digit()
                    && (*src.add(src_idx + 2)).is_ascii_digit()
                    && (*src.add(src_idx + 3)).is_ascii_digit()
                {
                    let glyphindex = (*src.add(src_idx + 3) - b'0') as c_int
                        + 10 * (*src.add(src_idx + 2) - b'0') as c_int
                        + 100 * (*src.add(src_idx + 1) - b'0') as c_int
                        + 1000 * (*src.add(src_idx) - b'0') as c_int;
                    if (glyphindex as usize) < NUM_ORIENTAL_HERSHEY_GLYPHS {
                        *dest.add(j) = RAW_ORIENTAL_HERSHEY_GLYPH | (glyphindex as u16);
                        j += 1;
                        src_idx += 4;
                        continue;
                    }
                }

                // Check for raw Japanese Hershey glyph \#J0001 (hex)
                if esc[0] == b'#'
                    && esc[1] == b'J'
                    && src_idx + 3 < src_len
                    && is_hex_digit(*src.add(src_idx))
                    && is_hex_digit(*src.add(src_idx + 1))
                    && is_hex_digit(*src.add(src_idx + 2))
                    && is_hex_digit(*src.add(src_idx + 3))
                {
                    let hexnum = [
                        hex_val(*src.add(src_idx)),
                        hex_val(*src.add(src_idx + 1)),
                        hex_val(*src.add(src_idx + 2)),
                        hex_val(*src.add(src_idx + 3)),
                    ];

                    let jis_glyphindex = hexnum[3] as c_int
                        + 16 * hexnum[2] as c_int
                        + 256 * hexnum[1] as c_int
                        + 4096 * hexnum[0] as c_int;
                    let jis_row = hexnum[1] as c_int + 16 * hexnum[0] as c_int;
                    let jis_col = hexnum[3] as c_int + 16 * hexnum[2] as c_int;

                    if GOOD_JIS_INDEX(jis_row, jis_col) {
                        if jis_glyphindex >= BEGINNING_OF_KANJI {
                            // Kanji range
                            let mut kanji = _builtin_kanji_glyphs.as_ptr();
                            let mut matched = false;

                            while (*kanji).jis != 0 {
                                if jis_glyphindex == (*kanji).jis {
                                    matched = true;
                                    break;
                                }
                                kanji = kanji.add(1);
                            }
                            if matched {
                                *dest.add(j) =
                                    RAW_ORIENTAL_HERSHEY_GLYPH | ((*kanji).nelson as u16);
                                j += 1;
                                src_idx += 4;
                                continue;
                            } else {
                                *dest.add(j) = RAW_HERSHEY_GLYPH | (UNDE as u16);
                                j += 1;
                                src_idx += 4;
                                continue;
                            }
                        } else {
                            // Not in Kanji range
                            let mut char_mapping = _builtin_jis_chars.as_ptr();
                            let mut matched = false;

                            while (*char_mapping).jis != 0 {
                                if jis_glyphindex == (*char_mapping).jis {
                                    matched = true;
                                    break;
                                }
                                char_mapping = char_mapping.add(1);
                            }
                            if matched {
                                let fontnum = (*char_mapping).font;
                                let charnum = (*char_mapping).charnum;

                                if charnum & RAW_HERSHEY_GLYPH != 0 {
                                    *dest.add(j) = RAW_HERSHEY_GLYPH | charnum;
                                } else {
                                    *dest.add(j) =
                                        (((fontnum as u16) << FONT_SHIFT) | charnum) as u16;
                                }
                                j += 1;
                                src_idx += 4;
                                continue;
                            } else {
                                *dest.add(j) = RAW_HERSHEY_GLYPH | (UNDE as u16);
                                j += 1;
                                src_idx += 4;
                                continue;
                            }
                        }
                    }
                }

                // Check for control code escape
                {
                    let mut matched = false;
                    let mut ctrl_i: usize = 0;
                    for i in 0..NUM_CONTROLS {
                        if _control_tbl[i] == esc {
                            matched = true;
                            ctrl_i = i;
                            break;
                        }
                    }
                    if matched {
                        *dest.add(j) = CONTROL_CODE | (ctrl_i as u16);
                        j += 1;
                        continue;
                    }
                }

                // Check for deligatured escape (ISO-8859-1 Hershey fonts)
                if (*_hershey_font_info.as_ptr().add(raw_fontnum as usize)).iso8859_1 {
                    let mut matched = false;
                    let mut dlig_i: usize = 0;
                    for i in 0..NUM_DELIGATURED_ESCAPES {
                        if _deligature_escape_tbl[i].0 == esc {
                            matched = true;
                            dlig_i = i;
                            break;
                        }
                    }
                    if matched {
                        if _deligature_escape_tbl[dlig_i].2 != raw_fontnum {
                            *dest.add(j) = fontword | (_deligature_escape_tbl[dlig_i].1[0] as u16);
                            j += 1;
                            *dest.add(j) = fontword | (_deligature_escape_tbl[dlig_i].1[1] as u16);
                            j += 1;
                            continue;
                        }
                    }
                }

                // Check for ISO-Latin-1 escape
                if (*_hershey_font_info.as_ptr().add(raw_fontnum as usize)).iso8859_1 {
                    let mut matched = false;
                    let mut iso_i: usize = 0;
                    for i in 0..NUM_ISO_ESCAPES {
                        if _iso_escape_tbl[i].1 == esc {
                            matched = true;
                            iso_i = i;
                            break;
                        }
                    }
                    if matched {
                        // Check if this is a raised character
                        let mut matched2 = false;
                        let mut rch_k: usize = 0;
                        for k in 0..NUM_RAISED_CHARS {
                            if _iso_escape_tbl[iso_i].0 == _raised_char_tbl[k].0 {
                                matched2 = true;
                                rch_k = k;
                                break;
                            }
                        }
                        if matched2 {
                            *dest.add(j) = CONTROL_CODE | (C_BEGIN_SUPERSCRIPT as u16);
                            j += 1;
                            if _raised_char_tbl[rch_k].2 != 0 {
                                *dest.add(j) = CONTROL_CODE | (C_PUSH_LOCATION as u16);
                                j += 1;
                                *dest.add(j) = fontword | (_raised_char_tbl[rch_k].1 as u16);
                                j += 1;
                                *dest.add(j) = CONTROL_CODE | (C_POP_LOCATION as u16);
                                j += 1;
                                *dest.add(j) = symbol_fontword | VECTOR_SYMBOL_FONT_UNDERSCORE;
                                j += 1;
                            } else {
                                *dest.add(j) = fontword | (_raised_char_tbl[rch_k].1 as u16);
                                j += 1;
                            }
                            *dest.add(j) = CONTROL_CODE | (C_END_SUPERSCRIPT as u16);
                            j += 1;
                            continue;
                        }

                        // Not raised, just pass through
                        *dest.add(j) = fontword | (_iso_escape_tbl[iso_i].0 as u16);
                        j += 1;
                        continue;
                    }
                }

                // Check for special Hershey glyph escape
                {
                    let mut matched = false;
                    let mut spec_i: usize = 0;
                    for i in 0..NUM_SPECIAL_ESCAPES {
                        if _special_escape_tbl[i].1 == esc {
                            matched = true;
                            spec_i = i;
                            break;
                        }
                    }
                    if matched {
                        if _special_escape_tbl[spec_i].0 as u16 == FINAL_LOWERCASE_S {
                            *dest.add(j) = fontword | (_special_escape_tbl[spec_i].0 as u16);
                        } else {
                            *dest.add(j) = symbol_fontword | (_special_escape_tbl[spec_i].0 as u16);
                        }
                        j += 1;
                        continue;
                    }
                }

                // Check for symbol escape
                {
                    let mut matched = false;
                    let mut sym_i: usize = 0;
                    for i in 0..NUM_SYMBOL_ESCAPES {
                        if _symbol_escape_tbl[i].1 != b"NO_ABBREV" && _symbol_escape_tbl[i].1 == esc
                        {
                            matched = true;
                            sym_i = i;
                            break;
                        }
                    }
                    if matched {
                        *dest.add(j) = symbol_fontword | (_symbol_escape_tbl[sym_i].0 as u16);
                        j += 1;
                        continue;
                    }
                }

                // Unknown escape sequence, pass through unchanged
                *dest.add(j) = fontword | (b'\\' as u16);
                j += 1;
                *dest.add(j) = fontword | (c as u16);
                j += 1;
                *dest.add(j) = fontword | (d as u16);
                j += 1;
            }
        }

        *dest.add(j) = 0u16; // terminate string

        dest
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

#[inline]
fn is_hex_digit(c: c_uchar) -> bool {
    c.is_ascii_digit() || (c >= b'a' && c <= b'f') || (c >= b'A' && c <= b'F')
}

#[inline]
fn hex_val(c: c_uchar) -> u8 {
    if c.is_ascii_digit() {
        c - b'0'
    } else if c >= b'a' && c <= b'f' {
        10 + c - b'a'
    } else {
        10 + c - b'A'
    }
}
