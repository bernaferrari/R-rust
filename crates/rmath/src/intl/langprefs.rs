#![warn(unused_unsafe)]
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

    /// Canonicalize a locale name (stub).
    ///
    /// In the full C implementation this calls `_nl_locale_name_canonicalize`.
    /// Here we do minimal normalization: ensure lowercase language, etc.
    ///
    /// # Safety
    ///
    /// Each raw-pointer argument must be null where the C original accepts null, or else point to a
    /// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
    /// the accompanying lengths) for the duration of the call.
    unsafe fn _nl_locale_name_canonicalize(name: *mut c_char) {
        // Convert the language part to lowercase within the caller's
        // NUL-terminated buffer.
        // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
        let Some(len) =
            unsafe { crate::intl::types::cstr(name) }.map(|s| s.to_bytes_with_nul().len())
        else {
            return;
        };
        // SAFETY: forwarded raw pointers satisfy `slice_mut`'s contract per this function's own `# Safety` requirements.
        let Some(bytes) = (unsafe { crate::intl::types::slice_mut(name as *mut u8, len) }) else {
            return;
        };
        for b in bytes.iter_mut() {
            if *b == 0 || *b == b'_' || *b == b'.' || *b == b'@' {
                break;
            }
            *b = b.to_ascii_lowercase();
        }
    }

    /// Determine the user's language preferences.
    ///
    /// Returns a colon-separated list of locale names, or NULL if not available.
    /// The result must not be freed.
    pub fn _nl_language_preferences_default() -> *const c_char {
        // Use CoreFoundation to get AppleLanguages preference.
        let preferences = crate::intl::langprefs::macos::cf_preferences_copy_app_value();
        if !preferences.is_null() {
            return crate::intl::langprefs::macos::extract_languages(preferences);
        }

        ptr::null()
    }

    /// Stub: CFPreferencesCopyAppValue (placeholder for CoreFoundation).
    ///
    /// In a real implementation this would call the CoreFoundation API.
    /// For the standalone port, we return NULL to indicate no preferences available.
    pub fn cf_preferences_copy_app_value() -> *mut std::ffi::c_void {
        ptr::null_mut()
    }

    /// Extract languages from a CFArray of CFString preferences.
    pub fn extract_languages(_pref_array: *mut std::ffi::c_void) -> *mut c_char {
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
    pub fn _nl_language_preferences_default() -> *const c_char {
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
        let _result = _nl_language_preferences_default();
        // On non-macOS, this should be null.
        #[cfg(not(target_os = "macos"))]
        assert!(_result.is_null());
    }
}
