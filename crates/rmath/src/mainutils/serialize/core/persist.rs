use super::*;

// ---------------------------------------------------------------------------
// Reference index packing/unpacking
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Version decoding
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Attribute / tag presence helpers
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn sexp_has_attributes(s: SEXP) -> bool {
    {
        let attrib = ATTRIB(s);
        !attrib.is_null() && attrib != R_NilValue()
    }
}

#[inline]
pub(crate) fn sexp_has_tag(s: SEXP) -> bool {
    {
        let tag = TAG(s);
        !tag.is_null() && tag != R_NilValue()
    }
}

// Match GNU's structural package/namespace predicates, plus namespaces
// represented by this runtime's package cache.
pub(crate) fn persistent_package_or_namespace(s: SEXP) -> bool {
    {
        let name = crate::sexp::attrib_core::getAttrib(s, install_str("name"));
        if TYPEOF(name) == SEXPTYPE::STRSXP
            && XLENGTH(name) > 0
            && charsxp_as_utf8(STRING_ELT(name, 0)).starts_with(b"package:")
        {
            return true;
        }
        // SAFETY: `s` is a caller-owned environment SEXP; frame lookups use
        // installed symbols and the instance borrow is strictly local.
        unsafe {
            let info = R_findVarInFrame(s, install_str(".__NAMESPACE__."));
            if TYPEOF(info) == SEXPTYPE::ENVSXP {
                let _info_guard = protect(info);
                let spec = R_findVarInFrame(info, install_str("spec"));
                if TYPEOF(spec) == SEXPTYPE::STRSXP && XLENGTH(spec) > 0 {
                    return true;
                }
            }
            with_required_current_instance(|instance| {
                (*instance)
                    .package_namespace_cache
                    .values()
                    .any(|(_, env)| *env == s)
            })
        }
    }
}

pub(crate) fn persistent_hook_eligible(s: SEXP) -> bool {
    match TYPEOF(s) {
        t if t == SEXPTYPE::EXTPTRSXP || t == SEXPTYPE::WEAKREFSXP => true,
        t if t == SEXPTYPE::ENVSXP => {
            s != R_GlobalEnv()
                && s != R_BaseEnv()
                && s != R_EmptyEnv()
                && !persistent_package_or_namespace(s)
        }
        _ => false,
    }
}

pub(crate) fn persistent_name(s: SEXP, hook: SEXP) -> SEXP {
    if !persistent_hook_eligible(s) || hook.is_null() || hook == R_NilValue() {
        return R_NilValue();
    }
    let name = CallHook(s, hook);
    if name == R_NilValue() || name.is_null() {
        return R_NilValue();
    }
    if TYPEOF(name) != SEXPTYPE::STRSXP || XLENGTH(name) == 0 {
        error("persistent hook must return a nonempty character vector");
    }
    name
}

// ---------------------------------------------------------------------------
// SaveSpecialHook -- detect special singleton values
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Internal WriteItem (recursive, writes to BinaryWriter)
// ---------------------------------------------------------------------------

