use r_embed::RSession;

fn raw_expression(bytes: &[u8]) -> String {
    format!(
        "as.raw(c({}))",
        bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn load(session: &mut RSession, bytes: &[u8]) {
    session
        .eval(&format!("f <- unserialize({})", raw_expression(bytes)))
        .unwrap();
}

fn unique_stream_offset(bytes: &[u8], words: &[i32]) -> usize {
    let encoded = words
        .iter()
        .flat_map(|word| word.to_be_bytes())
        .collect::<Vec<_>>();
    let offsets = bytes
        .windows(encoded.len())
        .enumerate()
        .filter_map(|(offset, candidate)| (candidate == encoded).then_some(offset))
        .collect::<Vec<_>>();
    assert_eq!(
        offsets.len(),
        1,
        "fixture must contain one exact instruction stream"
    );
    offsets[0]
}

// GETVAR x; MATH1 call=0, fun=6 (sin); RETURN.
const SIN_WORDS: [i32; 7] = [12, 20, 1, 118, 0, 6, 1];

#[test]
fn imported_gnu_math1_preserves_values_and_visibility() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-math1/sin.rds"),
    );
    assert_eq!(
        session
            .eval("isTRUE(all.equal(f(pi/2), 1)) && identical(typeof(f(0)), 'double')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(withVisible(f(0)), list(value=0, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("g<-unserialize(serialize(f,NULL)); isTRUE(all.equal(g(0), 0))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-math1/expm1.rds"),
    );
    assert_eq!(
        session
            .eval("isTRUE(all.equal(f(1), exp(1)-1)) && identical(withVisible(f(0)), list(value=0, visible=TRUE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn mutated_gnu_math1_instruction_runs_over_retained_source() {
    let original = include_bytes!("fixtures/gnu-bytecode-math1/sin.rds");
    // Flip MATH1 fun index sin=6 -> cos=5. GNU checks CAR(call)==math1funs[i];
    // retained source is still sin(x), so a source fallback would return 0.
    let offset = unique_stream_offset(original, &SIN_WORDS);
    let mut changed = original.to_vec();
    changed[offset + 5 * 4..offset + 6 * 4].copy_from_slice(&5_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &changed);
    let err = session.eval("f(0)");
    assert!(
        err.is_err(),
        "mutated MATH1 index must not run retained sin(x), got {err:?}"
    );
    assert!(
        format!("{err:?}").to_lowercase().contains("mismatch"),
        "GNU MATH1 index mutation errors with compiler/interpreter mismatch, got {err:?}"
    );
}

#[test]
fn malformed_math1_empty_stack_fails_before_source_fallback() {
    let original = include_bytes!("fixtures/gnu-bytecode-math1/sin.rds");
    let offset = unique_stream_offset(original, &SIN_WORDS);
    let mut malformed = original.to_vec();
    let replacement = [12_i32, 118, 0, 6, 1, 1, 1];
    for (i, word) in replacement.iter().enumerate() {
        let at = offset + i * 4;
        malformed[at..at + 4].copy_from_slice(&word.to_be_bytes());
    }

    let mut session = RSession::new().unwrap();
    let loaded = session.eval(&format!(
        "f <- unserialize({})",
        raw_expression(&malformed)
    ));
    if loaded.is_err() {
        assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
        return;
    }
    assert!(
        session.eval("f(0)").is_err(),
        "empty-stack MATH1 must not run retained source"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}

fn assert_identical(session: &mut RSession, label: &str, expr: &str, expected: &str) {
    let code = format!(
        "got <- ({expr}); expected <- ({expected}); \
         if (!identical(got, expected)) stop(paste0( \
            '{label}', ' typeof=', typeof(got), \
            ' got=', paste(capture.output(dput(got)), collapse=' '), \
            ' expected=', paste(capture.output(dput(expected)), collapse=' '))); \
         TRUE"
    );
    let got = session
        .eval(&code)
        .unwrap_or_else(|err| panic!("{label} failed: {err:?}"));
    assert_eq!(got.trim(), "[1] TRUE", "{label}: {got}");
}

fn assert_value_warning(
    session: &mut RSession,
    label: &str,
    expr: &str,
    expected: &str,
    warning_substring: Option<&str>,
) {
    let warn_check = match warning_substring {
        Some(text) => format!(
            "if (is.null(w) || !grepl('{text}', w, fixed=TRUE)) stop(paste0('{label} warn=', w))"
        ),
        None => format!("if (!is.null(w)) stop(paste0('{label} unexpected warn=', w))"),
    };
    let code = format!(
        "w <- NULL; got <- withCallingHandlers(({expr}), warning=function(wrn) {{ \
            w <<- conditionMessage(wrn); invokeRestart('muffleWarning') }}); \
         expected <- ({expected}); \
         if (!identical(got, expected)) stop(paste0( \
            '{label}', ' typeof=', typeof(got), \
            ' got=', paste(capture.output(dput(got)), collapse=' '), \
            ' expected=', paste(capture.output(dput(expected)), collapse=' '), \
            ' warn=', w)); \
         {warn_check}; TRUE"
    );
    let got = session
        .eval(&code)
        .unwrap_or_else(|err| panic!("{label} failed: {err:?}"));
    assert_eq!(got.trim(), "[1] TRUE", "{label}: {got}");
}

// GNU R 4.6.1 cmpfun: abs is CALLBUILTIN and sqrt is SQRT. These opcodes are
// LOG=116, LOGBASE=117, and MATH1=118 (sin index 6, floor index 0).
const FLOOR_WORDS: [i32; 7] = [12, 20, 1, 118, 0, 0, 1];

#[test]
fn gnu_log_logbase_and_math1_match_na_and_empty_vectors() {
    let floor = include_bytes!("fixtures/gnu-bytecode-math1/floor.rds");
    unique_stream_offset(floor, &FLOOR_WORDS);

    let mut session = RSession::new().unwrap();

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-log/log.rds"),
    );
    assert_value_warning(
        &mut session,
        "log vector",
        "f(c(1, NA_real_, 0, -1))",
        "c(0, NA_real_, -Inf, NaN)",
        Some("NaNs produced"),
    );
    assert_value_warning(
        &mut session,
        "log empty",
        "f(numeric(0))",
        "numeric(0)",
        None,
    );
    assert_identical(
        &mut session,
        "log integer empty",
        "f(integer(0))",
        "numeric(0)",
    );
    assert_identical(
        &mut session,
        "log integer NA",
        "f(c(1L, NA_integer_))",
        "c(0, NA_real_)",
    );
    assert_identical(
        &mut session,
        "log named NA",
        "f(c(a=1, b=NA_real_))",
        "c(a=0, b=NA_real_)",
    );
    assert_identical(
        &mut session,
        "log scalar NA",
        "f(NA_real_)",
        "NA_real_",
    );
    assert_identical(
        &mut session,
        "log named empty",
        "f(structure(numeric(0), names=character(0)))",
        "structure(numeric(0), names=character(0))",
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-logbase/logbase.rds"),
    );
    assert_value_warning(
        &mut session,
        "log10 vector",
        "f(c(1, 10, NA_real_, 100, -10))",
        "c(0, 1, NA_real_, 2, NaN)",
        Some("NaNs produced"),
    );
    assert_value_warning(
        &mut session,
        "log10 empty",
        "f(numeric(0))",
        "numeric(0)",
        None,
    );
    assert_identical(
        &mut session,
        "log10 integer NA",
        "f(c(1L, 10L, NA_integer_))",
        "c(0, 1, NA_real_)",
    );
    assert_identical(
        &mut session,
        "log10 named",
        "f(c(a=1, b=NA_real_, c=10))",
        "c(a=0, b=NA_real_, c=1)",
    );
    assert_identical(
        &mut session,
        "log10 scalar NA",
        "f(NA_real_)",
        "NA_real_",
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-logbase/logbase-var.rds"),
    );
    assert_identical(
        &mut session,
        "log base empty",
        "f(c(1, NA_real_, 10), numeric(0))",
        "numeric(0)",
    );
    assert_identical(
        &mut session,
        "log both empty",
        "f(numeric(0), numeric(0))",
        "numeric(0)",
    );
    assert_identical(
        &mut session,
        "log empty x keeps no base names",
        "f(c(a=1, b=10), numeric(0))",
        "numeric(0)",
    );
    assert_identical(
        &mut session,
        "log empty x keeps names",
        "f(structure(numeric(0), names=character(0)), 10)",
        "structure(numeric(0), names=character(0))",
    );

    load(
        &mut session,
        include_bytes!("fixtures/gnu-bytecode-math1/sin.rds"),
    );
    assert_value_warning(
        &mut session,
        "sin vector",
        "f(c(0, NA_real_, pi/2))",
        "c(0, NA_real_, 1)",
        None,
    );
    assert_value_warning(
        &mut session,
        "sin empty",
        "f(numeric(0))",
        "numeric(0)",
        None,
    );
    assert_identical(
        &mut session,
        "sin integer empty",
        "f(integer(0))",
        "numeric(0)",
    );
    assert_identical(
        &mut session,
        "sin integer NA",
        "f(c(0L, NA_integer_))",
        "c(0, NA_real_)",
    );
    assert_identical(
        &mut session,
        "sin named NA",
        "f(c(a=0, b=NA_real_))",
        "c(a=0, b=NA_real_)",
    );
    assert_identical(&mut session, "sin scalar NA", "f(NA_real_)", "NA_real_");

    load(&mut session, floor);
    assert_identical(
        &mut session,
        "floor vector",
        "f(c(1.7, NA_real_, -1.2))",
        "c(1, NA_real_, -2)",
    );
    assert_identical(&mut session, "floor empty", "f(numeric(0))", "numeric(0)");
    assert_identical(
        &mut session,
        "floor integer empty",
        "f(integer(0))",
        "numeric(0)",
    );
    assert_identical(
        &mut session,
        "floor integer NA",
        "f(c(1L, NA_integer_, -2L))",
        "c(1, NA_real_, -2)",
    );
    assert_identical(
        &mut session,
        "floor named NA",
        "f(c(a=1.2, b=NA_real_))",
        "c(a=1, b=NA_real_)",
    );
}
