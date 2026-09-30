use r_embed::RSession;

/// GNU R 4.6.1:
/// `new("mpDput", .Data = list(new("mp1Dput", prec = 1L, d = 3:5)))`
#[test]
fn s4_list_dput_includes_data_part() {
    let mut session = RSession::new().unwrap();
    let out = session
        .eval(
            r#"
invisible(require(methods, quietly=TRUE))
setClass("mp1Dput", slots = c(prec = "integer", d = "integer"))
setClass("mpDput", contains = "list")
m <- new("mpDput", list(new("mp1Dput", prec=1L, d=3:5)))
cat(paste(capture.output(dput(m)), collapse = "\n"), "\n")
"#,
        )
        .unwrap();
    let out = out.trim();
    assert!(
        out.contains(".Data") && out.contains("prec = 1L"),
        "dput of a list-class S4 object must emit .Data and prec: {out}"
    );
}
