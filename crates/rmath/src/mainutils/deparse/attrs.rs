#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use super::*;

// attr1 — determine attribute display type
// ---------------------------------------------------------------------------

/// Determine how to display attributes during deparsing.
///
/// Returns one of ATTR_SIMPLE, ATTR_OK_NAMES, ATTR_STRUC_ATTR, ATTR_STRUC_NMS_A.
pub fn attr1(s: SEXP, d: &mut LocalParseData) -> c_int {
    let a = ATTRIB(s);
    // For attr1 we need R_NamesSymbol and R_SrcrefSymbol - install them
    let names_sym = crate::sexp::symbol::install_str("names");
    let srcref_sym = crate::sexp::symbol::install_str("srcref");
    let nm = getAttrib(s, names_sym);

    let mut attr = ATTR_UNKNOWN;
    let nice_names = (d.opts & NICE_NAMES) != 0;
    let show_attr = (d.opts & SHOWATTRIBUTES) != 0;
    let has_names = !isNull(nm);

    if has_names {
        let ok_names = nice_names && usable_nice_names(nm, isVectorAtomic(s));
        if !ok_names {
            attr = if show_attr {
                ATTR_STRUC_NMS_A
            } else {
                ATTR_OK_NAMES
            };
        }
    }

    let mut cur = a;
    while attr == ATTR_UNKNOWN && !isNull(cur) {
        if has_names && TAG(cur) == names_sym {
            // skip names
        } else if show_attr && TAG(cur) != srcref_sym {
            attr = ATTR_STRUC_ATTR;
            break;
        }
        cur = CDR(cur);
    }
    if attr == ATTR_UNKNOWN {
        attr = if has_names {
            ATTR_OK_NAMES
        } else {
            ATTR_SIMPLE
        };
    }

    if attr >= ATTR_STRUC_ATTR {
        append_line_bytes(b"structure(", d);
    }
    attr
}

// ---------------------------------------------------------------------------
// attrEntry — deparse a single attribute entry
// ---------------------------------------------------------------------------

/// Deparse a single attribute as `<name> = <value>`, shared by attr2() (the
/// structure(..) form) and the OBJSXP / object(..) deparse below.
/// Port of upstream deparse.c attrEntry().
pub fn attrEntry(a: SEXP, d: &mut LocalParseData) {
    let names_sym = crate::sexp::symbol::install_str("names");
    let dim_sym = crate::sexp::symbol::install_str("dim");
    let dimnames_sym = crate::sexp::symbol::install_str("dimnames");
    let tsp_sym = crate::sexp::symbol::install_str("tsp");
    let levels_sym = crate::sexp::symbol::install_str("levels");

    if TAG(a) == dim_sym {
        append_line_bytes(b"dim", d); // was .Dim
    } else if TAG(a) == dimnames_sym {
        append_line_bytes(b"dimnames", d); // was .Dimnames
    } else if TAG(a) == names_sym {
        append_line_bytes(b"names", d); // was .Names
    } else if TAG(a) == tsp_sym {
        append_line_bytes(b"tsp", d); // was .Tsp
    } else if TAG(a) == levels_sym {
        append_line_bytes(b"levels", d); // was .Label
    } else {
        // TAG(a) might contain spaces etc
        let tag_name = char_cstr_bytes_opt(PRINTNAME(TAG(a)));
        let d_opts_in = d.opts;
        d.opts = SIMPLEDEPARSE; /* turn off quote()ing */
        if tag_name.as_deref().is_some_and(is_valid_r_name_bytes) {
            deparse2buff(TAG(a), d);
        } else {
            append_line_bytes(b"\"", d);
            deparse2buff(TAG(a), d);
            append_line_bytes(b"\"", d);
        }
        d.opts = d_opts_in;
    }
    append_line_bytes(b" = ", d);
    let fnarg = d.fnarg;
    d.fnarg = true;
    deparse2buff(CAR(a), d);
    d.fnarg = fnarg;
}

// ---------------------------------------------------------------------------
// attr2 — write attribute suffix to buffer
// ---------------------------------------------------------------------------

/// Write full attributes(s) to 'buff'.  Port of upstream deparse.c attr2().
pub fn attr2(s: SEXP, d: &mut LocalParseData, not_names: bool) {
    let names_sym = crate::sexp::symbol::install_str("names");
    let srcref_sym = crate::sexp::symbol::install_str("srcref");

    let mut a = ATTRIB(s);
    while !isNull(a) {
        if TAG(a) != srcref_sym && !(TAG(a) == names_sym && not_names) {
            append_line_bytes(b", ", d);
            attrEntry(a, d);
        }
        a = CDR(a);
    }
    append_line_bytes(b")", d);
}

// ---------------------------------------------------------------------------
// append_quoted_name / append_char_name — owned name appends
// ---------------------------------------------------------------------------

/// Append a symbol name using the compatibility quoting and byte limit.
pub(super) fn append_quoted_name(name: SEXP, quote: u8, d: &mut LocalParseData) {
    let Some(bytes) = char_cstr_bytes_opt(name) else {
        return;
    };
    if bytes.is_empty() || is_valid_r_name_bytes(&bytes) {
        append_line_bytes(&bytes, d);
        return;
    }
    let quote = if quote == b'`' { b'`' } else { b'"' };
    let mut buf = [0; 1024];
    let mut pos = 1;
    buf[0] = quote;
    for byte in bytes {
        if byte == quote || byte == b'\\' {
            buf[pos] = b'\\';
            pos += 1;
        }
        if pos + 1 >= 1022 {
            break;
        }
        buf[pos] = byte;
        pos += 1;
    }
    buf[pos] = quote;
    append_line_bytes(&buf[..=pos], d);
}

/// Append a CHARSXP's bytes, preserving the None no-op.
pub(super) fn append_char_name(name: SEXP, d: &mut LocalParseData) {
    if let Some(bytes) = char_cstr_bytes_opt(name) {
        append_line_bytes(&bytes, d);
    }
}

// ---------------------------------------------------------------------------
