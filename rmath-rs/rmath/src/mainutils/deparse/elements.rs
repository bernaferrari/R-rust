#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use super::*;

// deparse2buf_name — deparse a vector element name to buffer
// ---------------------------------------------------------------------------

/// Deparse a name from a names vector to the buffer, with quoting as needed.
pub unsafe fn deparse2buf_name(nv: SEXP, i: c_int, d: *mut LocalParseData) {
    unsafe {
        if isNull(nv) {
            return;
        }
        let d = &mut *d;
        let elt = STRING_ELT(nv, i as R_xlen_t);
        if isNull(elt) {
            return;
        }
        let name = CHAR(elt);
        if name.is_null() || *name == 0 {
            return;
        } // length test

        if !name.is_null() && isValidName(name) {
            deparse2buff(elt, d);
        } else if d.backtick != 0 {
            print2buff(b"`\0".as_ptr() as *const c_char, d);
            deparse2buff(elt, d);
            print2buff(b"`\0".as_ptr() as *const c_char, d);
        } else {
            print2buff(b"\"\0".as_ptr() as *const c_char, d);
            deparse2buff(elt, d);
            print2buff(b"\"\0".as_ptr() as *const c_char, d);
        }
        print2buff(b" = \0".as_ptr() as *const c_char, d);
    }
}

// ---------------------------------------------------------------------------
// EncodeNonFiniteComplexElement — encode non-finite complex number
// ---------------------------------------------------------------------------

/// Encode a complex value with non-finite components as a syntactically
/// correct string (using complex(real=..., imaginary=...) form).
pub unsafe fn EncodeNonFiniteComplexElement(x: Rcomplex, buff: *mut c_char) -> *const c_char {
    unsafe {
        // Simplified implementation: format real and imaginary parts
        let mut re_buf = [0 as libc::c_char; 64];
        let mut im_buf = [0 as libc::c_char; 64];
        if R_FINITE(x.r) {
            libc::snprintf(
                re_buf.as_mut_ptr(),
                64,
                b"%.17g\0".as_ptr() as *const c_char,
                x.r,
            );
        } else if ISNAN(x.r) {
            libc::snprintf(re_buf.as_mut_ptr(), 64, b"NaN\0".as_ptr() as *const c_char);
        } else {
            libc::snprintf(re_buf.as_mut_ptr(), 64, b"Inf\0".as_ptr() as *const c_char);
        }
        if R_FINITE(x.i) {
            libc::snprintf(
                im_buf.as_mut_ptr(),
                64,
                b"%.17g\0".as_ptr() as *const c_char,
                x.i,
            );
        } else if ISNAN(x.i) {
            libc::snprintf(im_buf.as_mut_ptr(), 64, b"NaN\0".as_ptr() as *const c_char);
        } else {
            libc::snprintf(im_buf.as_mut_ptr(), 64, b"Inf\0".as_ptr() as *const c_char);
        }
        libc::snprintf(
            buff,
            NB2 as usize,
            b"complex(real=%s, imaginary=%s)\0".as_ptr() as *const c_char,
            re_buf.as_ptr(),
            im_buf.as_ptr(),
        );
        buff
    }
}

// ---------------------------------------------------------------------------
// Format helpers for vectors
// ---------------------------------------------------------------------------

/// Format an integer element as a string.
pub unsafe fn format_int_element(val: c_int) -> *const c_char {
    unsafe {
        with_deparse_runtime(|state| {
            let buf = &mut state.int_buf;
            if val == NA_INTEGER {
                libc::snprintf(buf.as_mut_ptr(), 32, b"NA\0".as_ptr() as *const c_char);
            } else {
                libc::snprintf(buf.as_mut_ptr(), 32, b"%d\0".as_ptr() as *const c_char, val);
            }
            buf.as_ptr() as *const c_char
        })
    }
}

/// Format a logical element as a string.
pub unsafe fn format_logical_element(val: c_int) -> *const c_char {
    unsafe {
        with_deparse_runtime(|state| {
            let buf = &mut state.logical_buf;
            if val == NA_INTEGER {
                libc::snprintf(buf.as_mut_ptr(), 8, b"NA\0".as_ptr() as *const c_char);
            } else if val != 0 {
                buf[0] = b'T' as c_char;
                buf[1] = b'R' as c_char;
                buf[2] = b'U' as c_char;
                buf[3] = b'E' as c_char;
                buf[4] = 0;
            } else {
                buf[0] = b'F' as c_char;
                buf[1] = b'A' as c_char;
                buf[2] = b'L' as c_char;
                buf[3] = b'S' as c_char;
                buf[4] = b'E' as c_char;
                buf[5] = 0;
            }
            buf.as_ptr() as *const c_char
        })
    }
}

/// Format a real element as a string with maximal precision.
pub unsafe fn write_real_element(buf: &mut [c_char; 64], val: f64) {
    unsafe {
        if ISNAN(val) && (val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN) {
            libc::snprintf(buf.as_mut_ptr(), 64, b"NA\0".as_ptr() as *const c_char);
        } else if ISNAN(val) {
            libc::snprintf(buf.as_mut_ptr(), 64, b"NaN\0".as_ptr() as *const c_char);
        } else if !R_FINITE(val) {
            if val > 0.0 {
                libc::snprintf(buf.as_mut_ptr(), 64, b"Inf\0".as_ptr() as *const c_char);
            } else {
                libc::snprintf(buf.as_mut_ptr(), 64, b"-Inf\0".as_ptr() as *const c_char);
            }
        } else {
            libc::snprintf(
                buf.as_mut_ptr(),
                64,
                b"%.17g\0".as_ptr() as *const c_char,
                val,
            );
        }
    }
}

pub unsafe fn format_real_element(val: f64) -> *const c_char {
    unsafe {
        with_deparse_runtime(|state| {
            let buf = &mut state.real_buf;
            write_real_element(buf, val);
            buf.as_ptr() as *const c_char
        })
    }
}

/// Format a string element with quoting.
pub unsafe fn format_string_element(s: SEXP) -> *const c_char {
    unsafe {
        with_deparse_runtime(|state| {
            let buf = &mut state.string_buf;
            if s.is_null() || s == R_NilValue() {
                buf[0] = b'N';
                buf[1] = b'A';
                buf[2] = 0;
                return buf.as_ptr() as *const c_char;
            }
            let name = CHAR(s);
            if name.is_null() {
                buf[0] = b'N';
                buf[1] = b'A';
                buf[2] = 0;
                return buf.as_ptr() as *const c_char;
            }
            let bytes = std::ffi::CStr::from_ptr(name).to_bytes();
            let mut pos = 0;
            buf[pos] = b'"';
            pos += 1;
            for &b in bytes.iter() {
                if pos + 2 >= 2046 {
                    break;
                }
                match b {
                    b'"' | b'\\' => {
                        buf[pos] = b'\\';
                        pos += 1;
                        buf[pos] = b;
                        pos += 1;
                    }
                    b'\n' => {
                        buf[pos] = b'\\';
                        pos += 1;
                        buf[pos] = b'n';
                        pos += 1;
                    }
                    b'\r' => {
                        buf[pos] = b'\\';
                        pos += 1;
                        buf[pos] = b'r';
                        pos += 1;
                    }
                    b'\t' => {
                        buf[pos] = b'\\';
                        pos += 1;
                        buf[pos] = b't';
                        pos += 1;
                    }
                    _ => {
                        buf[pos] = b;
                        pos += 1;
                    }
                }
            }
            buf[pos] = b'"';
            pos += 1;
            buf[pos] = 0;
            buf.as_ptr() as *const c_char
        })
    }
}

/// Format a raw element as hex.
pub unsafe fn format_raw_element(val: Rbyte) -> *const c_char {
    unsafe {
        with_deparse_runtime(|state| {
            let buf = &mut state.raw_buf;
            libc::snprintf(
                buf.as_mut_ptr(),
                8,
                b"0x%02x\0".as_ptr() as *const c_char,
                val as c_uint,
            );
            buf.as_ptr() as *const c_char
        })
    }
}

// ---------------------------------------------------------------------------
// vector2buff — deparse atomic vectors to buffer
// ---------------------------------------------------------------------------

/// Deparse atomic vectors (LGLSXP, INTSXP, REALSXP, CPLXSXP, STRSXP, RAWSXP).
pub unsafe fn vector2buff(vector: SEXP, d: *mut LocalParseData) {
    unsafe {
        let d = &mut *d;
        let d_opts_in = d.opts;
        let tlen = LENGTH(vector);
        let quote = if TYPEOF(vector) == SEXPTYPE::STRSXP {
            b'"' as c_int
        } else {
            0
        };
        let mut surround = false;

        // Check for integer sequences (m:n)
        let mut int_seq = false;
        if TYPEOF(vector) == SEXPTYPE::INTSXP && tlen > 1 {
            let vec = INTEGER(vector);
            if !vec.is_null() {
                let v0 = *vec;
                let v1 = *vec.add(1);
                if v0 != NA_INTEGER && v1 != NA_INTEGER {
                    let d_i = (v1 as f64) - (v0 as f64);
                    if d_i.abs() == 1.0 {
                        int_seq = true;
                        for i in 2..tlen as usize {
                            let vi = *vec.add(i);
                            if vi == NA_INTEGER {
                                int_seq = false;
                                break;
                            }
                            let diff = (vi as f64) - (*vec.add(i - 1) as f64);
                            if (diff - d_i).abs() > 1e-10 {
                                int_seq = false;
                                break;
                            }
                        }
                    }
                }
            }
        }

        let names_sym = Rf_install(b"names\0".as_ptr() as *const c_char);
        let srcref_sym = Rf_install(b"srcref\0".as_ptr() as *const c_char);
        let mut nv = R_NilValue();
        let mut do_names = (d_opts_in & SHOW_ATTR_OR_NMS) != 0;
        if do_names {
            nv = getAttrib(vector, names_sym);
            if isNull(nv) {
                do_names = false;
            }
        }
        let _nv_guard = protect(nv);

        let mut str_names = false;
        let need_c = tlen > 1;
        str_names = do_names && (int_seq || tlen == 0);
        if str_names {
            d.opts &= !NICE_NAMES;
        }
        let attr = if (d_opts_in & SHOW_ATTR_OR_NMS) != 0 {
            attr1(vector, d)
        } else {
            ATTR_SIMPLE
        };
        if do_names {
            do_names = attr == ATTR_OK_NAMES || attr == ATTR_STRUC_ATTR;
        }

        if tlen == 0 {
            match TYPEOF(vector) {
                10 => print2buff(b"logical(0)\0".as_ptr() as *const c_char, d), // LGLSXP
                13 => print2buff(b"integer(0)\0".as_ptr() as *const c_char, d), // INTSXP
                14 => print2buff(b"numeric(0)\0".as_ptr() as *const c_char, d), // REALSXP
                15 => print2buff(b"complex(0)\0".as_ptr() as *const c_char, d), // CPLXSXP
                16 => print2buff(b"character(0)\0".as_ptr() as *const c_char, d), // STRSXP
                24 => print2buff(b"raw(0)\0".as_ptr() as *const c_char, d),     // RAWSXP
                _ => {} // intentionally unhandled: unsupported type for empty vector display
            }
        } else if TYPEOF(vector) == SEXPTYPE::INTSXP {
            if int_seq {
                let vec = INTEGER(vector);
                if !vec.is_null() {
                    let strp = format_int_element(*vec);
                    print2buff(strp, d);
                    print2buff(b":\0".as_ptr() as *const c_char, d);
                    let strp = format_int_element(*vec.add((tlen - 1) as usize));
                    print2buff(strp, d);
                }
            } else {
                let vec = INTEGER(vector);
                let add_l = (d.opts & KEEPINTEGER != 0) && (d.opts & S_COMPAT == 0);
                let mut all_na = (d.opts & KEEPNA != 0) || add_l;
                if !vec.is_null() {
                    for i in 0..tlen as usize {
                        if *vec.add(i) != NA_INTEGER {
                            all_na = false;
                            break;
                        }
                    }
                }
                if (d.opts & KEEPINTEGER != 0) && (d.opts & S_COMPAT != 0) {
                    print2buff(b"as.integer(\0".as_ptr() as *const c_char, d);
                    surround = true;
                }
                all_na = all_na && (d.opts & S_COMPAT == 0);
                if need_c {
                    print2buff(b"c(\0".as_ptr() as *const c_char, d);
                }
                if !vec.is_null() {
                    for i in 0..tlen as usize {
                        if do_names {
                            deparse2buf_name(nv, i as c_int, d);
                        }
                        if all_na && *vec.add(i) == NA_INTEGER {
                            print2buff(b"NA_integer_\0".as_ptr() as *const c_char, d);
                        } else {
                            let strp = format_int_element(*vec.add(i));
                            print2buff(strp, d);
                            if add_l && *vec.add(i) != NA_INTEGER {
                                print2buff(b"L\0".as_ptr() as *const c_char, d);
                            }
                        }
                        if i < (tlen as usize) - 1 {
                            print2buff(b", \0".as_ptr() as *const c_char, d);
                        }
                        if tlen > 1 && d.len > d.cutoff {
                            writeline(d);
                        }
                        if !d.active {
                            break;
                        }
                    }
                }
                if need_c {
                    print2buff(b")\0".as_ptr() as *const c_char, d);
                }
                if surround {
                    print2buff(b")\0".as_ptr() as *const c_char, d);
                }
            }
        } else {
            // tlen > 0; not INTSXP
            let mut all_na = d.opts & KEEPNA != 0;

            // Handle NA-heavy types
            if (d.opts & KEEPNA != 0) && TYPEOF(vector) == SEXPTYPE::REALSXP {
                let vec = REAL(vector);
                if !vec.is_null() {
                    for i in 0..tlen as usize {
                        if !ISNAN(*vec.add(i)) {
                            all_na = false;
                            break;
                        }
                    }
                }
                if all_na && (d.opts & S_COMPAT != 0) {
                    print2buff(b"as.double(\0".as_ptr() as *const c_char, d);
                    surround = true;
                }
            } else if (d.opts & KEEPNA != 0) && TYPEOF(vector) == SEXPTYPE::CPLXSXP {
                let vec = COMPLEX(vector);
                if !vec.is_null() {
                    for i in 0..tlen as usize {
                        let c = *vec.add(i);
                        if !ISNAN(c.r) && !ISNAN(c.i) {
                            all_na = false;
                            break;
                        }
                    }
                }
                if all_na && (d.opts & S_COMPAT != 0) {
                    print2buff(b"as.complex(\0".as_ptr() as *const c_char, d);
                    surround = true;
                }
            } else if TYPEOF(vector) == SEXPTYPE::RAWSXP {
                print2buff(b"as.raw(\0".as_ptr() as *const c_char, d);
                surround = true;
            }

            if need_c {
                print2buff(b"c(\0".as_ptr() as *const c_char, d);
            }
            all_na = all_na && (d.opts & S_COMPAT == 0);

            for i in 0..tlen as usize {
                if do_names {
                    deparse2buf_name(nv, i as c_int, d);
                }

                let mut strp: *const c_char = ptr::null();

                match TYPEOF(vector) {
                    10 => {
                        // LGLSXP
                        let vec = LOGICAL(vector);
                        if !vec.is_null() {
                            if all_na && *vec.add(i) == NA_INTEGER {
                                strp = b"NA\0".as_ptr() as *const c_char;
                            } else {
                                strp = format_logical_element(*vec.add(i));
                            }
                        }
                    }
                    14 => {
                        // REALSXP
                        let vec = REAL(vector);
                        if !vec.is_null() {
                            let v = *vec.add(i);
                            if all_na
                                && ISNAN(v)
                                && (v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN)
                            {
                                strp = b"NA_real_\0".as_ptr() as *const c_char;
                            } else if (d.opts & HEXNUMERIC != 0) && R_FINITE(v) {
                                with_deparse_runtime(|state| {
                                    let hex_buf = &mut state.hex_buf;
                                    libc::snprintf(
                                        hex_buf.as_mut_ptr(),
                                        64,
                                        b"%a\0".as_ptr() as *const c_char,
                                        v,
                                    );
                                    strp = hex_buf.as_ptr() as *const c_char;
                                });
                            } else if (d.opts & DIGITS17 != 0) && R_FINITE(v) {
                                with_deparse_runtime(|state| {
                                    let dig_buf = &mut state.dig_buf;
                                    libc::snprintf(
                                        dig_buf.as_mut_ptr(),
                                        64,
                                        b"%.17g\0".as_ptr() as *const c_char,
                                        v,
                                    );
                                    strp = dig_buf.as_ptr() as *const c_char;
                                });
                            } else {
                                strp = format_real_element(v);
                            }
                        }
                    }
                    15 => {
                        // CPLXSXP
                        let vec = COMPLEX(vector);
                        if !vec.is_null() {
                            let c = *vec.add(i);
                            if all_na && ISNAN(c.r) && ISNAN(c.i) {
                                strp = b"NA_complex_\0".as_ptr() as *const c_char;
                            } else if ISNAN(c.r) || !R_FINITE(c.i) {
                                with_deparse_runtime(|state| {
                                    let cplx_buf = &mut state.cplx_buf;
                                    strp = EncodeNonFiniteComplexElement(c, cplx_buf.as_mut_ptr());
                                });
                            } else if (d.opts & HEXNUMERIC != 0) && R_FINITE(c.r) && R_FINITE(c.i) {
                                with_deparse_runtime(|state| {
                                    let hex_cplx = &mut state.hex_cplx;
                                    libc::snprintf(
                                        hex_cplx.as_mut_ptr(),
                                        128,
                                        b"%a + %ai\0".as_ptr() as *const c_char,
                                        c.r,
                                        c.i,
                                    );
                                    strp = hex_cplx.as_ptr() as *const c_char;
                                });
                            } else if (d.opts & DIGITS17 != 0) && R_FINITE(c.r) && R_FINITE(c.i) {
                                with_deparse_runtime(|state| {
                                    let dig_cplx = &mut state.dig_cplx;
                                    libc::snprintf(
                                        dig_cplx.as_mut_ptr(),
                                        128,
                                        b"%.17g%+.17gi\0".as_ptr() as *const c_char,
                                        c.r,
                                        c.i,
                                    );
                                    strp = dig_cplx.as_ptr() as *const c_char;
                                });
                            } else {
                                with_deparse_runtime(|state| {
                                    let cplx_buf2 = &mut state.cplx_buf2;
                                    let mut re_buf = [0 as c_char; 64];
                                    let mut im_buf = [0 as c_char; 64];
                                    write_real_element(&mut re_buf, c.r);
                                    write_real_element(&mut im_buf, c.i);
                                    libc::snprintf(
                                        cplx_buf2.as_mut_ptr(),
                                        256,
                                        b"%s%s%si\0".as_ptr() as *const c_char,
                                        re_buf.as_ptr(),
                                        if c.i >= 0.0 {
                                            b"+\0".as_ptr() as *const c_char
                                        } else {
                                            b"\0".as_ptr() as *const c_char
                                        },
                                        im_buf.as_ptr(),
                                    );
                                    strp = cplx_buf2.as_ptr() as *const c_char;
                                });
                            }
                        }
                    }
                    16 => {
                        // STRSXP
                        let elt = STRING_ELT(vector, i as R_xlen_t);
                        if all_na && (elt.is_null() || elt == R_NilValue()) {
                            strp = b"NA_character_\0".as_ptr() as *const c_char;
                        } else {
                            strp = format_string_element(elt);
                        }
                    }
                    24 => {
                        // RAWSXP
                        let vec = RAW(vector);
                        if !vec.is_null() {
                            strp = format_raw_element(*vec.add(i));
                        }
                    }
                    _ => {} // intentionally unhandled: unsupported SEXPTYPE for element formatting
                }

                if !strp.is_null() {
                    print2buff(strp, d);
                }
                if i < (tlen as usize) - 1 {
                    print2buff(b", \0".as_ptr() as *const c_char, d);
                }
                if tlen > 1 && d.len > d.cutoff {
                    writeline(d);
                }
                if !d.active {
                    break;
                }
            }

            if need_c {
                print2buff(b")\0".as_ptr() as *const c_char, d);
            }
            if surround {
                print2buff(b")\0".as_ptr() as *const c_char, d);
            }
        }
        if attr >= ATTR_STRUC_ATTR {
            attr2(vector, d, attr == ATTR_STRUC_ATTR);
        }
        if str_names {
            d.opts = d_opts_in;
        }
    }
}

// ---------------------------------------------------------------------------
// vec2buff — deparse list/expression vectors to buffer
// ---------------------------------------------------------------------------

/// Deparse vectors of S-expressions (list() and expression() objects).
pub unsafe fn vec2buff(v: SEXP, d: *mut LocalParseData, do_names: bool) {
    unsafe {
        let d = &mut *d;
        let mut lbreak = false;
        let n = LENGTH(v);
        let names_sym = Rf_install(b"names\0".as_ptr() as *const c_char);
        let srcref_sym = Rf_install(b"srcref\0".as_ptr() as *const c_char);
        let mut nv = R_NilValue();
        let mut do_names = do_names;
        if do_names {
            nv = getAttrib(v, names_sym);
            if isNull(nv) {
                do_names = false;
            }
        }
        let _nv_guard = protect(nv);

        let mut sv = R_NilValue();
        if (d.opts & USESOURCE) != 0 {
            sv = getAttrib(v, srcref_sym);
            if TYPEOF(sv) != SEXPTYPE::VECSXP {
                sv = R_NilValue();
            }
        }

        for i in 0..n as usize {
            if i > 0 {
                print2buff(b", \0".as_ptr() as *const c_char, d);
            }
            linebreak(&mut lbreak, d);
            if do_names {
                deparse2buf_name(nv, i as c_int, d);
            }
            if !src2buff(sv, i as c_int, d) {
                deparse2buff(VECTOR_ELT(v, i as R_xlen_t), d);
            }
        }
        if lbreak {
            d.indent -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// args2buff — deparse argument list to buffer
// ---------------------------------------------------------------------------

/// Deparse an argument list (pairlist) to the buffer.
///
/// Handles named and unnamed arguments, default values for formals, and
/// line breaking for long argument lists.
pub unsafe fn args2buff(arglist: SEXP, _lineb: c_int, formals: c_int, d: *mut LocalParseData) {
    unsafe {
        let d = &mut *d;
        let mut lbreak = false;
        let mut cur = arglist;

        while !isNull(cur) {
            if TYPEOF(cur) != SEXPTYPE::LISTSXP && TYPEOF(cur) != SEXPTYPE::LANGSXP {
                break;
            }
            if !isNull(TAG(cur)) {
                let s = TAG(cur);
                if s == R_DotsSymbol() {
                    let pn = CHAR(PRINTNAME(s));
                    if !pn.is_null() {
                        print2buff(pn, d);
                    }
                } else if d.backtick != 0 {
                    let q = quotify(PRINTNAME(s), b'`' as c_int);
                    if !q.is_null() {
                        print2buff(q, d);
                    }
                } else {
                    let q = quotify(PRINTNAME(s), b'"' as c_int);
                    if !q.is_null() {
                        print2buff(q, d);
                    }
                }
                if formals != 0 {
                    if !isNull(CAR(cur)) && CAR(cur) != R_MissingArg() {
                        print2buff(b" = \0".as_ptr() as *const c_char, d);
                        d.fnarg = true;
                        deparse2buff(CAR(cur), d);
                    }
                } else {
                    print2buff(b" = \0".as_ptr() as *const c_char, d);
                    if !isNull(CAR(cur)) && CAR(cur) != R_MissingArg() {
                        d.fnarg = true;
                        deparse2buff(CAR(cur), d);
                    }
                }
            } else {
                d.fnarg = true;
                deparse2buff(CAR(cur), d);
            }
            cur = CDR(cur);
            if !isNull(cur) {
                print2buff(b", \0".as_ptr() as *const c_char, d);
                linebreak(&mut lbreak, d);
            }
        }
        if lbreak {
            d.indent -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
