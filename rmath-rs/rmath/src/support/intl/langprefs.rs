//! Port of langprefs.c -- Language preferences (macOS CoreFoundation).
//!
//! On macOS, this determines the user's language preferences by querying
//! the CoreFoundation preferences system. The preferences are cached after
//! the first call.
//!
//! On non-macOS platforms, this simply returns NULL.

#![allow(non_snake_case)]

use std::os::raw::c_char;
use std::ptr;

// ---------------------------------------------------------------------------
// macOS implementation
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

    /// Cached language preferences string.
    static CACHED_LANGUAGES: AtomicPtr<c_char> = AtomicPtr::new(ptr::null_mut());
    static CACHE_INITIALIZED: AtomicBool = AtomicBool::new(false);

    /// Canonicalize a locale name (stub).
    ///
    /// In the full C implementation this calls `_nl_locale_name_canonicalize`.
    /// Here we do minimal normalization: ensure lowercase language, etc.
    unsafe fn _nl_locale_name_canonicalize(name: *mut c_char) {
        unsafe {
            if name.is_null() {
                return;
            }
            // Convert language part to lowercase.
            let mut p = name;
            while *p != 0 && *p != b'_' as c_char && *p != b'.' as c_char && *p != b'@' as c_char {
                *p = (*p as u8).to_ascii_lowercase() as c_char;
                p = p.add(1);
            }
        }
    }

    /// Determine the user's language preferences.
    ///
    /// Returns a colon-separated list of locale names, or NULL if not available.
    /// The result must not be freed; it is statically allocated.
    pub unsafe fn _nl_language_preferences_default() -> *const c_char {
        unsafe {
            if CACHE_INITIALIZED.load(Ordering::Acquire) {
                return CACHED_LANGUAGES.load(Ordering::Acquire);
            }

            // Use CoreFoundation to get AppleLanguages preference.
            let preferences =
                crate::support::intl::langprefs::macos::cf_preferences_copy_app_value();
            if !preferences.is_null() {
                let result = crate::support::intl::langprefs::macos::extract_languages(preferences);
                CACHED_LANGUAGES.store(result, Ordering::Release);
            }

            CACHE_INITIALIZED.store(true, Ordering::Release);
            CACHED_LANGUAGES.load(Ordering::Acquire)
        }
    }

    /// Stub: CFPreferencesCopyAppValue (placeholder for CoreFoundation).
    ///
    /// In a real implementation this would call the CoreFoundation API.
    /// For the standalone port, we return NULL to indicate no preferences available.
    pub unsafe fn cf_preferences_copy_app_value() -> *mut std::ffi::c_void {
        ptr::null_mut()
    }

    /// Extract languages from a CFArray of CFString preferences.
    pub unsafe fn extract_languages(_pref_array: *mut std::ffi::c_void) -> *mut c_char {
        // Stub: return NULL.
        ptr::null_mut()
    }
}

#[cfg(target_os = "macos")]
pub use macos::_nl_language_preferences_default;

// ---------------------------------------------------------------------------
// Non-macOS implementation
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "macos"))]
mod fallback {
    use super::*;

    /// Determine the user's language preferences.
    ///
    /// On non-macOS platforms, always returns NULL.
    pub unsafe fn _nl_language_preferences_default() -> *const c_char {
        ptr::null()
    }
}

#[cfg(not(target_os = "macos"))]
pub use fallback::_nl_language_preferences_default;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_preferences_returns() {
        unsafe {
            let result = _nl_language_preferences_default();
            // On non-macOS, this should be null.
            #[cfg(not(target_os = "macos"))]
            assert!(result.is_null());
        }
    }
}
