//! Per-session gettext domain bindings.

use std::ffi::{CStr, CString, c_char};
use std::ptr;

use super::types::{IntlRuntimeState, cstr, with_intl_runtime};

fn dirname<'a>(state: &'a mut IntlRuntimeState, domain: &CStr, value: Option<&CStr>) -> &'a CStr {
    if let Some(value) = value {
        let is_new = !state.domain_bindings.contains_key(domain);
        let binding = state.domain_bindings.entry(domain.to_owned()).or_default();
        let changed = binding.dirname.as_c_str() != value;
        if changed {
            value.clone_into(&mut binding.dirname);
        }
        if is_new || changed {
            state.msg_cat_cntr = state.msg_cat_cntr.wrapping_add(1);
        }
        binding.dirname.as_c_str()
    } else {
        state
            .domain_bindings
            .get(domain)
            .map_or(c"/", |binding| binding.dirname.as_c_str())
    }
}

/// Query or set a domain directory, returning a copy independent of session state.
pub(crate) fn bind_domain(domain: Option<&CStr>, directory: Option<&CStr>) -> Option<CString> {
    let domain = domain.filter(|domain| !domain.is_empty())?;
    Some(with_intl_runtime(|state| {
        dirname(state, domain, directory).to_owned()
    }))
}

/// Legacy pointer adapter. The result is invalidated by replacing this domain's
/// directory or destroying its runtime; callers must not write through it.
///
/// # Safety
/// Non-null arguments must be valid NUL-terminated strings for the call.
pub unsafe fn libintl_bindtextdomain(
    domain: *const c_char,
    directory: *const c_char,
) -> *mut c_char {
    // SAFETY: the input pointers satisfy the C-string contract above.
    let (domain, directory) = unsafe { (cstr(domain), cstr(directory)) };
    let Some(domain) = domain.filter(|domain| !domain.is_empty()) else {
        return ptr::null_mut();
    };
    // Snapshot potential aliases of the session's own strings before mutation.
    let domain = domain.to_owned();
    let directory = directory.map(CStr::to_owned);
    with_intl_runtime(|state| {
        dirname(state, &domain, directory.as_deref())
            .as_ptr()
            .cast_mut()
    })
}

/// Legacy encoding adapter. The result is invalidated by replacing this
/// domain's encoding or destroying its runtime; callers must not write through it.
///
/// # Safety
/// Non-null arguments must be valid NUL-terminated strings for the call.
pub unsafe fn libintl_bind_textdomain_codeset(
    domain: *const c_char,
    encoding: *const c_char,
) -> *mut c_char {
    // SAFETY: the input pointers satisfy the C-string contract above.
    let (domain, encoding) = unsafe { (cstr(domain), cstr(encoding)) };
    let Some(domain) = domain.filter(|domain| !domain.is_empty()) else {
        return ptr::null_mut();
    };
    let domain = domain.to_owned();
    let encoding = encoding.map(CStr::to_owned);
    with_intl_runtime(|state| {
        if let Some(encoding) = encoding {
            let binding = state.domain_bindings.entry(domain.clone()).or_default();
            if binding.codeset.as_ref() != Some(&encoding) {
                binding.codeset = Some(encoding);
                state.msg_cat_cntr = state.msg_cat_cntr.wrapping_add(1);
            }
        }
        state
            .domain_bindings
            .get(domain.as_c_str())
            .and_then(|binding| binding.codeset.as_ref())
            .map_or(ptr::null_mut(), |encoding| {
                encoding.as_ptr().cast_mut()
            })
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_domain_rejects_null_and_empty_domains() {
        assert!(bind_domain(None, None).is_none());
        assert!(bind_domain(Some(c""), None).is_none());
    }

    #[test]
    fn test_bindtextdomain_null_domain() {
        // SAFETY: null is accepted for both arguments.
        let result = unsafe { libintl_bindtextdomain(ptr::null(), ptr::null()) };
        assert!(result.is_null());
    }

    #[test]
    fn test_bindtextdomain_empty_domain() {
        let empty = b"\0".as_ptr() as *const c_char;
        // SAFETY: `empty` is a NUL-terminated literal; null dirname is accepted.
        let result = unsafe { libintl_bindtextdomain(empty, ptr::null()) };
        assert!(result.is_null());
    }

    #[test]
    fn test_owned_binding_survives_update_and_state_drop() {
        let mut state = IntlRuntimeState::default();
        let saved = dirname(&mut state, c"domain", Some(c"/first")).to_owned();
        assert_eq!(dirname(&mut state, c"domain", Some(c"/second")), c"/second");
        assert_eq!(dirname(&mut state, c"domain", None), c"/second");
        assert_eq!(dirname(&mut state, c"other", None), c"/");
        drop(state);
        assert_eq!(saved.as_c_str(), c"/first");
    }
}
