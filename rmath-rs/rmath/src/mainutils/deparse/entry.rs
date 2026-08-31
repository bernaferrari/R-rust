#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use super::*;

// deparse2 — setup and call deparse2buff
// ---------------------------------------------------------------------------

/// Setup deparsing state and call the recursive deparse2buff.
pub unsafe fn deparse2(what: SEXP, svec: SEXP, d: *mut LocalParseData) {
    unsafe {
        let d = &mut *d;
        d.strvec = svec;
        d.linenumber = 0;
        d.indent = 0;
        deparse2buff(what, d);
        writeline(d);
    }
}

// ---------------------------------------------------------------------------
// deparse1WithCutoff — core deparse engine with configurable cutoff
// ---------------------------------------------------------------------------

/// Core deparsing routine with configurable line width cutoff.
///
/// Equivalent to C's `deparse1WithCutoff()`. If abbrev is true, returns a
/// single string with at most 13 characters (for plot labelling).
#[allow(clippy::field_reassign_with_default)]
pub unsafe fn deparse1WithCutoff(
    call: SEXP,
    abbrev: bool,
    cutoff: c_int,
    backtick: bool,
    opts: c_int,
    nlines: c_int,
) -> SEXP {
    unsafe {
        let mut local_data = LocalParseData::default();
        local_data.cutoff = cutoff;
        local_data.backtick = if backtick { 1 } else { 0 };
        local_data.opts = opts;
        local_data.strvec = R_NilValue();

        // Ensure buffer allocation
        print2buff(b"\0".as_ptr() as *const c_char, &mut local_data);

        let mut svec = R_NilValue();
        let mut need_ellipses = false;

        if nlines > 0 {
            local_data.linenumber = nlines;
            local_data.maxlines = nlines;
        } else {
            let browse_lines = get_browse_lines();
            if browse_lines > 0 {
                local_data.maxlines = browse_lines + 1;
            }
            deparse2(call, svec, &mut local_data);
            local_data.active = true;
            let browse_lines = get_browse_lines();
            if browse_lines > 0 && local_data.linenumber > browse_lines {
                local_data.linenumber = browse_lines + 1;
                need_ellipses = true;
            }
        }

        svec = Rf_allocVector(SEXPTYPE::STRSXP, local_data.linenumber);
        let _svec_guard = protect(svec);

        deparse2(call, svec, &mut local_data);

        if abbrev {
            let mut data = [0u8; 14];
            let first = STRING_ELT(svec, 0);
            if !first.is_null() {
                let name = CHAR(first);
                if !name.is_null() {
                    let bytes = std::ffi::CStr::from_ptr(name).to_bytes();
                    let copy_len = std::cmp::min(bytes.len(), 10);
                    data[..copy_len].copy_from_slice(&bytes[..copy_len]);
                    data[copy_len] = 0;
                    if bytes.len() > 10 {
                        data[10] = b'.';
                        data[11] = b'.';
                        data[12] = b'.';
                        data[13] = 0;
                    } else {
                        data[copy_len] = 0;
                    }
                }
            }
            let result = Rf_mkString(data.as_ptr() as *const c_char);
            R_FreeStringBuffer(&mut local_data.buffer);
            return result;
        } else if need_ellipses {
            let ellipsis = Rf_mkChar(b"  ...\0".as_ptr() as *const c_char);
            SET_STRING_ELT(svec, get_browse_lines() as R_xlen_t, ellipsis);
        }

        R_FreeStringBuffer(&mut local_data.buffer);
        svec
    }
}

// ---------------------------------------------------------------------------
// do_deparse — .Internal(deparse(expr, width.cutoff, backtick, .deparseOpts(control), nlines))
// ---------------------------------------------------------------------------

/// Implementation of R's `deparse()` function.
///
/// This is the equivalent of R's `do_deparse()` from deparse.c.
/// It converts an R expression to a character vector representation.
pub unsafe fn do_deparse(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, rho);
        let mut args = args;

        let expr = CAR(args);
        args = CDR(args);

        let mut cut0 = DEFAULT_CUTOFF;
        if !isNull(CAR(args)) {
            let v = Rf_asInteger(CAR(args));
            if v == NA_INTEGER || v < MIN_CUTOFF || v > MAX_CUTOFF {
                cut0 = DEFAULT_CUTOFF;
            } else {
                cut0 = v;
            }
        }
        args = CDR(args);

        let backtick = !isNull(CAR(args)) && Rf_asLogical(CAR(args)) != 0;
        args = CDR(args);

        let opts = if isNull(CAR(args)) {
            DEFAULT_USER_DEPARSE
        } else {
            Rf_asInteger(CAR(args))
        };
        args = CDR(args);

        let mut nlines = Rf_asInteger(CAR(args));
        if nlines == NA_INTEGER {
            nlines = -1;
        }

        deparse1WithCutoff(expr, false, cut0, backtick, opts, nlines)
    }
}

// ---------------------------------------------------------------------------
// do_dput — .Internal(dput(x, file, .deparseOpts(control)))
// ---------------------------------------------------------------------------

/// Implementation of R's `dput()` function.
///
/// Writes a deparsed representation of an R object to a file or connection.
/// Port of `do_dput` in deparse.c.
pub unsafe fn do_dput(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::relop::checkArity(_op, args);
        let tval_raw = CAR(args);
        let sfile = CADR(args);
        let opts_arg = CADDR(args);
        let opts = if isNull(opts_arg) {
            SHOWATTRIBUTES
        } else {
            Rf_asInteger(opts_arg)
        };

        let tval = deparse1(tval_raw, false, opts);
        let _tval_guard = protect(tval);

        // Write to stdout (connection index 1) or a connection
        let ifile = crate::mainutils::coerce::asInteger(sfile);
        if ifile == 1 {
            for i in 0..LENGTH(tval) {
                let s = CHAR(STRING_ELT(tval, i as R_xlen_t));
                if !s.is_null() {
                    let bytes = std::ffi::CStr::from_ptr(s).to_bytes();
                    let line = String::from_utf8_lossy(bytes);
                    println!("{}", line);
                }
            }
        } else if ifile >= 3 {
            // Write to a connection
            let con_sexp = sfile;
            let lines_sexp = tval;
            // Build a STRSXP with newlines appended for writeLines
            let n = LENGTH(lines_sexp);
            let text = Rf_allocVector(SEXPTYPE::STRSXP, n);
            let _text_guard = protect(text);
            for i in 0..n as R_xlen_t {
                SET_STRING_ELT(text, i, STRING_ELT(lines_sexp, i));
            }
            crate::mainutils::connections::do_writeLines(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                Rf_cons(
                    text,
                    Rf_cons(
                        con_sexp,
                        Rf_cons(Rf_mkString(b"\n\0".as_ptr() as *const c_char), R_NilValue()),
                    ),
                ),
                R_NilValue(),
            );
        }

        CAR(args)
    }
}

/// Implementation of R's `dump()` function.
///
/// Writes deparsed representations of named R objects to a file or connection.
/// Port of `do_dump` in deparse.c.
pub unsafe fn do_dump(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::relop::checkArity(_op, args);
        let names = CAR(args);
        let sfile = CADR(args);
        let _source = CADDR(args);
        let opts = Rf_asInteger(CADDDR(args));
        let _evaluate = CAR(CDR(CDR(CDR(CDR(args)))));

        if !isString(names) {
            return R_NilValue();
        }
        let nobjs = LENGTH(names);
        if nobjs < 1 {
            return R_NilValue();
        }

        let ifile = crate::mainutils::coerce::asInteger(sfile);

        for i in 0..nobjs as R_xlen_t {
            let name_charsxp = STRING_ELT(names, i);
            if name_charsxp.is_null() {
                continue;
            }
            let obj_name = CHAR(name_charsxp);
            if obj_name.is_null() {
                continue;
            }
            let name_str = std::ffi::CStr::from_ptr(obj_name)
                .to_string_lossy()
                .into_owned();

            // Deparse the object — in this port we deparse the name itself as a symbol
            let sym = Rf_install(obj_name);
            let tval = deparse1(
                sym,
                false,
                if opts == NA_INTEGER {
                    DEFAULTDEPARSE
                } else {
                    opts
                },
            );
            let _tval_guard = protect(tval);

            if ifile == 1 {
                if isValidName(obj_name) {
                    println!("{} <-", name_str);
                } else {
                    println!("`{}` <-", name_str);
                }
                for j in 0..LENGTH(tval) {
                    let s = CHAR(STRING_ELT(tval, j as R_xlen_t));
                    if !s.is_null() {
                        let bytes = std::ffi::CStr::from_ptr(s).to_bytes();
                        let line = String::from_utf8_lossy(bytes);
                        println!("{}", line);
                    }
                }
            }
        }

        let outnames = Rf_allocVector(SEXPTYPE::STRSXP, nobjs);
        for i in 0..nobjs as R_xlen_t {
            SET_STRING_ELT(outnames, i, STRING_ELT(names, i));
        }
        outnames
    }
}

// ---------------------------------------------------------------------------
// deparse1 — deparse with R_BrowseLines := 0
// ---------------------------------------------------------------------------

/// Deparse an expression with default cutoff (60), no line limit.
///
/// Used in bind.c, builtin.c, coerce.c, match.c, relop.c, and do_dput/do_dump.
pub unsafe fn deparse1(call: SEXP, abbrev: bool, opts: c_int) -> SEXP {
    unsafe {
        let old_bl = get_browse_lines();
        set_browse_lines(0);
        let result = deparse1WithCutoff(call, abbrev, DEFAULT_CUTOFF, true, opts, 0);
        set_browse_lines(old_bl);
        result
    }
}

/// Deparse a symbolic object (call, expression, symbol) with the R-level
/// `deparse()` defaults: cutoff 60, keepNA/keepInteger/niceNames/
/// showAttributes. `backtick` selects symbol-name quoting — format.default
/// deparses calls/expressions with backtick=TRUE and names with
/// backtick=FALSE, while str.default's deParse always uses the default.
pub unsafe fn deparse_symbolic(call: SEXP, backtick: bool) -> SEXP {
    unsafe {
        deparse1WithCutoff(
            call,
            false,
            DEFAULT_CUTOFF,
            backtick,
            DEFAULT_USER_DEPARSE,
            0,
        )
    }
}

// ---------------------------------------------------------------------------
// deparse1m — deparse looking at getOption("deparse.max.lines")
// ---------------------------------------------------------------------------

/// Deparse with default cutoff, respecting getOption("deparse.max.lines").
///
/// Unimplemented: requires getOption infrastructure.
pub unsafe fn deparse1m(call: SEXP, abbrev: bool, opts: c_int) -> SEXP {
    unsafe {
        let old_bl = get_browse_lines();
        let max_lines = {
            let val = crate::mainutils::options::GetOption(
                b"deparse.max.lines\0".as_ptr() as *const c_char
            );
            let n = crate::mainutils::coerce::asInteger(val);
            if n == NA_INTEGER { 100 } else { n }
        };
        set_browse_lines(max_lines);
        let result = deparse1WithCutoff(call, abbrev, DEFAULT_CUTOFF, true, opts, 0);
        set_browse_lines(old_bl);
        result
    }
}

// ---------------------------------------------------------------------------
// deparse1w — deparse for print() (uses R_print.cutoff)
// ---------------------------------------------------------------------------

/// Deparse for printing language objects (uses R_print.cutoff, nlines = -1).
///
/// Used in print.c for PrintLanguage, PrintClosure, PrintExpression.
pub unsafe fn deparse1w(call: SEXP, abbrev: bool, opts: c_int) -> SEXP {
    unsafe {
        // Use DEFAULT_CUTOFF since R_print.cutoff is not yet available as a global
        deparse1WithCutoff(call, abbrev, DEFAULT_CUTOFF, true, opts, -1)
    }
}

// ---------------------------------------------------------------------------
// deparse1line — concatenate all deparse lines into one
// ---------------------------------------------------------------------------

/// Deparse and concatenate all lines into a single string.
///
/// Used for non-trivial list entries in as.character(<list>) and in
/// terms.formula where a term label must be a single line.
pub unsafe fn deparse1line(call: SEXP, abbrev: bool) -> SEXP {
    unsafe {
        let temp = deparse1WithCutoff(call, abbrev, MAX_CUTOFF, true, SIMPLEDEPARSE, -1);
        let _temp_guard = protect(temp);
        let lines = LENGTH(temp);
        if lines > 1 {
            // Calculate total length
            let mut total_len: usize = 0;
            for i in 0..lines as usize {
                let s = STRING_ELT(temp, i as R_xlen_t);
                if !s.is_null() {
                    let name = CHAR(s);
                    if !name.is_null() {
                        total_len += libc::strlen(name);
                    }
                }
                total_len += 1; // newline
            }
            // Allocate buffer and concatenate
            let mut buf = vec![0u8; total_len + 1];
            let mut pos = 0;
            for i in 0..lines as usize {
                let s = STRING_ELT(temp, i as R_xlen_t);
                if !s.is_null() {
                    let name = CHAR(s);
                    if !name.is_null() {
                        let bytes = std::ffi::CStr::from_ptr(name).to_bytes();
                        for &b in bytes.iter() {
                            if pos < buf.len() {
                                buf[pos] = b;
                            }
                            pos += 1;
                        }
                    }
                }
                if i < (lines as usize) - 1 && pos < buf.len() {
                    buf[pos] = b'\n';
                    pos += 1;
                }
            }
            if pos < buf.len() {
                buf[pos] = 0;
            }
            let result = Rf_mkString(buf.as_ptr() as *const c_char);
            result
        } else {
            temp
        }
    }
}

// ---------------------------------------------------------------------------
// deparse1s — deparse for error/warning messages (single line)
// ---------------------------------------------------------------------------

/// Deparse for error/warning messages (single line, default deparse options).
///
/// Used in errors.c for warningcall_dflt() and PrintWarnings().
pub unsafe fn deparse1s(call: SEXP) -> SEXP {
    unsafe { deparse1WithCutoff(call, false, DEFAULT_CUTOFF, true, DEFAULTDEPARSE, 1) }
}

// ---------------------------------------------------------------------------
// R_inspect — inspect an R object (from inspect.c)
// ---------------------------------------------------------------------------

/// Inspect an R object, returning a string representation.
///
/// Unimplemented: requires full inspect infrastructure.
pub unsafe fn R_inspect(s: SEXP, deep: c_int, pvec: SEXP) -> c_int {
    let _ = (s, deep, pvec);
    0
}

/// R_inspect3 — inspect with additional options.
///
/// Unimplemented: requires full inspect infrastructure.
pub unsafe fn R_inspect3(
    s: SEXP,
    deep: c_int,
    pvec: SEXP,
    writefun: SEXP,
    callfun: SEXP,
    env: SEXP,
) -> c_int {
    let _ = (s, deep, pvec, writefun, callfun, env);
    0
}

// ---------------------------------------------------------------------------
// con_cleanup — connection cleanup handler (for do_dput/do_dump)
// ---------------------------------------------------------------------------

/// Connection cleanup handler used in do_dput and do_dump.
/// Closes the connection identified by the data pointer (an INTSXP containing
/// the connection index) if it was opened by the deparse routine.
///
/// Port of `con_cleanup` in deparse.c:378.
pub unsafe fn con_cleanup(data: *mut std::ffi::c_void) {
    unsafe {
        if data.is_null() {
            return;
        }
        let scon = data as SEXP;
        if scon.is_null() {
            return;
        }
        crate::mainutils::connections::do_close(
            scon,
            std::ptr::null_mut(),
            scon,
            std::ptr::null_mut(),
        );
    }
}

// ---------------------------------------------------------------------------
// Additional helper stubs needed by other modules
// ---------------------------------------------------------------------------

/// Rf_isValidName — check if a string is a valid R name.
///
/// Exported for use by other modules.
pub unsafe fn Rf_isValidName(s: *const c_char) -> c_int {
    unsafe { if isValidName(s) { 1 } else { 0 } }
}
