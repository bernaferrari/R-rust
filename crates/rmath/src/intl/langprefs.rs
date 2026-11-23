#![warn(unused_unsafe)]
//! The standalone runtime does not load OS-specific language preferences.

use std::ffi::CStr;

pub fn language_preferences_default() -> Option<&'static CStr> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_preferences_returns() {
        assert_eq!(language_preferences_default(), None);
    }
}
