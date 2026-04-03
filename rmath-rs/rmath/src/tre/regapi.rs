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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_fill_pmatch(
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_regncomp(
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
            *wstr.offset(i as isize) = *str.offset(i as isize) as tre_char_t;
        }
        *wstr.offset(n as isize) = 0;

        let ret = compile::tre_compile(preg, wregex, n, cflags);
        mem::xfree(wregex as *mut c_void);
        ret
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_regncompb(
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
            *wregex.offset(i as isize) = *str.offset(i as isize) as tre_char_t;
        }

        let ret = compile::tre_compile(preg, wregex, n, cflags | REG_USEBYTES);
        mem::xfree(wregex as *mut c_void);
        ret
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_regcomp(
    preg: *mut regex_t,
    regex: *const c_char,
    cflags: c_int,
) -> c_int {
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_regcompb(
    preg: *mut regex_t,
    regex: *const c_char,
    cflags: c_int,
) -> c_int {
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
            *wstr.offset(i as isize) = *str.offset(i as isize) as tre_char_t;
        }
        *wstr.offset(n as isize) = 0;

        let ret = compile::tre_compile(preg, wregex, n, cflags | REG_USEBYTES);
        mem::xfree(wregex as *mut c_void);
        ret
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_regfree(preg: *mut regex_t) {
    unsafe {
        compile::tre_free(preg);
    }
}

// ===== regexec functions =====

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_regnexec(
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_regexec(
    preg: *const regex_t,
    str: *const c_char,
    nmatch: usize,
    pmatch: *mut regmatch_t,
    eflags: c_int,
) -> c_int {
    unsafe { tre_regnexec(preg, str, usize::MAX, nmatch, pmatch, eflags) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_regexecb(
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_regnexecb(
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_regerror(
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
                    *errbuf.offset(i as isize) = err.as_bytes()[i] as c_char;
                }
                *errbuf.offset((errbuf_size - 1) as isize) = 0;
            } else {
                for (i, &b) in err.as_bytes().iter().enumerate() {
                    *errbuf.offset(i as isize) = b as c_char;
                }
                *errbuf.offset(err.len() as isize) = 0;
            }
        }

        err_len
    }
}

// ===== Approximate matching API =====

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_regaparams_default(params: *mut regaparams_t) {
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_reganexec(
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_regaexec(
    preg: *const regex_t,
    str: *const c_char,
    amatch: *mut regamatch_t,
    params: regaparams_t,
    eflags: c_int,
) -> c_int {
    unsafe { tre_reganexec(preg, str, usize::MAX, amatch, params, eflags) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_regaexecb(
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
