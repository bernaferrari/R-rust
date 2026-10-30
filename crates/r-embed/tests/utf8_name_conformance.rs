use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn utf8_syntactic_names_match_gnu() {
    let cases = [
        ("á", "c(á = 1)", true),
        ("β", "c(β = 1)", true),
        ("中", "c(中 = 1)", true),
        ("α١", "c(α١ = 1)", true),
        ("١x", "c(١x = 1)", true),
        (".٥", "c(.٥ = 1)", true),
        ("á", "c(\"á\" = 1)", false),
        ("a²", "c(\"a²\" = 1)", false),
        ("a·b", "c(\"a·b\" = 1)", false),
        ("😀", "c(\"😀\" = 1)", false),
        ("_name", "c(\"_name\" = 1)", false),
    ];
    let mut failures = Vec::new();
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        for (name, expected, parses) in cases {
            let expected_r = expected.replace('\\', "\\\\").replace('"', "\\\"");
            let expression = format!(
                "{{ n<-'{name}'; identical(deparse(setNames(1,n)),\"{expected_r}\") && identical(tryCatch({{parse(text=paste0(n,'<-1'));TRUE}},error=function(e)FALSE),{}) }}",
                if parses { "TRUE" } else { "FALSE" }
            );
            let result = session
                .eval(&expression)
                .unwrap_or_else(|e| format!("ERROR: {e}"));
            if result != "[1] TRUE\n" {
                failures.push((portable, name, result));
            }
        }
        for (name, expression) in [
            (
                "Arabic digit identifier",
                "identical(eval(parse(text='١x <- 7L; ١x')),7L)",
            ),
            (
                "named vector roundtrip",
                "{ x<-setNames(c(1L,2L),c('á','β')); identical(eval(parse(text=deparse(x))),x) }",
            ),
            ("pipe call", "identical(2 |> identity(),2)"),
            ("pipe placeholder", "identical(2 |> list(value = _),list(value=2))"),
        ] {
            let result = session
                .eval(expression)
                .unwrap_or_else(|e| format!("ERROR: {e}"));
            if result != "[1] TRUE\n" {
                failures.push((portable, name, result));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "GNU UTF-8 name mismatches: {failures:?}"
    );
}
