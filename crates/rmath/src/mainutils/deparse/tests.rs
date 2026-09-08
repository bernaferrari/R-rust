#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use super::*;
use crate::sexp::session::RSession;

#[test]
fn test_constants() {
    assert_eq!(BUFSIZE, 512);
    assert_eq!(MIN_CUTOFF, 20);
    assert_eq!(DEFAULT_CUTOFF, 60);
    assert_eq!(MAX_CUTOFF, BUFSIZE - 12);
}

#[test]
fn test_deparse_option_flags() {
    assert!(KEEPNA == 1);
    assert!(KEEPINTEGER == 2);
    assert!(SHOWATTRIBUTES == 4);
    assert!(USESOURCE == 8);
    assert!(DELAYPROMISES == 16);
    assert!(S_COMPAT == 32);
    assert!(QUOTEEXPRESSIONS == 64);
    assert!(HEXNUMERIC == 128);
    assert!(DIGITS17 == 256);
    assert!(NICE_NAMES == 512);
    assert!(WARNINCOMPLETE == 1024);
}

#[test]
fn test_precedence_constants() {
    assert!(PREC_COMPARE < PREC_SUM);
    assert!(PREC_SUM < PREC_SIGN);
    assert!(PREC_SUBSET > PREC_SIGN);
}

#[test]
fn test_ppinfo_kinds_are_distinct() {
    let kinds = [
        PP_BINARY,
        PP_BINARY2,
        PP_UNARY,
        PP_SUBSET,
        PP_SUBASS,
        PP_DOLLAR,
        PP_ASSIGN,
        PP_ASSIGN2,
        PP_IF,
        PP_WHILE,
        PP_FOR,
        PP_REPEAT,
        PP_FUNCALL,
        PP_RETURN,
        PP_PAREN,
        PP_CURLY,
        PP_FOREIGN,
        PP_FUNCTION,
        PP_BREAK,
        PP_NEXT,
    ];
    // Verify all kinds are distinct
    for i in 0..kinds.len() {
        for j in (i + 1)..kinds.len() {
            assert_ne!(kinds[i], kinds[j], "PPinfo kinds must be distinct");
        }
    }
}

#[test]
fn test_attr_type_constants() {
    assert_eq!(ATTR_UNKNOWN, -1);
    assert_eq!(ATTR_SIMPLE, 0);
    assert_eq!(ATTR_OK_NAMES, 1);
    assert_eq!(ATTR_STRUC_ATTR, 2);
    assert_eq!(ATTR_STRUC_NMS_A, 3);
}

#[test]
fn test_local_parse_data_default() {
    let d = LocalParseData::default();
    assert_eq!(d.linenumber, 0);
    assert_eq!(d.len, 0);
    assert_eq!(d.incurly, 0);
    assert_eq!(d.inlist, 0);
    assert!(d.startline);
    assert_eq!(d.indent, 0);
    assert_eq!(d.cutoff, DEFAULT_CUTOFF);
    assert_eq!(d.backtick, 0);
    assert_eq!(d.opts, 0);
    assert_eq!(d.sourceable, 1);
    assert_eq!(d.maxlines, c_int::MAX);
    assert!(d.active);
    assert_eq!(d.isS4, 0);
    assert!(!d.fnarg);
}

#[test]
fn test_do_deparse_simple_expr() {
    let _session = RSession::new();
    unsafe {
        // Create a simple expression: 1L
        let expr = Rf_ScalarInteger(1);
        let _expr_guard = protect(expr);
        let args = Rf_cons(
            expr,
            Rf_cons(
                Rf_ScalarInteger(DEFAULT_CUTOFF),
                Rf_cons(
                    Rf_ScalarLogical(0),
                    Rf_cons(
                        Rf_ScalarInteger(SHOWATTRIBUTES),
                        Rf_cons(Rf_ScalarInteger(-1), R_NilValue()),
                    ),
                ),
            ),
        );
        let _args_guard = protect(args);
        let result = do_deparse(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        // Should return a character vector (STRSXP)
        assert!(!result.is_null());
    }
}

#[test]
fn test_do_dput_returns_nil() {
    let _session = RSession::new();
    unsafe {
        let result = do_dput(
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        assert!(result.is_null() || result == R_NilValue());
    }
}

#[test]
fn test_do_dump_returns_nil() {
    unsafe {
        let result = do_dump(
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        assert!(result.is_null() || result == R_NilValue());
    }
}

#[test]
fn test_simple_opts_mask() {
    let opts = KEEPNA | KEEPINTEGER | USESOURCE | S_COMPAT | WARNINCOMPLETE | NICE_NAMES;
    assert_eq!(opts & SIMPLE_OPTS, opts);
}

#[test]
fn test_show_attr_or_nms() {
    assert_eq!(SHOW_ATTR_OR_NMS, SHOWATTRIBUTES | NICE_NAMES);
}

#[test]
fn test_deparse_objsxp_structure_form() {
    let _session = RSession::new();
    unsafe {
        // A bare OBJSXP (e.g. from S7 / .OBJSXP()) deparses to the
        // trunk structure() form — not the old "<object>" placeholder.
        let x = crate::mainutils::objects::R_allocObject();
        let _x_guard = protect(x);
        let out = deparse1s(x);
        assert!(!out.is_null() && TYPEOF(out) == SEXPTYPE::STRSXP);
        assert_eq!(
            std::ffi::CStr::from_ptr(CHAR(STRING_ELT(out, 0))).to_string_lossy(),
            "structure(.OBJSXP())"
        );

        // Attributes go through attrEntry(): `foo = 42`.
        crate::eval::attrib_core::setAttrib(
            x,
            Rf_install(b"foo\0".as_ptr() as *const c_char),
            Rf_ScalarReal(42.0),
        );
        let out2 = deparse1s(x);
        assert_eq!(
            std::ffi::CStr::from_ptr(CHAR(STRING_ELT(out2, 0))).to_string_lossy(),
            "structure(.OBJSXP(), foo = 42)"
        );
    }
}

#[test]
fn test_browse_lines_initial() {
    let _session = RSession::new();
    assert_eq!(get_browse_lines(), 0);
}

#[test]
fn test_is_valid_name() {
    unsafe {
        assert!(isValidName(b"foo\0".as_ptr() as *const c_char));
        assert!(isValidName(b".foo\0".as_ptr() as *const c_char));
        assert!(isValidName(b"foo_bar\0".as_ptr() as *const c_char));
        assert!(isValidName(b"foo.bar\0".as_ptr() as *const c_char));
        assert!(isValidName(b"foo1\0".as_ptr() as *const c_char));
        assert!(!isValidName(b"1foo\0".as_ptr() as *const c_char));
        assert!(!isValidName(b"foo bar\0".as_ptr() as *const c_char));
        assert!(!isValidName(b"\0".as_ptr() as *const c_char));
        assert!(!isValidName(ptr::null()));
    }
}

#[test]
fn test_streql() {
    unsafe {
        assert!(streql(
            b"foo\0".as_ptr() as *const c_char,
            b"foo\0".as_ptr() as *const c_char
        ));
        assert!(!streql(
            b"foo\0".as_ptr() as *const c_char,
            b"bar\0".as_ptr() as *const c_char
        ));
        assert!(!streql(ptr::null(), b"foo\0".as_ptr() as *const c_char));
        assert!(!streql(b"foo\0".as_ptr() as *const c_char, ptr::null()));
    }
}

#[test]
fn test_ppinfo_values_from_names_rs() {
    // Verify our PPkind constants match names.rs
    assert_eq!(PP_BINARY, N_PP_BINARY);
    assert_eq!(PP_UNARY, N_PP_UNARY);
    assert_eq!(PP_SUBSET, N_PP_SUBSET);
    assert_eq!(PP_DOLLAR, N_PP_DOLLAR);
    assert_eq!(PP_IF, N_PP_IF);
    assert_eq!(PP_WHILE, N_PP_WHILE);
    assert_eq!(PP_FOR, N_PP_FOR);
    assert_eq!(PP_REPEAT, N_PP_REPEAT);
    assert_eq!(PP_FUNCALL, N_PP_FUNCALL);
    assert_eq!(PP_RETURN, N_PP_RETURN);
    assert_eq!(PP_PAREN, N_PP_PAREN);
    assert_eq!(PP_CURLY, N_PP_CURLY);
    assert_eq!(PP_FUNCTION, N_PP_FUNCTION);
    assert_eq!(PP_BREAK, N_PP_BREAK);
    assert_eq!(PP_NEXT, N_PP_NEXT);
    // Verify precedence values match
    assert_eq!(PREC_COMPARE, N_PREC_COMPARE);
    assert_eq!(PREC_SUM, N_PREC_SUM);
    assert_eq!(PREC_SIGN, N_PREC_SIGN);
    assert_eq!(PREC_PERCENT, N_PREC_PERCENT);
    assert_eq!(PREC_SUBSET, N_PREC_SUBSET);
}
