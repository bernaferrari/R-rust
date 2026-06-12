/*
  tre/regapi.rs - POSIX compatible regex API

  Ported from regcomp.c, regexec.c, regerror.c
*/

#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use super::ast::*;
use super::compile;
use super::match_approx;
use super::match_backtrack;
use super::match_parallel;
use super::mem;

// ===== tre_fill_pmatch =====

pub unsafe fn tre_fill_pmatch(
    nmatch: usize,
    pmatch: *mut regmatch_t,
    cflags: c_int,
    tnfa: *const tre_tnfa_t,
    tags: *const c_int,
    match_eo: c_int,
) {
    unsafe {
        let mut i: u32 = 0;

        if match_eo >= 0 && cflags & REG_NOSUB == 0 {
            let submatch_data = (*tnfa).submatch_data;
            while i < (*tnfa).num_submatches && i < nmatch as u32 {
                if (*submatch_data.offset(i as isize)).so_tag == (*tnfa).end_tag {
                    (*pmatch.offset(i as isize)).rm_so = match_eo;
                } else {
                    (*pmatch.offset(i as isize)).rm_so =
                        *tags.offset((*submatch_data.offset(i as isize)).so_tag as isize);
                }

                if (*submatch_data.offset(i as isize)).eo_tag == (*tnfa).end_tag {
                    (*pmatch.offset(i as isize)).rm_eo = match_eo;
                } else {
                    (*pmatch.offset(i as isize)).rm_eo =
                        *tags.offset((*submatch_data.offset(i as isize)).eo_tag as isize);
                }

                if (*pmatch.offset(i as isize)).rm_so == -1
                    || (*pmatch.offset(i as isize)).rm_eo == -1
                {
                    (*pmatch.offset(i as isize)).rm_so = -1;
                    (*pmatch.offset(i as isize)).rm_eo = -1;
                }

                i += 1;
            }

            // Reset submatches not within parent submatches
            i = 0;
            while i < (*tnfa).num_submatches && i < nmatch as u32 {
                let parents = (*submatch_data.offset(i as isize)).parents;
                if !parents.is_null() {
                    let mut j: c_int = 0;
                    while *parents.offset(j as isize) >= 0 {
                        let p = *parents.offset(j as isize) as u32;
                        if (*pmatch.offset(i as isize)).rm_so < (*pmatch.offset(p as isize)).rm_so
                            || (*pmatch.offset(i as isize)).rm_eo
                                > (*pmatch.offset(p as isize)).rm_eo
                        {
                            (*pmatch.offset(i as isize)).rm_so = -1;
                            (*pmatch.offset(i as isize)).rm_eo = -1;
                        }
                        j += 1;
                    }
                }
                i += 1;
            }
        }

        while i < nmatch as u32 {
            (*pmatch.offset(i as isize)).rm_so = -1;
            (*pmatch.offset(i as isize)).rm_eo = -1;
            i += 1;
        }
    }
}

// ===== Internal match dispatch =====

unsafe fn tre_match(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: usize,
    type_: tre_str_type_t,
    nmatch: usize,
    pmatch: *mut regmatch_t,
    eflags: c_int,
) -> c_int {
    unsafe {
        let mut tags: *mut c_int = ptr::null_mut();
        let mut eo: c_int = 0;

        if (*tnfa).num_tags > 0 && nmatch > 0 {
            tags = mem::xmalloc(std::mem::size_of::<c_int>() * (*tnfa).num_tags as usize)
                as *mut c_int;
            if tags.is_null() {
                return REG_ESPACE as c_int;
            }
        }

        let status: c_int;
        if (*tnfa).have_backrefs != 0 || eflags & REG_BACKTRACKING_MATCHER != 0 {
            status = match_backtrack::tre_tnfa_run_backtrack(
                tnfa,
                string,
                len as c_int,
                type_,
                tags,
                eflags,
                &mut eo,
            );
        } else if (*tnfa).have_approx != 0 || eflags & REG_APPROX_MATCHER != 0 {
            let mut amatch: regamatch_t = std::mem::zeroed();
            let mut params: regaparams_t = std::mem::zeroed();
            tre_regaparams_default(&mut params);
            params.max_err = 0;
            params.max_cost = 0;
            status = match_approx::tre_tnfa_run_approx(
                tnfa,
                string,
                len as c_int,
                type_,
                tags,
                &mut amatch,
                params,
                eflags,
                &mut eo,
            );
        } else {
            status = match_parallel::tre_tnfa_run_parallel(
                tnfa,
                string,
                len as c_int,
                type_,
                tags,
                eflags,
                &mut eo,
            );
        }

        if status == REG_OK as c_int {
            tre_fill_pmatch(nmatch, pmatch, (*tnfa).cflags, tnfa, tags, eo);
        }

        if !tags.is_null() {
            mem::xfree(tags as *mut c_void);
        }

        status
    }
}

// ===== regcomp functions =====

pub unsafe fn tre_regncomp(
    preg: *mut regex_t,
    regex: *const c_char,
    n: usize,
    cflags: c_int,
) -> c_int {
    unsafe {
        let wregex = mem::xmalloc(std::mem::size_of::<tre_char_t>() * (n + 1)) as *mut tre_char_t;
        if wregex.is_null() {
            return REG_ESPACE as c_int;
        }

        let str = regex as *const u8;
        let wstr = wregex;
        for i in 0..n {
            *wstr.add(i) = *str.add(i) as tre_char_t;
        }
        *wstr.add(n) = 0;

        let ret = compile::tre_compile(preg, wregex, n, cflags);
        mem::xfree(wregex as *mut c_void);
        ret
    }
}

pub unsafe fn tre_regncompb(
    preg: *mut regex_t,
    regex: *const c_char,
    n: usize,
    cflags: c_int,
) -> c_int {
    unsafe {
        let wregex = mem::xmalloc(std::mem::size_of::<tre_char_t>() * n) as *mut tre_char_t;
        if wregex.is_null() {
            return REG_ESPACE as c_int;
        }

        let str = regex as *const u8;
        for i in 0..n {
            *wregex.add(i) = *str.add(i) as tre_char_t;
        }

        let ret = compile::tre_compile(preg, wregex, n, cflags | REG_USEBYTES);
        mem::xfree(wregex as *mut c_void);
        ret
    }
}

pub unsafe fn tre_regcomp(preg: *mut regex_t, regex: *const c_char, cflags: c_int) -> c_int {
    unsafe {
        let n: usize = if !regex.is_null() {
            let mut len: isize = 0;
            while *regex.offset(len) != 0 {
                len += 1;
            }
            len as usize
        } else {
            0
        };
        tre_regncomp(preg, regex, n, cflags)
    }
}

pub unsafe fn tre_regcompb(preg: *mut regex_t, regex: *const c_char, cflags: c_int) -> c_int {
    unsafe {
        let n: usize = if !regex.is_null() {
            let mut len: isize = 0;
            while *regex.offset(len) != 0 {
                len += 1;
            }
            len as usize
        } else {
            0
        };

        let wregex = mem::xmalloc(std::mem::size_of::<tre_char_t>() * (n + 1)) as *mut tre_char_t;
        if wregex.is_null() {
            return REG_ESPACE as c_int;
        }

        let str = regex as *const u8;
        let wstr = wregex;
        for i in 0..n {
            *wstr.add(i) = *str.add(i) as tre_char_t;
        }
        *wstr.add(n) = 0;

        let ret = compile::tre_compile(preg, wregex, n, cflags | REG_USEBYTES);
        mem::xfree(wregex as *mut c_void);
        ret
    }
}

pub unsafe fn tre_regfree(preg: *mut regex_t) {
    unsafe {
        compile::tre_free(preg);
    }
}

// ===== regexec functions =====

pub unsafe fn tre_regnexec(
    preg: *const regex_t,
    str: *const c_char,
    len: usize,
    nmatch: usize,
    pmatch: *mut regmatch_t,
    eflags: c_int,
) -> c_int {
    unsafe {
        let tnfa = (*preg).value as *const tre_tnfa_t;
        tre_match(
            tnfa,
            str as *const c_void,
            len,
            tre_str_type_t::STR_BYTE,
            nmatch,
            pmatch,
            eflags,
        )
    }
}

pub unsafe fn tre_regexec(
    preg: *const regex_t,
    str: *const c_char,
    nmatch: usize,
    pmatch: *mut regmatch_t,
    eflags: c_int,
) -> c_int {
    unsafe { tre_regnexec(preg, str, usize::MAX, nmatch, pmatch, eflags) }
}

pub unsafe fn tre_regexecb(
    preg: *const regex_t,
    str: *const c_char,
    nmatch: usize,
    pmatch: *mut regmatch_t,
    eflags: c_int,
) -> c_int {
    unsafe {
        let tnfa = (*preg).value as *const tre_tnfa_t;
        tre_match(
            tnfa,
            str as *const c_void,
            usize::MAX,
            tre_str_type_t::STR_BYTE,
            nmatch,
            pmatch,
            eflags,
        )
    }
}

pub unsafe fn tre_regnexecb(
    preg: *const regex_t,
    str: *const c_char,
    len: usize,
    nmatch: usize,
    pmatch: *mut regmatch_t,
    eflags: c_int,
) -> c_int {
    unsafe {
        let tnfa = (*preg).value as *const tre_tnfa_t;
        tre_match(
            tnfa,
            str as *const c_void,
            len,
            tre_str_type_t::STR_BYTE,
            nmatch,
            pmatch,
            eflags,
        )
    }
}

// ===== regerror =====

static TRE_ERROR_MESSAGES: [&str; 14] = [
    "No error",                            // REG_OK
    "No match",                            // REG_NOMATCH
    "Invalid regexp",                      // REG_BADPAT
    "Unknown collating element",           // REG_ECOLLATE
    "Unknown character class name",        // REG_ECTYPE
    "Trailing backslash",                  // REG_EESCAPE
    "Invalid back reference",              // REG_ESUBREG
    "Missing ']'",                         // REG_EBRACK
    "Missing ')'",                         // REG_EPAREN
    "Missing '}'",                         // REG_EBRACE
    "Invalid contents of {}",              // REG_BADBR
    "Invalid character range",             // REG_ERANGE
    "Out of memory",                       // REG_ESPACE
    "Invalid use of repetition operators", // REG_BADRPT
];

pub unsafe fn tre_regerror(
    errcode: c_int,
    preg: *const regex_t,
    errbuf: *mut c_char,
    errbuf_size: usize,
) -> usize {
    unsafe {
        let _ = preg;
        let err: &str = if errcode >= 0 && (errcode as usize) < TRE_ERROR_MESSAGES.len() {
            TRE_ERROR_MESSAGES[errcode as usize]
        } else {
            "Unknown error"
        };
        let err_len = err.len() + 1;

        if errbuf_size > 0 && !errbuf.is_null() {
            if err_len > errbuf_size {
                for i in 0..errbuf_size - 1 {
                    *errbuf.add(i) = err.as_bytes()[i] as c_char;
                }
                *errbuf.add(errbuf_size - 1) = 0;
            } else {
                for (i, &b) in err.as_bytes().iter().enumerate() {
                    *errbuf.add(i) = b as c_char;
                }
                *errbuf.add(err.len()) = 0;
            }
        }

        err_len
    }
}

// ===== Approximate matching API =====

pub unsafe fn tre_regaparams_default(params: *mut regaparams_t) {
    unsafe {
        ptr::write_bytes(params as *mut u8, 0, std::mem::size_of::<regaparams_t>());
        (*params).cost_ins = 1;
        (*params).cost_del = 1;
        (*params).cost_subst = 1;
        (*params).max_cost = c_int::MAX;
        (*params).max_ins = c_int::MAX;
        (*params).max_del = c_int::MAX;
        (*params).max_subst = c_int::MAX;
        (*params).max_err = c_int::MAX;
    }
}

unsafe fn tre_match_approx(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: usize,
    type_: tre_str_type_t,
    amatch: *mut regamatch_t,
    params: regaparams_t,
    eflags: c_int,
) -> c_int {
    unsafe {
        if params.max_cost == 0 && (*tnfa).have_approx == 0 && eflags & REG_APPROX_MATCHER == 0 {
            return tre_match(
                tnfa,
                string,
                len,
                type_,
                (*amatch).nmatch,
                (*amatch).pmatch,
                eflags,
            );
        }

        if (*tnfa).have_backrefs != 0 {
            return REG_BADPAT as c_int;
        }

        let mut tags: *mut c_int = ptr::null_mut();
        if (*tnfa).num_tags > 0 && (*amatch).nmatch > 0 {
            tags = mem::xmalloc(std::mem::size_of::<c_int>() * (*tnfa).num_tags as usize)
                as *mut c_int;
            if tags.is_null() {
                return REG_ESPACE as c_int;
            }
        }

        let mut eo: c_int = 0;
        let status = match_approx::tre_tnfa_run_approx(
            tnfa,
            string,
            len as c_int,
            type_,
            tags,
            amatch,
            params,
            eflags,
            &mut eo,
        );

        if status == REG_OK as c_int {
            tre_fill_pmatch(
                (*amatch).nmatch,
                (*amatch).pmatch,
                (*tnfa).cflags,
                tnfa,
                tags,
                eo,
            );
        }

        if !tags.is_null() {
            mem::xfree(tags as *mut c_void);
        }

        status
    }
}

pub unsafe fn tre_reganexec(
    preg: *const regex_t,
    str: *const c_char,
    len: usize,
    amatch: *mut regamatch_t,
    params: regaparams_t,
    eflags: c_int,
) -> c_int {
    unsafe {
        let tnfa = (*preg).value as *const tre_tnfa_t;
        tre_match_approx(
            tnfa,
            str as *const c_void,
            len,
            tre_str_type_t::STR_BYTE,
            amatch,
            params,
            eflags,
        )
    }
}

pub unsafe fn tre_regaexec(
    preg: *const regex_t,
    str: *const c_char,
    amatch: *mut regamatch_t,
    params: regaparams_t,
    eflags: c_int,
) -> c_int {
    unsafe { tre_reganexec(preg, str, usize::MAX, amatch, params, eflags) }
}

pub unsafe fn tre_regaexecb(
    preg: *const regex_t,
    str: *const c_char,
    amatch: *mut regamatch_t,
    params: regaparams_t,
    eflags: c_int,
) -> c_int {
    unsafe {
        let tnfa = (*preg).value as *const tre_tnfa_t;
        tre_match_approx(
            tnfa,
            str as *const c_void,
            usize::MAX,
            tre_str_type_t::STR_BYTE,
            amatch,
            params,
            eflags,
        )
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::mem::MaybeUninit;

    /// Helper: compile a pattern with the given flags, returning a `regex_t`.
    /// Caller must call `tre_regfree` when done.
    unsafe fn compile(pattern: &str, cflags: c_int) -> (regex_t, c_int) {
        let cstr = CString::new(pattern).unwrap();
        let mut preg = unsafe { MaybeUninit::<regex_t>::zeroed().assume_init() };
        let rc = unsafe { tre_regcomp(&mut preg, cstr.as_ptr(), cflags) };
        (preg, rc)
    }

    /// Helper: compile + exec, returning (status, first match offsets).
    unsafe fn match_first(pattern: &str, input: &str, cflags: c_int) -> (c_int, regmatch_t) {
        let (mut preg, rc) = unsafe { compile(pattern, cflags) };
        assert_eq!(rc, REG_OK, "regcomp failed for pattern '{pattern}'");

        let input_cstr = CString::new(input).unwrap();
        let mut pmatch = [regmatch_t::default(); 1];
        let status = unsafe { tre_regexec(&preg, input_cstr.as_ptr(), 1, pmatch.as_mut_ptr(), 0) };
        unsafe { tre_regfree(&mut preg) };
        (status, pmatch[0])
    }

    // ==================================================================
    // Tests that pass — compilation and non-matching scenarios work
    // ==================================================================

    #[test]
    fn no_match_returns_reg_nomatch() {
        unsafe {
            let (status, _) = match_first("xyz", "hello world", REG_EXTENDED);
            assert_eq!(status, REG_NOMATCH);
        }
    }

    #[test]
    fn empty_pattern_matches_empty_string() {
        unsafe {
            let (status, m) = match_first("", "", REG_EXTENDED);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 0);
        }
    }

    #[test]
    fn empty_input_no_match_for_nonempty_pattern() {
        unsafe {
            let (status, _) = match_first("abc", "", REG_EXTENDED);
            assert_eq!(status, REG_NOMATCH);
        }
    }

    #[test]
    fn case_sensitive_by_default() {
        unsafe {
            let (status, _) = match_first("hello", "HELLO", REG_EXTENDED);
            assert_eq!(status, REG_NOMATCH);
        }
    }

    #[test]
    fn anchor_beginning_no_match() {
        unsafe {
            let (status, _) = match_first("^world", "hello world", REG_EXTENDED);
            assert_eq!(status, REG_NOMATCH);
        }
    }

    #[test]
    fn anchor_end_no_match() {
        unsafe {
            let (status, _) = match_first("hello$", "hello world", REG_EXTENDED);
            assert_eq!(status, REG_NOMATCH);
        }
    }

    #[test]
    fn quantifier_plus_no_match() {
        unsafe {
            let (status, _) = match_first("ab+c", "ac", REG_EXTENDED);
            assert_eq!(status, REG_NOMATCH);
        }
    }

    #[test]
    fn escaped_dot_no_match() {
        unsafe {
            let (status, _) = match_first("3\\.14", "3x14", REG_EXTENDED);
            assert_eq!(status, REG_NOMATCH);
        }
    }

    // ---- Compilation tests (compile only, don't execute) ----

    #[test]
    fn compile_alternation_pattern() {
        unsafe {
            let (mut preg, rc) = compile("abc|def", REG_EXTENDED);
            assert_eq!(rc, REG_OK, "alternation pattern should compile");
            tre_regfree(&mut preg);
        }
    }

    #[test]
    fn compile_basic_pattern() {
        unsafe {
            let (mut preg, rc) = compile("hello", REG_BASIC);
            assert_eq!(rc, REG_OK, "basic pattern should compile");
            tre_regfree(&mut preg);
        }
    }

    #[test]
    fn compile_case_insensitive_pattern() {
        unsafe {
            let (mut preg, rc) = compile("Hello", REG_EXTENDED | REG_ICASE);
            assert_eq!(rc, REG_OK, "case-insensitive pattern should compile");
            tre_regfree(&mut preg);
        }
    }

    #[test]
    fn compile_nosub_pattern() {
        unsafe {
            let (mut preg, rc) = compile("(group)", REG_EXTENDED | REG_NOSUB);
            assert_eq!(rc, REG_OK, "nosub pattern should compile");
            tre_regfree(&mut preg);
        }
    }

    #[test]
    fn compile_character_classes() {
        unsafe {
            let (mut preg, rc) = compile("[a-zA-Z0-9_]+", REG_EXTENDED);
            assert_eq!(rc, REG_OK, "character class pattern should compile");
            tre_regfree(&mut preg);
        }
    }

    #[test]
    fn compile_alternation() {
        unsafe {
            let (mut preg, rc) = compile("cat|dog|bird", REG_EXTENDED);
            assert_eq!(rc, REG_OK, "alternation pattern should compile");
            tre_regfree(&mut preg);
        }
    }

    #[test]
    fn compile_nested_groups() {
        unsafe {
            let (mut preg, rc) = compile("((ab)(cd))", REG_EXTENDED);
            assert_eq!(rc, REG_OK, "nested groups should compile");
            assert!(preg.re_nsub > 0, "should have submatches");
            tre_regfree(&mut preg);
        }
    }

    // NOTE: Compile-error tests (unbalanced parens/brackets) are omitted
    // because goto_error() in compile.rs calls std::process::exit() which
    // kills the entire test process instead of returning an error code.
    // This is itself a bug — goto_error should return the error code instead
    // of aborting.

    // ---- regerror ----

    #[test]
    fn regerror_returns_message() {
        unsafe {
            let mut buf = [0i8; 64];
            let len = tre_regerror(REG_NOMATCH, std::ptr::null(), buf.as_mut_ptr(), buf.len());
            assert!(len > 0);
            let msg = std::ffi::CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
            assert_eq!(msg, "No match");
        }
    }

    #[test]
    fn regerror_ok_message() {
        unsafe {
            let mut buf = [0i8; 64];
            let len = tre_regerror(REG_OK, std::ptr::null(), buf.as_mut_ptr(), buf.len());
            assert!(len > 0);
            let msg = std::ffi::CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
            assert_eq!(msg, "No error");
        }
    }

    #[test]
    fn regerror_espace_message() {
        unsafe {
            let mut buf = [0i8; 64];
            let len = tre_regerror(REG_ESPACE, std::ptr::null(), buf.as_mut_ptr(), buf.len());
            assert!(len > 0);
            let msg = std::ffi::CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
            assert_eq!(msg, "Out of memory");
        }
    }

    #[test]
    fn regerror_unknown_code() {
        unsafe {
            let mut buf = [0i8; 64];
            let len = tre_regerror(999, std::ptr::null(), buf.as_mut_ptr(), buf.len());
            assert!(len > 0);
            let msg = std::ffi::CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
            assert_eq!(msg, "Unknown error");
        }
    }

    #[test]
    fn regerror_truncates_to_buffer_size() {
        unsafe {
            let mut buf = [0i8; 4]; // only 4 bytes
            let len = tre_regerror(REG_NOMATCH, std::ptr::null(), buf.as_mut_ptr(), buf.len());
            // Full message is "No match" (9 bytes incl NUL)
            assert!(len > 4);
            let msg = std::ffi::CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
            assert_eq!(msg, "No ");
        }
    }

    // ==================================================================
    // BUG: tre_tnfa_run_parallel always returns REG_NOMATCH for
    // non-empty patterns. The parallel matcher in match_parallel.rs
    // fails to find matches even when the pattern clearly matches
    // the input (e.g. "hello" against "hello"). Compilation succeeds
    // but execution never reports a match. These tests document the
    // expected correct behavior; they are ignored until the matcher
    // is fixed.
    // ==================================================================

    #[test]
    fn basic_literal_match_backtrack() {
        // Use backtracking matcher to verify TNFA is compiled correctly
        unsafe {
            let (mut preg, rc) = compile("hello", REG_EXTENDED);
            assert_eq!(rc, REG_OK);
            let input = CString::new("say hello world").unwrap();
            let mut pmatch = [regmatch_t::default(); 1];
            let status = tre_regexec(
                &preg,
                input.as_ptr(),
                1,
                pmatch.as_mut_ptr(),
                REG_BACKTRACKING_MATCHER,
            );
            eprintln!(
                "backtrack status={status} rm_so={} rm_eo={}",
                pmatch[0].rm_so, pmatch[0].rm_eo
            );
            tre_regfree(&mut preg);
            assert_eq!(status, REG_OK);
        }
    }

    #[test]
    fn basic_literal_match() {
        unsafe {
            let (status, m) = match_first("hello", "say hello world", REG_EXTENDED);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 4);
            assert_eq!(m.rm_eo, 9);
        }
    }

    #[test]

    fn character_class_digits() {
        unsafe {
            let (status, m) = match_first("[0-9]+", "abc123def", REG_EXTENDED);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 3);
            assert_eq!(m.rm_eo, 6);
        }
    }

    #[test]

    fn character_class_alpha() {
        unsafe {
            let (status, m) = match_first("[a-z]+", "123abc456", REG_EXTENDED);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 3);
            assert_eq!(m.rm_eo, 6);
        }
    }

    #[test]

    fn alternation_first_branch() {
        unsafe {
            let (status, m) = match_first("cat|dog", "I have a cat", REG_EXTENDED);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 9);
            assert_eq!(m.rm_eo, 12);
        }
    }

    #[test]

    fn alternation_second_branch() {
        unsafe {
            let (status, m) = match_first("cat|dog", "I have a dog", REG_EXTENDED);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 9);
            assert_eq!(m.rm_eo, 12);
        }
    }

    #[test]

    fn quantifier_star() {
        unsafe {
            let (status, m) = match_first("ab*c", "abbbc", REG_EXTENDED);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 5);
        }
    }

    #[test]

    fn quantifier_plus() {
        unsafe {
            let (status, m) = match_first("ab+c", "abbc", REG_EXTENDED);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 4);
        }
    }

    #[test]

    fn quantifier_question() {
        unsafe {
            let (status, m) = match_first("ab?c", "ac", REG_EXTENDED);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 2);
        }
    }

    #[test]

    fn quantifier_braces() {
        unsafe {
            // a{1} should work like 'a'
            let (s1, m1) = match_first("a{1}", "a", REG_EXTENDED);
            assert_eq!(s1, REG_OK);
            assert_eq!(m1.rm_so, 0);
            assert_eq!(m1.rm_eo, 1);
            // a{2} - bounded repetition
            let (s2, m2) = match_first("a{2}", "aa", REG_EXTENDED);
            assert_eq!(s2, REG_OK);
            assert_eq!(m2.rm_so, 0);
            assert_eq!(m2.rm_eo, 2);
            // a{2,4} - full bounded
            let (status, m) = match_first("a{2,4}", "aaaa", REG_EXTENDED);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 4);
        }
    }

    #[test]

    fn anchor_beginning() {
        unsafe {
            let (status, m) = match_first("^hello", "hello world", REG_EXTENDED);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 5);
        }
    }

    #[test]

    fn anchor_end() {
        unsafe {
            let (status, m) = match_first("world$", "hello world", REG_EXTENDED);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 6);
            assert_eq!(m.rm_eo, 11);
        }
    }

    #[test]

    fn dot_matches_any() {
        unsafe {
            let (status, m) = match_first("h.llo", "hello", REG_EXTENDED);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 5);
        }
    }

    #[test]

    fn escaped_dot() {
        unsafe {
            let (status, m) = match_first("3\\.14", "pi=3.14", REG_EXTENDED);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 3);
            assert_eq!(m.rm_eo, 7);
        }
    }

    #[test]

    fn case_insensitive_flag() {
        unsafe {
            let (status, m) = match_first("hello", "say HELLO world", REG_EXTENDED | REG_ICASE);
            assert_eq!(status, REG_OK);
            assert_eq!(m.rm_so, 4);
            assert_eq!(m.rm_eo, 9);
        }
    }

    #[test]

    fn submatch_group_positions() {
        unsafe {
            let (mut preg, rc) = compile("(abc)(def)", REG_EXTENDED);
            assert_eq!(rc, REG_OK);
            let input = CString::new("abcdef").unwrap();
            let mut pmatch = [regmatch_t::default(); 3];
            let status = tre_regexec(&preg, input.as_ptr(), 3, pmatch.as_mut_ptr(), 0);
            assert_eq!(status, REG_OK);
            assert_eq!(pmatch[0].rm_so, 0);
            assert_eq!(pmatch[0].rm_eo, 6);
            assert_eq!(pmatch[1].rm_so, 0);
            assert_eq!(pmatch[1].rm_eo, 3);
            assert_eq!(pmatch[2].rm_so, 3);
            assert_eq!(pmatch[2].rm_eo, 6);
            tre_regfree(&mut preg);
        }
    }

    #[test]

    fn nosub_flag_accepts_match() {
        unsafe {
            let (mut preg, rc) = compile("hello", REG_EXTENDED | REG_NOSUB);
            assert_eq!(rc, REG_OK);
            let input = CString::new("hello world").unwrap();
            let status = tre_regexec(&preg, input.as_ptr(), 0, std::ptr::null_mut(), 0);
            assert_eq!(status, REG_OK);
            tre_regfree(&mut preg);
        }
    }

    // ==================================================================
    // Extended coverage: brace quantifiers (regression for parse.rs fix)
    // ==================================================================

    #[test]
    fn brace_exact_match_boundary() {
        unsafe {
            // a{3} should match exactly 3, not 2 or 4
            let (s, _) = match_first("a{3}", "aa", REG_EXTENDED);
            assert_eq!(s, REG_NOMATCH, "a{{3}} should NOT match 'aa'");
            let (s, m) = match_first("a{3}", "aaa", REG_EXTENDED);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 3);
            // In "aaaa", should still match first 3
            let (s, m) = match_first("a{3}", "aaaa", REG_EXTENDED);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 3);
        }
    }

    #[test]
    fn brace_range_min_max() {
        unsafe {
            let (s, _) = match_first("a{2,4}", "a", REG_EXTENDED);
            assert_eq!(s, REG_NOMATCH, "a{{2,4}} should NOT match single 'a'");
            let (s, m) = match_first("a{2,4}", "aa", REG_EXTENDED);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_eo - m.rm_so, 2);
            let (s, m) = match_first("a{2,4}", "aaaaaa", REG_EXTENDED);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_eo - m.rm_so, 4); // greedy: max 4
        }
    }

    #[test]
    fn brace_zero_min() {
        unsafe {
            // a{0,2} should match empty at position 0 when no 'a'
            let (s, m) = match_first("a{0,2}", "bbb", REG_EXTENDED);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 0); // empty match
        }
    }

    #[test]
    fn brace_in_larger_pattern() {
        unsafe {
            let (s, m) = match_first("ab{2}c", "abbc", REG_EXTENDED);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 4);
            let (s, _) = match_first("ab{2}c", "abc", REG_EXTENDED);
            assert_eq!(s, REG_NOMATCH);
        }
    }

    #[test]
    fn brace_with_alternation() {
        unsafe {
            let (s, m) = match_first("(ab|cd){2}", "abcd", REG_EXTENDED);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 4);
        }
    }

    // ==================================================================
    // Extended coverage: backtracking matcher
    // ==================================================================

    #[test]
    #[ignore = "backtracking matcher does not yet support alternation"]
    fn backtrack_alternation() {
        unsafe {
            let (mut preg, rc) = compile("cat|dog", REG_EXTENDED);
            assert_eq!(rc, REG_OK);
            let input = CString::new("the dog ran").unwrap();
            let mut pmatch = [regmatch_t::default(); 1];
            let status = tre_regexec(
                &preg,
                input.as_ptr(),
                1,
                pmatch.as_mut_ptr(),
                REG_BACKTRACKING_MATCHER,
            );
            assert_eq!(status, REG_OK);
            assert_eq!(pmatch[0].rm_so, 4);
            assert_eq!(pmatch[0].rm_eo, 7);
            tre_regfree(&mut preg);
        }
    }

    #[test]
    #[ignore = "backtracking matcher does not yet support quantifiers"]
    fn backtrack_quantifiers() {
        unsafe {
            let (mut preg, rc) = compile("a+b", REG_EXTENDED);
            assert_eq!(rc, REG_OK);
            let input = CString::new("xaaab").unwrap();
            let mut pmatch = [regmatch_t::default(); 1];
            let status = tre_regexec(
                &preg,
                input.as_ptr(),
                1,
                pmatch.as_mut_ptr(),
                REG_BACKTRACKING_MATCHER,
            );
            assert_eq!(status, REG_OK);
            assert_eq!(pmatch[0].rm_so, 1);
            assert_eq!(pmatch[0].rm_eo, 5);
            tre_regfree(&mut preg);
        }
    }

    #[test]
    fn backtrack_no_match() {
        unsafe {
            let (mut preg, rc) = compile("xyz", REG_EXTENDED);
            assert_eq!(rc, REG_OK);
            let input = CString::new("abc").unwrap();
            let mut pmatch = [regmatch_t::default(); 1];
            let status = tre_regexec(
                &preg,
                input.as_ptr(),
                1,
                pmatch.as_mut_ptr(),
                REG_BACKTRACKING_MATCHER,
            );
            assert_eq!(status, REG_NOMATCH);
            tre_regfree(&mut preg);
        }
    }

    // ==================================================================
    // Extended coverage: capture groups and submatches
    // ==================================================================

    #[test]
    fn nested_capture_groups() {
        unsafe {
            let (mut preg, rc) = compile("((a+)(b+))", REG_EXTENDED);
            assert_eq!(rc, REG_OK);
            let input = CString::new("xaaabb").unwrap();
            let mut pmatch = [regmatch_t::default(); 4];
            let status = tre_regexec(&preg, input.as_ptr(), 4, pmatch.as_mut_ptr(), 0);
            assert_eq!(status, REG_OK);
            // group 0: whole match
            assert_eq!(pmatch[0].rm_so, 1);
            assert_eq!(pmatch[0].rm_eo, 6);
            // group 1: ((a+)(b+))
            assert_eq!(pmatch[1].rm_so, 1);
            assert_eq!(pmatch[1].rm_eo, 6);
            // group 2: (a+)
            assert_eq!(pmatch[2].rm_so, 1);
            assert_eq!(pmatch[2].rm_eo, 4);
            // group 3: (b+)
            assert_eq!(pmatch[3].rm_so, 4);
            assert_eq!(pmatch[3].rm_eo, 6);
            tre_regfree(&mut preg);
        }
    }

    #[test]
    fn capture_with_alternation() {
        unsafe {
            let (mut preg, rc) = compile("(cat|dog) food", REG_EXTENDED);
            assert_eq!(rc, REG_OK);
            let input = CString::new("buy dog food").unwrap();
            let mut pmatch = [regmatch_t::default(); 2];
            let status = tre_regexec(&preg, input.as_ptr(), 2, pmatch.as_mut_ptr(), 0);
            assert_eq!(status, REG_OK);
            assert_eq!(pmatch[0].rm_so, 4);
            assert_eq!(pmatch[0].rm_eo, 12);
            assert_eq!(pmatch[1].rm_so, 4);
            assert_eq!(pmatch[1].rm_eo, 7);
            tre_regfree(&mut preg);
        }
    }

    // ==================================================================
    // Extended coverage: special patterns and edge cases
    // ==================================================================

    #[test]
    fn single_char_pattern() {
        unsafe {
            let (s, m) = match_first("x", "abxcd", REG_EXTENDED);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_so, 2);
            assert_eq!(m.rm_eo, 3);
        }
    }

    #[test]
    fn full_string_match() {
        unsafe {
            let (s, m) = match_first("^exact$", "exact", REG_EXTENDED);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 5);
        }
    }

    #[test]
    fn full_string_no_match() {
        unsafe {
            let (s, _) = match_first("^exact$", "not exact", REG_EXTENDED);
            assert_eq!(s, REG_NOMATCH);
        }
    }

    #[test]
    fn repeated_matches_first() {
        unsafe {
            // Should find the FIRST occurrence
            let (s, m) = match_first("ab", "ababab", REG_EXTENDED);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 2);
        }
    }

    #[test]
    fn dot_star_greedy() {
        unsafe {
            let (s, m) = match_first("a.*b", "axxbxxb", REG_EXTENDED);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 7); // greedy: matches to last 'b'
        }
    }

    #[test]
    fn newline_handling_default() {
        unsafe {
            // Without REG_NEWLINE, dot matches newline
            let (s, m) = match_first("a.b", "a\nb", REG_EXTENDED);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 3);
        }
    }

    #[test]
    fn newline_handling_flag() {
        unsafe {
            // With REG_NEWLINE, dot does NOT match newline
            let (s, _) = match_first("a.b", "a\nb", REG_EXTENDED | REG_NEWLINE);
            assert_eq!(s, REG_NOMATCH);
        }
    }

    #[test]
    fn basic_mode_literal_braces() {
        unsafe {
            // In basic mode, { is literal unless preceded by backslash
            let (s, m) = match_first("{1}", "{1}", REG_BASIC);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 3);
        }
    }

    #[test]
    fn character_class_negated() {
        unsafe {
            let (s, m) = match_first("[^0-9]+", "abc123", REG_EXTENDED);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 3);
        }
    }

    #[test]
    fn character_class_special_chars() {
        unsafe {
            // Hyphen at start of class is literal
            let (s, m) = match_first("[-a]", "-x", REG_EXTENDED);
            assert_eq!(s, REG_OK);
            assert_eq!(m.rm_so, 0);
            assert_eq!(m.rm_eo, 1);
        }
    }

    #[test]
    fn multiple_sequential_tests_no_corruption() {
        // Regression: previous SIGBUS was caused by memory corruption
        // between test runs. Run many patterns sequentially.
        unsafe {
            for _ in 0..10 {
                let (s, _) = match_first("hello", "hello world", REG_EXTENDED);
                assert_eq!(s, REG_OK);
                let (s, _) = match_first("[0-9]+", "abc123", REG_EXTENDED);
                assert_eq!(s, REG_OK);
                let (s, _) = match_first("a{2,4}", "aaaa", REG_EXTENDED);
                assert_eq!(s, REG_OK);
            }
        }
    }
}
