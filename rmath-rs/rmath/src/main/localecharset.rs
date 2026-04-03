#![allow(
    unsafe_op_in_unsafe_fn,
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]

//! Port of R's src/main/localecharset.c -- locale to charset mapping
//!
//! Provides locale2charset() which maps locale strings to encoding names.
//! On macOS, all real locales without encoding are UTF-8.
//!
//! Ported from r-source/src/main/localecharset.c

use std::ffi::CStr;
use std::os::raw::c_char;

// ---------------------------------------------------------------------------
// Known encoding mappings (from R's `known[]` table)
// ---------------------------------------------------------------------------

/// Mapping from lowercase encoding name to canonical NUL-terminated name.
/// All values include a trailing NUL byte so they can be returned as C strings.
static KNOWN: &[(&[u8], &[u8])] = &[
    (b"iso88591", b"ISO8859-1\0"),
    (b"iso88592", b"ISO8859-2\0"),
    (b"iso88593", b"ISO8859-3\0"),
    (b"iso88596", b"ISO8859-6\0"),
    (b"iso88597", b"ISO8859-7\0"),
    (b"iso88598", b"ISO8859-8\0"),
    (b"iso88599", b"ISO8859-9\0"),
    (b"iso885910", b"ISO8859-10\0"),
    (b"iso885913", b"ISO8859-13\0"),
    (b"iso885914", b"ISO8859-14\0"),
    (b"iso885915", b"ISO8859-15\0"),
    (b"cp1251", b"CP1251\0"),
    (b"cp1255", b"CP1255\0"),
    (b"eucjp", b"EUC-JP\0"),
    (b"euckr", b"EUC-KR\0"),
    (b"euctw", b"EUC-TW\0"),
    (b"georgianps", b"GEORGIAN-PS\0"),
    (b"koi8u", b"KOI8-U\0"),
    (b"tcvn", b"TCVN\0"),
    (b"big5", b"BIG5\0"),
    (b"gb2312", b"GB2312\0"),
    (b"gb18030", b"GB18030\0"),
    (b"gbk", b"GBK\0"),
    (b"tis-620", b"TIS-620\0"),
    (b"sjis", b"SHIFT_JIS\0"),
    (b"euccn", b"GB2312\0"),
    (b"big5-hkscs", b"BIG5-HKSCS\0"),
    // macOS-specific entries
    (b"iso8859-1", b"ISO8859-1\0"),
    (b"iso8859-2", b"ISO8859-2\0"),
    (b"iso8859-4", b"ISO8859-4\0"),
    (b"iso8859-7", b"ISO8859-7\0"),
    (b"iso8859-9", b"ISO8859-9\0"),
    (b"iso8859-13", b"ISO8859-13\0"),
    (b"iso8859-15", b"ISO8859-15\0"),
    (b"koi8-u", b"KOI8-U\0"),
    (b"koi8-r", b"KOI8-R\0"),
    (b"pt154", b"PT154\0"),
    (b"us-ascii", b"ASCII\0"),
    (b"armscii-8", b"ARMSCII-8\0"),
    (b"iscii-dev", b"ISCII-DEV\0"),
    (b"big5hkscs", b"BIG5-HKSCS\0"),
];

// ---------------------------------------------------------------------------
// Non-Apple locale guess table (from R's `guess[]` table)
// ---------------------------------------------------------------------------

/// Mapping from locale name (without encoding) to encoding name.
/// Only used on non-Apple, non-Windows platforms.
#[cfg(not(target_os = "macos"))]
static GUESS: &[(&[u8], &[u8])] = &[
    (b"Cextend", b"ISO8859-1\0"),
    (b"English_United-States.437", b"C\0"),
    (b"ISO-8859-1", b"ISO8859-1\0"),
    (b"ISO8859-1", b"ISO8859-1\0"),
    (b"Japanese-EUC", b"EUC-JP\0"),
    (b"Jp_JP", b"EUC-JP\0"),
    (b"POSIX", b"C\0"),
    (b"POSIX-UTF2", b"C\0"),
    (b"aa_DJ", b"ISO8859-1\0"),
    (b"aa_ER", b"UTF-8\0"),
    (b"aa_ER@saaho", b"UTF-8\0"),
    (b"aa_ET", b"UTF-8\0"),
    (b"af", b"ISO8859-1\0"),
    (b"af_ZA", b"ISO8859-1\0"),
    (b"am", b"UTF-8\0"),
    (b"am_ET", b"UTF-8\0"),
    (b"an_ES", b"ISO8859-15\0"),
    (b"ar", b"ISO8859-6\0"),
    (b"ar_AA", b"ISO8859-6\0"),
    (b"ar_AE", b"ISO8859-6\0"),
    (b"ar_BH", b"ISO8859-6\0"),
    (b"ar_DZ", b"ISO8859-6\0"),
    (b"ar_EG", b"ISO8859-6\0"),
    (b"ar_IN", b"UTF-8\0"),
    (b"ar_IQ", b"ISO8859-6\0"),
    (b"ar_JO", b"ISO8859-6\0"),
    (b"ar_KW", b"ISO8859-6\0"),
    (b"ar_LB", b"ISO8859-6\0"),
    (b"ar_LY", b"ISO8859-6\0"),
    (b"ar_MA", b"ISO8859-6\0"),
    (b"ar_OM", b"ISO8859-6\0"),
    (b"ar_QA", b"ISO8859-6\0"),
    (b"ar_SA", b"ISO8859-6\0"),
    (b"ar_SD", b"ISO8859-6\0"),
    (b"ar_SY", b"ISO8859-6\0"),
    (b"ar_TN", b"ISO8859-6\0"),
    (b"ar_YE", b"ISO8859-6\0"),
    (b"be", b"CP1251\0"),
    (b"be_BY", b"CP1251\0"),
    (b"bg", b"CP1251\0"),
    (b"bg_BG", b"CP1251\0"),
    (b"bn_BD", b"UTF-8\0"),
    (b"bn_IN", b"UTF-8\0"),
    (b"bokmal", b"ISO8859-1\0"),
    (b"br", b"ISO8859-1\0"),
    (b"br_FR", b"ISO8859-1\0"),
    (b"br_FR@euro", b"ISO8859-15\0"),
    (b"bs_BA", b"ISO8859-2\0"),
    (b"bulgarian", b"CP1251\0"),
    (b"byn_ER", b"UTF-8\0"),
    (b"c-french.iso88591", b"ISO8859-1\0"),
    (b"ca", b"ISO8859-1\0"),
    (b"ca_ES", b"ISO8859-1\0"),
    (b"ca_ES@euro", b"ISO8859-15\0"),
    (b"catalan", b"ISO8859-1\0"),
    (b"chinese-s", b"EUC-CN\0"),
    (b"chinese-t", b"EUC-TW\0"),
    (b"croatian", b"ISO8859-2\0"),
    (b"cs", b"ISO8859-2\0"),
    (b"cs_CS", b"ISO8859-2\0"),
    (b"cs_CZ", b"ISO8859-2\0"),
    (b"cy", b"ISO8859-1\0"),
    (b"cy_GB", b"ISO8859-1\0"),
    (b"cz", b"ISO8859-2\0"),
    (b"cz_CZ", b"ISO8859-2\0"),
    (b"czech", b"ISO8859-2\0"),
    (b"da", b"ISO8859-1\0"),
    (b"da_DK", b"ISO8859-1\0"),
    (b"danish", b"ISO8859-1\0"),
    (b"dansk", b"ISO8859-1\0"),
    (b"de", b"ISO8859-1\0"),
    (b"de_AT", b"ISO8859-1\0"),
    (b"de_AT@euro", b"ISO8859-15\0"),
    (b"de_BE", b"ISO8859-1\0"),
    (b"de_BE@euro", b"ISO8859-15\0"),
    (b"de_CH", b"ISO8859-1\0"),
    (b"de_DE", b"ISO8859-1\0"),
    (b"de_DE@euro", b"ISO8859-15\0"),
    (b"de_LI", b"ISO8859-1\0"),
    (b"de_LI@euro", b"ISO8859-15\0"),
    (b"de_LU", b"ISO8859-1\0"),
    (b"de_LU@euro", b"ISO8859-15\0"),
    (b"deutsch", b"ISO8859-1\0"),
    (b"dutch", b"ISO8859-1\0"),
    (b"eesti", b"ISO8859-1\0"),
    (b"el", b"ISO8859-7\0"),
    (b"el_GR", b"ISO8859-7\0"),
    (b"en", b"ISO8859-1\0"),
    (b"en_AU", b"ISO8859-1\0"),
    (b"en_BW", b"ISO8859-1\0"),
    (b"en_CA", b"ISO8859-1\0"),
    (b"en_DK", b"ISO8859-1\0"),
    (b"en_GB", b"ISO8859-1\0"),
    (b"en_HK", b"ISO8859-1\0"),
    (b"en_IE", b"ISO8859-1\0"),
    (b"en_IE@euro", b"ISO8859-15\0"),
    (b"en_IN", b"UTF-8\0"),
    (b"en_NZ", b"ISO8859-1\0"),
    (b"en_PH", b"ISO8859-1\0"),
    (b"en_SG", b"ISO8859-1\0"),
    (b"en_UK", b"ISO8859-1\0"),
    (b"en_US", b"ISO8859-1\0"),
    (b"en_ZA", b"ISO8859-1\0"),
    (b"en_ZW", b"ISO8859-1\0"),
    (b"es", b"ISO8859-1\0"),
    (b"es_AR", b"ISO8859-1\0"),
    (b"es_BO", b"ISO8859-1\0"),
    (b"es_CL", b"ISO8859-1\0"),
    (b"es_CO", b"ISO8859-1\0"),
    (b"es_CR", b"ISO8859-1\0"),
    (b"es_DO", b"ISO8859-1\0"),
    (b"es_EC", b"ISO8859-1\0"),
    (b"es_ES", b"ISO8859-1\0"),
    (b"es_ES@euro", b"ISO8859-15\0"),
    (b"es_GT", b"ISO8859-1\0"),
    (b"es_HN", b"ISO8859-1\0"),
    (b"es_MX", b"ISO8859-1\0"),
    (b"es_NI", b"ISO8859-1\0"),
    (b"es_PA", b"ISO8859-1\0"),
    (b"es_PE", b"ISO8859-1\0"),
    (b"es_PR", b"ISO8859-1\0"),
    (b"es_PY", b"ISO8859-1\0"),
    (b"es_SV", b"ISO8859-1\0"),
    (b"es_US", b"ISO8859-1\0"),
    (b"es_UY", b"ISO8859-1\0"),
    (b"es_VE", b"ISO8859-1\0"),
    (b"estonian", b"ISO8859-1\0"),
    (b"et", b"ISO8859-15\0"),
    (b"et_EE", b"ISO8859-15\0"),
    (b"eu", b"ISO8859-1\0"),
    (b"eu_ES", b"ISO8859-1\0"),
    (b"eu_ES@euro", b"ISO8859-15\0"),
    (b"eu_FR", b"ISO8859-1\0"),
    (b"eu_FR@euro", b"ISO8859-15\0"),
    (b"fa", b"UTF-8\0"),
    (b"fa_IR", b"UTF-8\0"),
    (b"fi", b"ISO8859-1\0"),
    (b"fi_FI", b"ISO8859-1\0"),
    (b"fi_FI@euro", b"ISO8859-15\0"),
    (b"finnish", b"ISO8859-1\0"),
    (b"fo", b"ISO8859-1\0"),
    (b"fo_FO", b"ISO8859-1\0"),
    (b"fr", b"ISO8859-1\0"),
    (b"fr_BE", b"ISO8859-1\0"),
    (b"fr_BE@euro", b"ISO8859-15\0"),
    (b"fr_CA", b"ISO8859-1\0"),
    (b"fr_CH", b"ISO8859-1\0"),
    (b"fr_FR", b"ISO8859-1\0"),
    (b"fr_FR@euro", b"ISO8859-15\0"),
    (b"fr_LU", b"ISO8859-1\0"),
    (b"fr_LU@euro", b"ISO8859-15\0"),
    (b"french", b"ISO8859-1\0"),
    (b"ga", b"ISO8859-1\0"),
    (b"ga_IE", b"ISO8859-1\0"),
    (b"ga_IE@euro", b"ISO8859-15\0"),
    (b"galego", b"ISO8859-1\0"),
    (b"galician", b"ISO8859-1\0"),
    (b"gd", b"ISO8859-1\0"),
    (b"gd_GB", b"ISO8859-1\0"),
    (b"german", b"ISO8859-1\0"),
    (b"gez_ER", b"UTF-8\0"),
    (b"gez_ER@abegede", b"UTF-8\0"),
    (b"gez_ET", b"UTF-8\0"),
    (b"gez_ET@abegede", b"UTF-8\0"),
    (b"gl", b"ISO8859-1\0"),
    (b"gl_ES", b"ISO8859-1\0"),
    (b"gl_ES@euro", b"ISO8859-15\0"),
    (b"greek", b"ISO8859-7\0"),
    (b"gu_IN", b"UTF-8\0"),
    (b"gv", b"ISO8859-1\0"),
    (b"gv_GB", b"ISO8859-1\0"),
    (b"he", b"ISO8859-8\0"),
    (b"he_IL", b"ISO8859-8\0"),
    (b"hebrew", b"ISO8859-8\0"),
    (b"hr", b"ISO8859-2\0"),
    (b"hr_HR", b"ISO8859-2\0"),
    (b"hrvatski", b"ISO8859-2\0"),
    (b"hu", b"ISO8859-2\0"),
    (b"hu_HU", b"ISO8859-2\0"),
    (b"hungarian", b"ISO8859-2\0"),
    (b"hy", b"ARMSCII-8\0"),
    (b"hy_AM", b"ARMSCII-8\0"),
    (b"icelandic", b"ISO8859-1\0"),
    (b"id", b"ISO8859-1\0"),
    (b"id_ID", b"ISO8859-1\0"),
    (b"in", b"ISO8859-1\0"),
    (b"in_ID", b"ISO8859-1\0"),
    (b"is", b"ISO8859-1\0"),
    (b"is_IS", b"ISO8859-1\0"),
    (b"iso_8859_1", b"ISO8859-1\0"),
    (b"it", b"ISO8859-1\0"),
    (b"it_CH", b"ISO8859-1\0"),
    (b"it_IT", b"ISO8859-1\0"),
    (b"it_IT@euro", b"ISO8859-15\0"),
    (b"italian", b"ISO8859-1\0"),
    (b"iw", b"ISO8859-8\0"),
    (b"iw_IL", b"ISO8859-8\0"),
    (b"ja", b"EUC-JP\0"),
    (b"ja_JP", b"EUC-JP\0"),
    (b"japan", b"EUC-JP\0"),
    (b"japanese", b"EUC-JP\0"),
    (b"ka", b"GEORGIAN-ACADEMY\0"),
    (b"ka_GE", b"GEORGIAN-ACADEMY\0"),
    (b"kl", b"ISO8859-1\0"),
    (b"kl_GL", b"ISO8859-1\0"),
    (b"kn_IN", b"UTF-8\0"),
    (b"ko", b"EUC-KR\0"),
    (b"ko_KR", b"EUC-KR\0"),
    (b"korean", b"EUC-KR\0"),
    (b"kw", b"ISO8859-1\0"),
    (b"kw_GB", b"ISO8859-1\0"),
    (b"lg_UG", b"ISO8859-10\0"),
    (b"lithuanian", b"ISO8859-13\0"),
    (b"lt", b"ISO8859-13\0"),
    (b"lt_LT", b"ISO8859-13\0"),
    (b"lv", b"ISO8859-13\0"),
    (b"lv_LV", b"ISO8859-13\0"),
    (b"mi", b"ISO8859-13\0"),
    (b"mi_NZ", b"ISO8859-13\0"),
    (b"mk", b"ISO8859-5\0"),
    (b"mk_MK", b"ISO8859-5\0"),
    (b"ml_IN", b"UTF-8\0"),
    (b"mn_MN", b"UTF-8\0"),
    (b"mr_IN", b"UTF-8\0"),
    (b"ms", b"ISO8859-1\0"),
    (b"ms_MY", b"ISO8859-1\0"),
    (b"mt", b"ISO8859-3\0"),
    (b"mt_MT", b"ISO8859-3\0"),
    (b"nb", b"ISO8859-1\0"),
    (b"nb_NO", b"ISO8859-1\0"),
    (b"ne_NP", b"UTF-8\0"),
    (b"nl", b"ISO8859-1\0"),
    (b"nl_BE", b"ISO8859-1\0"),
    (b"nl_BE@euro", b"ISO8859-15\0"),
    (b"nl_NL", b"ISO8859-1\0"),
    (b"nl_NL@euro", b"ISO8859-15\0"),
    (b"nn", b"ISO8859-1\0"),
    (b"nn_NO", b"ISO8859-1\0"),
    (b"no", b"ISO8859-1\0"),
    (b"no@nynorsk", b"ISO8859-1\0"),
    (b"no_NO", b"ISO8859-1\0"),
    (b"norwegian", b"ISO8859-1\0"),
    (b"nynorsk", b"ISO8859-1\0"),
    (b"oc", b"ISO8859-1\0"),
    (b"oc_FR", b"ISO8859-1\0"),
    (b"oc_FR@euro", b"ISO8859-15\0"),
    (b"om_ET", b"UTF-8\0"),
    (b"om_KE", b"ISO8859-1\0"),
    (b"pa_IN", b"UTF-8\0"),
    (b"ph", b"ISO8859-1\0"),
    (b"ph_PH", b"ISO8859-1\0"),
    (b"pl", b"ISO8859-2\0"),
    (b"pl_PL", b"ISO8859-2\0"),
    (b"polish", b"ISO8859-2\0"),
    (b"portuguese", b"ISO8859-1\0"),
    (b"pp", b"ISO8859-1\0"),
    (b"pp_AN", b"ISO8859-1\0"),
    (b"pt", b"ISO8859-1\0"),
    (b"pt_BR", b"ISO8859-1\0"),
    (b"pt_PT", b"ISO8859-1\0"),
    (b"pt_PT@euro", b"ISO8859-15\0"),
    (b"ro", b"ISO8859-2\0"),
    (b"ro_RO", b"ISO8859-2\0"),
    (b"romanian", b"ISO8859-2\0"),
    (b"ru", b"KOI8-R\0"),
    (b"ru_RU", b"KOI8-R\0"),
    (b"ru_UA", b"KOI8-U\0"),
    (b"rumanian", b"ISO8859-2\0"),
    (b"russian", b"ISO8859-5\0"),
    (b"se_NO", b"UTF-8\0"),
    (b"serbocroatian", b"ISO8859-2\0"),
    (b"sh", b"ISO8859-2\0"),
    (b"sh_SP", b"ISO8859-2\0"),
    (b"sh_YU", b"ISO8859-2\0"),
    (b"sid_ET", b"UTF-8\0"),
    (b"sk", b"ISO8859-2\0"),
    (b"sk_SK", b"ISO8859-2\0"),
    (b"sl", b"ISO8859-2\0"),
    (b"sl_SI", b"ISO8859-2\0"),
    (b"slovak", b"ISO8859-2\0"),
    (b"slovene", b"ISO8859-2\0"),
    (b"slovenian", b"ISO8859-2\0"),
    (b"so_DJ", b"ISO8859-1\0"),
    (b"so_ET", b"UTF-8\0"),
    (b"so_KE", b"ISO8859-1\0"),
    (b"so_SO", b"ISO8859-1\0"),
    (b"sp", b"ISO8859-5\0"),
    (b"sp_YU", b"ISO8859-5\0"),
    (b"spanish", b"ISO8859-1\0"),
    (b"sq", b"ISO8859-2\0"),
    (b"sq_AL", b"ISO8859-2\0"),
    (b"sr", b"ISO8859-5\0"),
    (b"sr@cyrillic", b"ISO8859-5\0"),
    (b"sr_SP", b"ISO8859-2\0"),
    (b"sr_YU", b"ISO8859-5\0"),
    (b"sr_YU@cyrillic", b"ISO8859-5\0"),
    (b"st_ZA", b"ISO8859-1\0"),
    (b"sv", b"ISO8859-1\0"),
    (b"sv_FI", b"ISO8859-1\0"),
    (b"sv_FI@euro", b"ISO8859-15\0"),
    (b"sv_SE", b"ISO8859-1\0"),
    (b"sv_SE@euro", b"ISO8859-15\0"),
    (b"swedish", b"ISO8859-1\0"),
    (b"te_IN", b"UTF-8\0"),
    (b"th", b"ISO8859-11\0"),
    (b"th_TH", b"ISO8859-11\0"),
    (b"thai", b"ISO8859-11\0"),
    (b"ti_ER", b"UTF-8\0"),
    (b"ti_ET", b"UTF-8\0"),
    (b"tig_ER", b"UTF-8\0"),
    (b"tl", b"ISO8859-1\0"),
    (b"tl_PH", b"ISO8859-1\0"),
    (b"tr", b"ISO8859-9\0"),
    (b"tr_TR", b"ISO8859-9\0"),
    (b"turkish", b"ISO8859-9\0"),
    (b"uk", b"KOI8-U\0"),
    (b"uk_UA", b"KOI8-U\0"),
    (b"ur", b"CP1256\0"),
    (b"ur_PK", b"CP1256\0"),
    (b"uz_UZ", b"ISO8859-1\0"),
    (b"uz_UZ@cyrillic", b"UTF-8\0"),
    (b"vi", b"TCVN\0"),
    (b"vi_VN", b"TCVN\0"),
    (b"wa", b"ISO8859-1\0"),
    (b"wa_BE", b"ISO8859-1\0"),
    (b"wa_BE@euro", b"ISO8859-15\0"),
    (b"xh_ZA", b"ISO8859-1\0"),
    (b"yi", b"CP1255\0"),
    (b"yi_US", b"CP1255\0"),
    (b"zh_CN", b"GBK\0"),
    (b"zh_HK", b"BIG5-HKSCS\0"),
    (b"zh_SG", b"GB2312\0"),
    (b"zh_TW", b"BIG5\0"),
    (b"zu_ZA", b"ISO8859-1\0"),
];

// ---------------------------------------------------------------------------
// Helper: binary search in guess table (non-Apple only)
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "macos"))]
fn guess_lookup(name: &[u8]) -> Option<&'static [u8]> {
    let mut min = 0usize;
    let max = GUESS.len();
    if max == 0 {
        return None;
    }
    // Check bounds
    if name < GUESS[0].0 || name > GUESS[max - 1].0 {
        return None;
    }
    let mut lo = 0;
    let mut hi = max - 1;
    while lo <= hi {
        let mid = (lo + hi) / 2;
        if GUESS[mid].0 < name {
            lo = mid + 1;
        } else if GUESS[mid].0 > name {
            if mid == 0 {
                break;
            }
            hi = mid - 1;
        } else {
            return Some(GUESS[mid].1);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// locale2charset -- the main function
// ---------------------------------------------------------------------------

/// Map a locale string to a character encoding name.
///
/// This is the equivalent of R's `locale2charset()` from localecharset.c.
/// Note: the C-visible `locale2charset` symbol is exported from
/// `cport::localecharset`; this is the module-private counterpart.
///
/// # Arguments
/// * `locale` - The locale string (e.g., "en_US.UTF-8"). If NULL or "NULL",
///   uses the current locale from setlocale.
///
/// # Returns
/// * "ASCII" for C/POSIX locales
/// * "UTF-8" for macOS locales without encoding part
/// * The appropriate encoding name otherwise
#[allow(unreachable_code)]
pub unsafe fn locale2charset(locale: *const c_char) -> *const c_char {
    static mut CHARSET_BUF: [u8; 128] = [0; 128];

    let locale_str = if locale.is_null() || {
        let s = CStr::from_ptr(locale).to_str().unwrap_or("");
        s == "NULL"
    } {
        // Get current locale
        match CStr::from_ptr(libc::setlocale(libc::LC_CTYPE, std::ptr::null())).to_str() {
            Ok(s) => s,
            Err(_) => return b"ASCII\0".as_ptr() as *const c_char,
        }
    } else {
        match CStr::from_ptr(locale).to_str() {
            Ok(s) => s,
            Err(_) => return b"ASCII\0".as_ptr() as *const c_char,
        }
    };

    if locale_str.is_empty() || locale_str == "C" || locale_str == "POSIX" {
        return b"ASCII\0".as_ptr() as *const c_char;
    }

    // Separate language_locale.encoding
    let (la_loc, enc) = if let Some(dot_pos) = locale_str.rfind('.') {
        let (la, en) = locale_str.split_at(dot_pos);
        (la, &en[1..])
    } else {
        // No encoding part
        #[cfg(target_os = "macos")]
        {
            // On macOS, all real locales without encoding are UTF-8
            return b"UTF-8\0".as_ptr() as *const c_char;
        }
        #[cfg(not(target_os = "macos"))]
        {
            // On non-macOS, look up locale name in guess table
            let la_bytes = la_loc.as_bytes();
            if let Some(value) = guess_lookup(la_bytes) {
                return value.as_ptr() as *const c_char;
            }
            return b"ASCII\0".as_ptr() as *const c_char;
        }
    };

    // Check for UTF-8 variants
    if enc.eq_ignore_ascii_case("UTF-8") || enc.eq_ignore_ascii_case("UTF8") {
        return b"UTF-8\0".as_ptr() as *const c_char;
    }

    // For AIX: "UTF-8" is mapped to "utf8"
    let enc = if enc == "UTF-8" { "utf8" } else { enc };

    if enc.is_empty() {
        return b"UTF-8\0".as_ptr() as *const c_char;
    }

    // Check if this is utf8 (after AIX normalization)
    if enc == "utf8" {
        return b"UTF-8\0".as_ptr() as *const c_char;
    }

    // Look up encoding in known table (case-insensitive)
    let enc_lower: Vec<u8> = enc.bytes().map(|b| b.to_ascii_lowercase()).collect();
    for (name, value) in KNOWN.iter() {
        if enc_lower == *name {
            return value.as_ptr() as *const c_char;
        }
    }

    // Check for cp- prefix
    if enc_lower.starts_with(b"cp-") {
        let cp_num = &enc[3..];
        let result = format!("CP{}", cp_num);
        let buf = std::ptr::addr_of_mut!(CHARSET_BUF) as *mut u8;
        let bytes = result.as_bytes();
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
        *buf.add(result.len()) = 0;
        return buf.cast::<c_char>();
    }

    // Check for ibm- prefix (IBM codepages)
    if enc_lower.starts_with(b"ibm") {
        let rest = &enc[3..];
        // Try to parse as number (IBM-XXXX)
        let num_str = if rest.starts_with('-') {
            &rest[1..]
        } else {
            rest
        };
        if let Ok(_cp) = num_str.parse::<i32>() {
            if _cp != 0 {
                // IBM-NNNN case
                let result = format!("IBM-{}", _cp.abs());
                let buf = std::ptr::addr_of_mut!(CHARSET_BUF) as *mut u8;
                let bytes = result.as_bytes();
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
                *buf.add(result.len()) = 0;
                return buf.cast::<c_char>();
            }
        }
        // IBM-eucXX case
        let euc_str = if rest.starts_with('-') {
            &rest[1..]
        } else {
            rest
        };
        if euc_str.starts_with("euc") && euc_str.len() > 3 {
            let mut result = String::from("EUC-");
            result.push_str(&euc_str[3..].to_uppercase());
            let buf = std::ptr::addr_of_mut!(CHARSET_BUF) as *mut u8;
            let bytes = result.as_bytes();
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
            *buf.add(result.len()) = 0;
            return buf.cast::<c_char>();
        }
    }

    // Fallback for euc encoding based on language
    if enc_lower == b"euc" {
        if la_loc.starts_with("ja") {
            return b"EUC-JP\0".as_ptr() as *const c_char;
        } else if la_loc.starts_with("ko") {
            return b"EUC-KR\0".as_ptr() as *const c_char;
        } else if la_loc.starts_with("zh") {
            return b"GB2312\0".as_ptr() as *const c_char;
        }
    }

    // On macOS, if we reach here, return UTF-8
    #[cfg(target_os = "macos")]
    {
        return b"UTF-8\0".as_ptr() as *const c_char;
    }

    // On non-macOS, try the guess table
    #[cfg(not(target_os = "macos"))]
    {
        let la_bytes = la_loc.as_bytes();
        if let Some(value) = guess_lookup(la_bytes) {
            return value.as_ptr() as *const c_char;
        }
    }

    // Default fallback
    b"ASCII\0".as_ptr() as *const c_char
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_locale() {
        unsafe {
            let c_locale = std::ffi::CString::new("C").unwrap();
            let result = CStr::from_ptr(locale2charset(c_locale.as_ptr()));
            assert_eq!(result.to_str().unwrap(), "ASCII");
        }
    }

    #[test]
    fn test_posix_locale() {
        unsafe {
            let locale = std::ffi::CString::new("POSIX").unwrap();
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap(), "ASCII");
        }
    }

    #[test]
    fn test_utf8_locale() {
        unsafe {
            let locale = std::ffi::CString::new("en_US.UTF-8").unwrap();
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap(), "UTF-8");
        }
    }

    #[test]
    fn test_utf8_uppercase_locale() {
        unsafe {
            let locale = std::ffi::CString::new("en_US.UTF8").unwrap();
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap(), "UTF-8");
        }
    }

    #[test]
    fn test_known_encoding_iso88591() {
        unsafe {
            let locale = std::ffi::CString::new("en_US.ISO8859-1").unwrap();
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap(), "ISO8859-1");
        }
    }

    #[test]
    fn test_known_encoding_big5() {
        unsafe {
            let locale = std::ffi::CString::new("zh_TW.BIG5").unwrap();
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap(), "BIG5");
        }
    }

    #[test]
    fn test_known_encoding_gb2312() {
        unsafe {
            let locale = std::ffi::CString::new("zh_CN.GB2312").unwrap();
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap(), "GB2312");
        }
    }

    #[test]
    fn test_euc_jp() {
        unsafe {
            let locale = std::ffi::CString::new("ja_JP.eucJP").unwrap();
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap(), "EUC-JP");
        }
    }

    #[test]
    fn test_euc_kr() {
        unsafe {
            let locale = std::ffi::CString::new("ko_KR.eucKR").unwrap();
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap(), "EUC-KR");
        }
    }

    #[test]
    fn test_cp_prefix() {
        unsafe {
            let locale = std::ffi::CString::new("ru_RU.CP-1251").unwrap();
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap(), "CP1251");
        }
    }

    #[test]
    fn test_euc_fallback_by_language() {
        unsafe {
            let locale = std::ffi::CString::new("ja_JP.euc").unwrap();
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap(), "EUC-JP");
        }
    }

    #[test]
    fn test_empty_locale() {
        unsafe {
            let locale = std::ffi::CString::new("").unwrap();
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap(), "ASCII");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_macos_locale_no_encoding() {
        unsafe {
            let locale = std::ffi::CString::new("en_US").unwrap();
            let result = CStr::from_ptr(locale2charset(locale.as_ptr()));
            assert_eq!(result.to_str().unwrap(), "UTF-8");
        }
    }
}
