use r_embed::RSession;

#[test]
fn chartr_enforces_unicode_old_new_length_and_preserves_controls() {
    let mut session = RSession::new().unwrap();
    let got = session
        .eval(
            r#"local({
                short_new_error <- tryCatch(
                    chartr("éβγ", "X", "éβγ"),
                    error = function(e) conditionMessage(e)
                )
                decreasing_range_error <- tryCatch(
                    chartr("γ-α", "Α-Γ", "αβγ"),
                    error = function(e) conditionMessage(e)
                )
                identical(short_new_error, "'old' is longer than 'new'") &&
                    identical(decreasing_range_error, "decreasing range specification ('γ-α')") &&
                    identical(chartr("a-c", "A-C", "abc"), "ABC") &&
                    identical(chartr("α-γ", "Α-Γ", "αβγ"), "ΑΒΓ") &&
                    identical(chartr("aba", "XYZ", "abacaba"), "ZYZcZYZ") &&
                    identical(chartr("éβé", "αβγ", "éβé"), "γβγ") &&
                    identical(chartr("Ā-࿿", "Ā-࿿", "Ā࿿"), "Ā࿿") &&
                    identical(chartr("", "X", "é"), "é") &&
                    identical(chartr("éβγ", "αβγ", "éβγ"), "αβγ")
            })"#,
        )
        .unwrap();
    assert_eq!(got, "[1] TRUE\n");
}
