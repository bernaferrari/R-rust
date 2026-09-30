use r_embed::RSession;

/// GNU R 4.6.1 prints these slot values:
/// A. `setClass("foo", representation(x="numeric", y="numeric")); c(xx@x, xx@y)` → `[1] 1 2`
/// B. `new("B", x=3)` after `contains="A"` → `[1] 3`
/// C. `new("B", a)` copies the superclass slot → `[1] 4`
fn show(session: &mut RSession, code: &str) -> String {
    match session.eval(code) {
        Ok(text) => text.trim_end().to_string(),
        Err(err) => format!("ERR: {err}"),
    }
}

#[test]
fn initialize_copies_representation_and_superclass_slots() {
    let mut session = RSession::new().unwrap();

    let direct = show(
        &mut session,
        r#"local({
            setClass("foo", representation(x="numeric", y="numeric"))
            xx <- new("foo", x=1, y=2)
            c(xx@x, xx@y)
        })"#,
    );
    let inherited = show(
        &mut session,
        r#"local({
            setClass("A", slots=c(x="numeric"))
            setClass("B", contains="A")
            b <- new("B", x=3)
            b@x
        })"#,
    );
    let copied = show(
        &mut session,
        r#"local({
            setClass("A", slots=c(x="numeric"))
            setClass("B", contains="A")
            a <- new("A", x=4)
            b <- new("B", a)
            b@x
        })"#,
    );
    let extends_class = show(
        &mut session,
        r#"local({
            setClass("A", slots=c(x="numeric"))
            setClass("B", contains="A")
            class(possibleExtends("B", "A"))
        })"#,
    );

    assert_eq!(
        (direct.as_str(), inherited.as_str(), copied.as_str()),
        ("[1] 1 2", "[1] 3", "[1] 4"),
        "possibleExtends class: {extends_class}\nA: {direct}\nB: {inherited}\nC: {copied}"
    );
}
