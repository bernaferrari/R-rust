#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use super::*;

// deparse2buff — recursive deparsing workhorse
// ---------------------------------------------------------------------------

pub unsafe fn deparse_s4_object(s: SEXP, d: *mut LocalParseData) -> bool {
    unsafe {
        let Some(class_name) = s4_class_name(s) else {
            return false;
        };

        print2buff(b"new(\0".as_ptr() as *const c_char, d);
        print_r_string_literal(&class_name, d);

        let mut slots = crate::mainutils::objects::s4_all_slots(&class_name).unwrap_or_default();
        if slots.is_empty() {
            slots = string_attribute_values(s, b"names\0");
        }

        for (position, slot_name) in slots.iter().enumerate() {
            let Some(value) = s4_slot_value(s, slot_name, position) else {
                continue;
            };
            print2buff(b", \0".as_ptr() as *const c_char, d);
            print_argument_name(slot_name, d);
            print2buff(b" = \0".as_ptr() as *const c_char, d);
            let old_fnarg = (*d).fnarg;
            (*d).fnarg = true;
            deparse2buff(value, d);
            (*d).fnarg = old_fnarg;
        }

        print2buff(b")\0".as_ptr() as *const c_char, d);
        true
    }
}

pub unsafe fn s4_class_name(s: SEXP) -> Option<String> {
    unsafe {
        string_attribute_values(s, b"class\0")
            .into_iter()
            .next()
            .filter(|name| !name.is_empty())
    }
}

pub unsafe fn string_attribute_values(s: SEXP, attribute: &'static [u8]) -> Vec<String> {
    unsafe {
        let sym = Rf_install(attribute.as_ptr() as *const c_char);
        let value = getAttrib(s, sym);
        if value.is_null() || value == R_NilValue() || TYPEOF(value) != SEXPTYPE::STRSXP {
            return Vec::new();
        }

        let mut out = Vec::with_capacity(LENGTH(value).max(0) as usize);
        for i in 0..LENGTH(value) {
            let elt = STRING_ELT(value, i as R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                out.push(String::new());
                continue;
            }
            let chars = CHAR(elt);
            if chars.is_null() {
                out.push(String::new());
            } else {
                out.push(
                    std::ffi::CStr::from_ptr(chars)
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        out
    }
}

pub unsafe fn s4_slot_value(s: SEXP, slot_name: &str, position: usize) -> Option<SEXP> {
    unsafe {
        if TYPEOF(s) != SEXPTYPE::VECSXP {
            return None;
        }

        let names = string_attribute_values(s, b"names\0");
        for (i, name) in names.iter().enumerate() {
            if name == slot_name && (i as R_xlen_t) < XLENGTH(s) {
                return Some(VECTOR_ELT(s, i as R_xlen_t));
            }
        }

        if (position as R_xlen_t) < XLENGTH(s) {
            Some(VECTOR_ELT(s, position as R_xlen_t))
        } else {
            None
        }
    }
}

pub unsafe fn print_argument_name(name: &str, d: *mut LocalParseData) {
    unsafe {
        if let Ok(c_name) = std::ffi::CString::new(name) {
            if isValidName(c_name.as_ptr()) {
                print2buff(c_name.as_ptr(), d);
                return;
            }
        }
        print2buff(b"`\0".as_ptr() as *const c_char, d);
        print_owned_string(name.replace('`', "\\`"), d);
        print2buff(b"`\0".as_ptr() as *const c_char, d);
    }
}

pub unsafe fn print_r_string_literal(value: &str, d: *mut LocalParseData) {
    unsafe {
        print2buff(b"\"\0".as_ptr() as *const c_char, d);
        print_owned_string(
            value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t"),
            d,
        );
        print2buff(b"\"\0".as_ptr() as *const c_char, d);
    }
}

pub unsafe fn print_owned_string(value: String, d: *mut LocalParseData) {
    unsafe {
        if let Ok(c_value) = std::ffi::CString::new(value) {
            print2buff(c_value.as_ptr(), d);
        }
    }
}

/// The recursive part of deparsing. Handles all SEXP types.
///
/// This is the main recursive function that dispatches based on the SEXPTYPE
/// of the input and builds the deparsed string representation.
pub unsafe fn deparse2buff(s: SEXP, d: *mut LocalParseData) {
    unsafe {
        let d = &mut *d;
        let d_opts_in = d.opts;
        let fnarg = d.fnarg;
        d.fnarg = false;

        // This flag should only be set when recursing through the LHS
        // of binary ops, so by default we reset to zero
        let prev_left = d.left;
        d.left = 0;

        if !d.active {
            d.left = prev_left;
            return;
        }

        let s4_check = IS_S4_OBJECT(s);
        if s4_check != 0 {
            d.isS4 = 1;
            if !deparse_s4_object(s, d) {
                d.sourceable = 0;
                print2buff(b"<S4 object>\0".as_ptr() as *const c_char, d);
            }
            d.left = prev_left;
            return;
        }

        // non-S4 cases:
        let sexp_type = TYPEOF(s);

        if sexp_type == SEXPTYPE::NILSXP {
            print2buff(b"NULL\0".as_ptr() as *const c_char, d);
        } else if sexp_type == SEXPTYPE::SYMSXP {
            let doquote = (d_opts_in & QUOTEEXPRESSIONS != 0) && {
                let pn = CHAR(PRINTNAME(s));
                !pn.is_null() && *pn != 0
            };
            if doquote {
                let attr = if (d_opts_in & SHOW_ATTR_OR_NMS) != 0 {
                    attr1(s, d)
                } else {
                    ATTR_SIMPLE
                };
                print2buff(b"quote(\0".as_ptr() as *const c_char, d);
                // Now print the name
                if (d_opts_in & S_COMPAT) != 0 {
                    let q = quotify(PRINTNAME(s), b'"' as c_int);
                    if !q.is_null() {
                        print2buff(q, d);
                    }
                } else if d.backtick != 0 {
                    let q = quotify(PRINTNAME(s), b'`' as c_int);
                    if !q.is_null() {
                        print2buff(q, d);
                    }
                } else {
                    let pn = CHAR(PRINTNAME(s));
                    if !pn.is_null() {
                        print2buff(pn, d);
                    }
                }
                print2buff(b")\0".as_ptr() as *const c_char, d);
                if attr >= ATTR_STRUC_ATTR {
                    attr2(s, d, attr == ATTR_STRUC_ATTR);
                }
            } else {
                if (d_opts_in & S_COMPAT) != 0 {
                    let q = quotify(PRINTNAME(s), b'"' as c_int);
                    if !q.is_null() {
                        print2buff(q, d);
                    }
                } else if d.backtick != 0 {
                    let q = quotify(PRINTNAME(s), b'`' as c_int);
                    if !q.is_null() {
                        print2buff(q, d);
                    }
                } else {
                    let pn = CHAR(PRINTNAME(s));
                    if !pn.is_null() {
                        print2buff(pn, d);
                    }
                }
            }
        } else if sexp_type == SEXPTYPE::CHARSXP {
            let name = CHAR(s);
            if !name.is_null() {
                print2buff(name, d);
            }
        } else if sexp_type == SEXPTYPE::SPECIALSXP || sexp_type == SEXPTYPE::BUILTINSXP {
            print2buff(b".Primitive(\"\0".as_ptr() as *const c_char, d);
            let pname = primname_c(s);
            print2buff(pname, d);
            print2buff(b"\")\0".as_ptr() as *const c_char, d);
        } else if sexp_type == SEXPTYPE::PROMSXP {
            if (d.opts & DELAYPROMISES) != 0 {
                d.sourceable = 0;
                print2buff(b"<promise: \0".as_ptr() as *const c_char, d);
                d.opts &= !QUOTEEXPRESSIONS;
                // PREXPR is not available, just print <promise>
                print2buff(b">\0".as_ptr() as *const c_char, d);
            } else {
                print2buff(b"<promise>\0".as_ptr() as *const c_char, d);
            }
        } else if sexp_type == SEXPTYPE::CLOSXP {
            let attr = if (d_opts_in & SHOW_ATTR_OR_NMS) != 0 {
                attr1(s, d)
            } else {
                ATTR_SIMPLE
            };
            let srcref_sym = Rf_install(b"srcref\0".as_ptr() as *const c_char);
            let t = getAttrib(s, srcref_sym);
            if (d.opts & USESOURCE != 0) && !isNull(t) {
                src2buff1(t, d);
            } else {
                d.opts &= SIMPLE_OPTS & !USESOURCE;
                print2buff(b"function (\0".as_ptr() as *const c_char, d);
                args2buff(FORMALS(s), 0, 1, d);
                print2buff(b") \0".as_ptr() as *const c_char, d);
                writeline(d);
                deparse2buff(BODY(s), d);
                d.opts = d_opts_in;
            }
            if attr >= ATTR_STRUC_ATTR {
                attr2(s, d, attr == ATTR_STRUC_ATTR);
            }
        } else if sexp_type == SEXPTYPE::ENVSXP {
            d.sourceable = 0;
            print2buff(b"<environment>\0".as_ptr() as *const c_char, d);
        } else if sexp_type == SEXPTYPE::VECSXP {
            let attr = if (d_opts_in & SHOW_ATTR_OR_NMS) != 0 {
                attr1(s, d)
            } else {
                ATTR_SIMPLE
            };
            print2buff(b"list(\0".as_ptr() as *const c_char, d);
            d.opts = d_opts_in;
            vec2buff(s, d, attr == ATTR_OK_NAMES || attr == ATTR_STRUC_ATTR);
            d.opts |= NICE_NAMES;
            print2buff(b")\0".as_ptr() as *const c_char, d);
            if attr >= ATTR_STRUC_ATTR {
                attr2(s, d, attr == ATTR_STRUC_ATTR);
            }
            d.opts = d_opts_in;
        } else if sexp_type == SEXPTYPE::EXPRSXP {
            let attr = if (d_opts_in & SHOW_ATTR_OR_NMS) != 0 {
                attr1(s, d)
            } else {
                ATTR_SIMPLE
            };
            if LENGTH(s) <= 0 {
                print2buff(b"expression()\0".as_ptr() as *const c_char, d);
            } else {
                let loc_opts = d.opts;
                print2buff(b"expression(\0".as_ptr() as *const c_char, d);
                d.opts &= SIMPLE_OPTS;
                vec2buff(s, d, attr == ATTR_OK_NAMES || attr == ATTR_STRUC_ATTR);
                d.opts = loc_opts;
                print2buff(b")\0".as_ptr() as *const c_char, d);
            }
            if attr >= ATTR_STRUC_ATTR {
                attr2(s, d, attr == ATTR_STRUC_ATTR);
            }
            d.opts = d_opts_in;
        } else if sexp_type == SEXPTYPE::LISTSXP {
            let attr = if (d_opts_in & SHOW_ATTR_OR_NMS) != 0 {
                attr1(s, d)
            } else {
                ATTR_SIMPLE
            };
            // Check for missing args
            let mut missing = false;
            let mut t = s;
            while !isNull(t) {
                if CAR(t) == R_MissingArg() {
                    missing = true;
                    break;
                }
                t = CDR(t);
            }
            if missing {
                print2buff(b"as.pairlist(alist(\0".as_ptr() as *const c_char, d);
            } else {
                print2buff(b"pairlist(\0".as_ptr() as *const c_char, d);
            }
            d.inlist += 1;
            let mut t = s;
            while !isNull(CDR(t)) {
                if !isNull(TAG(t)) {
                    d.opts = SIMPLEDEPARSE;
                    deparse2buff(TAG(t), d);
                    d.opts = d_opts_in;
                    print2buff(b" = \0".as_ptr() as *const c_char, d);
                }
                deparse2buff(CAR(t), d);
                print2buff(b", \0".as_ptr() as *const c_char, d);
                t = CDR(t);
            }
            if !isNull(TAG(t)) {
                d.opts = SIMPLEDEPARSE;
                deparse2buff(TAG(t), d);
                d.opts = d_opts_in;
                print2buff(b" = \0".as_ptr() as *const c_char, d);
            }
            deparse2buff(CAR(t), d);
            if missing {
                print2buff(b"))\0".as_ptr() as *const c_char, d);
            } else {
                print2buff(b")\0".as_ptr() as *const c_char, d);
            }
            d.inlist -= 1;
            if attr >= ATTR_STRUC_ATTR {
                attr2(s, d, attr == ATTR_STRUC_ATTR);
            }
        } else if sexp_type == SEXPTYPE::LANGSXP {
            if !isNull(ATTRIB(s)) {
                d.sourceable = 0;
            }
            let op = CAR(s);
            let mut doquote = false;
            let maybe_quote = (d_opts_in & QUOTEEXPRESSIONS) != 0;
            if maybe_quote {
                // do *not* quote() formulas (tilde):
                let is_tilde = isSymbol(op) && {
                    let pn = CHAR(PRINTNAME(op));
                    !pn.is_null() && streql(pn, b"~\0".as_ptr() as *const c_char)
                };
                doquote = !is_tilde;
                if doquote {
                    print2buff(b"quote(\0".as_ptr() as *const c_char, d);
                    d.opts &= SIMPLE_OPTS;
                } else {
                    d.opts &= !QUOTEEXPRESSIONS;
                }
            }

            if isSymbol(op) {
                let mut userbinop = 0;
                let symval = SYMVALUE(op);
                let symval_type = TYPEOF(symval);
                let is_builtin =
                    symval_type == SEXPTYPE::BUILTINSXP || symval_type == SEXPTYPE::SPECIALSXP;
                let syntax_pp = if is_builtin {
                    None
                } else {
                    getPPinfo_for_symbol(op)
                };
                if is_builtin {
                    userbinop = 0;
                } else if isUserBinop(op) {
                    userbinop = 1;
                } else {
                    userbinop = 0;
                }

                if is_builtin || userbinop != 0 || syntax_pp.is_some() {
                    let mut fop: PPinfo;
                    let s = CDR(s);
                    if userbinop != 0 {
                        let names_sym = Rf_install(b"names\0".as_ptr() as *const c_char);
                        if isNull(getAttrib(s, names_sym)) {
                            fop = PPinfo::new(PP_BINARY2, PREC_PERCENT, 0);
                        } else {
                            fop = PPinfo::new(PP_FUNCALL, 0, 0);
                        }
                    } else {
                        fop = syntax_pp.unwrap_or_else(|| getPPinfo(symval));
                    }

                    // Adjust kind based on argument count
                    match fop.kind {
                        PP_BINARY => {
                            let nargs = Rf_length(s);
                            match nargs {
                                1 => {
                                    fop.kind = PP_UNARY;
                                    if fop.prec == PREC_SUM {
                                        fop.prec = PREC_SIGN;
                                    }
                                }
                                2 => {}
                                _ => {
                                    fop.kind = PP_FUNCALL;
                                }
                            }
                        }
                        PP_BINARY2 => {
                            if Rf_length(s) != 2 {
                                fop.kind = PP_FUNCALL;
                            } else if userbinop != 0 {
                                fop.kind = PP_BINARY;
                            }
                        }
                        PP_DOLLAR => {
                            if Rf_length(s) != 2 {
                                fop.kind = PP_FUNCALL;
                            } else {
                                let rhs = CADR(s);
                                if !(isSymbol(rhs)
                                    || (isValidString(rhs) && !isNull(STRING_ELT(rhs, 0))))
                                {
                                    fop.kind = PP_FUNCALL;
                                }
                            }
                        }
                        _ => {} // intentionally unhandled: SEXPTYPE not relevant for function op detection
                    }

                    // Dispatch on operator kind
                    match fop.kind {
                        PP_IF => {
                            print2buff(b"if (\0".as_ptr() as *const c_char, d);
                            deparse2buff(CAR(s), d);
                            print2buff(b") \0".as_ptr() as *const c_char, d);
                            if d.incurly != 0 && d.inlist == 0 {
                                let lookahead = curlyahead(CADR(s));
                                if !lookahead {
                                    writeline(d);
                                    d.indent += 1;
                                }
                            }
                            if Rf_length(s) > 2 {
                                deparse2buff(CADR(s), d);
                                if d.incurly != 0 && d.inlist == 0 {
                                    writeline(d);
                                    if !curlyahead(CADR(s)) {
                                        d.indent -= 1;
                                    }
                                } else {
                                    print2buff(b" \0".as_ptr() as *const c_char, d);
                                }
                                print2buff(b"else \0".as_ptr() as *const c_char, d);
                                deparse2buff(CADDR(s), d);
                            } else {
                                deparse2buff(CADR(s), d);
                                if d.incurly != 0 && !curlyahead(CADR(s)) && d.inlist == 0 {
                                    d.indent -= 1;
                                }
                            }
                        }
                        PP_WHILE => {
                            print2buff(b"while (\0".as_ptr() as *const c_char, d);
                            deparse2buff(CAR(s), d);
                            print2buff(b") \0".as_ptr() as *const c_char, d);
                            deparse2buff(CADR(s), d);
                        }
                        PP_FOR => {
                            print2buff(b"for (\0".as_ptr() as *const c_char, d);
                            deparse2buff(CAR(s), d);
                            print2buff(b" in \0".as_ptr() as *const c_char, d);
                            deparse2buff(CADR(s), d);
                            print2buff(b") \0".as_ptr() as *const c_char, d);
                            deparse2buff(CADDR(s), d);
                        }
                        PP_REPEAT => {
                            print2buff(b"repeat \0".as_ptr() as *const c_char, d);
                            deparse2buff(CAR(s), d);
                        }
                        PP_CURLY => {
                            print2buff(b"{\0".as_ptr() as *const c_char, d);
                            d.incurly += 1;
                            d.indent += 1;
                            writeline(d);
                            let mut cur = s;
                            while !isNull(cur) {
                                deparse2buff(CAR(cur), d);
                                writeline(d);
                                cur = CDR(cur);
                            }
                            d.indent -= 1;
                            print2buff(b"}\0".as_ptr() as *const c_char, d);
                            d.incurly -= 1;
                        }
                        PP_PAREN => {
                            print2buff(b"(\0".as_ptr() as *const c_char, d);
                            deparse2buff(CAR(s), d);
                            print2buff(b")\0".as_ptr() as *const c_char, d);
                        }
                        PP_SUBSET => {
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CAR(s),
                                1,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            deparse2buff(CAR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            // Determine [ or [[
                            let primval = subset_primval(op, symval);
                            if primval == 1 {
                                print2buff(b"[\0".as_ptr() as *const c_char, d);
                            } else {
                                print2buff(b"[[\0".as_ptr() as *const c_char, d);
                            }
                            args2buff(CDR(s), 0, 0, d);
                            if primval == 1 {
                                print2buff(b"]\0".as_ptr() as *const c_char, d);
                            } else {
                                print2buff(b"]]\0".as_ptr() as *const c_char, d);
                            }
                        }
                        PP_FUNCALL | PP_RETURN => {
                            if d.backtick != 0 {
                                let q = quotify(PRINTNAME(op), b'`' as c_int);
                                if !q.is_null() {
                                    print2buff(q, d);
                                }
                            } else {
                                let q = quotify(PRINTNAME(op), b'"' as c_int);
                                if !q.is_null() {
                                    print2buff(q, d);
                                }
                            }
                            print2buff(b"(\0".as_ptr() as *const c_char, d);
                            d.inlist += 1;
                            args2buff(s, 0, 0, d);
                            d.inlist -= 1;
                            print2buff(b")\0".as_ptr() as *const c_char, d);
                        }
                        PP_FOREIGN => {
                            let pn = CHAR(PRINTNAME(op));
                            if !pn.is_null() {
                                print2buff(pn, d);
                            } // ASCII
                            print2buff(b"(\0".as_ptr() as *const c_char, d);
                            d.inlist += 1;
                            args2buff(s, 1, 0, d);
                            d.inlist -= 1;
                            print2buff(b")\0".as_ptr() as *const c_char, d);
                        }
                        PP_FUNCTION => {
                            if (d.opts & USESOURCE == 0) || !isString(CADDR(s)) {
                                let pn = CHAR(PRINTNAME(op));
                                if !pn.is_null() {
                                    print2buff(pn, d);
                                } // ASCII
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                                args2buff(FORMALS(s), 0, 1, d);
                                print2buff(b") \0".as_ptr() as *const c_char, d);
                                deparse2buff(CADR(s), d);
                            } else {
                                // Use source reference
                                let src = CADDR(s);
                                let n = LENGTH(src);
                                for i in 0..n as usize {
                                    let elt = STRING_ELT(src, i as R_xlen_t);
                                    let name = CHAR(elt);
                                    if !name.is_null() {
                                        print2buff(name, d);
                                    }
                                    writeline(d);
                                }
                            }
                        }
                        PP_ASSIGN | PP_ASSIGN2 => {
                            let op_name = CHAR(PRINTNAME(op));
                            let is_eq = !op_name.is_null()
                                && streql(op_name, b"=\0".as_ptr() as *const c_char);
                            let outerparens = fnarg && is_eq;
                            if outerparens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CAR(s),
                                1,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { fop.prec };
                            deparse2buff(CAR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            print2buff(b" \0".as_ptr() as *const c_char, d);
                            if !op_name.is_null() {
                                print2buff(op_name, d);
                            } // ASCII
                            print2buff(b" \0".as_ptr() as *const c_char, d);
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CADR(s),
                                0,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { prev_left };
                            deparse2buff(CADR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            if outerparens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            d.left = 0;
                        }
                        PP_DOLLAR => {
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CAR(s),
                                1,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { fop.prec };
                            deparse2buff(CAR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            let op_name = CHAR(PRINTNAME(op));
                            if !op_name.is_null() {
                                print2buff(op_name, d);
                            } // ASCII ($)
                            // Handle x$a's
                            let rhs = CADR(s);
                            if isString(rhs) {
                                let elt = STRING_ELT(rhs, 0);
                                if !elt.is_null() {
                                    let name = CHAR(elt);
                                    if !name.is_null() && isValidName(name) {
                                        deparse2buff(elt, d);
                                    } else {
                                        let parens = needsparens(
                                            fop.kind,
                                            fop.prec,
                                            fop.rightassoc,
                                            rhs,
                                            0,
                                            prev_left,
                                        );
                                        if parens {
                                            print2buff(b"(\0".as_ptr() as *const c_char, d);
                                        }
                                        d.left = if parens { 0 } else { prev_left };
                                        deparse2buff(rhs, d);
                                        if parens {
                                            print2buff(b")\0".as_ptr() as *const c_char, d);
                                        }
                                    }
                                }
                            } else {
                                let parens = needsparens(
                                    fop.kind,
                                    fop.prec,
                                    fop.rightassoc,
                                    rhs,
                                    0,
                                    prev_left,
                                );
                                if parens {
                                    print2buff(b"(\0".as_ptr() as *const c_char, d);
                                }
                                d.left = if parens { 0 } else { prev_left };
                                deparse2buff(rhs, d);
                                if parens {
                                    print2buff(b")\0".as_ptr() as *const c_char, d);
                                }
                            }
                            d.left = 0;
                        }
                        PP_BINARY => {
                            let mut lbreak = false;
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CAR(s),
                                1,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { fop.prec };
                            deparse2buff(CAR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            print2buff(b" \0".as_ptr() as *const c_char, d);
                            let op_name = CHAR(PRINTNAME(op));
                            if !op_name.is_null() {
                                print2buff(op_name, d);
                            } // ASCII
                            print2buff(b" \0".as_ptr() as *const c_char, d);
                            linebreak(&mut lbreak, d);
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CADR(s),
                                0,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { prev_left };
                            deparse2buff(CADR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            if lbreak {
                                d.indent -= 1;
                            }
                            d.left = 0;
                        }
                        PP_BINARY2 => {
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CAR(s),
                                1,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { fop.prec };
                            deparse2buff(CAR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            let op_name = CHAR(PRINTNAME(op));
                            if !op_name.is_null() {
                                print2buff(op_name, d);
                            } // ASCII
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CADR(s),
                                0,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { prev_left };
                            deparse2buff(CADR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            d.left = 0;
                        }
                        PP_UNARY => {
                            let op_name = CHAR(PRINTNAME(op));
                            if !op_name.is_null() {
                                print2buff(op_name, d);
                            } // ASCII
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CAR(s),
                                0,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { prev_left };
                            deparse2buff(CAR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            d.left = 0;
                        }
                        PP_BREAK => {
                            print2buff(b"break\0".as_ptr() as *const c_char, d);
                        }
                        PP_NEXT => {
                            print2buff(b"next\0".as_ptr() as *const c_char, d);
                        }
                        PP_SUBASS => {
                            if (d.opts & S_COMPAT) != 0 {
                                print2buff(b"\"\0".as_ptr() as *const c_char, d);
                                let op_name = CHAR(PRINTNAME(op));
                                if !op_name.is_null() {
                                    print2buff(op_name, d);
                                } // ASCII
                                print2buff(b"\'(\0".as_ptr() as *const c_char, d);
                            } else {
                                print2buff(b"`\0".as_ptr() as *const c_char, d);
                                let op_name = CHAR(PRINTNAME(op));
                                if !op_name.is_null() {
                                    print2buff(op_name, d);
                                } // ASCII
                                print2buff(b"`(\0".as_ptr() as *const c_char, d);
                            }
                            args2buff(s, 0, 0, d);
                            print2buff(b")\0".as_ptr() as *const c_char, d);
                        }
                        _ => {
                            d.sourceable = 0;
                        }
                    }
                } else {
                    // op is a symbol but not builtin/special/userbinop
                    let op_name = CHAR(PRINTNAME(op));
                    let val = if isSymbol(op) {
                        SYMVALUE(op)
                    } else {
                        R_NilValue()
                    };

                    if isSymbol(op)
                        && TYPEOF(val) == SEXPTYPE::CLOSXP
                        && !op_name.is_null()
                        && streql(op_name, b"::\0".as_ptr() as *const c_char)
                    {
                        deparse2buff(CADR(s), d);
                        print2buff(b"::\0".as_ptr() as *const c_char, d);
                        deparse2buff(CADDR(s), d);
                    } else if isSymbol(op)
                        && TYPEOF(val) == SEXPTYPE::CLOSXP
                        && !op_name.is_null()
                        && streql(op_name, b":::\0".as_ptr() as *const c_char)
                    {
                        deparse2buff(CADR(s), d);
                        print2buff(b":::\0".as_ptr() as *const c_char, d);
                        deparse2buff(CADDR(s), d);
                    } else {
                        if isSymbol(op) {
                            if (d.opts & S_COMPAT) != 0 {
                                let q = quotify(PRINTNAME(op), b'\'' as c_int);
                                if !q.is_null() {
                                    print2buff(q, d);
                                }
                            } else {
                                let q = quotify(PRINTNAME(op), b'`' as c_int);
                                if !q.is_null() {
                                    print2buff(q, d);
                                }
                            }
                        } else {
                            deparse2buff(CAR(s), d);
                        }
                        print2buff(b"(\0".as_ptr() as *const c_char, d);
                        args2buff(CDR(s), 0, 0, d);
                        print2buff(b")\0".as_ptr() as *const c_char, d);
                    }
                }
            } else if TYPEOF(op) == SEXPTYPE::CLOSXP
                || TYPEOF(op) == SEXPTYPE::SPECIALSXP
                || TYPEOF(op) == SEXPTYPE::BUILTINSXP
            {
                if parenthesizeCaller(op) {
                    print2buff(b"(\0".as_ptr() as *const c_char, d);
                    deparse2buff(op, d);
                    print2buff(b")\0".as_ptr() as *const c_char, d);
                } else {
                    deparse2buff(op, d);
                }
                print2buff(b"(\0".as_ptr() as *const c_char, d);
                args2buff(CDR(s), 0, 0, d);
                print2buff(b")\0".as_ptr() as *const c_char, d);
            } else {
                // lambda expression or other
                if parenthesizeCaller(op) {
                    print2buff(b"(\0".as_ptr() as *const c_char, d);
                    deparse2buff(op, d);
                    print2buff(b")\0".as_ptr() as *const c_char, d);
                } else {
                    deparse2buff(op, d);
                }
                print2buff(b"(\0".as_ptr() as *const c_char, d);
                args2buff(CDR(s), 0, 0, d);
                print2buff(b")\0".as_ptr() as *const c_char, d);
            }
            if maybe_quote {
                d.opts = d_opts_in;
                if doquote {
                    print2buff(b")\0".as_ptr() as *const c_char, d);
                }
            }
        } else if sexp_type == SEXPTYPE::LGLSXP
            || sexp_type == SEXPTYPE::INTSXP
            || sexp_type == SEXPTYPE::REALSXP
            || sexp_type == SEXPTYPE::CPLXSXP
            || sexp_type == SEXPTYPE::STRSXP
            || sexp_type == SEXPTYPE::RAWSXP
        {
            vector2buff(s, d);
        } else if sexp_type == SEXPTYPE::EXTPTRSXP {
            print2buff(b"<pointer: 0x0>\0".as_ptr() as *const c_char, d);
        } else if sexp_type == SEXPTYPE::BCODESXP {
            let source = crate::eval::bc_eval::BCODE_EXPR(s);
            if !isNull(source) {
                deparse2buff(source, d);
            } else {
                d.sourceable = 0;
                print2buff(b"<bytecode>\0".as_ptr() as *const c_char, d);
            }
        } else if sexp_type == SEXPTYPE::WEAKREFSXP {
            d.sourceable = 0;
            print2buff(b"<weak reference>\0".as_ptr() as *const c_char, d);
        } else if sexp_type == SEXPTYPE::OBJSXP {
            // A bare OBJSXP, e.g. from S7; objects with the S4 bit are
            // dealt with above.  Deparse to .OBJSXP() or
            // structure(.OBJSXP(), <attrs>).
            if (d_opts_in & SHOW_ATTR_OR_NMS) != 0 {
                let srcref_sym = Rf_install(b"srcref\0".as_ptr() as *const c_char);
                let mut a = ATTRIB(s);
                print2buff(b"structure(.OBJSXP()\0".as_ptr() as *const c_char, d);
                while !isNull(a) {
                    if TAG(a) != srcref_sym {
                        print2buff(b", \0".as_ptr() as *const c_char, d);
                        attrEntry(a, d);
                    }
                    a = CDR(a);
                }
            }
            print2buff(b")\0".as_ptr() as *const c_char, d);
        } else {
            d.sourceable = 0;
        }

        d.left = prev_left;
    }
}

// ---------------------------------------------------------------------------
