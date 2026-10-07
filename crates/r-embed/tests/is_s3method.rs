use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn is_s3method_follows_gnu_stop_list_and_registry() {
    for portable in [false, true] {
        let session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        check_stop_list_and_registry(session, portable);
    }
}

fn check_stop_list_and_registry(mut session: RSession, portable: bool) {
    // Stock checks run before registerS3method. t.test is a visible non-method
    // on the search path; the registered method is visible only from an
    // environment that cannot see that function. The user generic is defined
    // in .GlobalEnv so registration and topenv() share one environment.
    let result = session
        .eval(
            r#"
        b <- base::isS3method
        e <- new.env(parent = emptyenv())
        head <- c(
            b(".Internal"),
            b("print", "default"),
            b("print.default"),
            b(f = "print", class = "default"),
            b("t", "test"),
            b("t.test"),
            b(f = "t", class = "test"),
            b("mean.default"),
            b("summary.default"),
            b("print.data.frame"),
            b("print.summaryDefault"),
            b("all.equal"),
            b("seq.int"),
            b("rep.int"),
            b("foo.bar"),
            b(".a"),
            b("a."),
            b(""),
            b("print.default.extra"),
            b("t.test", envir = e)
        )
        mygen <- function(x) UseMethod("mygen")
        mygen.foo <- function(x) 1
        registerS3method("mygen", "hid", function(x) 1)
        registerS3method("t", "test", function(x) x)
        tail <- c(
            b("t.test", envir = e),
            b(f = "t", class = "test", envir = e),
            b("mygen.foo"),
            b("mygen.hid")
        )
        u <- c(
            isS3method(".Internal"),
            isS3method("print", "default"),
            isS3method("print.default"),
            isS3method(f = "print", class = "default"),
            isS3method("t", "test"),
            isS3method("t.test"),
            isS3method(f = "t", class = "test")
        )
        line1 <- paste(c(head, tail, "OK"), collapse = " ")
        line2 <- paste(u, collapse = " ")
        cat(line1, "\n", line2, "\n", sep = "")
    "#,
        )
        .unwrap();
    let expected = "FALSE FALSE TRUE TRUE FALSE FALSE FALSE TRUE TRUE TRUE TRUE FALSE FALSE FALSE FALSE FALSE FALSE FALSE FALSE FALSE TRUE TRUE TRUE TRUE OK\nFALSE FALSE TRUE TRUE FALSE FALSE FALSE";
    // Diagnose only after the unchanged public program has computed its result.
    // Preforcing t.test here would hide fresh-session lookup failures.
    let diagnostic = if result.trim() != expected {
        session.eval("local({f<-get('isS3method',mode='function');m<-get('t.test',mode='function');print(c(reflection=environmentName(environment(f)),visible=environmentName(environment(m))));print(tools::nonS3methods('stats'));print(c(isNamespace(environment(m)),isS3method('t.test')))})")
    } else {
        Ok(String::new())
    };
    assert_eq!(
        result.trim(),
        expected,
        "portable={portable}; post-result diagnostic={diagnostic:?}"
    );
}
