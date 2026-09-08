#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use super::*;

// attr1 — determine attribute display type
// ---------------------------------------------------------------------------

/// Determine how to display attributes during deparsing.
///
/// Returns one of ATTR_SIMPLE, ATTR_OK_NAMES, ATTR_STRUC_ATTR, ATTR_STRUC_NMS_A.
pub unsafe fn attr1(s: SEXP, d: *mut LocalParseData) -> c_int {
    unsafe {
        let d = &mut *d;
        let a = ATTRIB(s);
        // For attr1 we need R_NamesSymbol and R_SrcrefSymbol - install them
        let names_sym = Rf_install(b"names\0".as_ptr() as *const c_char);
        let srcref_sym = Rf_install(b"srcref\0".as_ptr() as *const c_char);
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
            print2buff(b"structure(\0".as_ptr() as *const c_char, d);
        }
        attr
    }
}

// ---------------------------------------------------------------------------
// attrEntry — deparse a single attribute entry
// ---------------------------------------------------------------------------

/// Deparse a single attribute as `<name> = <value>`, shared by attr2() (the
/// structure(..) form) and the OBJSXP / object(..) deparse below.
/// Port of upstream deparse.c attrEntry().
pub unsafe fn attrEntry(a: SEXP, d: *mut LocalParseData) {
    unsafe {
        let d = &mut *d;
        let names_sym = Rf_install(b"names\0".as_ptr() as *const c_char);
        let dim_sym = Rf_install(b"dim\0".as_ptr() as *const c_char);
        let dimnames_sym = Rf_install(b"dimnames\0".as_ptr() as *const c_char);
        let tsp_sym = Rf_install(b"tsp\0".as_ptr() as *const c_char);
        let levels_sym = Rf_install(b"levels\0".as_ptr() as *const c_char);

        if TAG(a) == dim_sym {
            print2buff(b"dim\0".as_ptr() as *const c_char, d); // was .Dim
        } else if TAG(a) == dimnames_sym {
            print2buff(b"dimnames\0".as_ptr() as *const c_char, d); // was .Dimnames
        } else if TAG(a) == names_sym {
            print2buff(b"names\0".as_ptr() as *const c_char, d); // was .Names
        } else if TAG(a) == tsp_sym {
            print2buff(b"tsp\0".as_ptr() as *const c_char, d); // was .Tsp
        } else if TAG(a) == levels_sym {
            print2buff(b"levels\0".as_ptr() as *const c_char, d); // was .Label
        } else {
            // TAG(a) might contain spaces etc
            let tag_name = CHAR(PRINTNAME(TAG(a)));
            let d_opts_in = d.opts;
            d.opts = SIMPLEDEPARSE; /* turn off quote()ing */
            if !tag_name.is_null() && isValidName(tag_name) {
                deparse2buff(TAG(a), d);
            } else {
                print2buff(b"\"\0".as_ptr() as *const c_char, d);
                deparse2buff(TAG(a), d);
                print2buff(b"\"\0".as_ptr() as *const c_char, d);
            }
            d.opts = d_opts_in;
        }
        print2buff(b" = \0".as_ptr() as *const c_char, d);
        let fnarg = d.fnarg;
        d.fnarg = true;
        deparse2buff(CAR(a), d);
        d.fnarg = fnarg;
    }
}

// ---------------------------------------------------------------------------
// attr2 — write attribute suffix to buffer
// ---------------------------------------------------------------------------

/// Write full attributes(s) to 'buff'.  Port of upstream deparse.c attr2().
pub unsafe fn attr2(s: SEXP, d: *mut LocalParseData, not_names: bool) {
    unsafe {
        let d = &mut *d;
        let names_sym = Rf_install(b"names\0".as_ptr() as *const c_char);
        let srcref_sym = Rf_install(b"srcref\0".as_ptr() as *const c_char);

        let mut a = ATTRIB(s);
        while !isNull(a) {
            if TAG(a) != srcref_sym && !(TAG(a) == names_sym && not_names) {
                print2buff(b", \0".as_ptr() as *const c_char, d);
                attrEntry(a, d);
            }
            a = CDR(a);
        }
        print2buff(b")\0".as_ptr() as *const c_char, d);
    }
}

// ---------------------------------------------------------------------------
// quotify — quote a symbol name if needed
// ---------------------------------------------------------------------------

/// If a symbol is not a valid R name, return a quoted/escaped version.
/// Otherwise return the name as-is.
pub unsafe fn quotify(name: SEXP, quote: c_int) -> *const c_char {
    unsafe {
        if name.is_null() {
            return ptr::null();
        }
        let s = CHAR(name);
        if s.is_null() {
            return ptr::null();
        }
        if isValidName(s) || *s == 0 {
            return s;
        }
        // For backtick or double-quote quoting, just return the name with quotes
        // Full EncodeString is in printutils but currently stubbed.
        // We implement basic quoting here.
        with_deparse_runtime(|state| {
            let buf = &mut state.quote_buf;
            let bytes = std::ffi::CStr::from_ptr(s).to_bytes();
            let quote_char = if quote == b'`' as c_int { b'`' } else { b'"' };
            let mut pos = 0;
            buf[pos] = quote_char;
            pos += 1;
            for &b in bytes.iter() {
                if b == quote_char || b == b'\\' {
                    buf[pos] = b'\\';
                    pos += 1;
                }
                if pos + 1 >= 1022 {
                    break;
                }
                buf[pos] = b;
                pos += 1;
            }
            buf[pos] = quote_char;
            pos += 1;
            buf[pos] = 0;
            buf.as_ptr() as *const c_char
        })
    }
}

// ---------------------------------------------------------------------------
