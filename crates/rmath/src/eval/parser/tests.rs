use super::*;

use crate::sexp::accessors::{
    CADR, CAR, CDR, CHAR, COMPLEX, PRINTNAME, STRING_ELT, TAG, TYPEOF, XLENGTH,
};
use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::globals::R_NilValue;
use crate::sexp::session::RSession;

fn parse_str<'session>(
    session: &'session RSession,
    input: &str,
) -> Result<Sexp<'session>, ParseError> {
    let domain =
        SessionNodeFactory::new(session.owner_token().expect("active parser fixture")).domain();
    session.with_active_in(|owner| unsafe {
        // SAFETY: only the arena is lent, and the domain was captured before it.
        crate::sexp::memory::with_arena_in(owner, |arena| parse(input, arena, domain))
    })
}

fn parse_list<'session>(
    session: &'session RSession,
    input: &str,
    with_spans: bool,
) -> Result<Vec<Sexp<'session>>, ParseError> {
    let domain =
        SessionNodeFactory::new(session.owner_token().expect("active parser fixture")).domain();
    session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| {
            let mut parser = Parser::new(input, arena, domain);
            if with_spans {
                parser.parse_top_level_with_spans().map(|spans| {
                    spans
                        .into_iter()
                        .map(|(expression, _, _)| expression)
                        .collect()
                })
            } else {
                parser.parse_top_level_expressions()
            }
        })
    })
}

#[test]
fn passive_parser_domain_rejects_foreign_arena_before_allocating() {
    let first = RSession::new_for_gc_tests();
    let first_domain = SessionNodeFactory::new(first.owner_token().unwrap()).domain();
    let second = RSession::new_for_gc_tests();
    let second_domain = SessionNodeFactory::new(second.owner_token().unwrap()).domain();
    second.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| {
            let roots_before = crate::sexp::memory::automatic_roots(&arena.heap_identity()).len();
            let mut parser = Parser::new("foreign_symbol + 1", arena, first_domain);
            let error = parser.parse_program().unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("arena belongs to another runtime")
            );
            assert_eq!(
                crate::sexp::memory::automatic_roots(&parser.arena.heap_identity()).len(),
                roots_before
            );
            assert!(!(*owner).symbols.contains_key("foreign_symbol"));
        });
    });
    // Rejection must not poison the receiving arena or its original bank.
    second.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| {
            let mut parser = Parser::new("3", arena, second_domain);
            assert_eq!(parser.parse_program().unwrap().real_elt(0), Some(3.0));
        });
    });
}

#[test]
fn passive_parser_domain_rejects_foreign_active_symbol_bank() {
    let first = RSession::new_for_gc_tests();
    let first_domain = SessionNodeFactory::new(first.owner_token().unwrap()).domain();
    let first_owner = first.with_active_in(|owner| owner);
    let second = RSession::new_for_gc_tests();
    second.with_active_in(|second_owner| unsafe {
        // Intentionally provide the correct physical arena/domain with
        // the wrong active symbol bank. No symbol or graph may be built.
        crate::sexp::memory::with_arena_in(first_owner, |arena| {
            let mut parser = Parser::new("foreign_symbol + 1", arena, first_domain);
            let error = parser.parse_top_level_expressions().unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("symbol bank belongs to another runtime")
            );
            assert!(!(*second_owner).symbols.contains_key("foreign_symbol"));
        });
    });
}

#[test]
fn passive_parser_domain_rejects_revocation_even_for_empty_program() {
    let mut session = RSession::new_for_gc_tests();
    let token = session.owner_token().unwrap();
    let owner = token.weak_owner().unwrap();
    let domain = owner.node_factory().unwrap().domain();
    let pin = owner.pin().unwrap();
    session.close();
    unsafe {
        // Pin retains cleanup storage after the facade closes. Passive
        // publication still rejects revocation, including nil output.
        crate::sexp::memory::with_arena_in(pin.as_ptr(), |arena| {
            let mut parser = Parser::new("", arena, domain);
            assert!(parser.parse_program().is_err());
        });
    }
}

#[test]
fn failed_parse_releases_completed_automatic_roots() {
    let session = RSession::new_for_gc_tests();
    let existing = parse_list(&session, "99", false).unwrap();
    let preserve_count =
        || unsafe { crate::sexp::protect::with_preserved_objects(|roots| roots.len()) };
    let baseline = preserve_count();
    for input in ["1; )", "1; _", "1; 2; function("] {
        for with_spans in [false, true] {
            for _ in 0..3 {
                assert!(parse_list(&session, input, with_spans).is_err());
                session.gc();
                assert_eq!(preserve_count(), baseline);
                assert_eq!(existing[0].real_elt(0), Some(99.0));
                let roots = crate::sexp::memory::automatic_roots(&unsafe {
                    session.with_active_in(|owner| (*owner).heap_identity.clone())
                });
                assert_eq!(roots.len(), 1, "failed parse retained an automatic root");
            }
        }
    }
    drop(existing);
    session.gc();
    assert_eq!(preserve_count(), baseline);
}

#[test]
fn successful_parse_returns_automatic_roots_and_drop_releases_them() {
    let session = RSession::new_for_gc_tests();
    for with_spans in [false, true] {
        let expressions = parse_list(&session, "1; 2", with_spans).unwrap();
        let allocations: Vec<_> = expressions
            .iter()
            .map(|expression| {
                crate::sexp::memory::checked_projection(expression.clone().as_raw())
                    .unwrap()
                    .1
            })
            .collect();
        session.with_active(|| {
            crate::sexp::gengc::full_gc();
        });
        assert_eq!(expressions[0].real_elt(0), Some(1.0));
        assert_eq!(expressions[1].real_elt(0), Some(2.0));
        unsafe {
            crate::sexp::protect::with_preserved_objects(|roots| assert!(roots.is_empty()));
        }
        drop(expressions);
        session.with_active(|| {
            crate::sexp::gengc::full_gc();
        });
        assert!(allocations.iter().all(|allocation| !allocation.is_live()));
    }
}

fn must<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("test failed: {e:?}"),
    }
}

unsafe fn call_head_name(call: SEXP) -> String {
    unsafe {
        let printname = PRINTNAME(CAR(call));
        let chars = CHAR(printname);
        std::ffi::CStr::from_ptr(chars)
            .to_string_lossy()
            .into_owned()
    }
}

fn generated_parser_input(mut seed: u64, len: usize) -> String {
    const ALPHABET: &[u8] = b"abcxyz0123456789+-*/^<>=!&|(){}[],$@_.'\"`# \n\t;:";
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        out.push(ALPHABET[((seed >> 32) as usize) % ALPHABET.len()] as char);
    }
    out
}

fn adversarial_iterations(default: u64) -> u64 {
    std::env::var("RPORT_ADVERSARIAL_ITERS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[test]
fn adversarial_parser_inputs_do_not_panic() {
    let session = RSession::new_for_gc_tests();
    let fixed = [
        ")",
        "(((((((((",
        "\"unterminated",
        "'unterminated\\",
        "`unterminated",
        "x[[[[1]]",
        "function(x,,y) x",
        "if (TRUE) { # comment without close",
        "a <- 1 +\n# comment\n)",
        "repeat { next; break; } }",
    ];

    for input in fixed {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parse_str(&session, input)));
        assert!(result.is_ok(), "parser panicked for fixed input: {input:?}");
    }

    for seed in 0..adversarial_iterations(256) {
        let input = generated_parser_input(seed, (seed as usize % 96) + 1);
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parse_str(&session, &input)));
        assert!(result.is_ok(), "parser panicked for seed {seed}: {input:?}");
    }
}

// --- Basic atoms ---

#[test]
fn test_integer_literal() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "42"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
    }
}

#[test]
fn test_real_literal() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "3.14"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
    }
}

#[test]
fn test_complex_literal() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "2i"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::CPLXSXP);
        assert_eq!(XLENGTH(result), 1);
        let data = COMPLEX(result);
        assert_eq!((*data).r, 0.0);
        assert_eq!((*data).i, 2.0);
    }
}

#[test]
fn test_na_complex_literal() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "NA_complex_"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::CPLXSXP);
        assert_eq!(XLENGTH(result), 1);
        let data = COMPLEX(result);
        assert_eq!((*data).r.to_bits(), crate::sexp::ffi::R_NA_BIT_PATTERN);
        assert_eq!((*data).i.to_bits(), crate::sexp::ffi::R_NA_BIT_PATTERN);
    }
}

#[test]
fn test_int_suffix() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "42L"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
    }
}

#[test]
fn test_string_literal() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "\"hello\""));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
    }
}

#[test]
fn test_raw_string_delimiters_and_dashes() {
    let session = RSession::new_for_gc_tests();
    for (input, expected) in [
        (r#"r"(back\slash)""#, "back\\slash"),
        (r#"R"--[brackets]--""#, "brackets"),
        (r#"r"-{braces}-""#, "braces"),
    ] {
        let result_handle = must(parse_str(&session, input));
        let result = result_handle.clone().as_raw();
        unsafe {
            let chars = CHAR(STRING_ELT(result, 0));
            assert_eq!(std::ffi::CStr::from_ptr(chars).to_string_lossy(), expected);
        }
    }
}

#[test]
fn gnu_decimal_source_literals_preserve_original_long_mantissa_and_underflow_precision() {
    let session = RSession::new_for_gc_tests();
    let decode = |hex: &str| {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>()
    };
    let mut cases = 0;
    for row in include_str!("numeric-oracle.tsv").lines() {
        let columns = row.split('|').collect::<Vec<_>>();
        let source = String::from_utf8(decode(columns[0])).unwrap();
        let expected = decode(columns[2]);
        let value = must(parse_str(&session, &source));
        match columns[1] {
            "double" => assert_eq!(
                value.try_real_elt(0).unwrap().to_le_bytes().as_slice(),
                expected,
                "original GNU source {source}"
            ),
            "integer" => assert_eq!(
                value.try_integer_elt(0).unwrap().to_le_bytes().as_slice(),
                expected,
                "original GNU source {source}"
            ),
            "complex" => {
                let actual = value.try_complex_elt(0).unwrap();
                let mut bytes = actual.r.to_le_bytes().to_vec();
                bytes.extend_from_slice(&actual.i.to_le_bytes());
                assert_eq!(bytes, expected, "original GNU source {source}");
            }
            other => panic!("unexpected original literal type {other}"),
        }
        cases += 1;
    }
    assert_eq!(cases, 21);
}

#[test]
fn test_numeric_overflow_and_malformed_literals_match_supported_r_behavior() {
    let session = RSession::new_for_gc_tests();
    for input in ["1e9999", "0x1p1024"] {
        let value_handle = must(parse_str(&session, input));
        let value = value_handle.clone().as_raw();
        unsafe {
            assert!((*crate::sexp::accessors::REAL(value)).is_infinite());
        }
    }

    for input in ["0x", "0x1p", "1e", "1e+"] {
        assert!(
            parse_str(&session, input).is_err(),
            "{input:?} must not become zero"
        );
    }
}

#[test]
fn test_true_false_null() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let t_handle = must(parse_str(&session, "TRUE"));
        let t = t_handle.clone().as_raw();
        assert_eq!(TYPEOF(t), SEXPTYPE::LGLSXP);

        let f_handle = must(parse_str(&session, "FALSE"));
        let f = f_handle.clone().as_raw();
        assert_eq!(TYPEOF(f), SEXPTYPE::LGLSXP);

        let n_handle = must(parse_str(&session, "NULL"));
        let n = n_handle.clone().as_raw();
        assert_eq!(n, R_NilValue());
    }
}

#[test]
fn t_and_f_parse_as_symbols_not_logicals() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let t_handle = must(parse_str(&session, "T"));
        let t = t_handle.clone().as_raw();
        assert_eq!(TYPEOF(t), SEXPTYPE::SYMSXP);
        let f_handle = must(parse_str(&session, "F"));
        let f = f_handle.clone().as_raw();
        assert_eq!(TYPEOF(f), SEXPTYPE::SYMSXP);
        let call_handle = must(parse_str(&session, "F()"));
        let call = call_handle.clone().as_raw();
        assert_eq!(TYPEOF(call), SEXPTYPE::LANGSXP);
        assert_eq!(TYPEOF(CAR(call)), SEXPTYPE::SYMSXP);
    }
}

#[test]
fn test_identifier() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "x"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::SYMSXP);
    }
}

#[test]
fn test_na_inf_nan() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let na_handle = must(parse_str(&session, "NA"));
        let na = na_handle.clone().as_raw();
        assert_eq!(TYPEOF(na), SEXPTYPE::LGLSXP);

        let inf_handle = must(parse_str(&session, "Inf"));
        let inf = inf_handle.clone().as_raw();
        assert_eq!(TYPEOF(inf), SEXPTYPE::REALSXP);

        let nan_handle = must(parse_str(&session, "NaN"));
        let nan = nan_handle.clone().as_raw();
        assert_eq!(TYPEOF(nan), SEXPTYPE::REALSXP);
    }
}

#[test]
fn test_na_character() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let na_chr_handle = must(parse_str(&session, "NA_character_"));
        let na_chr = na_chr_handle.clone().as_raw();
        assert_eq!(TYPEOF(na_chr), SEXPTYPE::STRSXP);
        assert_eq!(crate::sexp::accessors::LENGTH(na_chr), 1);
        let elt = crate::sexp::accessors::STRING_ELT(na_chr, 0);
        assert!(!elt.is_null());
        assert_eq!(
            crate::sexp::accessors::TYPEOF(elt),
            SEXPTYPE::CHARSXP.as_c_int()
        );
        assert_eq!((*elt).sxpinfo.gp(), 1);
    }
}

// --- Binary operators ---

#[test]
fn test_addition() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "1 + 2"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        let op = CAR(result);
        assert_eq!(TYPEOF(op), SEXPTYPE::SYMSXP);
    }
}

#[test]
fn test_chained_addition() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "1 + 2 + 3"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_operator_precedence() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "2 + 3 * 4"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_power() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "2^3"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_integer_div() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "5 %/% 2"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_modulus() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "5 %% 2"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

// --- Unary ---

#[test]
fn test_unary_minus() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "-5"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn unary_minus_is_looser_than_power() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "exp(-x^2)"));
        let result = result_handle.clone().as_raw();
        let call = CADR(result);
        assert_eq!(call_head_name(call), "-");
        assert_eq!(call_head_name(CADR(call)), "^");
    }
}

#[test]
fn unary_exponent_is_allowed_and_power_remains_right_associative() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "2^-2"));
        let result = result_handle.clone().as_raw();
        assert_eq!(call_head_name(result), "^");
        assert_eq!(call_head_name(crate::sexp::accessors::CADDR(result)), "-");

        let nested_handle = must(parse_str(&session, "2^3^2"));
        let nested = nested_handle.clone().as_raw();
        assert_eq!(call_head_name(nested), "^");
        assert_eq!(call_head_name(crate::sexp::accessors::CADDR(nested)), "^");
    }
}

// --- Assignment ---

#[test]
fn test_assignment() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "x <- 42"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_equals_assign() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "x = 42"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        assert_eq!(call_head_name(result), "=");
    }
}

#[test]
fn test_right_assign() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "42 -> x"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_super_assignment_preserves_operator() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "x <<- 42"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        assert_eq!(call_head_name(result), "<<-");
    }
}

// --- Function calls ---

#[test]
fn test_function_call() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "f(x, y)"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        let fun = CAR(result);
        assert_eq!(TYPEOF(fun), SEXPTYPE::SYMSXP);
    }
}

#[test]
fn test_subscript_argument_order() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "x[2]"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        let args = CDR(result);
        assert_eq!(TYPEOF(CAR(args)), SEXPTYPE::SYMSXP);
        assert_eq!(TYPEOF(CADR(args)), SEXPTYPE::REALSXP);
    }
}

#[test]
fn test_named_arg() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "f(x = 1)"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_quoted_named_arg() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, r#"c(1:2, "2" = 4)"#));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        let named = CDR(CDR(result));
        let tag = TAG(named);
        assert!(!tag.is_null());
        assert_ne!(tag, R_NilValue());
    }
}

#[test]
fn test_parenthesized() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "(1 + 2)"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
        assert_eq!(call_head_name(result), "(");

        let inner = CADR(result);
        assert_eq!(TYPEOF(inner), SEXPTYPE::LANGSXP);
        assert_eq!(call_head_name(inner), "+");
    }
}

// --- Logical ---

#[test]
fn test_comparison() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "x < 10"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_logical_ops() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let and_handle = must(parse_str(&session, "x && y"));
        let and = and_handle.clone().as_raw();
        assert_eq!(TYPEOF(and), SEXPTYPE::LANGSXP);

        let or_handle = must(parse_str(&session, "x || y"));
        let or = or_handle.clone().as_raw();
        assert_eq!(TYPEOF(or), SEXPTYPE::LANGSXP);

        let not_handle = must(parse_str(&session, "!x"));
        let not = not_handle.clone().as_raw();
        assert_eq!(TYPEOF(not), SEXPTYPE::LANGSXP);
    }
}

// --- Programs ---

#[test]
fn test_multi_expr() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "x <- 1; y <- 2"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_comment_preserves_newline_terminator() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "x <- 1 # comment\ny <- 2\nx + y"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_comment_stops_at_carriage_return_and_hash_in_string_stays_literal() {
    let session = RSession::new_for_gc_tests();
    let carriage_return_comment = parse_list(&session, "# ignored\rx <- 1\rx", false)
        .expect("bare CR must end a line comment");
    assert_eq!(carriage_return_comment.len(), 2);

    let hash_in_string = parse_list(&session, "x <- \"# literal\"\rx", false)
        .expect("hash in string is not a comment");
    assert_eq!(hash_in_string.len(), 2);
}

#[test]
fn test_newline_after_infix_continues_expression() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "x <- 1 +\n 2 *\n 3"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_empty_input() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, ""));
        let result = result_handle.clone().as_raw();
        assert_eq!(result, R_NilValue());
    }
}

#[test]
fn test_complex_expr() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "sqrt(x^2 + y^2)"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_error_unexpected_rparen() {
    let session = RSession::new_for_gc_tests();
    assert!(parse_str(&session, ")").is_err());
}

#[test]
fn test_malformed_lexemes_are_explicit_parse_errors() {
    let session = RSession::new_for_gc_tests();
    for input in [
        "\"unterminated",
        "'unterminated",
        "`unterminated",
        "r\"(unterminated",
        "1; \u{0}",
    ] {
        let error = parse_str(&session, input).expect_err("malformed lexeme must not parse");
        assert!(
            error.to_string().contains("unexpected"),
            "unexpected error for {input:?}: {error}"
        );
        assert!(
            error.to_string().contains(input),
            "lexical error omitted its source span for {input:?}: {error}"
        );
    }
}

#[test]
fn test_parser_reports_arena_exhaustion_as_resource_error() {
    let mut session = RSession::new_for_gc_tests();
    session.set_arena_budget(crate::sexp::memory::ArenaBudget::new(1, 1));

    let error = parse_str(&session, "1 + 2").expect_err("allocation exhaustion must fail parsing");

    assert!(error.to_string().contains("allocation"), "{error}");
    assert!(error.to_string().contains("(<input>:1:"), "{error}");
}

#[test]
fn zz_strict_gate_unit() {
    // Direct unit test of the strict gate: parse_expressions_strict
    // must reject top-level newline-crossing `else` (gram.y ELSE rule).
    let session = RSession::new_for_gc_tests();
    let err = parse_list(&session, "if (TRUE)\n1\nelse 2\n", false)
        .expect_err("strict path must reject newline-else");
    assert!(err.to_string().contains("unexpected 'else'"), "{err}");
}
#[test]
fn zz_paren_newline_plus() {
    let session = RSession::new_for_gc_tests();
    // Newline BEFORE a binary operator continues the expression
    // (EatLines): `(1\n + 2)` must parse like `(1 +\n 2)`.
    let __handle = must(parse_str(&session, "(1\n + 2)"));
    let _ = __handle.clone().as_raw();
    let __handle = must(parse_str(&session, "(1 +\n 2)"));
    let _ = __handle.clone().as_raw();
}

// --- NEW: Control flow ---

#[test]
fn test_if() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "if (x > 0) x"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_if_else() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "if (x > 0) x else -x"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn if_body_newline_paren_is_next_expression() {
    let session = RSession::new_for_gc_tests();

    let exprs = parse_list(&session, "if (FALSE)\n    identity(1)\n(2)\n", false)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        exprs.len(),
        2,
        "if-body newline then (2) must be two top-level expressions"
    );
    unsafe {
        assert_eq!(call_head_name(exprs[0].clone().as_raw()), "if");
        assert_eq!(call_head_name(exprs[1].clone().as_raw()), "(");
    }
}

#[test]
fn test_for_loop() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "for (i in 1:10) print(i)"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_while_loop() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "while (x > 0) x <- x - 1"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_repeat_loop() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "repeat { x <- x + 1 }"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_break_next() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let brk_handle = must(parse_str(&session, "break"));
        let brk = brk_handle.clone().as_raw();
        assert_eq!(TYPEOF(brk), SEXPTYPE::LANGSXP);

        let nxt_handle = must(parse_str(&session, "next"));
        let nxt = nxt_handle.clone().as_raw();
        assert_eq!(TYPEOF(nxt), SEXPTYPE::LANGSXP);
    }
}

// --- NEW: Blocks ---

#[test]
fn test_block() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "{ a <- 1; b <- 2; a + b }"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_empty_block() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "{}"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

// --- NEW: Function definitions ---

#[test]
fn test_function_def() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "function(x) x^2"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_function_multi_args() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "function(x, y = 1) x + y"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_function_no_args() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "function() 42"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

// --- NEW: Subscript ---

#[test]
fn test_subscript() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "x[1]"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_subscript_multi() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "x[i, j]"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_double_subscript() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "x[[1]]"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

// --- NEW: Member access ---

#[test]
fn test_dollar_access() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "df$col"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_at_access() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "obj@slot"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

// --- NEW: Custom infix ---

#[test]
fn test_custom_infix() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "x %in% y"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_matrix_multiply() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "A %*% B"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

// --- NEW: Backtick names ---

#[test]
fn test_backtick_name() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "`weird name`"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::SYMSXP);
    }
}

// --- NEW: Formula ---

#[test]
fn test_formula() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "y ~ x + z"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

// --- NEW: DotDotDot ---

#[test]
fn test_varargs() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "f(...)"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

// --- NEW: Chained postfix ---

#[test]
fn test_chained_dollar_subscript() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "df$col[1]"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_function_in_block() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "{ f <- function(x) x^2; f(3) }"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}

#[test]
fn test_form_feed_is_whitespace() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        let result_handle = must(parse_str(&session, "1\u{0C}+\n2"));
        let result = result_handle.clone().as_raw();
        assert_eq!(TYPEOF(result), SEXPTYPE::LANGSXP);
    }
}
