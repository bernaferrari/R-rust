use super::*;
use std::ffi::CString;

unsafe fn assert_compile_error_then_recover(pattern: &str, expected: c_int) {
    unsafe {
        let pattern = CString::new(pattern).unwrap();
        let mut regex: regex_t = std::mem::zeroed();
        assert_eq!(
            tre_regcomp(&mut regex, pattern.as_ptr(), REG_EXTENDED),
            expected
        );
        assert!(
            regex.value.is_null(),
            "failed compile must not publish a dangling TNFA"
        );
        tre_regfree(&mut regex);
        tre_regfree(&mut regex);
        assert_eq!(
            tre_regcomp(&mut regex, c"([a-z]+)".as_ptr(), REG_EXTENDED),
            REG_OK
        );
        let mut matches = [regmatch_t::default(); 2];
        assert_eq!(
            tre_regexec(
                &regex,
                c"cat".as_ptr(),
                matches.len(),
                matches.as_mut_ptr(),
                0
            ),
            REG_OK
        );
        assert_eq!((matches[0].rm_so, matches[0].rm_eo), (0, 3));
        assert_eq!((matches[1].rm_so, matches[1].rm_eo), (0, 3));
        tre_regfree(&mut regex);
        assert!(
            regex.value.is_null(),
            "final free must revoke the published TNFA"
        );
        tre_regfree(&mut regex);
    }
}

macro_rules! compile_error {
    ($name:ident, $pattern:expr, $code:expr) => {
        #[test]
        fn $name() {
            unsafe { assert_compile_error_then_recover($pattern, $code) }
        }
    };
}

compile_error!(compile_error_unclosed_bracket, "[", REG_EBRACK);
compile_error!(compile_error_unclosed_group, "(abc", REG_EPAREN);
compile_error!(compile_error_trailing_escape, "abc\\", REG_EESCAPE);
compile_error!(compile_error_reversed_range, "[z-a]", REG_ERANGE);
compile_error!(compile_error_unknown_class, "[[:notaclass:]]", REG_ECTYPE);
compile_error!(compile_error_invalid_repeat_bounds, "a{3,1}", REG_BADBR);

#[test]
fn compile_error_literal_class_ids_and_parameter_addresses_remain_distinct() {
    unsafe {
        let arena = mem::tre_mem_new();
        assert!(!arena.is_null());
        let node = super::super::ast::tre_ast_new_literal(arena, 65, 65, 0);
        assert!(!node.is_null());
        let literal = &mut *((*node).obj as *mut tre_literal_t);
        assert_eq!(literal.get_class(), 0);
        assert!(literal.get_params().is_null());
        let mut parameters = [1, 2, 3];
        literal.set_class(usize::MAX);
        literal.set_params(parameters.as_mut_ptr());
        assert_eq!(literal.get_class(), usize::MAX);
        assert_eq!(literal.get_params(), parameters.as_mut_ptr());
        literal.set_class(7);
        assert_eq!(literal.get_params(), parameters.as_mut_ptr());
        literal.set_params(ptr::null_mut());
        assert_eq!(literal.get_class(), 7);
        mem::tre_mem_destroy(arena);
    }
}

#[test]
fn compile_error_repeated_group_capture_control_matches_gnu() {
    unsafe {
        let mut regex: regex_t = std::mem::zeroed();
        assert_eq!(
            tre_regcomp(&mut regex, c"((ab|[c-e]){1,3})+".as_ptr(), REG_EXTENDED),
            REG_OK
        );
        let mut matches = [regmatch_t::default(); 3];
        assert_eq!(
            tre_regexec(
                &regex,
                c"ababecab".as_ptr(),
                matches.len(),
                matches.as_mut_ptr(),
                0
            ),
            REG_OK
        );
        let offsets = matches.map(|m| (m.rm_so, m.rm_eo));
        tre_regfree(&mut regex);
        assert_eq!(offsets, [(0, 8), (4, 8), (6, 8)]);
    }
}

#[test]
fn compile_error_every_native_allocation_failure_releases_partial_state() {
    let compile = || unsafe {
        let mut regex: regex_t = std::mem::zeroed();
        let status = tre_regcomp(&mut regex, c"((ab|[c-e]){1,3})+".as_ptr(), REG_EXTENDED);
        if status != REG_OK {
            assert!(regex.value.is_null());
        }
        tre_regfree(&mut regex);
        assert!(regex.value.is_null());
        status
    };
    let (status, baseline) = mem::fault::observe(None, compile);
    assert_eq!(status, REG_OK);
    assert_eq!(baseline.remaining, 0);
    assert!(baseline.attempts > 10);
    eprintln!("TRE compile allocation failure sites={}", baseline.attempts);
    for failure in 0..baseline.attempts {
        let (status, report) = mem::fault::observe(Some(failure), compile);
        assert_eq!(status, REG_ESPACE, "allocation failure site {failure}");
        assert_eq!(
            report.remaining, 0,
            "allocation failure site {failure} leaked"
        );
    }
    let (status, recovered) = mem::fault::observe(None, compile);
    assert_eq!(status, REG_OK);
    assert_eq!(recovered.remaining, 0);
}
