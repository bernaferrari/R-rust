//! Port of R's src/main/localecharset.c
//!
//! Converts locale names to charset/encoding names.
//! Original by Ei-ji Nakama, Copyright (C) 2005-2021 The R Core Team (GPL).
//!
//! Usage: locale2charset(locale) -> charset name string
//! Returns "ASCII" for default/unknown locales.

#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr;

// ---------------------------------------------------------------------------
// Encoding name constants
// ---------------------------------------------------------------------------

thread_local! { static ENC_ARMSCII_8: Cell<&[u8]> = Cell::new(b"ARMSCII-8"); }
thread_local! { static ENC_BIG5: Cell<&[u8]> = Cell::new(b"BIG5"); }
thread_local! { static ENC_BIG5_HKSCS: Cell<&[u8]> = Cell::new(b"BIG5-HKSCS"); }
thread_local! { static ENC_C: Cell<&[u8]> = Cell::new(b"C"); }
thread_local! { static ENC_CP1251: Cell<&[u8]> = Cell::new(b"CP1251"); }
thread_local! { static ENC_CP1255: Cell<&[u8]> = Cell::new(b"CP1255"); }
thread_local! { static ENC_CP1256: Cell<&[u8]> = Cell::new(b"CP1256"); }
thread_local! { static ENC_EUC_CN: Cell<&[u8]> = Cell::new(b"EUC-CN"); }
thread_local! { static ENC_EUC_JP: Cell<&[u8]> = Cell::new(b"EUC-JP"); }
thread_local! { static ENC_EUC_KR: Cell<&[u8]> = Cell::new(b"EUC-KR"); }
thread_local! { static ENC_EUC_TW: Cell<&[u8]> = Cell::new(b"EUC-TW"); }
thread_local! { static ENC_GB2312: Cell<&[u8]> = Cell::new(b"GB2312"); }
thread_local! { static ENC_GBK: Cell<&[u8]> = Cell::new(b"GBK"); }
thread_local! { static ENC_GEORGIAN_ACADEMY: Cell<&[u8]> = Cell::new(b"GEORGIAN-ACADEMY"); }
thread_local! { static ENC_ISO8859_1: Cell<&[u8]> = Cell::new(b"ISO8859-1"); }
thread_local! { static ENC_ISO8859_10: Cell<&[u8]> = Cell::new(b"ISO8859-10"); }
thread_local! { static ENC_ISO8859_11: Cell<&[u8]> = Cell::new(b"ISO8859-11"); }
thread_local! { static ENC_ISO8859_13: Cell<&[u8]> = Cell::new(b"ISO8859-13"); }
thread_local! { static ENC_ISO8859_15: Cell<&[u8]> = Cell::new(b"ISO8859-15"); }
thread_local! { static ENC_ISO8859_2: Cell<&[u8]> = Cell::new(b"ISO8859-2"); }
thread_local! { static ENC_ISO8859_3: Cell<&[u8]> = Cell::new(b"ISO8859-3"); }
thread_local! { static ENC_ISO8859_5: Cell<&[u8]> = Cell::new(b"ISO8859-5"); }
thread_local! { static ENC_ISO8859_6: Cell<&[u8]> = Cell::new(b"ISO8859-6"); }
thread_local! { static ENC_ISO8859_7: Cell<&[u8]> = Cell::new(b"ISO8859-7"); }
thread_local! { static ENC_ISO8859_8: Cell<&[u8]> = Cell::new(b"ISO8859-8"); }
thread_local! { static ENC_ISO8859_9: Cell<&[u8]> = Cell::new(b"ISO8859-9"); }
thread_local! { static ENC_KOI8_R: Cell<&[u8]> = Cell::new(b"KOI8-R"); }
thread_local! { static ENC_KOI8_U: Cell<&[u8]> = Cell::new(b"KOI8-U"); }
thread_local! { static ENC_TCVN: Cell<&[u8]> = Cell::new(b"TCVN"); }
thread_local! { static ENC_UTF_8: Cell<&[u8]> = Cell::new(b"UTF-8"); }

// ---------------------------------------------------------------------------
// Lookup table for locale -> charset guessing (non-Apple platforms)
// ---------------------------------------------------------------------------

struct NameValue {
    name: &'static [u8],
    get_value: fn() -> *const u8,
}

#[cfg(not(target_os = "macos"))]
static GUESS: &[NameValue] = &[
    nv(b"Cextend", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"English_United-States.437", || {
        ENC_C.with(|v| v.get().as_ptr())
    }),
    nv(b"ISO-8859-1", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"ISO8859-1", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"Japanese-EUC", || ENC_EUC_JP.with(|v| v.get().as_ptr())),
    nv(b"Jp_JP", || ENC_EUC_JP.with(|v| v.get().as_ptr())),
    nv(b"POSIX", || ENC_C.with(|v| v.get().as_ptr())),
    nv(b"POSIX-UTF2", || ENC_C.with(|v| v.get().as_ptr())),
    nv(b"aa_DJ", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"aa_ER", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"aa_ET", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"af", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"af_ZA", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"am", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"am_ET", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"an_ES", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"ar", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_AA", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_AE", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_BH", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_DZ", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_EG", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_IN", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"ar_IQ", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_JO", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_KW", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_LB", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_LY", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_MA", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_OM", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_QA", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_SA", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_SD", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_SY", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_TN", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"ar_YE", || ENC_ISO8859_6.with(|v| v.get().as_ptr())),
    nv(b"be", || ENC_CP1251.with(|v| v.get().as_ptr())),
    nv(b"be_BY", || ENC_CP1251.with(|v| v.get().as_ptr())),
    nv(b"bg", || ENC_CP1251.with(|v| v.get().as_ptr())),
    nv(b"bg_BG", || ENC_CP1251.with(|v| v.get().as_ptr())),
    nv(b"bn_BD", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"bn_IN", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"br", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"br_FR", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"br_FR@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"bs_BA", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"ca", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"ca_ES", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"ca_ES@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"chinese-s", || ENC_EUC_CN.with(|v| v.get().as_ptr())),
    nv(b"chinese-t", || ENC_EUC_TW.with(|v| v.get().as_ptr())),
    nv(b"cs", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"cs_CZ", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"cy", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"cy_GB", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"cz", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"da", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"da_DK", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"de", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"de_AT", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"de_AT@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"de_BE", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"de_BE@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"de_CH", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"de_DE", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"de_DE@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"de_LI", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"de_LI@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"de_LU", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"de_LU@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"el", || ENC_ISO8859_7.with(|v| v.get().as_ptr())),
    nv(b"el_GR", || ENC_ISO8859_7.with(|v| v.get().as_ptr())),
    nv(b"en", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"en_AU", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"en_BW", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"en_CA", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"en_DK", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"en_GB", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"en_HK", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"en_IE", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"en_IE@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"en_IN", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"en_NZ", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"en_PH", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"en_SG", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"en_UK", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"en_US", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"en_ZA", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"en_ZW", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_AR", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_BO", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_CL", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_CO", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_CR", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_DO", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_EC", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_ES", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_ES@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"es_GT", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_HN", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_MX", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_NI", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_PA", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_PE", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_PR", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_PY", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_SV", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_US", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_UY", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"es_VE", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"et", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"et_EE", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"eu", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"eu_ES", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"eu_ES@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"eu_FR", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"eu_FR@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"fa", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"fa_IR", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"fi", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"fi_FI", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"fi_FI@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"fo", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"fo_FO", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"fr", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"fr_BE", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"fr_BE@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"fr_CA", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"fr_CH", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"fr_FR", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"fr_FR@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"fr_LU", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"fr_LU@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"ga", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"ga_IE", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"ga_IE@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"gd", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"gd_GB", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"gez_ER", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"gez_ET", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"gl", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"gl_ES", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"gl_ES@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"gu_IN", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"gv", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"gv_GB", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"he", || ENC_ISO8859_8.with(|v| v.get().as_ptr())),
    nv(b"he_IL", || ENC_ISO8859_8.with(|v| v.get().as_ptr())),
    nv(b"hr", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"hr_HR", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"hu", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"hu_HU", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"hy", || ENC_ARMSCII_8.with(|v| v.get().as_ptr())),
    nv(b"hy_AM", || ENC_ARMSCII_8.with(|v| v.get().as_ptr())),
    nv(b"id", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"id_ID", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"is", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"is_IS", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"it", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"it_CH", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"it_IT", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"it_IT@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"ja", || ENC_EUC_JP.with(|v| v.get().as_ptr())),
    nv(b"ja_JP", || ENC_EUC_JP.with(|v| v.get().as_ptr())),
    nv(b"ka", || ENC_GEORGIAN_ACADEMY.with(|v| v.get().as_ptr())),
    nv(b"ka_GE", || ENC_GEORGIAN_ACADEMY.with(|v| v.get().as_ptr())),
    nv(b"kl", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"kl_GL", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"kn_IN", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"ko", || ENC_EUC_KR.with(|v| v.get().as_ptr())),
    nv(b"ko_KR", || ENC_EUC_KR.with(|v| v.get().as_ptr())),
    nv(b"kw", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"kw_GB", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"lg_UG", || ENC_ISO8859_10.with(|v| v.get().as_ptr())),
    nv(b"lt", || ENC_ISO8859_13.with(|v| v.get().as_ptr())),
    nv(b"lt_LT", || ENC_ISO8859_13.with(|v| v.get().as_ptr())),
    nv(b"lv", || ENC_ISO8859_13.with(|v| v.get().as_ptr())),
    nv(b"lv_LV", || ENC_ISO8859_13.with(|v| v.get().as_ptr())),
    nv(b"mi", || ENC_ISO8859_13.with(|v| v.get().as_ptr())),
    nv(b"mi_NZ", || ENC_ISO8859_13.with(|v| v.get().as_ptr())),
    nv(b"mk", || ENC_ISO8859_5.with(|v| v.get().as_ptr())),
    nv(b"mk_MK", || ENC_ISO8859_5.with(|v| v.get().as_ptr())),
    nv(b"ml_IN", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"mn_MN", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"mr_IN", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"ms", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"ms_MY", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"mt", || ENC_ISO8859_3.with(|v| v.get().as_ptr())),
    nv(b"mt_MT", || ENC_ISO8859_3.with(|v| v.get().as_ptr())),
    nv(b"nb", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"nb_NO", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"ne_NP", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"nl", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"nl_BE", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"nl_BE@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"nl_NL", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"nl_NL@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"nn", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"nn_NO", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"no", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"no_NO", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"oc", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"oc_FR", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"oc_FR@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"om_ET", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"om_KE", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"pa_IN", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"ph", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"ph_PH", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"pl", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"pl_PL", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"pt", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"pt_BR", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"pt_PT", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"pt_PT@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"ro", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"ro_RO", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"ru", || ENC_KOI8_R.with(|v| v.get().as_ptr())),
    nv(b"ru_RU", || ENC_KOI8_R.with(|v| v.get().as_ptr())),
    nv(b"ru_UA", || ENC_KOI8_U.with(|v| v.get().as_ptr())),
    nv(b"se_NO", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"sh", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"sh_SP", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"sh_YU", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"sid_ET", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"sk", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"sk_SK", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"sl", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"sl_SI", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"so_DJ", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"so_ET", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"so_KE", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"so_SO", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"sq", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"sq_AL", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"sr", || ENC_ISO8859_5.with(|v| v.get().as_ptr())),
    nv(b"sr_SP", || ENC_ISO8859_2.with(|v| v.get().as_ptr())),
    nv(b"sr_YU", || ENC_ISO8859_5.with(|v| v.get().as_ptr())),
    nv(b"st_ZA", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"sv", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"sv_FI", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"sv_FI@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"sv_SE", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"sv_SE@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"te_IN", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"th", || ENC_ISO8859_11.with(|v| v.get().as_ptr())),
    nv(b"th_TH", || ENC_ISO8859_11.with(|v| v.get().as_ptr())),
    nv(b"ti_ER", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"ti_ET", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"tig_ER", || ENC_UTF_8.with(|v| v.get().as_ptr())),
    nv(b"tl", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"tl_PH", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"tr", || ENC_ISO8859_9.with(|v| v.get().as_ptr())),
    nv(b"tr_TR", || ENC_ISO8859_9.with(|v| v.get().as_ptr())),
    nv(b"uk", || ENC_KOI8_U.with(|v| v.get().as_ptr())),
    nv(b"uk_UA", || ENC_KOI8_U.with(|v| v.get().as_ptr())),
    nv(b"ur", || ENC_CP1256.with(|v| v.get().as_ptr())),
    nv(b"ur_PK", || ENC_CP1256.with(|v| v.get().as_ptr())),
    nv(b"uz_UZ", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"vi", || ENC_TCVN.with(|v| v.get().as_ptr())),
    nv(b"vi_VN", || ENC_TCVN.with(|v| v.get().as_ptr())),
    nv(b"wa", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"wa_BE", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"wa_BE@euro", || ENC_ISO8859_15.with(|v| v.get().as_ptr())),
    nv(b"xh_ZA", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
    nv(b"yi", || ENC_CP1255.with(|v| v.get().as_ptr())),
    nv(b"yi_US", || ENC_CP1255.with(|v| v.get().as_ptr())),
    nv(b"zh_CN", || ENC_GBK.with(|v| v.get().as_ptr())),
    nv(b"zh_HK", || ENC_BIG5_HKSCS.with(|v| v.get().as_ptr())),
    nv(b"zh_SG", || ENC_GB2312.with(|v| v.get().as_ptr())),
    nv(b"zh_TW", || ENC_BIG5.with(|v| v.get().as_ptr())),
    nv(b"zu_ZA", || ENC_ISO8859_1.with(|v| v.get().as_ptr())),
];

#[cfg(not(target_os = "macos"))]
const fn nv(name: &'static [u8], get_value: fn() -> *const u8) -> NameValue {
    NameValue { name, get_value }
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
            return Some((table[mid].get_value)());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// locale2charset - main entry point
// ---------------------------------------------------------------------------

pub unsafe fn locale2charset(locale: *const c_char) -> *const c_char {
    unsafe {
        thread_local! { static CHARSET_BUF: RefCell<[u8; 128]> = RefCell::new([0u8; 128]); }

        let locale = if locale.is_null() {
            return b"ASCII\0".as_ptr() as *const c_char;
        } else {
            let s = CStr::from_ptr(locale).to_bytes();
            if s.is_empty() || s == b"NULL" {
                return b"ASCII\0".as_ptr() as *const c_char;
            }
            s
        };

        if locale == b"C" || locale == b"POSIX" {
            return b"ASCII\0".as_ptr() as *const c_char;
        }

        let mut enc: &[u8] = b"";
        let mut la_loc: &[u8] = locale;

        if let Some(dot_pos) = locale.iter().rposition(|&c| c == b'.') {
            enc = &locale[dot_pos + 1..];
            la_loc = &locale[..dot_pos];
        }

        if !enc.is_empty() {
            let enc_lower: Vec<u8> = enc.iter().map(|&c| c.to_ascii_lowercase()).collect();
            if enc_lower == b"utf-8" || enc_lower == b"utf8" {
                return b"UTF-8\0".as_ptr() as *const c_char;
            }
        }

        if !enc.is_empty() && !(enc.len() == 4 && &enc[..4] == b"utf8") {
            let enc_lower: Vec<u8> = enc.iter().map(|&c| c.to_ascii_lowercase()).collect();

            for (name, value) in KNOWN.iter() {
                if *name == enc_lower.as_slice() {
                    return value.as_ptr() as *const c_char;
                }
            }

            if enc_lower.starts_with(b"cp-") {
                let cp_str: String = enc_lower[3..].iter().map(|&c| c as char).collect();
                if let Ok(cp) = cp_str.parse::<u32>() {
                    let s = format!("CP{}", cp);
                    return CHARSET_BUF.with(|cell| {
                        let mut buf = cell.borrow_mut();
                        ptr::copy_nonoverlapping(s.as_ptr(), buf.as_mut_ptr(), s.len());
                        buf[s.len()] = 0;
                        buf.as_ptr() as *const c_char
                    });
                }
            }

            if enc_lower.starts_with(b"ibm") {
                let ibm_str: String = enc_lower[3..].iter().map(|&c| c as char).collect();
                if let Ok(cp) = ibm_str.parse::<i32>() {
                    if cp != 0 {
                        let s = format!("IBM-{}", cp.abs());
                        return CHARSET_BUF.with(|cell| {
                            let mut buf = cell.borrow_mut();
                            ptr::copy_nonoverlapping(s.as_ptr(), buf.as_mut_ptr(), s.len());
                            buf[s.len()] = 0;
                            buf.as_ptr() as *const c_char
                        });
                    }
                }
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
                    return CHARSET_BUF.with(|cell| {
                        let mut buf = cell.borrow_mut();
                        ptr::copy_nonoverlapping(
                            charset_str.as_ptr(),
                            buf.as_mut_ptr(),
                            charset_str.len(),
                        );
                        buf[charset_str.len()] = 0;
                        buf.as_ptr() as *const c_char
                    });
                }
            }

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

        #[cfg(target_os = "macos")]
        {
            b"UTF-8\0".as_ptr() as *const c_char
        }

        #[cfg(not(target_os = "macos"))]
        {
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

    fn some<T>(opt: Option<T>) -> T {
        opt.unwrap_or_else(|| panic!("unexpected None in test"))
    }
    fn must<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("test failed: {e:?}"),
        }
    }

    #[test]
    fn test_null_locale() {
        unsafe {
            let result = locale2charset(std::ptr::null());
            let s = CStr::from_ptr(result).to_str().unwrap_or("");
            assert_eq!(s, "ASCII");
        }
    }

    #[test]
    fn test_c_locale() {
        unsafe {
            let result = locale2charset(b"C\0".as_ptr() as *const c_char);
            let s = CStr::from_ptr(result).to_str().unwrap_or("");
            assert_eq!(s, "ASCII");
        }
    }

    #[test]
    fn test_posix_locale() {
        unsafe {
            let result = locale2charset(b"POSIX\0".as_ptr() as *const c_char);
            let s = CStr::from_ptr(result).to_str().unwrap_or("");
            assert_eq!(s, "ASCII");
        }
    }

    #[test]
    fn test_utf8_encoding() {
        unsafe {
            let result = locale2charset(b"en_US.UTF-8\0".as_ptr() as *const c_char);
            let s = CStr::from_ptr(result).to_str().unwrap_or("");
            assert_eq!(s, "UTF-8");
        }
    }

    #[test]
    fn test_macos_locale() {
        unsafe {
            let result = locale2charset(b"en_US\0".as_ptr() as *const c_char);
            let s = CStr::from_ptr(result).to_str().unwrap_or("");
            assert_eq!(s, "UTF-8");
        }
    }
}
