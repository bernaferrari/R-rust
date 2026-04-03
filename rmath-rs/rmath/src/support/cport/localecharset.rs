//! Port of R's src/main/localecharset.c
//!
//! Converts locale names to charset/encoding names.
//! Original by Ei-ji Nakama, Copyright (C) 2005-2021 The R Core Team (GPL).
//!
//! Usage: locale2charset(locale) -> charset name string
//! Returns "ASCII" for default/unknown locales.

#![allow(non_snake_case)]

use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr;

// ---------------------------------------------------------------------------
// Encoding name constants
// ---------------------------------------------------------------------------

static mut ENC_ARMSCII_8: &[u8] = b"ARMSCII-8";
static mut ENC_BIG5: &[u8] = b"BIG5";
static mut ENC_BIG5_HKSCS: &[u8] = b"BIG5-HKSCS";
static mut ENC_C: &[u8] = b"C";
static mut ENC_CP1251: &[u8] = b"CP1251";
static mut ENC_CP1255: &[u8] = b"CP1255";
static mut ENC_CP1256: &[u8] = b"CP1256";
static mut ENC_EUC_CN: &[u8] = b"EUC-CN";
static mut ENC_EUC_JP: &[u8] = b"EUC-JP";
static mut ENC_EUC_KR: &[u8] = b"EUC-KR";
static mut ENC_EUC_TW: &[u8] = b"EUC-TW";
static mut ENC_GB2312: &[u8] = b"GB2312";
static mut ENC_GBK: &[u8] = b"GBK";
static mut ENC_GEORGIAN_ACADEMY: &[u8] = b"GEORGIAN-ACADEMY";
static mut ENC_ISO8859_1: &[u8] = b"ISO8859-1";
static mut ENC_ISO8859_10: &[u8] = b"ISO8859-10";
static mut ENC_ISO8859_11: &[u8] = b"ISO8859-11";
static mut ENC_ISO8859_13: &[u8] = b"ISO8859-13";
static mut ENC_ISO8859_15: &[u8] = b"ISO8859-15";
static mut ENC_ISO8859_2: &[u8] = b"ISO8859-2";
static mut ENC_ISO8859_3: &[u8] = b"ISO8859-3";
static mut ENC_ISO8859_5: &[u8] = b"ISO8859-5";
static mut ENC_ISO8859_6: &[u8] = b"ISO8859-6";
static mut ENC_ISO8859_7: &[u8] = b"ISO8859-7";
static mut ENC_ISO8859_8: &[u8] = b"ISO8859-8";
static mut ENC_ISO8859_9: &[u8] = b"ISO8859-9";
static mut ENC_KOI8_R: &[u8] = b"KOI8-R";
static mut ENC_KOI8_U: &[u8] = b"KOI8-U";
static mut ENC_TCVN: &[u8] = b"TCVN";
static mut ENC_UTF_8: &[u8] = b"UTF-8";

// ---------------------------------------------------------------------------
// Lookup table for locale -> charset guessing (non-Apple platforms)
// ---------------------------------------------------------------------------

struct NameValue {
    name: &'static [u8],
    value: *const u8,
}

#[cfg(not(target_os = "macos"))]
static GUESS: &[NameValue] = &[
    // Sorted by name for binary search
    nv(b"Cextend", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"English_United-States.437", || unsafe { ENC_C.as_ptr() }),
    nv(b"ISO-8859-1", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"ISO8859-1", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"Japanese-EUC", || unsafe { ENC_EUC_JP.as_ptr() }),
    nv(b"Jp_JP", || unsafe { ENC_EUC_JP.as_ptr() }),
    nv(b"POSIX", || unsafe { ENC_C.as_ptr() }),
    nv(b"POSIX-UTF2", || unsafe { ENC_C.as_ptr() }),
    nv(b"aa_DJ", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"aa_ER", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"aa_ET", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"af", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"af_ZA", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"am", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"am_ET", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"an_ES", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"ar", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_AA", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_AE", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_BH", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_DZ", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_EG", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_IN", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"ar_IQ", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_JO", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_KW", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_LB", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_LY", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_MA", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_OM", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_QA", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_SA", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_SD", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_SY", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_TN", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"ar_YE", || unsafe { ENC_ISO8859_6.as_ptr() }),
    nv(b"be", || unsafe { ENC_CP1251.as_ptr() }),
    nv(b"be_BY", || unsafe { ENC_CP1251.as_ptr() }),
    nv(b"bg", || unsafe { ENC_CP1251.as_ptr() }),
    nv(b"bg_BG", || unsafe { ENC_CP1251.as_ptr() }),
    nv(b"bn_BD", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"bn_IN", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"br", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"br_FR", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"br_FR@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"bs_BA", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"ca", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"ca_ES", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"ca_ES@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"chinese-s", || unsafe { ENC_EUC_CN.as_ptr() }),
    nv(b"chinese-t", || unsafe { ENC_EUC_TW.as_ptr() }),
    nv(b"cs", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"cs_CZ", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"cy", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"cy_GB", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"cz", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"da", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"da_DK", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"de", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"de_AT", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"de_AT@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"de_BE", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"de_BE@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"de_CH", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"de_DE", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"de_DE@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"de_LI", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"de_LI@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"de_LU", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"de_LU@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"el", || unsafe { ENC_ISO8859_7.as_ptr() }),
    nv(b"el_GR", || unsafe { ENC_ISO8859_7.as_ptr() }),
    nv(b"en", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"en_AU", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"en_BW", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"en_CA", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"en_DK", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"en_GB", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"en_HK", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"en_IE", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"en_IE@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"en_IN", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"en_NZ", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"en_PH", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"en_SG", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"en_UK", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"en_US", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"en_ZA", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"en_ZW", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_AR", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_BO", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_CL", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_CO", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_CR", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_DO", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_EC", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_ES", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_ES@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"es_GT", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_HN", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_MX", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_NI", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_PA", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_PE", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_PR", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_PY", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_SV", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_US", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_UY", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"es_VE", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"et", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"et_EE", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"eu", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"eu_ES", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"eu_ES@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"eu_FR", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"eu_FR@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"fa", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"fa_IR", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"fi", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"fi_FI", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"fi_FI@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"fo", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"fo_FO", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"fr", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"fr_BE", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"fr_BE@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"fr_CA", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"fr_CH", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"fr_FR", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"fr_FR@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"fr_LU", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"fr_LU@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"ga", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"ga_IE", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"ga_IE@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"gd", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"gd_GB", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"gez_ER", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"gez_ET", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"gl", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"gl_ES", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"gl_ES@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"gu_IN", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"gv", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"gv_GB", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"he", || unsafe { ENC_ISO8859_8.as_ptr() }),
    nv(b"he_IL", || unsafe { ENC_ISO8859_8.as_ptr() }),
    nv(b"hr", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"hr_HR", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"hu", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"hu_HU", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"hy", || unsafe { ENC_ARMSCII_8.as_ptr() }),
    nv(b"hy_AM", || unsafe { ENC_ARMSCII_8.as_ptr() }),
    nv(b"id", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"id_ID", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"is", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"is_IS", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"it", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"it_CH", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"it_IT", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"it_IT@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"ja", || unsafe { ENC_EUC_JP.as_ptr() }),
    nv(b"ja_JP", || unsafe { ENC_EUC_JP.as_ptr() }),
    nv(b"ka", || unsafe { ENC_GEORGIAN_ACADEMY.as_ptr() }),
    nv(b"ka_GE", || unsafe { ENC_GEORGIAN_ACADEMY.as_ptr() }),
    nv(b"kl", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"kl_GL", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"kn_IN", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"ko", || unsafe { ENC_EUC_KR.as_ptr() }),
    nv(b"ko_KR", || unsafe { ENC_EUC_KR.as_ptr() }),
    nv(b"kw", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"kw_GB", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"lg_UG", || unsafe { ENC_ISO8859_10.as_ptr() }),
    nv(b"lt", || unsafe { ENC_ISO8859_13.as_ptr() }),
    nv(b"lt_LT", || unsafe { ENC_ISO8859_13.as_ptr() }),
    nv(b"lv", || unsafe { ENC_ISO8859_13.as_ptr() }),
    nv(b"lv_LV", || unsafe { ENC_ISO8859_13.as_ptr() }),
    nv(b"mi", || unsafe { ENC_ISO8859_13.as_ptr() }),
    nv(b"mi_NZ", || unsafe { ENC_ISO8859_13.as_ptr() }),
    nv(b"mk", || unsafe { ENC_ISO8859_5.as_ptr() }),
    nv(b"mk_MK", || unsafe { ENC_ISO8859_5.as_ptr() }),
    nv(b"ml_IN", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"mn_MN", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"mr_IN", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"ms", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"ms_MY", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"mt", || unsafe { ENC_ISO8859_3.as_ptr() }),
    nv(b"mt_MT", || unsafe { ENC_ISO8859_3.as_ptr() }),
    nv(b"nb", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"nb_NO", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"ne_NP", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"nl", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"nl_BE", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"nl_BE@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"nl_NL", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"nl_NL@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"nn", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"nn_NO", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"no", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"no_NO", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"oc", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"oc_FR", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"oc_FR@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"om_ET", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"om_KE", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"pa_IN", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"ph", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"ph_PH", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"pl", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"pl_PL", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"pt", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"pt_BR", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"pt_PT", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"pt_PT@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"ro", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"ro_RO", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"ru", || unsafe { ENC_KOI8_R.as_ptr() }),
    nv(b"ru_RU", || unsafe { ENC_KOI8_R.as_ptr() }),
    nv(b"ru_UA", || unsafe { ENC_KOI8_U.as_ptr() }),
    nv(b"se_NO", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"sh", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"sh_SP", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"sh_YU", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"sid_ET", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"sk", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"sk_SK", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"sl", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"sl_SI", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"so_DJ", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"so_ET", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"so_KE", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"so_SO", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"sq", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"sq_AL", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"sr", || unsafe { ENC_ISO8859_5.as_ptr() }),
    nv(b"sr_SP", || unsafe { ENC_ISO8859_2.as_ptr() }),
    nv(b"sr_YU", || unsafe { ENC_ISO8859_5.as_ptr() }),
    nv(b"st_ZA", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"sv", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"sv_FI", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"sv_FI@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"sv_SE", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"sv_SE@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"te_IN", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"th", || unsafe { ENC_ISO8859_11.as_ptr() }),
    nv(b"th_TH", || unsafe { ENC_ISO8859_11.as_ptr() }),
    nv(b"ti_ER", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"ti_ET", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"tig_ER", || unsafe { ENC_UTF_8.as_ptr() }),
    nv(b"tl", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"tl_PH", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"tr", || unsafe { ENC_ISO8859_9.as_ptr() }),
    nv(b"tr_TR", || unsafe { ENC_ISO8859_9.as_ptr() }),
    nv(b"uk", || unsafe { ENC_KOI8_U.as_ptr() }),
    nv(b"uk_UA", || unsafe { ENC_KOI8_U.as_ptr() }),
    nv(b"ur", || unsafe { ENC_CP1256.as_ptr() }),
    nv(b"ur_PK", || unsafe { ENC_CP1256.as_ptr() }),
    nv(b"uz_UZ", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"vi", || unsafe { ENC_TCVN.as_ptr() }),
    nv(b"vi_VN", || unsafe { ENC_TCVN.as_ptr() }),
    nv(b"wa", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"wa_BE", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"wa_BE@euro", || unsafe { ENC_ISO8859_15.as_ptr() }),
    nv(b"xh_ZA", || unsafe { ENC_ISO8859_1.as_ptr() }),
    nv(b"yi", || unsafe { ENC_CP1255.as_ptr() }),
    nv(b"yi_US", || unsafe { ENC_CP1255.as_ptr() }),
    nv(b"zh_CN", || unsafe { ENC_GBK.as_ptr() }),
    nv(b"zh_HK", || unsafe { ENC_BIG5_HKSCS.as_ptr() }),
    nv(b"zh_SG", || unsafe { ENC_GB2312.as_ptr() }),
    nv(b"zh_TW", || unsafe { ENC_BIG5.as_ptr() }),
    nv(b"zu_ZA", || unsafe { ENC_ISO8859_1.as_ptr() }),
];

#[cfg(not(target_os = "macos"))]
const fn nv(name: &'static [u8], value: fn() -> *const u8) -> NameValue {
    NameValue {
        name,
        value: value(),
    }
}

// Known encoding mappings
static KNOWN: &[(&[u8], &[u8])] = &[
    (b"iso88591", b"ISO8859-1"),
    (b"iso88592", b"ISO8859-2"),
    (b"iso88593", b"ISO8859-3"),
    (b"iso88596", b"ISO8859-6"),
    (b"iso88597", b"ISO8859-7"),
    (b"iso88598", b"ISO8859-8"),
    (b"iso88599", b"ISO8859-9"),
    (b"iso885910", b"ISO8859-10"),
    (b"iso885913", b"ISO8859-13"),
    (b"iso885914", b"ISO8859-14"),
    (b"iso885915", b"ISO8859-15"),
    (b"cp1251", b"CP1251"),
    (b"cp1255", b"CP1255"),
    (b"eucjp", b"EUC-JP"),
    (b"euckr", b"EUC-KR"),
    (b"euctw", b"EUC-TW"),
    (b"georgianps", b"GEORGIAN-PS"),
    (b"koi8u", b"KOI8-U"),
    (b"tcvn", b"TCVN"),
    (b"big5", b"BIG5"),
    (b"gb2312", b"GB2312"),
    (b"gb18030", b"GB18030"),
    (b"gbk", b"GBK"),
    (b"tis-620", b"TIS-620"),
    (b"sjis", b"SHIFT_JIS"),
    (b"euccn", b"GB2312"),
    (b"big5-hkscs", b"BIG5-HKSCS"),
    #[cfg(target_os = "macos")]
    (b"iso8859-1", b"ISO8859-1"),
    #[cfg(target_os = "macos")]
    (b"iso8859-2", b"ISO8859-2"),
    #[cfg(target_os = "macos")]
    (b"iso8859-4", b"ISO8859-4"),
    #[cfg(target_os = "macos")]
    (b"iso8859-7", b"ISO8859-7"),
    #[cfg(target_os = "macos")]
    (b"iso8859-9", b"ISO8859-9"),
    #[cfg(target_os = "macos")]
    (b"iso8859-13", b"ISO8859-13"),
    #[cfg(target_os = "macos")]
    (b"iso8859-15", b"ISO8859-15"),
    #[cfg(target_os = "macos")]
    (b"koi8-u", b"KOI8-U"),
    #[cfg(target_os = "macos")]
    (b"koi8-r", b"KOI8-R"),
    #[cfg(target_os = "macos")]
    (b"pt154", b"PT154"),
    #[cfg(target_os = "macos")]
    (b"us-ascii", b"ASCII"),
    #[cfg(target_os = "macos")]
    (b"armscii-8", b"ARMSCII-8"),
    #[cfg(target_os = "macos")]
    (b"iscii-dev", b"ISCII-DEV"),
    #[cfg(target_os = "macos")]
    (b"big5hkscs", b"BIG5-HKSCS"),
];

// ---------------------------------------------------------------------------
// Binary search for name in sorted table (non-Apple only)
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "macos"))]
fn name_value_search(name: &[u8], table: &[NameValue]) -> Option<*const u8> {
    if table.is_empty() {
        return None;
    }
    if name < table[0].name || name > table[table.len() - 1].name {
        return None;
    }
    let mut min = 0usize;
    let mut max = table.len() - 1;
    while max >= min {
        let mid = (min + max) / 2;
        if name > table[mid].name {
            min = mid + 1;
        } else if name < table[mid].name {
            if mid == 0 {
                return None;
            }
            max = mid - 1;
        } else {
            return Some(table[mid].value);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// locale2charset - main entry point
// ---------------------------------------------------------------------------

/// Convert a locale name to a charset/encoding name.
///
/// Ported from R's src/main/localecharset.c.
/// Returns "ASCII" for C/POSIX/unknown locales.
/// On macOS, all real locales without an explicit encoding are UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn locale2charset(locale: *const c_char) -> *const c_char {
    unsafe {
        static mut CHARSET_BUF: [u8; 128] = [0u8; 128];

        let locale = if locale.is_null() {
            // Fall back to ASCII when locale is null and we can't query the system
            return b"ASCII\0".as_ptr() as *const c_char;
        } else {
            let s = CStr::from_ptr(locale).to_bytes();
            if s.is_empty() || s == b"NULL" {
                return b"ASCII\0".as_ptr() as *const c_char;
            }
            s
        };

        // C and POSIX -> ASCII
        if locale == b"C" || locale == b"POSIX" {
            return b"ASCII\0".as_ptr() as *const c_char;
        }

        // Find the encoding part after the last dot
        let mut enc: &[u8] = b"";
        let mut la_loc: &[u8] = locale;

        if let Some(dot_pos) = locale.iter().rposition(|&c| c == b'.') {
            enc = &locale[dot_pos + 1..];
            la_loc = &locale[..dot_pos];
        }

        // Check for UTF-8 in encoding part
        if !enc.is_empty() {
            let enc_lower: Vec<u8> = enc.iter().map(|&c| c.to_ascii_lowercase()).collect();
            if enc_lower == b"utf-8" || enc_lower == b"utf8" {
                return b"UTF-8\0".as_ptr() as *const c_char;
            }
        }

        // If there's an encoding part, try to match it against known encodings
        if !enc.is_empty() && !(enc.len() == 4 && &enc[..4] == b"utf8") {
            let enc_lower: Vec<u8> = enc.iter().map(|&c| c.to_ascii_lowercase()).collect();

            for (name, value) in KNOWN.iter() {
                if *name == enc_lower.as_slice() {
                    return value.as_ptr() as *const c_char;
                }
            }

            // Handle cp- prefix
            if enc_lower.starts_with(b"cp-") {
                let cp_str: String = enc_lower[3..].iter().map(|&c| c as char).collect();
                if let Ok(cp) = cp_str.parse::<u32>() {
                    let s = format!("CP{}", cp);
                    let buf = std::ptr::addr_of_mut!(CHARSET_BUF);
                    ptr::copy_nonoverlapping(s.as_ptr(), (*buf).as_mut_ptr(), s.len());
                    *(*buf).as_mut_ptr().add(s.len()) = 0;
                    return (*buf).as_ptr() as *const c_char;
                }
            }

            // Handle IBM- prefix
            if enc_lower.starts_with(b"ibm") {
                let ibm_str: String = enc_lower[3..].iter().map(|&c| c as char).collect();
                if let Ok(cp) = ibm_str.parse::<i32>() {
                    if cp != 0 {
                        let s = format!("IBM-{}", cp.abs());
                        let buf = std::ptr::addr_of_mut!(CHARSET_BUF);
                        ptr::copy_nonoverlapping(s.as_ptr(), (*buf).as_mut_ptr(), s.len());
                        *(*buf).as_mut_ptr().add(s.len()) = 0;
                        return (*buf).as_ptr() as *const c_char;
                    }
                }
                // IBM-eucXX case
                let euc_part = if enc_lower.len() > 3 && enc_lower[3] == b'-' {
                    &enc_lower[4..]
                } else {
                    &enc_lower[3..]
                };
                if euc_part.starts_with(b"euc") {
                    let mut charset_str = String::from("euc");
                    if euc_part.len() > 3 && euc_part[3] != b'-' {
                        charset_str.push('-');
                        charset_str.push_str(
                            &euc_part[4..]
                                .iter()
                                .map(|&c| c.to_ascii_uppercase() as char)
                                .collect::<String>(),
                        );
                    } else {
                        charset_str.push_str(
                            &euc_part[3..]
                                .iter()
                                .map(|&c| c.to_ascii_uppercase() as char)
                                .collect::<String>(),
                        );
                    }
                    let buf = std::ptr::addr_of_mut!(CHARSET_BUF);
                    ptr::copy_nonoverlapping(
                        charset_str.as_ptr(),
                        (*buf).as_mut_ptr(),
                        charset_str.len(),
                    );
                    *(*buf).as_mut_ptr().add(charset_str.len()) = 0;
                    return (*buf).as_ptr() as *const c_char;
                }
            }

            // Handle "euc" encoding
            if enc_lower == b"euc" {
                if la_loc.len() >= 3
                    && la_loc[0].is_ascii_alphabetic()
                    && la_loc[1].is_ascii_alphabetic()
                    && la_loc[2] == b'_'
                {
                    if &la_loc[..2] == b"ja" {
                        return b"EUC-JP\0".as_ptr() as *const c_char;
                    }
                    if &la_loc[..2] == b"ko" {
                        return b"EUC-KR\0".as_ptr() as *const c_char;
                    }
                    if &la_loc[..2] == b"zh" {
                        return b"GB2312\0".as_ptr() as *const c_char;
                    }
                }
            }
        }

        // Platform-specific behavior
        #[cfg(target_os = "macos")]
        {
            // On macOS, all real locales without encoding part are UTF-8
            b"UTF-8\0".as_ptr() as *const c_char
        }

        #[cfg(not(target_os = "macos"))]
        {
            // On non-Apple, try to look up the locale in the guess table
            if let Some(value) = name_value_search(la_loc, GUESS) {
                value as *const c_char
            } else {
                b"ASCII\0".as_ptr() as *const c_char
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null_locale() {
        unsafe {
            let result = locale2charset(std::ptr::null());
            let s = CStr::from_ptr(result).to_str().unwrap();
            assert_eq!(s, "ASCII");
        }
    }

    #[test]
    fn test_c_locale() {
        unsafe {
            let result = locale2charset(b"C\0".as_ptr() as *const c_char);
            let s = CStr::from_ptr(result).to_str().unwrap();
            assert_eq!(s, "ASCII");
        }
    }

    #[test]
    fn test_posix_locale() {
        unsafe {
            let result = locale2charset(b"POSIX\0".as_ptr() as *const c_char);
            let s = CStr::from_ptr(result).to_str().unwrap();
            assert_eq!(s, "ASCII");
        }
    }

    #[test]
    fn test_utf8_encoding() {
        unsafe {
            let result = locale2charset(b"en_US.UTF-8\0".as_ptr() as *const c_char);
            let s = CStr::from_ptr(result).to_str().unwrap();
            assert_eq!(s, "UTF-8");
        }
    }

    #[test]
    fn test_macos_locale() {
        unsafe {
            let result = locale2charset(b"en_US\0".as_ptr() as *const c_char);
            let s = CStr::from_ptr(result).to_str().unwrap();
            assert_eq!(s, "UTF-8");
        }
    }
}
