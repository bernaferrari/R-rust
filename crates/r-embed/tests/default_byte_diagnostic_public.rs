//! Default rendering retains R's raw bytes and escapes only for UTF8 hosts.
use r_embed::{RSession, RuntimePathPolicy};
const SOURCE: &str = "x<-rawToChar(as.raw(c(97,255)));Encoding(x)<-\"UTF-8\";toupper(x)";

#[test]
fn default_diagnostic_preserves_raw_native_error_buffer() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert!(session.eval(SOURCE).is_err());
    assert_eq!(
        session
            .eval("paste(as.character(charToRaw(geterrmessage())),collapse=\"\")")
            .unwrap(),
        "[1] \"4572726f7220696e20746f7570706572287829203a20696e76616c696420696e707574202761ff2720696e202775746638746f776373270a\"\n"
    );
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}

#[test]
fn default_diagnostic_host_error_explicitly_escapes_the_original_invalid_byte() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    let error = session.eval(SOURCE).unwrap_err().to_string();
    assert!(
        error.contains("invalid input 'a\\xff' in 'utf8towcs'"),
        "host error: {error:?}"
    );
    assert!(!error.contains('\u{fffd}'));
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}

fn decode_field(value: &str) -> String {
    let mut result = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            result.push(ch);
            continue;
        }
        result.push(match chars.next().unwrap() {
            '\\' => '\\',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            other => panic!("invalid oracle escape {other}"),
        });
    }
    result
}

#[test]
fn default_diagnostic_matches_gnu_raw_limits_wrapping_calls_and_host_projection() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    let utf8 = session.eval("isTRUE(l10n_info()[[\"UTF-8\"]])").unwrap() == "[1] TRUE\n";
    let cases = include_str!("fixtures/default-byte-diagnostic-cases.tsv")
        .lines()
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 13);
    let mut failures = Vec::new();
    for row in cases {
        let fields = row.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 6);
        let expected_raw = fields[if utf8 { 3 } else { 2 }];
        let expected_host = decode_field(fields[if utf8 { 5 } else { 4 }]);
        let error = session.eval(fields[1]).unwrap_err().to_string();
        if !error.contains(&expected_host) || error.contains('\u{fffd}') {
            failures.push(format!(
                "{} host {error:?}, expected {expected_host:?}",
                fields[0]
            ));
        }
        let raw = session
            .eval("paste(as.character(charToRaw(geterrmessage())),collapse=\"\")")
            .unwrap();
        if raw != format!("[1] \"{expected_raw}\"\n") {
            failures.push(format!(
                "{} raw {raw:?}, expected {expected_raw}",
                fields[0]
            ));
        }
        assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
    }
    assert!(
        failures.is_empty(),
        "default byte oracle mismatches: {failures:?}"
    );
}
