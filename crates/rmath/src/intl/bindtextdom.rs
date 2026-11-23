//! Per-session gettext domain bindings.

use std::ffi::{CStr, CString};

use super::types::{IntlRuntimeState, with_intl_runtime};

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

/// Query or set a domain's encoding, returning an owned snapshot.
/// Setting to a different encoding bumps the catalog counter.
pub(crate) fn bind_codeset(domain: Option<&CStr>, encoding: Option<&CStr>) -> Option<CString> {
    let domain = domain.filter(|domain| !domain.is_empty())?;
    with_intl_runtime(|state| {
        if let Some(encoding) = encoding {
            let binding = state.domain_bindings.entry(domain.to_owned()).or_default();
            if binding.codeset.as_deref() != Some(encoding) {
                binding.codeset = Some(encoding.to_owned());
                state.msg_cat_cntr = state.msg_cat_cntr.wrapping_add(1);
            }
        }
        state
            .domain_bindings
            .get(domain)
            .and_then(|binding| binding.codeset.clone())
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
