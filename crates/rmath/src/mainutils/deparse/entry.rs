#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use super::*;
use crate::sexp::owner::{OwnerPin, OwnerToken, with_runtime};

/// Cleanup retains the exact original allocation, including after revocation.
/// It never reads ambient runtime state or invokes callbacks while unwinding.
struct RestorePrint {
    old: crate::mainutils::format::RPrint,
    owner: OwnerPin,
}

impl RestorePrint {
    fn enter(owner: OwnerPin) -> Self {
        let replacement = crate::mainutils::format::RPrint {
            digits: 15,
            scipen: 0,
            na_width: 2,
            na_width_noquote: 2,
        };
        let old = unsafe {
            std::mem::replace(&mut (*owner.as_ptr()).eval_state.format_print, replacement)
        };
        Self { old, owner }
    }
}

impl Drop for RestorePrint {
    fn drop(&mut self) {
        unsafe { (*self.owner.as_ptr()).eval_state.format_print = self.old };
    }
}

struct RestoreBrowseLines {
    old: c_int,
    owner: OwnerPin,
}

impl RestoreBrowseLines {
    fn enter(owner: OwnerPin, value: c_int) -> Self {
        let old = unsafe {
            std::mem::replace(
                &mut (*owner.as_ptr()).eval_state.deparse.browse_lines,
                value,
            )
        };
        Self { old, owner }
    }
}

impl Drop for RestoreBrowseLines {
    fn drop(&mut self) {
        unsafe { (*self.owner.as_ptr()).eval_state.deparse.browse_lines = self.old };
    }
}

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
        let allocation_error =
            || -> ! { buffer::buffer_error(owned_line_buffer::LineBufferError::Allocation) };
        let token = OwnerToken::current().unwrap_or_else(|_| allocation_error());
        let runtime = token.weak_owner().unwrap_or_else(|| allocation_error());
        let original_pin = runtime.pin().unwrap_or_else(|_| allocation_error());
        let result = with_runtime(&runtime, |access| {
            let domain = access.domain();
            let input = domain.wrap(call).unwrap_or_else(|_| allocation_error());
            // Install the original owning lease before mutating print state.
            let _restore = RestorePrint::enter(original_pin);
            let allocator = access
                .allocator(&domain)
                .unwrap_or_else(|_| allocation_error());

            let mut local_data = LocalParseData::default();
            local_data.cutoff = cutoff;
            local_data.backtick = if backtick { 1 } else { 0 };
            local_data.opts = opts;
            local_data.strvec = R_NilValue();

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
                access
                    .with_native(|_| {
                        deparse2(input.as_raw(), svec, &mut local_data);
                        Ok(())
                    })
                    .unwrap_or_else(|_| allocation_error());
                local_data.active = true;
                let browse_lines = get_browse_lines();
                if browse_lines > 0 && local_data.linenumber > browse_lines {
                    local_data.linenumber = browse_lines + 1;
                    need_ellipses = true;
                }
            }

            let mut output = allocator
                .allocate(|arena| {
                    Some(arena.alloc_vector(SEXPTYPE::STRSXP, local_data.linenumber.into()))
                })
                .unwrap_or_else(|_| allocation_error());
            svec = output.as_raw();

            access
                .with_native(|_| {
                    deparse2(input.as_raw(), svec, &mut local_data);
                    Ok(())
                })
                .unwrap_or_else(|_| allocation_error());
            if nlines > 0
                && local_data.linenumber > 0
                && (local_data.linenumber as i64) < nlines as i64
            {
                let used = local_data.linenumber;
                let shrunk = allocator
                    .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, used.into())))
                    .unwrap_or_else(|_| allocation_error());
                let mut shrunk = crate::sexp::object::SexpMut::try_from_checked(shrunk)
                    .unwrap_or_else(|_| allocation_error());
                for i in 0..used {
                    let character = output
                        .try_string_elt(i as i64)
                        .unwrap_or_else(|_| allocation_error());
                    shrunk
                        .try_set_string_elt(i as i64, character)
                        .unwrap_or_else(|_| allocation_error());
                }
                // The actual result stays owned through warning handlers and all
                // later allocations, even after the original output is dropped.
                output = shrunk.freeze();
                svec = output.as_raw();
            }

            if abbrev {
                let mut data = [0u8; 14];
                let first = output
                    .clone()
                    .try_string_elt(0)
                    .unwrap_or_else(|_| allocation_error());
                let first = first.as_raw();
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
                let bytes = std::ffi::CStr::from_ptr(data.as_ptr().cast()).to_bytes();
                return allocator
                    .allocate(|arena| {
                        crate::sexp::builder::scalar_bytes_in(arena, bytes)
                            .map(|value| value.as_raw())
                    })
                    .unwrap_or_else(|_| allocation_error());
            } else if need_ellipses {
                let ellipsis = allocator
                    .character("  ...")
                    .unwrap_or_else(|_| allocation_error());
                let mut result = crate::sexp::object::SexpMut::try_from_checked(output.clone())
                    .unwrap_or_else(|_| allocation_error());
                result
                    .try_set_string_elt(get_browse_lines() as R_xlen_t, ellipsis)
                    .unwrap_or_else(|_| allocation_error());
            }

            if (opts & WARNINCOMPLETE) != 0 && local_data.sourceable == 0 {
                access
                    .with_native(|_| {
                        crate::mainutils::errors::Rf_warning1(
                            c"deparse may be incomplete".as_ptr(),
                        );
                        Ok(())
                    })
                    .unwrap_or_else(|_| allocation_error());
            }

            output
        })
        .unwrap_or_else(|_| allocation_error());
        result.as_raw()
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
        let parsed = deparse_call_args(args);
        deparse1WithCutoff(
            parsed.expr,
            false,
            parsed.cutoff,
            parsed.backtick,
            parsed.opts,
            parsed.nlines,
        )
    }
}

struct DeparseCallArgs {
    expr: SEXP,
    cutoff: c_int,
    backtick: bool,
    opts: c_int,
    nlines: c_int,
}

unsafe fn deparse_call_args(args: SEXP) -> DeparseCallArgs {
    unsafe {
        let mut expr = crate::sexp::globals::R_NilValue();
        let mut cutoff = DEFAULT_CUTOFF;
        let mut backtick = None;
        let mut opts = None;
        let mut nlines = -1;
        let mut positional = Vec::new();
        let mut current = args;
        while !current.is_null() && current != crate::sexp::globals::R_NilValue() {
            let value = CAR(current);
            let tag = TAG(current);
            let name = if !tag.is_null()
                && tag != crate::sexp::globals::R_NilValue()
                && TYPEOF(tag) == SEXPTYPE::SYMSXP
            {
                let chars = CHAR(PRINTNAME(tag));
                if chars.is_null() {
                    None
                } else {
                    Some(
                        std::ffi::CStr::from_ptr(chars)
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
            } else {
                None
            };
            match name.as_deref() {
                Some("expr") => expr = value,
                Some("width.cutoff") => {
                    let v = crate::mainutils::coerce::asInteger(value);
                    if v != NA_INTEGER && v >= MIN_CUTOFF && v <= MAX_CUTOFF {
                        cutoff = v;
                    }
                }
                Some("backtick") => {
                    backtick = Some(crate::mainutils::coerce::asLogical(value) != 0)
                }
                Some("control") => opts = Some(deparse_opts_from_control(value)),
                Some("nlines") => {
                    let v = crate::mainutils::coerce::asInteger(value);
                    nlines = if v == NA_INTEGER { -1 } else { v };
                }
                _ => positional.push(value),
            }
            current = CDR(current);
        }
        let mut pos = 0;
        if expr.is_null() || expr == crate::sexp::globals::R_NilValue() {
            if let Some(value) = positional.get(pos).copied() {
                expr = value;
                pos += 1;
            }
        }
        if let Some(value) = positional.get(pos).copied() {
            if TYPEOF(value) == SEXPTYPE::STRSXP && opts.is_none() {
                // `deparse(x, "all")` or leftover positional control
                opts = Some(deparse_opts_from_control(value));
            } else if TYPEOF(value) == SEXPTYPE::INTSXP || TYPEOF(value) == SEXPTYPE::REALSXP {
                let v = crate::mainutils::coerce::asInteger(value);
                if v != NA_INTEGER && v >= MIN_CUTOFF && v <= MAX_CUTOFF {
                    cutoff = v;
                } else if opts.is_none() && v != NA_INTEGER {
                    opts = Some(v);
                }
            }
            pos += 1;
        }
        if backtick.is_none() {
            if let Some(value) = positional.get(pos).copied() {
                if TYPEOF(value) == SEXPTYPE::LGLSXP {
                    backtick = Some(crate::mainutils::coerce::asLogical(value) != 0);
                    pos += 1;
                }
            }
        }
        if opts.is_none() {
            if let Some(value) = positional.get(pos).copied() {
                if TYPEOF(value) == SEXPTYPE::STRSXP {
                    opts = Some(deparse_opts_from_control(value));
                } else if TYPEOF(value) == SEXPTYPE::INTSXP || TYPEOF(value) == SEXPTYPE::REALSXP {
                    opts = Some(crate::mainutils::coerce::asInteger(value));
                }
                pos += 1;
            }
        }
        if let Some(value) = positional.get(pos).copied() {
            if TYPEOF(value) == SEXPTYPE::INTSXP || TYPEOF(value) == SEXPTYPE::REALSXP {
                let v = crate::mainutils::coerce::asInteger(value);
                nlines = if v == NA_INTEGER { -1 } else { v };
            }
        }
        let backtick = backtick.unwrap_or_else(|| deparse_default_backtick(expr));
        DeparseCallArgs {
            expr,
            cutoff,
            backtick,
            opts: opts.unwrap_or(DEFAULT_USER_DEPARSE),
            nlines,
        }
    }
}

fn deparse_default_backtick(expr: SEXP) -> bool {
    unsafe {
        matches!(
            TYPEOF(expr),
            t if t == SEXPTYPE::LANGSXP
                || t == SEXPTYPE::EXPRSXP
                || t == SEXPTYPE::CLOSXP
        )
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
        let token = OwnerToken::current().unwrap_or_else(|_| {
            buffer::buffer_error(owned_line_buffer::LineBufferError::Allocation)
        });
        let pin = token
            .weak_owner()
            .and_then(|owner| owner.pin().ok())
            .unwrap_or_else(|| {
                buffer::buffer_error(owned_line_buffer::LineBufferError::Allocation)
            });
        let _restore = RestoreBrowseLines::enter(pin, 0);
        deparse1WithCutoff(call, abbrev, DEFAULT_CUTOFF, true, opts, 0)
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
        let allocation_error =
            || -> ! { buffer::buffer_error(owned_line_buffer::LineBufferError::Allocation) };
        let token = OwnerToken::current().unwrap_or_else(|_| allocation_error());
        let runtime = token.weak_owner().unwrap_or_else(|| allocation_error());
        let pin = runtime.pin().unwrap_or_else(|_| allocation_error());
        let result = with_runtime(&runtime, |access| {
            let domain = access.domain();
            let input = domain.wrap(call).unwrap_or_else(|_| allocation_error());
            let value = access
                .with_native(|_| {
                    Ok(crate::mainutils::options::GetOption(
                        c"deparse.max.lines".as_ptr(),
                    ))
                })
                .unwrap_or_else(|_| allocation_error());
            let value = domain.wrap(value).unwrap_or_else(|_| allocation_error());
            let n = access
                .with_native(|_| Ok(crate::mainutils::coerce::asInteger(value.as_raw())))
                .unwrap_or_else(|_| allocation_error());
            let _restore = RestoreBrowseLines::enter(pin, if n == NA_INTEGER { 100 } else { n });
            let raw = access
                .with_native(|_| {
                    Ok(deparse1WithCutoff(
                        input.as_raw(),
                        abbrev,
                        DEFAULT_CUTOFF,
                        true,
                        opts,
                        0,
                    ))
                })
                .unwrap_or_else(|_| allocation_error());
            domain.wrap(raw).unwrap_or_else(|_| allocation_error())
        })
        .unwrap_or_else(|_| allocation_error());
        result.as_raw()
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
                        total_len += std::ffi::CStr::from_ptr(name).to_bytes().len();
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

#[cfg(test)]
mod ownership_tests {
    use super::*;
    use crate::sexp::object::{SessionNodeFactory, Sexp};
    use crate::sexp::session::RSession;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    fn scalar(factory: &SessionNodeFactory<'_>) -> Sexp<'static> {
        factory
            .allocate(|arena| {
                crate::sexp::builder::scalar_integer_in(arena, 7).map(|value| value.as_raw())
            })
            .unwrap()
            .into_owned()
            .unwrap()
    }

    unsafe fn install_original_state(owner: *mut crate::sexp::instance::RInstance) {
        unsafe {
            (*owner).eval_state.format_print = crate::mainutils::format::RPrint {
                digits: 6,
                scipen: 8,
                na_width: 17,
                na_width_noquote: 22,
            };
            (*owner).eval_state.deparse.browse_lines = 9;
            (*owner).memory_state.gc_force_gap = 1;
            (*owner).memory_state.gc_force_wait = 1;
        }
    }

    unsafe fn assert_original_state(owner: *mut crate::sexp::instance::RInstance) {
        unsafe {
            let print = (*owner).eval_state.format_print;
            assert_eq!(
                (
                    print.digits,
                    print.scipen,
                    print.na_width,
                    print.na_width_noquote
                ),
                (6, 8, 17, 22)
            );
            assert_eq!((*owner).eval_state.deparse.browse_lines, 9);
        }
    }

    #[test]
    fn owned_error_deparse_restores_original_state_after_callback_switches_runtime() {
        let original = RSession::new_for_gc_tests();
        let original_owner = original.owner_token().unwrap().weak_owner().unwrap();
        let factory = original_owner.node_factory().unwrap();
        let input = scalar(&factory);
        let other = RSession::new_for_gc_tests();
        let other_pin = other
            .owner_token()
            .unwrap()
            .weak_owner()
            .unwrap()
            .pin()
            .unwrap();
        let other_pointer = other_pin.as_ptr();
        unsafe {
            (*other_pointer).eval_state.format_print.digits = 3;
        }
        original.with_active_in(|owner| unsafe {
            install_original_state(owner);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                assert_eq!((*owner).eval_state.format_print.digits, 15);
                crate::sexp::instance::set_current_instance(other_pointer);
                assert_eq!(
                    crate::sexp::instance::current_instance_ptr(),
                    Some(other_pointer)
                );
            }));
            // GC notifications run inside with_instance_active. Its owning
            // scope restores the original runtime after each callback, making
            // successful publication correct once availability is rechecked.
            let result = factory
                .wrap(deparse1(input.as_raw(), false, DEFAULTDEPARSE))
                .unwrap();
            assert_eq!(crate::sexp::instance::current_instance_ptr(), Some(owner));
            assert_eq!(result.string_value_elt(0), Some(Some("7L".into())));
            assert_original_state(owner);
            assert_eq!((*other_pointer).eval_state.format_print.digits, 3);
        });
    }

    #[test]
    fn owned_error_deparse_restores_original_state_after_callback_revokes_runtime() {
        let original = RSession::new_for_gc_tests();
        let factory = original.owner_token().unwrap().node_factory();
        let input = scalar(&factory);
        original.with_active_in(|owner| unsafe {
            install_original_state(owner);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                assert_eq!((*owner).eval_state.format_print.digits, 15);
                crate::sexp::instance::revoke_instance_availability(owner);
            }));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                deparse1(input.as_raw(), false, DEFAULTDEPARSE)
            }));
            assert!(result.is_err());
            assert!(result.unwrap_err().is::<crate::sexp::context::RError>());
            assert_original_state(owner);
        });
    }

    #[test]
    fn owned_error_deparse_restores_pinned_original_after_callback_drops_facade() {
        let original = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
        let runtime = original
            .borrow()
            .as_ref()
            .unwrap()
            .owner_token()
            .unwrap()
            .weak_owner()
            .unwrap();
        let pin = runtime.pin().unwrap();
        let owner = pin.as_ptr();
        let factory = runtime.node_factory().unwrap();
        let input = scalar(&factory);
        let raw_input = input.as_raw();
        let facade = Rc::downgrade(&original);
        unsafe {
            install_original_state(owner);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                assert_eq!((*owner).eval_state.format_print.digits, 15);
                drop(facade.upgrade().unwrap().borrow_mut().take());
            }));
        }
        drop(input);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            deparse1(raw_input, false, DEFAULTDEPARSE)
        }));
        assert!(result.is_err());
        assert!(result.unwrap_err().is::<crate::sexp::context::RError>());
        assert!(original.borrow().is_none());
        assert!(!runtime.is_live());
        unsafe {
            assert_original_state(owner);
        }
    }

    #[test]
    fn owned_error_deparse_input_and_output_survive_reentrant_collection() {
        let original = RSession::new_for_gc_tests();
        let factory = original.owner_token().unwrap().node_factory();
        let input = scalar(&factory);
        let raw_input = input.as_raw();
        let input_node = crate::sexp::memory::checked_projection(raw_input)
            .unwrap()
            .1;
        let observed = input_node.clone();
        let notifications = Rc::new(Cell::new(0));
        let calls = notifications.clone();
        original.with_active_in(|owner| unsafe {
            install_original_state(owner);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                calls.set(calls.get() + 1);
                assert_eq!((*owner).eval_state.format_print.digits, 15);
                assert!(!crate::sexp::memory::is_arena_lent(owner));
                crate::sexp::gengc::full_gc();
                assert!(observed.is_live());
            }));
            drop(input);
            let output = factory
                .wrap(deparse1(raw_input, false, DEFAULTDEPARSE))
                .unwrap();
            assert_original_state(owner);
            assert_eq!(output.string_value_elt(0), Some(Some("7L".into())));
            assert!(notifications.get() >= 2);
            (*owner).memory_state.gc_force_gap = 0;
            (*owner).gc_state.callbacks.clear();
            crate::sexp::gengc::full_gc();
            assert!(!input_node.is_live());
            assert_eq!(output.string_value_elt(0), Some(Some("7L".into())));
            let output_node = crate::sexp::memory::checked_projection(output.as_raw())
                .unwrap()
                .1;
            drop(output);
            crate::sexp::gengc::full_gc();
            assert!(!output_node.is_live());
        });
    }
}
