use r_embed::{RSession, RuntimePathPolicy};
use rmath::android::RValue;

fn capture_example(session: &mut RSession, topic: &str, package: &str) -> Vec<Option<String>> {
    let code = format!("capture.output(example({topic:?}, package={package:?}, echo=TRUE))");
    capture_code(session, &code)
}

fn capture_code(session: &mut RSession, code: &str) -> Vec<Option<String>> {
    let result = session
        .eval_result(code)
        .unwrap_or_else(|error| panic!("evaluation failed: {error}"));
    match result.value {
        RValue::StringVector(lines) => lines,
        value => panic!("expected captured example lines, got {value:?}"),
    }
}

fn fixture_lines(fixture: &'static str) -> Vec<Option<String>> {
    fixture
        .lines()
        .map(|line| {
            Some(
                line.strip_suffix('␠')
                    .map_or_else(|| line.to_owned(), |prefix| format!("{prefix} ")),
            )
        })
        .collect()
}

#[test]
fn portable_package_example_prompt_prefixes_match_gnu() {
    let mut session = RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp"))
        .expect("filesystem-free portable session initializes");
    session.enable_browser_files();

    let new_string_topic = capture_example(&mut session, "new", "methods");
    let new_symbol_topic = capture_code(
        &mut session,
        "capture.output(example(new, package=\"methods\", echo=TRUE))",
    );
    let initialize_alias = capture_example(&mut session, "initialize", "methods");
    let filter = capture_example(&mut session, "filter", "stats");
    let custom_options = capture_code(
        &mut session,
        r#"local({ old <- options(prompt="R> ", continue="C> "); on.exit(options(old)); capture.output(example("new", package="methods", echo=TRUE)) })"#,
    );
    let echo_disabled = capture_code(
        &mut session,
        "capture.output(example(\"new\", package=\"methods\", echo=FALSE))",
    );
    assert!(
        echo_disabled.is_empty(),
        "echo=FALSE must suppress captured output"
    );

    let comparisons = [
        (
            "methods::new character topic",
            new_string_topic,
            fixture_lines(include_str!(
                "fixtures/portable-example-methods-new-echo.gnu.out"
            )),
        ),
        (
            "methods::new symbol topic",
            new_symbol_topic,
            fixture_lines(include_str!(
                "fixtures/portable-example-methods-new-echo.gnu.out"
            )),
        ),
        (
            "methods::initialize alias",
            initialize_alias,
            fixture_lines(include_str!(
                "fixtures/portable-example-methods-initialize-echo.gnu.out"
            )),
        ),
        (
            "stats::filter",
            filter,
            fixture_lines(include_str!(
                "fixtures/portable-example-stats-filter-echo.gnu.out"
            )),
        ),
        (
            "methods::new custom prompt/continue",
            custom_options,
            fixture_lines(include_str!(
                "fixtures/portable-example-methods-new-custom-prompt.gnu.out"
            )),
        ),
    ];
    for (name, actual, expected) in &comparisons {
        if actual != expected {
            let differences = (0..actual.len().max(expected.len()))
                .filter(|index| actual.get(*index) != expected.get(*index))
                .collect::<Vec<_>>();
            eprintln!("{name}: {} captured lines differ", differences.len());
            for index in differences.iter().take(6) {
                eprintln!(
                    "  line {}: port={:?}; GNU={:?}",
                    index + 1,
                    actual.get(*index),
                    expected.get(*index)
                );
            }
        }
    }
    assert!(
        comparisons
            .iter()
            .all(|(_, actual, expected)| actual == expected),
        "one or more embedded example echoes differ from pinned GNU"
    );
}
