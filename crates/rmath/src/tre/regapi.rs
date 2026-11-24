/*
  tre/regapi.rs - POSIX compatible regex API

  Ported from regcomp.c, regexec.c, regerror.c
*/

#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::ffi::CStr;
use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use super::ast::*;
use super::compile;
use super::match_approx;
use super::match_backtrack;
use super::match_parallel;
use crate::mainutils::grep::RegexWorkspace;

#[cfg(test)]
mod compile_error_tests;

// ===== tre_fill_pmatch =====

const UNSET_MATCH: regmatch_t = regmatch_t {
    rm_so: -1,
    rm_eo: -1,
};

fn capture_from_tags(
    submatch: &tre_submatch_data_t,
    tags: &[c_int],
    end_tag: c_int,
    match_eo: c_int,
) -> regmatch_t {
    let offset = |tag| {
        if tag == end_tag {
            match_eo
        } else {
            usize::try_from(tag)
                .ok()
                .and_then(|index| tags.get(index).copied())
                .unwrap_or(-1)
        }
    };
    let rm_so = offset(submatch.so_tag);
    let rm_eo = offset(submatch.eo_tag);
    if rm_so == -1 || rm_eo == -1 {
        UNSET_MATCH
    } else {
        regmatch_t { rm_so, rm_eo }
    }
}

/// # Safety
/// A non-null `parents` points to an initialized, sentinel-terminated list
/// from the compiler. It must remain readable through the first negative id
/// or `earlier.len() + 1` entries, whichever comes first.
unsafe fn capture_within_parents(
    parents: *const c_int,
    earlier: &[regmatch_t],
    capture: regmatch_t,
) -> bool {
    if parents.is_null() {
        return true;
    }
    // Capture ids are assigned in opening order, so each parent precedes
    // this capture. Leave room for the sentinel, even at maximum depth.
    for index in 0..=earlier.len() {
        // SAFETY: the loop stops on the first sentinel and bounds its reads.
        let parent = unsafe { parents.add(index).read() };
        if parent < 0 {
            return true;
        }
        let Some(parent) = earlier.get(parent as usize) else {
            return false;
        };
        if capture.rm_so < parent.rm_so || capture.rm_eo > parent.rm_eo {
            return false;
        }
    }
    false
}

/// Write capture offsets, filling unused output slots with `-1/-1`.
///
/// # Safety
/// If `nmatch` is nonzero, `pmatch` must be aligned and valid for `nmatch`
/// writes in one allocation (at most `isize::MAX` bytes), disjoint from the
/// other arguments. The output need not be initialized. For a successful
/// match without `REG_NOSUB` and with output requested, `tnfa` must point to
/// a live descriptor with its complete initialized submatch table and
/// sentinel-terminated parent lists in the compiler's layout. When non-null,
/// `tags` must hold the descriptor's `num_tags` initialized entries.
pub unsafe fn tre_fill_pmatch(
    nmatch: usize,
    pmatch: *mut regmatch_t,
    cflags: c_int,
    tnfa: *const tre_tnfa_t,
    tags: *const c_int,
    match_eo: c_int,
) {
    if nmatch == 0 {
        return;
    }
    // SAFETY: the raw API requires this extent of writable, disjoint storage;
    // MaybeUninit does not assume any prior writes by the caller.
    let output = unsafe {
        std::slice::from_raw_parts_mut(pmatch.cast::<MaybeUninit<regmatch_t>>(), nmatch)
    };
    if match_eo < 0 || cflags & REG_NOSUB != 0 {
        output.fill(MaybeUninit::new(UNSET_MATCH));
        return;
    }
    // SAFETY: the caller supplies the compiled descriptor and tag storage.
    let (submatches, tags, end_tag) = unsafe {
        let tnfa = &*tnfa;
        let count = nmatch.min(tnfa.num_submatches as usize);
        let submatches = if count == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(tnfa.submatch_data, count)
        };
        let tag_count = usize::try_from(tnfa.num_tags).unwrap_or(0);
        let tags = if count == 0 || tag_count == 0 || tags.is_null() {
            &[]
        } else {
            std::slice::from_raw_parts(tags, tag_count)
        };
        (submatches, tags, tnfa.end_tag)
    };
    let (captures, unused) = output.split_at_mut(submatches.len());
    for (slot, submatch) in captures.iter_mut().zip(submatches) {
        slot.write(capture_from_tags(submatch, tags, end_tag, match_eo));
    }
    unused.fill(MaybeUninit::new(UNSET_MATCH));

    // SAFETY: the preceding loop initialized every slot in this prefix.
    let captures = unsafe {
        std::slice::from_raw_parts_mut(
            captures.as_mut_ptr().cast::<regmatch_t>(),
            captures.len(),
        )
    };
    for (index, submatch) in submatches.iter().enumerate() {
        let (earlier, current) = captures.split_at_mut(index);
        let capture = &mut current[0];
        // SAFETY: the descriptor owns the sentinel-terminated parent list.
        if !unsafe { capture_within_parents(submatch.parents, earlier, *capture) } {
            *capture = UNSET_MATCH;
        }
    }
}

// ===== Internal match dispatch =====

fn match_input_length(len: usize) -> Result<c_int, c_int> {
    if len == usize::MAX {
        Ok(-1)
    } else {
        c_int::try_from(len).map_err(|_| REG_ESPACE)
    }
}

/// # Safety
///
/// Every raw-pointer argument must point to a live, properly initialized TRE object (or a buffer
/// sized per the accompanying length/count arguments) valid for the reads and writes performed
/// here; null is accepted only where the original C TRE API accepts it.
pub(super) unsafe fn tre_match(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: usize,
    type_: tre_str_type_t,
    nmatch: usize,
    pmatch: *mut regmatch_t,
    eflags: c_int,
) -> c_int {
    unsafe {
        tre_match_with_workspace(
            tnfa,
            string,
            len,
            type_,
            nmatch,
            pmatch,
            eflags,
            &RegexWorkspace::unbounded(),
        )
    }
}

/// # Safety
///
/// Every raw-pointer argument must point to a live, properly initialized TRE object (or a buffer
/// sized per the accompanying length/count arguments) valid for the reads and writes performed
/// here; null is accepted only where the original C TRE API accepts it.
pub(super) unsafe fn tre_match_with_workspace(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: usize,
    type_: tre_str_type_t,
    nmatch: usize,
    pmatch: *mut regmatch_t,
    eflags: c_int,
    workspace: &RegexWorkspace,
) -> c_int {
    let len = match match_input_length(len) {
        Ok(len) => len,
        Err(status) => return status,
    };
    unsafe {
        let Ok(count) = usize::try_from((*tnfa).num_tags) else {
            return REG_ESPACE;
        };
        let count = if nmatch == 0 { 0 } else { count };
        let Ok(mut tag_buffer) = match_parallel::filled(workspace, count, -1) else {
            return REG_ESPACE;
        };
        let tags = if count == 0 {
            ptr::null_mut()
        } else {
            tag_buffer.as_mut_ptr()
        };
        let mut eo: c_int = 0;

        let status: c_int;
        if (*tnfa).have_backrefs != 0 || eflags & REG_BACKTRACKING_MATCHER != 0 {
            status = match_backtrack::run_backtrack(
                tnfa, string, len, type_, tags, eflags, &mut eo, workspace,
            );
        } else if (*tnfa).have_approx != 0 || eflags & REG_APPROX_MATCHER != 0 {
            let mut amatch: regamatch_t = std::mem::zeroed();
            let mut params: regaparams_t = std::mem::zeroed();
            tre_regaparams_default(&mut params);
            params.max_err = 0;
            params.max_cost = 0;
            status = match_approx::run_approx(
                tnfa,
                string,
                len,
                type_,
                tags,
                &mut amatch,
                params,
                eflags,
                &mut eo,
                workspace,
            );
        } else {
            status = match_parallel::run_parallel(
                tnfa,
                string,
                len,
                type_,
                &mut tag_buffer,
                eflags,
                &mut eo,
                workspace,
            );
        }

        if status == REG_OK as c_int {
            let tags = if count == 0 {
                ptr::null_mut()
            } else {
                tag_buffer.as_mut_ptr()
            };
            tre_fill_pmatch(nmatch, pmatch, (*tnfa).cflags, tnfa, tags, eo);
        }

        status
    }
}

// ===== regcomp functions =====

/// # Safety
/// `preg` must be valid for writes of a `regex_t`.
unsafe fn compile_byte_pattern(
    preg: *mut regex_t,
    pattern: &[u8],
    cflags: c_int,
    workspace: RegexWorkspace,
) -> c_int {
    if c_int::try_from(pattern.len()).is_err() {
        return REG_ESPACE;
    }
    let Some(capacity) = pattern.len().checked_add(1) else {
        return REG_ESPACE;
    };
    let Ok(mut wide) = match_parallel::filled(&workspace, capacity, 0) else {
        return REG_ESPACE;
    };
    for (slot, &byte) in wide.iter_mut().zip(pattern) {
        *slot = tre_char_t::from(byte);
    }
    // SAFETY: `preg` is writable; the initialized wide buffer outlives compilation.
    // The last zero is lookahead storage, not part of the pattern.
    unsafe { compile::tre_compile_with_workspace(preg, wide.as_ptr(), pattern.len(), cflags, workspace) }
}

/// # Safety
/// `preg` must be writable; for nonzero `n`, `regex` must be valid for `n`
/// byte reads within a single allocation for the duration of the call.
pub unsafe fn tre_regncomp(
    preg: *mut regex_t,
    regex: *const c_char,
    n: usize,
    cflags: c_int,
) -> c_int {
    if c_int::try_from(n).is_err() {
        return REG_ESPACE;
    }
    let pattern = if n == 0 {
        &[]
    } else {
        // SAFETY: the caller provides the readable range and its length fits a slice.
        unsafe { std::slice::from_raw_parts(regex.cast::<u8>(), n) }
    };
    // SAFETY: the caller provides the writable descriptor.
    unsafe { compile_byte_pattern(preg, pattern, cflags, RegexWorkspace::unbounded()) }
}

/// # Safety
/// Same requirements as [`tre_regncomp`].
pub unsafe fn tre_regncompb(
    preg: *mut regex_t,
    regex: *const c_char,
    n: usize,
    cflags: c_int,
) -> c_int {
    if n == 0 {
        // The legacy byte adapter returned REG_ESPACE for a zero-size allocation.
        return REG_ESPACE;
    }
    // SAFETY: forwards the original writable descriptor and readable pattern.
    unsafe { tre_regncomp(preg, regex, n, cflags | REG_USEBYTES) }
}

/// # Safety
/// `preg` must be writable; `regex` must be null or a live NUL-terminated string.
pub unsafe fn tre_regcomp(preg: *mut regex_t, regex: *const c_char, cflags: c_int) -> c_int {
    let pattern = if regex.is_null() {
        &[]
    } else {
        // SAFETY: the caller guarantees a live NUL-terminated string.
        unsafe { CStr::from_ptr(regex) }.to_bytes()
    };
    // SAFETY: the caller provides the writable descriptor.
    unsafe { compile_byte_pattern(preg, pattern, cflags, RegexWorkspace::unbounded()) }
}

/// # Safety
/// Same requirements as [`tre_regcomp`].
pub unsafe fn tre_regcompb(preg: *mut regex_t, regex: *const c_char, cflags: c_int) -> c_int {
    // SAFETY: forwards the original writable descriptor and NUL-terminated input.
    unsafe { tre_regcomp(preg, regex, cflags | REG_USEBYTES) }
}

/// # Safety
///
/// `preg` must point to a `regex_t` initialized by a successful `tre_regcomp`-family call, not freed before.
pub unsafe fn tre_regfree(preg: *mut regex_t) {
    // SAFETY: pointers are live TRE objects forwarded under the enclosing function's `# Safety` contract.
    unsafe { compile::tre_free(preg) };
}

// ===== regexec functions =====

/// C-shaped TRE match entry point; Rust callers should use
/// `CompiledTre::match_bytes` instead.
///
/// # Safety
///
/// `preg` must point to a regex compiled by a successful `tre_regcomp`-family
/// call and not yet freed, `str` must be valid for `len` byte reads (or be
/// NUL-terminated when `len == usize::MAX`), and `pmatch` must be valid for
/// `nmatch` writes (or null when `nmatch == 0`).
pub unsafe fn tre_regnexec(
    preg: *const regex_t,
    str: *const c_char,
    len: usize,
    nmatch: usize,
    pmatch: *mut regmatch_t,
    eflags: c_int,
) -> c_int {
    // SAFETY: the caller upholds this function's `# Safety` contract.
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

/// Match a bounded Unicode scalar buffer. Offsets count scalar values; the
/// UTF-8 adapter translates them back to original byte positions.
///
/// # Safety
///
/// `preg` must come from a successful TRE compile and remain live and unfreed
/// for the duration of the call.
#[cfg(test)]
pub(super) unsafe fn tre_regwnexec_legacy(
    preg: &regex_t,
    text: &[tre_char_t],
    matches: &mut [regmatch_t],
    flags: c_int,
) -> c_int {
    let Ok(len) = c_int::try_from(text.len()) else {
        return REG_ESPACE;
    };
    // SAFETY: `preg` is a compiled regex, so `value` is its live tnfa; `text` and `matches` are live slices whose lengths are passed alongside.
    unsafe {
        tre_match(
            preg.value as *const tre_tnfa_t,
            text.as_ptr().cast(),
            len as usize,
            tre_str_type_t::STR_WIDE,
            matches.len(),
            matches.as_mut_ptr(),
            flags,
        )
    }
}

/// # Safety
///
/// Every raw-pointer argument must point to a live, properly initialized TRE object (or a buffer
/// sized per the accompanying length/count arguments) valid for the reads and writes performed
/// here; null is accepted only where the original C TRE API accepts it.
pub unsafe fn tre_regexec(
    preg: *const regex_t,
    str: *const c_char,
    nmatch: usize,
    pmatch: *mut regmatch_t,
    eflags: c_int,
) -> c_int {
    // SAFETY: forwarded unchanged under this function's `# Safety` contract.
    unsafe { tre_regnexec(preg, str, usize::MAX, nmatch, pmatch, eflags) }
}

/// # Safety
///
/// Every raw-pointer argument must point to a live, properly initialized TRE object (or a buffer
/// sized per the accompanying length/count arguments) valid for the reads and writes performed
/// here; null is accepted only where the original C TRE API accepts it.
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

/// # Safety
///
/// Every raw-pointer argument must point to a live, properly initialized TRE object (or a buffer
/// sized per the accompanying length/count arguments) valid for the reads and writes performed
/// here; null is accepted only where the original C TRE API accepts it.
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

/// # Safety
///
/// `errbuf` must be null or valid for writes of `errbuf_size` bytes.
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

/// # Safety
///
/// `params` must be valid for writes of a `regaparams_t`.
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

/// # Safety
///
/// Every raw-pointer argument must point to a live, properly initialized TRE object (or a buffer
/// sized per the accompanying length/count arguments) valid for the reads and writes performed
/// here; null is accepted only where the original C TRE API accepts it.
unsafe fn tre_match_approx(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: usize,
    type_: tre_str_type_t,
    amatch: *mut regamatch_t,
    params: regaparams_t,
    eflags: c_int,
    workspace: &RegexWorkspace,
) -> c_int {
    unsafe {
        if params.max_cost == 0 && (*tnfa).have_approx == 0 && eflags & REG_APPROX_MATCHER == 0 {
            return tre_match_with_workspace(
                tnfa,
                string,
                len,
                type_,
                (*amatch).nmatch,
                (*amatch).pmatch,
                eflags,
                workspace,
            );
        }

        if (*tnfa).have_backrefs != 0 {
            return REG_BADPAT as c_int;
        }

        let len = match match_input_length(len) {
            Ok(len) => len,
            Err(status) => return status,
        };
        let nmatch = (*amatch).nmatch;
        let count = if nmatch == 0 {
            0
        } else {
            let Ok(count) = usize::try_from((*tnfa).num_tags) else {
                return REG_ESPACE;
            };
            count
        };
        let Ok(mut tag_buffer) = match_parallel::filled(workspace, count, -1) else {
            return REG_ESPACE;
        };
        let tags = if tag_buffer.is_empty() {
            ptr::null_mut()
        } else {
            tag_buffer.as_mut_ptr()
        };
        let mut eo = 0;
        let status = match_approx::run_approx(
            tnfa, string, len, type_, tags, amatch, params, eflags, &mut eo, workspace,
        );
        if status == REG_OK {
            tre_fill_pmatch(nmatch, (*amatch).pmatch, (*tnfa).cflags, tnfa, tags, eo);
        }
        status
    }
}

/// Match a byte buffer, or a NUL-terminated string when `len == usize::MAX`.
///
/// # Safety
///
/// `preg` must point to a live compiled regex. `str` must be valid for reads
/// of `len` bytes, or through the terminating NUL for the sentinel length.
/// `amatch` must be writable with initialized `nmatch` and `pmatch` fields.
/// `pmatch` must be valid for `nmatch` writes, or null if `nmatch` is zero.
pub unsafe fn tre_reganexec(
    preg: *const regex_t,
    str: *const c_char,
    len: usize,
    amatch: *mut regamatch_t,
    params: regaparams_t,
    eflags: c_int,
) -> c_int {
    let workspace = if crate::sexp::instance::current_instance_ptr().is_some() {
        crate::mainutils::grep::regex_workspace()
    } else {
        RegexWorkspace::unbounded()
    };
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
            &workspace,
        )
    }
}

/// # Safety
///
/// Every raw-pointer argument must point to a live, properly initialized TRE object (or a buffer
/// sized per the accompanying length/count arguments) valid for the reads and writes performed
/// here; null is accepted only where the original C TRE API accepts it.
pub unsafe fn tre_regaexec(
    preg: *const regex_t,
    str: *const c_char,
    amatch: *mut regamatch_t,
    params: regaparams_t,
    eflags: c_int,
) -> c_int {
    // SAFETY: pointers are live TRE objects forwarded under the enclosing function's `# Safety` contract.
    unsafe { tre_reganexec(preg, str, usize::MAX, amatch, params, eflags) }
}

/// # Safety
///
/// Every raw-pointer argument must point to a live, properly initialized TRE object (or a buffer
/// sized per the accompanying length/count arguments) valid for the reads and writes performed
/// here; null is accepted only where the original C TRE API accepts it.
pub unsafe fn tre_regaexecb(
    preg: *const regex_t,
    str: *const c_char,
    amatch: *mut regamatch_t,
    params: regaparams_t,
    eflags: c_int,
) -> c_int {
    // SAFETY: both entry points match a NUL-terminated byte string with the same contracts.
    unsafe { tre_regaexec(preg, str, amatch, params, eflags) }
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
    fn legacy_compile_adapters_preserve_bounded_and_terminated_patterns() {
        let bytes = [b'a', 0, 0xff];
        let mut compiled = regex_t {
            re_nsub: 0,
            value: ptr::null_mut(),
        };
        // SAFETY: the byte array is both bounded and NUL-terminated and the descriptor is writable.
        unsafe {
            for compile in [tre_regncomp, tre_regncompb] {
                assert_eq!(
                    compile(&mut compiled, bytes.as_ptr().cast(), bytes.len(), REG_EXTENDED),
                    REG_OK
                );
                let mut out = regmatch_t::default();
                assert_eq!(
                    tre_regnexec(&compiled, bytes.as_ptr().cast(), bytes.len(), 1, &mut out, 0),
                    REG_OK
                );
                assert_eq!((out.rm_so, out.rm_eo), (0, 3));
                tre_regfree(&mut compiled);
            }
            for compile in [tre_regcomp, tre_regcompb] {
                assert_eq!(
                    compile(&mut compiled, bytes.as_ptr().cast(), REG_EXTENDED),
                    REG_OK
                );
                let mut out = regmatch_t::default();
                assert_eq!(
                    tre_regnexec(&compiled, bytes.as_ptr().cast(), bytes.len(), 1, &mut out, 0),
                    REG_OK
                );
                assert_eq!((out.rm_so, out.rm_eo), (0, 1));
                tre_regfree(&mut compiled);
                assert_eq!(compile(&mut compiled, ptr::null(), REG_EXTENDED), REG_OK);
                tre_regfree(&mut compiled);
            }
            assert_eq!(
                tre_regncomp(&mut compiled, ptr::null(), 0, REG_EXTENDED),
                REG_OK
            );
            tre_regfree(&mut compiled);
            assert_eq!(
                tre_regncompb(&mut compiled, ptr::null(), 0, REG_EXTENDED),
                REG_ESPACE
            );
            assert!(compiled.value.is_null());
        }
    }

    #[test]
    fn legacy_compile_adapter_releases_widening_workspace() {
        use crate::sexp::{memory::ArenaBudget, session::RSession};
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let workspace = crate::mainutils::grep::regex_workspace();
            let baseline = owner.with_arena(|arena| arena.workspace_bytes()).unwrap();
            owner
                .with_arena(|arena| {
                    arena.set_budget(ArenaBudget::new(
                        arena.total_bytes_allocated() + baseline + 1,
                        0,
                    ));
                })
                .unwrap();
            let mut regex = regex_t {
                re_nsub: 0,
                value: ptr::null_mut(),
            };
            // SAFETY: the original writable descriptor remains live through each compile.
            unsafe {
                assert_eq!(
                    compile_byte_pattern(&mut regex, b"(ab)", REG_EXTENDED, workspace.clone()),
                    REG_ESPACE
                );
                assert!(regex.value.is_null());
                assert_eq!(
                    owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                    baseline
                );
                owner
                    .with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))
                    .unwrap();
                assert_eq!(
                    compile_byte_pattern(&mut regex, b"(ab)", REG_EXTENDED, workspace),
                    REG_OK
                );
                assert_eq!(
                    owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                    baseline
                );
                tre_regfree(&mut regex);
            }
        });
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
            assert_eq!(pmatch[0].rm_so, 4);
            assert_eq!(pmatch[0].rm_eo, 9);
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

    #[test]
    fn legacy_nul_terminated_backreference_stops_at_the_terminator() {
        unsafe {
            let (mut preg, rc) = compile(r"(ab)\1", REG_EXTENDED);
            assert_eq!(rc, REG_OK);
            let mut matches = [regmatch_t::default(); 2];
            let status = tre_regexec(
                &preg,
                c"xababy".as_ptr(),
                matches.len(),
                matches.as_mut_ptr(),
                0,
            );
            tre_regfree(&mut preg);
            assert_eq!(status, REG_OK);
            assert_eq!(matches.map(|m| (m.rm_so, m.rm_eo)), [(1, 5), (1, 3)]);
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
    fn capture_publication_writes_legacy_output_prefix() {
        let (mut preg, rc) = unsafe { compile("((a+)(b+))", REG_EXTENDED) };
        assert_eq!(rc, REG_OK);
        let expected = [(1, 6), (1, 6), (1, 4), (4, 6), (-1, -1)];
        for nmatch in 0..=expected.len() {
            let mut output = [MaybeUninit::<regmatch_t>::uninit(); 6];
            output[nmatch].write(regmatch_t {
                rm_so: 42,
                rm_eo: 43,
            });
            let pmatch = if nmatch == 0 {
                ptr::null_mut()
            } else {
                output.as_mut_ptr().cast()
            };
            // SAFETY: the regex and input are live, and the output is writable.
            assert_eq!(
                unsafe { tre_regexec(&preg, c"xaaabb".as_ptr(), nmatch, pmatch, 0) },
                REG_OK
            );
            for (slot, &expected) in output[..nmatch].iter().zip(&expected) {
                // SAFETY: a successful match writes the entire requested prefix.
                let value = unsafe { slot.assume_init() };
                assert_eq!((value.rm_so, value.rm_eo), expected);
            }
            // SAFETY: the canary was initialized before the call.
            let canary = unsafe { output[nmatch].assume_init() };
            assert_eq!((canary.rm_so, canary.rm_eo), (42, 43));
        }
        // SAFETY: the regex was compiled above and has no remaining borrowers.
        unsafe { tre_regfree(&mut preg) };
    }

    #[test]
    fn capture_publication_bounds_tag_and_parent_indices() {
        let parents = [0, -1];
        let outside = [3, -1];
        let mut submatches = [
            tre_submatch_data_t {
                so_tag: 0,
                eo_tag: 3,
                parents: ptr::null_mut(),
            },
            tre_submatch_data_t {
                so_tag: 1,
                eo_tag: 2,
                parents: parents.as_ptr().cast_mut(),
            },
            tre_submatch_data_t {
                so_tag: 0,
                eo_tag: 3,
                parents: outside.as_ptr().cast_mut(),
            },
            tre_submatch_data_t {
                so_tag: 4,
                eo_tag: 3,
                parents: ptr::null_mut(),
            },
        ];
        // SAFETY: this C record contains only raw pointers and integers.
        let mut tnfa: tre_tnfa_t = unsafe { std::mem::zeroed() };
        tnfa.num_submatches = submatches.len() as u32;
        tnfa.submatch_data = submatches.as_mut_ptr();
        tnfa.num_tags = 3;
        tnfa.end_tag = 3;
        let tags = [1, 0, 3];
        let mut output = [MaybeUninit::<regmatch_t>::uninit(); 4];
        // SAFETY: all backing ranges and parent terminators are live. The
        // synthetic indices are inconsistent, but their source tables are valid.
        unsafe {
            tre_fill_pmatch(
                output.len(),
                output.as_mut_ptr().cast(),
                0,
                &tnfa,
                tags.as_ptr(),
                4,
            )
        };
        // SAFETY: the call wrote all output slots.
        let pairs = output.map(|slot| {
            let value = unsafe { slot.assume_init() };
            (value.rm_so, value.rm_eo)
        });
        assert_eq!(pairs, [(1, 4), (-1, -1), (-1, -1), (-1, -1)]);
    }

    #[test]
    fn approximate_deletions_report_native_costs() {
        let (mut preg, rc) = unsafe { compile("^abc$", REG_EXTENDED) };
        assert_eq!(rc, REG_OK);
        let mut result = regmatch_t::default();
        let mut matched = regamatch_t {
            nmatch: 1,
            pmatch: &mut result,
            cost: -1,
            num_ins: -1,
            num_del: -1,
            num_subst: -1,
        };
        let params = regaparams_t {
            cost_ins: 1,
            cost_del: 1,
            cost_subst: 1,
            max_cost: 3,
            max_ins: 0,
            max_del: 3,
            max_subst: 0,
            max_err: 3,
        };
        // SAFETY: the regex, input and output buffers remain live for the call.
        let status = unsafe { tre_reganexec(&preg, c"".as_ptr(), 0, &mut matched, params, 0) };
        unsafe { tre_regfree(&mut preg) };
        assert_eq!(status, REG_OK);
        assert_eq!(
            (matched.cost, matched.num_ins, matched.num_del, matched.num_subst),
            (3, 0, 3, 0)
        );
        assert_eq!((result.rm_so, result.rm_eo), (0, 0));
    }

    #[test]
    fn approximate_matching_advances_states_and_capture_tags() {
        let params = regaparams_t {
            cost_ins: 1,
            cost_del: 1,
            cost_subst: 1,
            max_cost: 1,
            max_ins: 1,
            max_del: 1,
            max_subst: 1,
            max_err: 1,
        };
        let (mut anchored, rc) = unsafe { compile("^(ab)(cd)$", REG_EXTENDED) };
        assert_eq!(rc, REG_OK);
        let (mut unanchored, rc) = unsafe { compile("(ab)(cd)", REG_EXTENDED) };
        assert_eq!(rc, REG_OK);

        // Zero-based, half-open offsets from GNU R 4.6.2 aregexec(useBytes = TRUE).
        for (regex, text, expected) in [
            (&anchored, c"abcd", Some(([(0, 4), (0, 2), (2, 4)], (0, 0, 0, 0)))),
            (
                &anchored,
                c"abxd",
                Some(([(0, 4), (0, 2), (2, 4)], (1, 0, 0, 1))),
            ),
            (
                &anchored,
                c"abXcd",
                Some(([(0, 5), (0, 2), (2, 5)], (1, 1, 0, 0))),
            ),
            (
                &anchored,
                c"abd",
                Some(([(0, 3), (0, 2), (2, 3)], (1, 0, 1, 0))),
            ),
            (&anchored, c"abXYcd", None),
            (
                &unanchored,
                c"axcd__abcd",
                Some(([(6, 10), (6, 8), (8, 10)], (0, 0, 0, 0))),
            ),
        ] {
            let mut output = [regmatch_t::default(); 3];
            let mut matched = regamatch_t {
                nmatch: output.len(),
                pmatch: output.as_mut_ptr(),
                cost: -1,
                num_ins: -1,
                num_del: -1,
                num_subst: -1,
            };
            // SAFETY: the compiled regex, input, and output array are live for the call.
            let status = unsafe {
                tre_reganexec(
                    regex,
                    text.as_ptr(),
                    text.to_bytes().len(),
                    &mut matched,
                    params,
                    0,
                )
            };
            if let Some((offsets, costs)) = expected {
                assert_eq!(status, REG_OK, "{text:?}");
                assert_eq!(output.map(|m| (m.rm_so, m.rm_eo)), offsets, "{text:?}");
                assert_eq!(
                    (matched.cost, matched.num_ins, matched.num_del, matched.num_subst),
                    costs,
                    "{text:?}"
                );
            } else {
                assert_eq!(status, REG_NOMATCH, "{text:?}");
            }
        }
        // SAFETY: each compiled regex has no remaining borrowers.
        unsafe {
            tre_regfree(&mut anchored);
            tre_regfree(&mut unanchored);
        }
    }

    #[test]
    fn match_input_length_reserves_only_the_nul_terminated_sentinel() {
        assert_eq!(match_input_length(0), Ok(0));
        assert_eq!(match_input_length(c_int::MAX as usize), Ok(c_int::MAX));
        assert_eq!(match_input_length(c_int::MAX as usize + 1), Err(REG_ESPACE));
        assert_eq!(match_input_length(usize::MAX - 1), Err(REG_ESPACE));
        assert_eq!(match_input_length(usize::MAX), Ok(-1));
    }

    #[test]
    fn legacy_approximate_adapter_releases_session_workspace_and_retains_output_contract() {
        use crate::sexp::{memory::ArenaBudget, session::RSession};

        // SAFETY: the test helper compiles this valid pattern into an owned descriptor.
        let (mut regex, rc) = unsafe { compile("^(ab)(cd)$", REG_EXTENDED) };
        assert_eq!(rc, REG_OK);
        let params = regaparams_t {
            cost_ins: 1,
            cost_del: 1,
            cost_subst: 1,
            max_cost: 1,
            max_ins: 1,
            max_del: 1,
            max_subst: 1,
            max_err: 1,
        };
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let baseline = owner.with_arena(|arena| arena.workspace_bytes()).unwrap();
            owner
                .with_arena(|arena| {
                    arena.set_budget(ArenaBudget::new(
                        arena.total_bytes_allocated() + baseline + 1,
                        0,
                    ));
                })
                .unwrap();

            let mut output = [MaybeUninit::<regmatch_t>::uninit(); 4];
            output[3].write(regmatch_t {
                rm_so: 99,
                rm_eo: 99,
            });
            let mut matched = regamatch_t {
                nmatch: 3,
                pmatch: output.as_mut_ptr().cast(),
                cost: -1,
                num_ins: -1,
                num_del: -1,
                num_subst: -1,
            };
            // SAFETY: the descriptor, NUL-terminated input and writable prefix are live.
            assert_eq!(
                unsafe { tre_regaexec(&regex, c"abXcd".as_ptr(), &mut matched, params, 0) },
                REG_ESPACE
            );
            assert_eq!(matched.cost, -1);
            assert_eq!(
                owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                baseline
            );
            owner
                .with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))
                .unwrap();

            // SAFETY: the same descriptor, input and writable prefix remain live.
            assert_eq!(
                unsafe { tre_regaexecb(&regex, c"abXcd".as_ptr(), &mut matched, params, 0) },
                REG_OK
            );
            // SAFETY: success initializes the requested prefix; slot 3 is initialized separately.
            let pairs = output.map(|slot| {
                let m = unsafe { slot.assume_init() };
                (m.rm_so, m.rm_eo)
            });
            assert_eq!(pairs, [(0, 5), (0, 2), (2, 5), (99, 99)]);
            assert_eq!(
                (
                    matched.cost,
                    matched.num_ins,
                    matched.num_del,
                    matched.num_subst
                ),
                (1, 1, 0, 0)
            );
            matched.nmatch = 0;
            matched.pmatch = ptr::null_mut();
            // SAFETY: a null output pointer is allowed for zero captures.
            assert_eq!(
                unsafe { tre_regaexec(&regex, c"abxd".as_ptr(), &mut matched, params, 0) },
                REG_OK
            );
            assert_eq!(
                (
                    matched.cost,
                    matched.num_ins,
                    matched.num_del,
                    matched.num_subst
                ),
                (1, 0, 0, 1)
            );
            assert_eq!(
                owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                baseline
            );
        });
        // SAFETY: the descriptor was compiled successfully and has no borrowers.
        unsafe { tre_regfree(&mut regex) };
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

#[cfg(test)]
mod owned_match_tests {
    use super::*;
    use crate::mainutils::grep::RegexWorkspace;

    #[test]
    fn owned_match_respects_bounded_input_and_capture_offsets() {
        let pattern: Vec<_> = "(a+)".chars().map(u32::from).collect();
        let regex = compile::tre_compile_owned_slice(
            &pattern,
            REG_EXTENDED,
            RegexWorkspace::unbounded(),
        )
        .unwrap();
        assert_eq!(regex.capture_count(), 1);
        let mut matches = [regmatch_t::default(); 2];
        assert_eq!(regex.match_bytes(b"x\0aa", &mut matches, 0), REG_OK);
        assert_eq!(matches.map(|m| (m.rm_so, m.rm_eo)), [(2, 4), (2, 4)]);
        assert_eq!(regex.match_bytes(b"", &mut matches, 0), REG_NOMATCH);
        let wide = [u32::from('\u{e9}'), u32::from('a'), u32::from('a')];
        assert_eq!(regex.match_wide(&wide, &mut matches, 0), REG_OK);
        assert_eq!(matches.map(|m| (m.rm_so, m.rm_eo)), [(1, 3), (1, 3)]);
    }

    #[test]
    fn owned_parallel_match_releases_workspace_after_denial() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        session.with_active(|| {
            let pattern: Vec<_> = "(a+?)(a{1,2})b".chars().map(u32::from).collect();
            let regex = compile::tre_compile_owned_slice(
                &pattern,
                REG_EXTENDED,
                crate::mainutils::grep::regex_workspace(),
            )
            .unwrap();
            let mut matches = [regmatch_t::default(); 3];
            let baseline = owner.with_arena(|a| a.workspace_bytes()).unwrap();
            owner
                .with_arena(|a| {
                    let used = a.total_bytes_allocated();
                    a.set_budget(crate::sexp::memory::ArenaBudget::new(used + 256, 0));
                })
                .unwrap();
            assert_eq!(
                regex.match_bytes(b"xaaab", &mut matches, 0),
                REG_ESPACE
            );
            assert_eq!(
                owner.with_arena(|a| a.workspace_bytes()).unwrap(),
                baseline
            );
            owner
                .with_arena(|a| {
                    a.set_budget(crate::sexp::memory::ArenaBudget::unlimited());
                })
                .unwrap();
            assert_eq!(regex.match_bytes(b"xaaab", &mut matches, 0), REG_OK);
            assert_eq!(
                matches.map(|m| (m.rm_so, m.rm_eo)),
                [(1, 5), (1, 2), (2, 4)]
            );
            let wide: Vec<u32> = "éaaab".chars().map(u32::from).collect();
            assert_eq!(regex.match_wide(&wide, &mut matches, 0), REG_OK);
            assert_eq!(
                matches.map(|m| (m.rm_so, m.rm_eo)),
                [(1, 5), (1, 2), (2, 4)]
            );
            assert_eq!(
                regex.match_bytes(b"nomatch", &mut matches, 0),
                REG_NOMATCH
            );
            assert_eq!(
                owner.with_arena(|a| a.workspace_bytes()).unwrap(),
                baseline
            );
        });
    }

    #[test]
    fn owned_backtrack_releases_workspace_after_denial() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        session.with_active(|| {
            let pattern: Vec<_> = "(ab|ac)d".chars().map(u32::from).collect();
            let regex = compile::tre_compile_owned_slice(
                &pattern,
                REG_EXTENDED,
                crate::mainutils::grep::regex_workspace(),
            )
            .unwrap();
            let baseline = owner.with_arena(|a| a.workspace_bytes()).unwrap();
            let mut matches = [regmatch_t {
                rm_so: 99,
                rm_eo: 99,
            }; 2];
            owner
                .with_arena(|a| {
                    // The small scratch rows fit, but stack growth does not.
                    let used = a.total_bytes_allocated();
                    a.set_budget(crate::sexp::memory::ArenaBudget::new(used + 256, 0));
                })
                .unwrap();
            assert_eq!(
                regex.match_bytes(b"xacd", &mut matches, REG_BACKTRACKING_MATCHER),
                REG_ESPACE
            );
            assert_eq!(matches.map(|m| (m.rm_so, m.rm_eo)), [(99, 99); 2]);
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), baseline);
            owner
                .with_arena(|a| a.set_budget(crate::sexp::memory::ArenaBudget::unlimited()))
                .unwrap();
            assert_eq!(
                regex.match_bytes(b"xacd", &mut matches, REG_BACKTRACKING_MATCHER),
                REG_OK
            );
            assert_eq!(matches.map(|m| (m.rm_so, m.rm_eo)), [(1, 4), (1, 3)]);
            assert_eq!(
                regex.match_bytes(b"xad", &mut matches, REG_BACKTRACKING_MATCHER),
                REG_NOMATCH
            );
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), baseline);
        });
    }

    #[test]
    fn backreferences_use_bounded_byte_and_wide_positions() {
        let compile = |pattern: &str| {
            let units: Vec<_> = pattern.chars().map(u32::from).collect();
            compile::tre_compile_owned_slice(&units, REG_EXTENDED, RegexWorkspace::unbounded())
                .unwrap()
        };
        let mut matches = [regmatch_t::default(); 2];
        let bytes = compile(r"(ab)\1");
        for (input, expected) in [
            (&b"xababy"[..], [(1, 5), (1, 3)]),
            (&b"abaabab"[..], [(3, 7), (3, 5)]),
            (&b"\0abab"[..], [(1, 5), (1, 3)]),
        ] {
            assert_eq!(bytes.match_bytes(input, &mut matches, 0), REG_OK);
            assert_eq!(matches.map(|m| (m.rm_so, m.rm_eo)), expected);
        }
        assert_eq!(bytes.match_bytes(b"ab", &mut matches, 0), REG_NOMATCH);
        let wide = compile(r"(.)\1");
        assert_eq!(wide.match_wide(&[0xe9, 0xe9], &mut matches, 0), REG_OK);
        assert_eq!(matches.map(|m| (m.rm_so, m.rm_eo)), [(0, 2), (0, 1)]);
        let empty = compile(r"(a*)\1b");
        assert_eq!(empty.match_bytes(b"b", &mut matches, 0), REG_OK);
        assert_eq!(matches.map(|m| (m.rm_so, m.rm_eo)), [(0, 1), (0, 0)]);
    }
}
