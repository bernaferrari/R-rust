use r_embed::RSession;

fn large_supported_body() -> String {
    let statements = std::iter::repeat_n("x + 1L", 60)
        .collect::<Vec<_>>()
        .join(";");
    format!("function(x) {{{statements}}}")
}

#[test]
fn enable_jit_matches_gnu_query_coercion_and_arity_contracts() {
    let mut session = RSession::new().unwrap();
    let output = session
        .eval(
            r#"
original <- compiler::enableJIT(-1L)
baseline <- compiler::enableJIT(3L)
a <- compiler::enableJIT(0L)
b <- compiler::enableJIT(-1L)
c <- compiler::enableJIT(NULL)
d <- compiler::enableJIT(-1L)
e <- compiler::enableJIT(TRUE)
f <- compiler::enableJIT(2.8)
g <- compiler::enableJIT("4")
h <- compiler::enableJIT(-1L)
restored <- compiler::enableJIT(original)
cat(typeof(a), a, b, c, d, e, f, g, h)
"#,
        )
        .unwrap();
    // Each result is the previous level. NULL and negative values only query;
    // real, logical and character inputs use GNU's asInteger coercion.
    assert_eq!(output.trim(), "integer 3 0 0 0 0 1 2 4");

    let errors = session
        .eval(
            r#"
missing <- tryCatch(compiler::enableJIT(), error=function(e) TRUE)
extra <- tryCatch(compiler::enableJIT(1L, 2L), error=function(e) TRUE)
unknown <- tryCatch(compiler::enableJIT(other=1L), error=function(e) TRUE)
partial <- tryCatch({ old <- compiler::enableJIT(lev=1L); compiler::enableJIT(old); TRUE }, error=function(e) FALSE)
cat(missing, extra, unknown, partial, typeof(compiler::enableJIT))
"#,
        )
        .unwrap();
    assert_eq!(errors.trim(), "TRUE TRUE TRUE TRUE builtin");
}

#[test]
fn enable_jit_controls_real_closure_compilation_and_preserves_failed_sources() {
    let body = large_supported_body();
    let mut session = RSession::new().unwrap();
    let output = session
        .eval(&format!(
            r#"
original <- compiler::enableJIT(-1L)
disabled_old <- compiler::enableJIT(0L)
cold <- {body}
cold_value <- cold(2L)
cold_source <- tryCatch({{ serialize(cold, NULL); TRUE }}, error=function(e) FALSE)

enabled_old <- compiler::enableJIT(3L)
hot <- {body}
hot_value <- hot(2L)
hot_error <- tryCatch({{ serialize(hot, NULL); "" }}, error=function(e) conditionMessage(e))
hot_is_private_bytecode <- grepl("cannot serialize private bytecode dialect", hot_error, fixed=TRUE)

jit_unknown <- function(x) x
unsupported <- function(x) {{ {}; jit_unknown(x) }}
unsupported_value <- unsupported(7L)
unsupported_stayed_source <- tryCatch({{ serialize(unsupported, NULL); TRUE }}, error=function(e) FALSE)
restored <- compiler::enableJIT(original)
cat(cold_value, cold_source, hot_value, hot_is_private_bytecode,
    unsupported_value, unsupported_stayed_source)
"#,
            std::iter::repeat_n("x + 1L", 60)
                .collect::<Vec<_>>()
                .join(";")
        ))
        .unwrap();
    assert_eq!(output.trim(), "3 TRUE 3 TRUE 7 TRUE");
}

#[test]
fn enable_jit_level_is_session_local() {
    let mut left = RSession::new().unwrap();
    let mut right = RSession::new().unwrap();

    left.eval("left_original <- compiler::enableJIT(0L)")
        .unwrap();
    right
        .eval("right_original <- compiler::enableJIT(2L)")
        .unwrap();
    assert_eq!(left.eval("compiler::enableJIT(-1L)").unwrap(), "[1] 0");
    assert_eq!(right.eval("compiler::enableJIT(-1L)").unwrap(), "[1] 2");

    left.eval("compiler::enableJIT(left_original)").unwrap();
    right.eval("compiler::enableJIT(right_original)").unwrap();
}
