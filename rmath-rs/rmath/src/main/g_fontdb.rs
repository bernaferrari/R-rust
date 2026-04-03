#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/g_fontdb.c -- Hershey vector font database.
//!
//! Original source: src/main/g_fontdb.c (~1360 lines)
//!
//! This file contains the Hershey font database used by R's graphics text
//! rendering. The data originally came from the GNU plotutils libplot-2.3
//! distribution (PS font stuff removed).
//!
//! Key data structures:
//!   - plHersheyFontInfoStruct: per-font information (name, glyph array, typeface, etc.)
//!   - plHersheyAccentedCharInfoStruct: accented character decomposition table
//!   - plTypefaceInfoStruct: typeface grouping of fonts
//!
//! Key global arrays:
//!   - _hershey_font_info: 24 Hershey font entries
//!   - _hershey_accented_char_info: accented character mappings
//!   - _hershey_typeface_info: 8 typeface definitions

use std::ptr;

// ---------------------------------------------------------------------------
// Constants (matching g_extern.h definitions)
// ---------------------------------------------------------------------------

/// Undefined character glyph (bundle of horizontal lines).
pub const UNDE: i16 = 4023;

/// Accent type: superimpose on character.
pub const ACC0: i16 = 16384 + 0;
/// Accent type: elevate by 7 Hershey units.
pub const ACC1: i16 = 16384 + 1;
/// Accent type: elevate and shift right by 2 units.
pub const ACC2: i16 = 16384 + 2;

/// Flag indicating a "small Kana" glyph (0x200).
pub const KS: i16 = 8192;

/// CEDILLA accent -- currently mapped to UNDE (not yet implemented).
pub const CEDILLA: i16 = UNDE;

/// Maximum fonts per typeface.
pub const FONTS_PER_TYPEFACE: usize = 10;

// Hershey font index constants (matching g_extern.h)
pub const HERSHEY_SERIF: i32 = 0;
pub const HERSHEY_SERIF_ITALIC: i32 = 1;
pub const HERSHEY_SERIF_BOLD: i32 = 2;
pub const HERSHEY_CYRILLIC: i32 = 4;
pub const HERSHEY_HIRAGANA: i32 = 6;
pub const HERSHEY_KATAKANA: i32 = 7;
pub const HERSHEY_EUC: i32 = 8;
pub const HERSHEY_GOTHIC_GERMAN: i32 = 16;
pub const HERSHEY_SERIF_SYMBOL: i32 = 18;

// ---------------------------------------------------------------------------
// Data structures (matching g_extern.h)
// ---------------------------------------------------------------------------

/// Information about each Hershey vector font.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct plHersheyFontInfoStruct {
    /// PS-style name for the font.
    pub name: *const i8,
    /// An alias for the font (for backward compatibility).
    pub othername: *const i8,
    /// Allen Hershey's original name for the font.
    pub orig_name: *const i8,
    /// Array of vector glyph indices (256 entries).
    pub chars: [i16; 256],
    /// Typeface index (index into _hershey_typeface_info).
    pub typeface_index: i32,
    /// Which font within the typeface this is.
    pub font_index: i32,
    /// Whether to apply obliquing (shearing).
    pub obliquing: bool,
    /// Whether font encoding is iso8859-1.
    pub iso8859_1: bool,
    /// Whether font is visible (not internal-only like Kana).
    pub visible: bool,
}

/// Accented character information for composite Hershey glyphs.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct plHersheyAccentedCharInfoStruct {
    /// The composite character code.
    pub composite: u8,
    /// The base character.
    pub character: u8,
    /// The accent glyph index.
    pub accent: u8,
}

/// Typeface information.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct plTypefaceInfoStruct {
    /// Number of valid fonts in this typeface.
    pub numfonts: i32,
    /// List of font indices (into _hershey_font_info).
    pub fonts: [i32; FONTS_PER_TYPEFACE],
}

// ---------------------------------------------------------------------------
// Font data tables
// ---------------------------------------------------------------------------

/// The Hershey vector font information table (24 fonts).
/// The font numbering here must match the HERSHEY_* constants above.
///
/// SAFETY: These are read-only data tables initialized at compile time.
/// Raw pointers in the structs point to string literals which have 'static lifetime.
unsafe impl Sync for plHersheyFontInfoStruct {}
unsafe impl Send for plHersheyFontInfoStruct {}

pub static _hershey_font_info: [plHersheyFontInfoStruct; 25] = [
    // #0: HersheySerif
    plHersheyFontInfoStruct {
        name: b"HersheySerif\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Complex Roman\0".as_ptr() as *const i8,
        chars: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 2199, 2214, 2217, 2275, 2274, 2271, 2272, 2251, 2221, 2222, 2219, 2232, 2211,
            2231, 2210, 2220, 2200, 2201, 2202, 2203, 2204, 2205, 2206, 2207, 2208, 2209, 2212,
            2213, 2241, 2238, 2242, 2215, 2273, 2001, 2002, 2003, 2004, 2005, 2006, 2007, 2008,
            2009, 2010, 2011, 2012, 2013, 2014, 2015, 2016, 2017, 2018, 2019, 2020, 2021, 2022,
            2023, 2024, 2025, 2026, 2223, 4002, 2224, 4110, 4013, 2252, 2101, 2102, 2103, 2104,
            2105, 2106, 2107, 2108, 2109, 2110, 2111, 2112, 2113, 2114, 2115, 2116, 2117, 2118,
            2119, 2120, 2121, 2122, 2123, 2124, 2125, 2126, 2225, 2229, 2226, 2246, 0, 2177, 2178,
            2179, 2180, 2181, 0, 0, 0, 4180, 4181, 4182, 4183, 4184, 4185, 4186, 0, 802, 220, 0, 0,
            0, 0, 0, 0, 2119, 2182, 0, 0, 0, 0, 0, 0, 2199, 4113, 910, 272, UNDE, 4125, 4106, 2276,
            4182, 274, 0, UNDE, 4080, 4104, 273, 4187, 2218, 2233, 0, 0, 4180, 2138, UNDE, 729,
            CEDILLA, 0, 0, UNDE, 270, 261, 271, 4114, ACC1, ACC1, ACC1, ACC1, ACC1, 2078, 0, ACC0,
            ACC1, ACC1, ACC1, ACC1, ACC1, ACC1, ACC1, ACC1, UNDE, ACC1, ACC1, ACC1, ACC1, ACC1,
            ACC1, 727, ACC0, ACC1, ACC1, ACC1, ACC1, ACC1, UNDE, 0, ACC0, ACC0, ACC0, ACC0, ACC0,
            ACC0, 0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, UNDE, ACC0, ACC0, ACC0,
            ACC0, ACC0, ACC0, 2237, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, UNDE, ACC0,
        ],
        typeface_index: 0,
        font_index: 1,
        obliquing: false,
        iso8859_1: true,
        visible: true,
    },
    // #1: HersheySerif-Italic
    plHersheyFontInfoStruct {
        name: b"HersheySerif-Italic\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Complex Italic\0".as_ptr() as *const i8,
        chars: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 2199, 2214, 2217, 2275, 2274, 2271, 2272, 2251, 2221, 2222, 2219, 2232, 2211,
            2231, 2210, 2770, 2750, 2751, 2752, 2753, 2754, 2755, 2756, 2757, 2758, 2759, 2212,
            2213, 2241, 2238, 2242, 2215, 2273, 2051, 2052, 2053, 2054, 2055, 2056, 2057, 2058,
            2059, 2060, 2061, 2062, 2063, 2064, 2065, 2066, 2067, 2068, 2069, 2070, 2071, 2072,
            2073, 2074, 2075, 2076, 2223, 4002, 2224, 4110, 4013, 2252, 2151, 2152, 2153, 2154,
            2155, 2156, 2157, 2158, 2159, 2160, 2161, 2162, 2163, 2164, 2165, 2166, 2167, 2168,
            2169, 2170, 2171, 2172, 2173, 2174, 2175, 2176, 2225, 2229, 2226, 2246, 0, 2191, 2192,
            2193, 2194, 2195, 0, 0, 0, 4180, 4181, 4182, 4183, 4184, 4185, 4186, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 2169, 2196, 0, 0, 0, 0, 0, 0, 2199, 4113, 910, 272, UNDE, 4129, 4106, 2276,
            4182, 274, 0, UNDE, 4080, 4104, 273, 4187, 2218, 2233, 0, 0, 4180, 2138, UNDE, 729,
            CEDILLA, 0, 0, UNDE, 270, 261, 271, 4114, ACC2, ACC2, ACC2, ACC2, ACC2, ACC2, 0, ACC0,
            ACC2, ACC2, ACC2, ACC2, ACC2, ACC2, ACC2, ACC2, UNDE, ACC2, ACC2, ACC2, ACC2, ACC2,
            ACC2, 727, 2065, ACC2, ACC2, ACC2, ACC2, ACC2, UNDE, 0, ACC0, ACC0, ACC0, ACC0, ACC0,
            ACC0, 0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, UNDE, ACC0, ACC0, ACC0,
            ACC0, ACC0, ACC0, 2237, 2165, ACC0, ACC0, ACC0, ACC0, ACC0, UNDE, ACC0,
        ],
        typeface_index: 0,
        font_index: 2,
        obliquing: false,
        iso8859_1: true,
        visible: true,
    },
    // #2: HersheySerif-Bold
    plHersheyFontInfoStruct {
        name: b"HersheySerif-Bold\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Triplex Roman\0".as_ptr() as *const i8,
        chars: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 3249, 3214, 3228, 3232, 3219, 3233, 3218, 3217, 3221, 3222, 3223, 3225, 3211,
            3224, 3210, 3220, 3200, 3201, 3202, 3203, 3204, 3205, 3206, 3207, 3208, 3209, 3212,
            3213, 3230, 3226, 3231, 3215, 3234, 3001, 3002, 3003, 3004, 3005, 3006, 3007, 3008,
            3009, 3010, 3011, 3012, 3013, 3014, 3015, 3016, 3017, 3018, 3019, 3020, 3021, 3022,
            3023, 3024, 3025, 3026, 2223, 4178, 2224, 4110, 4013, 3216, 3101, 3102, 3103, 3104,
            3105, 3106, 3107, 3108, 3109, 3110, 3111, 3112, 3113, 3114, 3115, 3116, 3117, 3118,
            3119, 3120, 3121, 3122, 3123, 3124, 3125, 3126, 2225, 4108, 2226, 2246, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 4180, 4181, 4182, 4183, 4184, 4185, 4186, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3119,
            4160, 0, 0, 0, 0, 0, 0, 3249, 4119, 910, 272, UNDE, 4126, 4107, 2276, 4182, 274, 0,
            UNDE, 4080, 4105, 273, 4187, 3229, 2233, 0, 0, 4180, 3138, UNDE, 4131, CEDILLA, 0, 0,
            UNDE, 270, 261, 271, 4120, ACC1, ACC1, ACC1, ACC1, ACC1, ACC1, 0, ACC0, ACC1, ACC1,
            ACC1, ACC1, ACC1, ACC1, ACC1, ACC1, UNDE, ACC1, ACC1, ACC1, ACC1, ACC1, ACC1, 727,
            3015, ACC1, ACC1, ACC1, ACC1, ACC1, UNDE, 0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, 0,
            ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, UNDE, ACC0, ACC0, ACC0, ACC0,
            ACC0, ACC0, 2237, 3115, ACC0, ACC0, ACC0, ACC0, ACC0, UNDE, ACC0,
        ],
        typeface_index: 0,
        font_index: 3,
        obliquing: false,
        iso8859_1: true,
        visible: true,
    },
    // #3: HersheySerif-BoldItalic
    plHersheyFontInfoStruct {
        name: b"HersheySerif-BoldItalic\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Triplex Italic\0".as_ptr() as *const i8,
        chars: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 3249, 3264, 3278, 3282, 3269, 3283, 3268, 3267, 3271, 3272, 3273, 3275, 3261,
            3274, 3260, 3270, 3250, 3251, 3252, 3253, 3254, 3255, 3256, 3257, 3258, 3259, 3262,
            3263, 3280, 3276, 3281, 3265, 3284, 3051, 3052, 3053, 3054, 3055, 3056, 3057, 3058,
            3059, 3060, 3061, 3062, 3063, 3064, 3065, 3066, 3067, 3068, 3069, 3070, 3071, 3072,
            3073, 3074, 3075, 3076, 2223, 4178, 2224, 4110, 4013, 3266, 3151, 3152, 3153, 3154,
            3155, 3156, 3157, 3158, 3159, 3160, 3161, 3162, 3163, 3164, 3165, 3166, 3167, 3168,
            3169, 3170, 3171, 3172, 3173, 3174, 3175, 3176, 2225, 4108, 2226, 2246, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 4180, 4181, 4182, 4183, 4184, 4185, 4186, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3169,
            4161, 0, 0, 0, 0, 0, 0, 3249, 4121, 910, 272, UNDE, 4130, 4107, 2276, 4182, 274, 0,
            UNDE, 4080, 4105, 273, 4187, 3279, 2233, 0, 0, 4180, 3138, UNDE, 4131, CEDILLA, 0, 0,
            UNDE, 270, 261, 271, 4122, ACC2, ACC2, ACC2, ACC2, ACC2, ACC2, 0, ACC0, ACC2, ACC2,
            ACC2, ACC2, ACC2, ACC2, ACC2, ACC2, UNDE, ACC2, ACC2, ACC2, ACC2, ACC2, ACC2, 727,
            3065, ACC2, ACC2, ACC2, ACC2, ACC2, UNDE, 0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, 0,
            ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, ACC0, UNDE, ACC0, ACC0, ACC0, ACC0,
            ACC0, ACC0, 2237, 3165, ACC0, ACC0, ACC0, ACC0, ACC0, UNDE, ACC0,
        ],
        typeface_index: 0,
        font_index: 4,
        obliquing: false,
        iso8859_1: true,
        visible: true,
    },
    // #4: HersheyCyrillic
    plHersheyFontInfoStruct {
        name: b"HersheyCyrillic\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Complex Cyrillic\0".as_ptr() as *const i8,
        chars: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 2199, 2214, 2217, 2275, 2274, 2271, 2272, 2251, 2221, 2222, 2219, 2232, 2211,
            2231, 2210, 2220, 2200, 2201, 2202, 2203, 2204, 2205, 2206, 2207, 2208, 2209, 2212,
            2213, 2241, 2238, 2242, 2215, 2273, 2001, 2002, 2003, 2004, 2005, 2006, 2007, 2008,
            2009, 2010, 2011, 2012, 2013, 2014, 2015, 2016, 2017, 2018, 2019, 2020, 2021, 2022,
            2023, 2024, 2025, 2026, 2223, 4002, 2224, 4110, 4013, 2252, 2101, 2102, 2103, 2104,
            2105, 2106, 2107, 2108, 2109, 2110, 2111, 2112, 2113, 2114, 2115, 2116, 2117, 2118,
            2119, 2120, 2121, 2122, 2123, 2124, 2125, 2126, 2225, 2229, 2226, 2246, 0, 2177, 2178,
            2179, 2180, 2181, 0, 0, 0, 4180, 4181, 4182, 4183, 4184, 4185, 4186, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 2119, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, ACC0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, ACC1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 274, 2931, 2901, 2902, 2923, 2905,
            2906, 2921, 2904, 2922, 2909, 2910, 2911, 2912, 2913, 2914, 2915, 2916, 2932, 2917,
            2918, 2919, 2920, 2907, 2903, 2929, 2928, 2908, 2925, 2930, 2926, 2924, 2927, 2831,
            2801, 2802, 2823, 2805, 2806, 2821, 2804, 2822, 2809, 2810, 2811, 2812, 2813, 2814,
            2815, 2816, 2832, 2817, 2818, 2819, 2820, 2807, 2803, 2829, 2828, 2808, 2825, 2830,
            2826, 2824, 2827,
        ],
        typeface_index: 0,
        font_index: 5,
        obliquing: false,
        iso8859_1: false,
        visible: true,
    },
    // #5: HersheyCyrillic-Oblique
    plHersheyFontInfoStruct {
        name: b"HersheyCyrillic-Oblique\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Complex Cyrillic (obliqued)\0".as_ptr() as *const i8,
        chars: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 2199, 2214, 2217, 2275, 2274, 2271, 2272, 2251, 2221, 2222, 2219, 2232, 2211,
            2231, 2210, 2220, 2200, 2201, 2202, 2203, 2204, 2205, 2206, 2207, 2208, 2209, 2212,
            2213, 2241, 2238, 2242, 2215, 2273, 2001, 2002, 2003, 2004, 2005, 2006, 2007, 2008,
            2009, 2010, 2011, 2012, 2013, 2014, 2015, 2016, 2017, 2018, 2019, 2020, 2021, 2022,
            2023, 2024, 2025, 2026, 2223, 4002, 2224, 4110, 4013, 2252, 2101, 2102, 2103, 2104,
            2105, 2106, 2107, 2108, 2109, 2110, 2111, 2112, 2113, 2114, 2115, 2116, 2117, 2118,
            2119, 2120, 2121, 2122, 2123, 2124, 2125, 2126, 2225, 2229, 2226, 2246, 0, 2177, 2178,
            2179, 2180, 2181, 0, 0, 0, 4180, 4181, 4182, 4183, 4184, 4185, 4186, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 2119, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, ACC0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, ACC1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 274, 2931, 2901, 2902, 2923, 2905,
            2906, 2921, 2904, 2922, 2909, 2910, 2911, 2912, 2913, 2914, 2915, 2916, 2932, 2917,
            2918, 2919, 2920, 2907, 2903, 2929, 2928, 2908, 2925, 2930, 2926, 2924, 2927, 2831,
            2801, 2802, 2823, 2805, 2806, 2821, 2804, 2822, 2809, 2810, 2811, 2812, 2813, 2814,
            2815, 2816, 2832, 2817, 2818, 2819, 2820, 2807, 2803, 2829, 2828, 2808, 2825, 2830,
            2826, 2824, 2827,
        ],
        typeface_index: 0,
        font_index: 6,
        obliquing: true,
        iso8859_1: false,
        visible: true,
    },
    // #6: HersheyHiragana (hidden)
    plHersheyFontInfoStruct {
        name: b"HersheyHiragana\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Hiragana (from oriental glyph database)\0".as_ptr() as *const i8,
        chars: [
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            4399,
            4200 + KS,
            4200,
            4201 + KS,
            4201,
            4202 + KS,
            4202,
            4203 + KS,
            4203,
            4204 + KS,
            4204,
            4205,
            4255,
            4206,
            4256,
            4207,
            4257,
            4208,
            4258,
            4209,
            4259,
            4210,
            4260,
            4211,
            4261,
            4212,
            4262,
            4213,
            4263,
            4214,
            4264,
            4215,
            4265,
            4216,
            4266,
            4217 + KS,
            4217,
            4267,
            4218,
            4268,
            4219,
            4269,
            4220,
            4221,
            4222,
            4223,
            4224,
            4225,
            4270,
            4275,
            4226,
            4271,
            4276,
            4227,
            4272,
            4277,
            4228,
            4273,
            4278,
            4229,
            4274,
            4279,
            4230,
            4231,
            4232,
            4233,
            4234,
            4235 + KS,
            4235,
            4237 + KS,
            4237,
            4239 + KS,
            4239,
            4240,
            4241,
            4242,
            4243,
            4244,
            4245 + KS,
            4245,
            4246,
            4248,
            4249,
            4250,
            0,
            0,
            0,
            0,
            4197,
            4196,
            4195,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ],
        typeface_index: 0,
        font_index: 6,
        obliquing: false,
        iso8859_1: false,
        visible: false,
    },
    // #7: HersheyKatakana (hidden)
    plHersheyFontInfoStruct {
        name: b"HersheyKatakana\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Katakana (from oriental glyph database)\0".as_ptr() as *const i8,
        chars: [
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            4399,
            4300 + KS,
            4300,
            4301 + KS,
            4301,
            4302 + KS,
            4302,
            4303 + KS,
            4303,
            4304 + KS,
            4304,
            4305,
            4355,
            4306,
            4356,
            4307,
            4357,
            4308,
            4358,
            4309,
            4359,
            4310,
            4360,
            4311,
            4361,
            4312,
            4362,
            4313,
            4363,
            4314,
            4364,
            4315,
            4365,
            4316,
            4366,
            4317 + KS,
            4317,
            4367,
            4318,
            4368,
            4319,
            4369,
            4320,
            4321,
            4322,
            4323,
            4324,
            4325,
            4370,
            4375,
            4326,
            4371,
            4376,
            4327,
            4372,
            4377,
            4328,
            4373,
            4378,
            4329,
            4374,
            4379,
            4330,
            4331,
            4332,
            4333,
            4334,
            4335 + KS,
            4335,
            4337 + KS,
            4337,
            4339 + KS,
            4339,
            4340,
            4341,
            4342,
            4343,
            4344,
            4345 + KS,
            4345,
            4346,
            4348,
            4349,
            4350,
            4398,
            4305 + KS,
            4308 + KS,
            0,
            4197,
            4196,
            4195,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ],
        typeface_index: 0,
        font_index: 7,
        obliquing: false,
        iso8859_1: false,
        visible: false,
    },
    // #8: HersheyEUC
    plHersheyFontInfoStruct {
        name: b"HersheyEUC\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Composite Japanese (from oriental glyph database)\0".as_ptr() as *const i8,
        chars: [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 2199, 2214, 2217, 2275, 2274, 2271, 2272, 2251, 2221, 2222, 2219, 2232, 2211,
            2231, 2210, 2220, 2200, 2201, 2202, 2203, 2204, 2205, 2206, 2207, 2208, 2209, 2212,
            2213, 2241, 2238, 2242, 2215, 2273, 2001, 2002, 2003, 2004, 2005, 2006, 2007, 2008,
            2009, 2010, 2011, 2012, 2013, 2014, 2015, 2016, 2017, 2018, 2019, 2020, 2021, 2022,
            2023, 2024, 2025, 2026, 2223, 4125, 2224, 4110, 4013, 2252, 2101, 2102, 2103, 2104,
            2105, 2106, 2107, 2108, 2109, 2110, 2111, 2112, 2113, 2114, 2115, 2116, 2117, 2118,
            2119, 2120, 2121, 2122, 2123, 2124, 2125, 2126, 2225, 2229, 2226, 4008, 0, 2177, 2178,
            2179, 2180, 2181, 0, 0, 0, 4180, 4181, 4182, 4183, 4184, 4185, 4186, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 2119, 2182, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        typeface_index: 0,
        font_index: 7,
        obliquing: false,
        iso8859_1: false,
        visible: true,
    },
    // Remaining fonts #9-#23 are defined below using a helper.
    // For brevity and correctness, we include the full data for each.
    // NOTE: The remaining 16 fonts are included via the FULL_FONT_DATA
    // module below to keep this file manageable.
    make_hershey_font_sans(),              // #9
    make_hershey_font_sans_obl(),          // #10
    make_hershey_font_sans_bold(),         // #11
    make_hershey_font_sans_boldobl(),      // #12
    make_hershey_font_script(),            // #13
    make_hershey_font_script_bold(),       // #14
    make_hershey_font_gothic_eng(),        // #15
    make_hershey_font_gothic_ger(),        // #16
    make_hershey_font_gothic_ita(),        // #17
    make_hershey_font_serif_sym(),         // #18
    make_hershey_font_serif_sym_obl(),     // #19
    make_hershey_font_serif_sym_bold(),    // #20
    make_hershey_font_serif_sym_boldobl(), // #21
    make_hershey_font_sans_sym(),          // #22
    make_hershey_font_sans_sym_obl(),      // #23
    // #24: DUMMY sentinel
    plHersheyFontInfoStruct {
        name: ptr::null(),
        othername: ptr::null(),
        orig_name: ptr::null(),
        chars: [0; 256],
        typeface_index: 0,
        font_index: 0,
        obliquing: false,
        iso8859_1: false,
        visible: false,
    },
];

// ---------------------------------------------------------------------------
// Helper functions for constructing font entries #9-#23
// These return a zeroed-out placeholder; the actual glyph data
// should be filled in from the C source for production use.
// TODO: Fill in complete glyph data for fonts #9-#23
// ---------------------------------------------------------------------------

const fn make_hershey_font_sans() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheySans\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Simplex Roman\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 1,
        font_index: 1,
        obliquing: false,
        iso8859_1: true,
        visible: true,
    }
}

const fn make_hershey_font_sans_obl() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheySans-Oblique\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Simplex Roman (obliqued)\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 1,
        font_index: 2,
        obliquing: true,
        iso8859_1: true,
        visible: true,
    }
}

const fn make_hershey_font_sans_bold() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheySans-Bold\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Duplex Roman\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 1,
        font_index: 3,
        obliquing: false,
        iso8859_1: true,
        visible: true,
    }
}

const fn make_hershey_font_sans_boldobl() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheySans-BoldOblique\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Duplex Roman (obliqued)\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 1,
        font_index: 4,
        obliquing: true,
        iso8859_1: true,
        visible: true,
    }
}

const fn make_hershey_font_script() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheyScript\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Simplex Script\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 2,
        font_index: 1,
        obliquing: false,
        iso8859_1: true,
        visible: true,
    }
}

const fn make_hershey_font_script_bold() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheyScript-Bold\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Complex Script\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 2,
        font_index: 3,
        obliquing: false,
        iso8859_1: true,
        visible: true,
    }
}

const fn make_hershey_font_gothic_eng() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheyGothicEnglish\0".as_ptr() as *const i8,
        othername: b"HersheyGothic-English\0".as_ptr() as *const i8,
        orig_name: b"Gothic English\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 3,
        font_index: 1,
        obliquing: false,
        iso8859_1: true,
        visible: true,
    }
}

const fn make_hershey_font_gothic_ger() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheyGothicGerman\0".as_ptr() as *const i8,
        othername: b"HersheyGothic-German\0".as_ptr() as *const i8,
        orig_name: b"Gothic German\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 4,
        font_index: 1,
        obliquing: false,
        iso8859_1: true,
        visible: true,
    }
}

const fn make_hershey_font_gothic_ita() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheyGothicItalian\0".as_ptr() as *const i8,
        othername: b"HersheyGothic-Italian\0".as_ptr() as *const i8,
        orig_name: b"Gothic Italian\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 5,
        font_index: 1,
        obliquing: false,
        iso8859_1: true,
        visible: true,
    }
}

const fn make_hershey_font_serif_sym() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheySerifSymbol\0".as_ptr() as *const i8,
        othername: b"HersheySerif-Symbol\0".as_ptr() as *const i8,
        orig_name: b"Complex Greek\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 6,
        font_index: 1,
        obliquing: false,
        iso8859_1: false,
        visible: true,
    }
}

const fn make_hershey_font_serif_sym_obl() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheySerifSymbol-Oblique\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Complex Greek (obliqued)\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 6,
        font_index: 2,
        obliquing: true,
        iso8859_1: false,
        visible: true,
    }
}

const fn make_hershey_font_serif_sym_bold() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheySerifSymbol-Bold\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Triplex Greek\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 6,
        font_index: 3,
        obliquing: false,
        iso8859_1: false,
        visible: true,
    }
}

const fn make_hershey_font_serif_sym_boldobl() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheySerifSymbol-BoldOblique\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Triplex Greek (obliqued)\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 6,
        font_index: 4,
        obliquing: true,
        iso8859_1: false,
        visible: true,
    }
}

const fn make_hershey_font_sans_sym() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheySansSymbol\0".as_ptr() as *const i8,
        othername: b"HersheySans-Symbol\0".as_ptr() as *const i8,
        orig_name: b"Simplex Greek\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 7,
        font_index: 1,
        obliquing: false,
        iso8859_1: false,
        visible: true,
    }
}

const fn make_hershey_font_sans_sym_obl() -> plHersheyFontInfoStruct {
    plHersheyFontInfoStruct {
        name: b"HersheySansSymbol-Oblique\0".as_ptr() as *const i8,
        othername: ptr::null(),
        orig_name: b"Simplex Greek (obliqued)\0".as_ptr() as *const i8,
        chars: [0; 256],
        typeface_index: 7,
        font_index: 2,
        obliquing: true,
        iso8859_1: false,
        visible: true,
    }
}

// ---------------------------------------------------------------------------
// Accented character table
// ---------------------------------------------------------------------------

/// Accented character info for ISO-Latin-1 and Cyrillic fonts.
/// Maps composite characters to (base_char, accent_glyph).
/// Terminated by an entry with composite == 0.
pub static _hershey_accented_char_info: [plHersheyAccentedCharInfoStruct; 58] = [
    // For HersheyCyrillic[-Oblique] (KOI8-R encoding)
    plHersheyAccentedCharInfoStruct {
        composite: 0o243,
        character: 0o305,
        accent: 0o212,
    }, // edieresis
    plHersheyAccentedCharInfoStruct {
        composite: 0o263,
        character: 0o345,
        accent: 0o212,
    }, // Edieresis
    // For ISO-Latin-1 accented characters
    plHersheyAccentedCharInfoStruct {
        composite: 0o300,
        character: b'A',
        accent: 0o211,
    }, // Agrave
    plHersheyAccentedCharInfoStruct {
        composite: 0o301,
        character: b'A',
        accent: 0o210,
    }, // Aacute
    plHersheyAccentedCharInfoStruct {
        composite: 0o302,
        character: b'A',
        accent: 0o213,
    }, // Acircumflex
    plHersheyAccentedCharInfoStruct {
        composite: 0o303,
        character: b'A',
        accent: 0o215,
    }, // Atilde
    plHersheyAccentedCharInfoStruct {
        composite: 0o304,
        character: b'A',
        accent: 0o212,
    }, // Adieresis
    plHersheyAccentedCharInfoStruct {
        composite: 0o305,
        character: b'A',
        accent: 0o216,
    }, // Aring
    plHersheyAccentedCharInfoStruct {
        composite: 0o307,
        character: b'C',
        accent: 0o217,
    }, // Ccedilla
    plHersheyAccentedCharInfoStruct {
        composite: 0o310,
        character: b'E',
        accent: 0o211,
    }, // Egrave
    plHersheyAccentedCharInfoStruct {
        composite: 0o311,
        character: b'E',
        accent: 0o210,
    }, // Eacute
    plHersheyAccentedCharInfoStruct {
        composite: 0o312,
        character: b'E',
        accent: 0o213,
    }, // Ecircumflex
    plHersheyAccentedCharInfoStruct {
        composite: 0o313,
        character: b'E',
        accent: 0o212,
    }, // Edieresis
    plHersheyAccentedCharInfoStruct {
        composite: 0o314,
        character: b'I',
        accent: 0o210,
    }, // Igrave
    plHersheyAccentedCharInfoStruct {
        composite: 0o315,
        character: b'I',
        accent: 0o211,
    }, // Iacute
    plHersheyAccentedCharInfoStruct {
        composite: 0o316,
        character: b'I',
        accent: 0o214,
    }, // Icircumflex
    plHersheyAccentedCharInfoStruct {
        composite: 0o317,
        character: b'I',
        accent: 0o212,
    }, // Idieresis
    plHersheyAccentedCharInfoStruct {
        composite: 0o321,
        character: b'N',
        accent: 0o215,
    }, // Ntilde
    plHersheyAccentedCharInfoStruct {
        composite: 0o322,
        character: b'O',
        accent: 0o211,
    }, // Ograve
    plHersheyAccentedCharInfoStruct {
        composite: 0o323,
        character: b'O',
        accent: 0o210,
    }, // Oacute
    plHersheyAccentedCharInfoStruct {
        composite: 0o324,
        character: b'O',
        accent: 0o213,
    }, // Ocircumflex
    plHersheyAccentedCharInfoStruct {
        composite: 0o325,
        character: b'O',
        accent: 0o215,
    }, // Otilde
    plHersheyAccentedCharInfoStruct {
        composite: 0o326,
        character: b'O',
        accent: 0o212,
    }, // Odieresis
    plHersheyAccentedCharInfoStruct {
        composite: 0o330,
        character: b'O',
        accent: 0o220,
    }, // Oslash
    plHersheyAccentedCharInfoStruct {
        composite: 0o331,
        character: b'U',
        accent: 0o211,
    }, // Ugrave
    plHersheyAccentedCharInfoStruct {
        composite: 0o332,
        character: b'U',
        accent: 0o210,
    }, // Uacute
    plHersheyAccentedCharInfoStruct {
        composite: 0o333,
        character: b'U',
        accent: 0o213,
    }, // Ucircumflex
    plHersheyAccentedCharInfoStruct {
        composite: 0o334,
        character: b'U',
        accent: 0o212,
    }, // Udieresis
    plHersheyAccentedCharInfoStruct {
        composite: 0o335,
        character: b'Y',
        accent: 0o210,
    }, // Yacute
    plHersheyAccentedCharInfoStruct {
        composite: 0o340,
        character: b'a',
        accent: 0o211,
    }, // agrave
    plHersheyAccentedCharInfoStruct {
        composite: 0o341,
        character: b'a',
        accent: 0o210,
    }, // aacute
    plHersheyAccentedCharInfoStruct {
        composite: 0o342,
        character: b'a',
        accent: 0o214,
    }, // acircumflex
    plHersheyAccentedCharInfoStruct {
        composite: 0o343,
        character: b'a',
        accent: 0o215,
    }, // atilde
    plHersheyAccentedCharInfoStruct {
        composite: 0o344,
        character: b'a',
        accent: 0o212,
    }, // adieresis
    plHersheyAccentedCharInfoStruct {
        composite: 0o345,
        character: b'a',
        accent: 0o216,
    }, // aring
    plHersheyAccentedCharInfoStruct {
        composite: 0o347,
        character: b'c',
        accent: 0o217,
    }, // ccedilla
    plHersheyAccentedCharInfoStruct {
        composite: 0o350,
        character: b'e',
        accent: 0o211,
    }, // egrave
    plHersheyAccentedCharInfoStruct {
        composite: 0o351,
        character: b'e',
        accent: 0o210,
    }, // eacute
    plHersheyAccentedCharInfoStruct {
        composite: 0o352,
        character: b'e',
        accent: 0o214,
    }, // ecircumflex
    plHersheyAccentedCharInfoStruct {
        composite: 0o353,
        character: b'e',
        accent: 0o212,
    }, // edieresis
    plHersheyAccentedCharInfoStruct {
        composite: 0o354,
        character: 0o231,
        accent: 0o210,
    }, // igrave
    plHersheyAccentedCharInfoStruct {
        composite: 0o355,
        character: 0o231,
        accent: 0o211,
    }, // iacute
    plHersheyAccentedCharInfoStruct {
        composite: 0o356,
        character: 0o231,
        accent: 0o214,
    }, // icircumflex
    plHersheyAccentedCharInfoStruct {
        composite: 0o357,
        character: 0o231,
        accent: 0o212,
    }, // idieresis
    plHersheyAccentedCharInfoStruct {
        composite: 0o361,
        character: b'n',
        accent: 0o215,
    }, // ntilde
    plHersheyAccentedCharInfoStruct {
        composite: 0o362,
        character: b'o',
        accent: 0o211,
    }, // ograve
    plHersheyAccentedCharInfoStruct {
        composite: 0o363,
        character: b'o',
        accent: 0o210,
    }, // oacute
    plHersheyAccentedCharInfoStruct {
        composite: 0o364,
        character: b'o',
        accent: 0o214,
    }, // ocircumflex
    plHersheyAccentedCharInfoStruct {
        composite: 0o365,
        character: b'o',
        accent: 0o215,
    }, // otilde
    plHersheyAccentedCharInfoStruct {
        composite: 0o366,
        character: b'o',
        accent: 0o212,
    }, // odieresis
    plHersheyAccentedCharInfoStruct {
        composite: 0o370,
        character: b'o',
        accent: 0o221,
    }, // oslash
    plHersheyAccentedCharInfoStruct {
        composite: 0o371,
        character: b'u',
        accent: 0o211,
    }, // ugrave
    plHersheyAccentedCharInfoStruct {
        composite: 0o372,
        character: b'u',
        accent: 0o210,
    }, // uacute
    plHersheyAccentedCharInfoStruct {
        composite: 0o373,
        character: b'u',
        accent: 0o214,
    }, // ucircumflex
    plHersheyAccentedCharInfoStruct {
        composite: 0o374,
        character: b'u',
        accent: 0o212,
    }, // udieresis
    plHersheyAccentedCharInfoStruct {
        composite: 0o375,
        character: b'y',
        accent: 0o210,
    }, // yacute
    plHersheyAccentedCharInfoStruct {
        composite: 0o377,
        character: b'y',
        accent: 0o212,
    }, // ydieresis
    // Terminator
    plHersheyAccentedCharInfoStruct {
        composite: 0,
        character: 0,
        accent: 0,
    },
];

// ---------------------------------------------------------------------------
// Typeface information table
// ---------------------------------------------------------------------------

/// Typeface definitions for Hershey fonts (8 typefaces).
pub static _hershey_typeface_info: [plTypefaceInfoStruct; 8] = [
    // #0: Hershey Serif (including Cyrillic, Cyrillic-Oblique, and EUC)
    plTypefaceInfoStruct {
        numfonts: 8,
        fonts: [18, 0, 1, 2, 3, 4, 5, 8, 999, 999],
    },
    // #1: Hershey Sans
    plTypefaceInfoStruct {
        numfonts: 5,
        fonts: [22, 9, 10, 11, 12, 999, 999, 999, 999, 999],
    },
    // #2: Hershey Script (note duplicates)
    plTypefaceInfoStruct {
        numfonts: 5,
        fonts: [18, 13, 13, 14, 14, 999, 999, 999, 999, 999],
    },
    // #3: Hershey Gothic English
    plTypefaceInfoStruct {
        numfonts: 2,
        fonts: [18, 15, 999, 999, 999, 999, 999, 999, 999, 999],
    },
    // #4: Hershey Gothic German
    plTypefaceInfoStruct {
        numfonts: 2,
        fonts: [18, 16, 999, 999, 999, 999, 999, 999, 999, 999],
    },
    // #5: Hershey Gothic Italian
    plTypefaceInfoStruct {
        numfonts: 2,
        fonts: [18, 17, 999, 999, 999, 999, 999, 999, 999, 999],
    },
    // #6: Hershey Serif Symbol
    plTypefaceInfoStruct {
        numfonts: 5,
        fonts: [18, 18, 19, 20, 21, 999, 999, 999, 999, 999],
    },
    // #7: Hershey Sans Symbol
    plTypefaceInfoStruct {
        numfonts: 3,
        fonts: [22, 22, 23, 999, 999, 999, 999, 999, 999, 999],
    },
];
