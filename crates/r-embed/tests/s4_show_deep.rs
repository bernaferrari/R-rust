use r_embed::RSession;
use std::process::Command;

fn isolated(name: &str, run: impl FnOnce()) {
    const CHILD: &str = "RPORT_S4_SHOW_DEEP_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(name) {
        run();
        return;
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env(CHILD, name)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("spawn S4 show regression on the ordinary test thread stack");
    assert!(
        output.status.success(),
        "S4 show child failed ({}):\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn recursive_custom_show_reaches_the_expression_limit() {
    isolated("recursive_custom_show_reaches_the_expression_limit", || {
        let mut session = RSession::new().unwrap();
        let output = session
            .eval(
                r#"
            setClass("RecursiveShow", slots=c(value="numeric"))
            setMethod("show", "RecursiveShow", function(object) {
                cat("RECURSIVE-SHOW\n")
                print(object)
            })
            options(expressions=40)
            tryCatch(show(new("RecursiveShow", value=1)),
                     error=function(e) cat("CAUGHT", conditionMessage(e), "\n"))
        "#,
            )
            .expect("tryCatch must catch the custom show recursion limit");
        assert!(output.contains("RECURSIVE-SHOW"), "{output}");
        assert!(output.contains("CAUGHT"), "{output}");
        assert!(output.contains("evaluation nested too deeply"), "{output}");
    });
}
