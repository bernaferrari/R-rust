//! GNU R's Ops conflict resolver: chooseOpsMethod and date/time precedence.

use r_embed::RSession;

#[test]
fn choose_ops_method_can_select_the_right_hand_method() {
    let mut session = RSession::new().unwrap();
    let value = session
        .eval(
            r#"
            `+.foo` <- function(e1, e2) "foo"
            `+.bar` <- function(e1, e2) "bar"
            chooseOpsMethod.bar <- function(x, y, mx, my, cl, reverse) TRUE
            foo <- structure(1, class = "foo")
            bar <- structure(1, class = "bar")
            c(foo + bar, bar + foo)
            "#,
        )
        .unwrap();
    assert_eq!(value.trim(), "[1] \"bar\" \"bar\"");
}

#[test]
fn date_and_posix_methods_beat_difftime_on_both_sides() {
    let mut session = RSession::new().unwrap();
    let value = session
        .eval(
            r#"
            `+.Date` <- function(e1, e2) "date"
            `+.POSIXt` <- function(e1, e2) "posix"
            Ops.difftime <- function(e1, e2) "difftime"
            d <- structure(1, class = "Date")
            p <- structure(1, class = "POSIXt")
            t <- structure(1, class = "difftime", units = "days")
            paste(d + t, t + d, p + t, t + p, sep="|")
            "#,
        )
        .unwrap();
    assert_eq!(value.trim(), "[1] \"date|date|posix|posix\"");
}

#[test]
fn choose_ops_method_requires_a_scalar_non_na_logical() {
    let mut session = RSession::new().unwrap();
    let error = session
        .eval(
            r#"
            `+.foo` <- function(e1, e2) "foo"
            `+.bar` <- function(e1, e2) "bar"
            chooseOpsMethod.bar <- function(x, y, mx, my, cl, reverse) {
                gc()
                c(TRUE, FALSE)
            }
            foo <- structure(1, class = "foo")
            bar <- structure(1, class = "bar")
            foo + bar
            "#,
        )
        .expect_err("invalid chooseOpsMethod result should error");
    assert!(error.to_string().contains("length"));
}

#[test]
fn chooser_receives_original_call_and_reversed_method_objects() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            r#"
        `+.foo` <- function(e1,e2) 10L
        `+.bar` <- function(e1,e2) 20L
        chooseOpsMethod.foo <- function(x,y,mx,my,cl,reverse) NULL
        chooseOpsMethod.bar <- function(x,y,mx,my,cl,reverse) {
            gc()
            stopifnot(identical(mx, `+.bar`), identical(my, `+.foo`),
                identical(cl, quote(foo + bar)), reverse,
                identical(substitute(x), quote(x)),
                identical(substitute(reverse), quote(rev)))
            1L
        }
        foo <- structure(1,class='foo'); bar <- structure(2,class='bar')
        identical(foo + bar,20L)
    "#,
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
    for value in ["NA", "logical(0)", "c(TRUE,FALSE)"] {
        assert!(
            session
                .eval(&format!(
                    "chooseOpsMethod.bar<-function(x,y,mx,my,cl,reverse) {value};foo+bar"
                ))
                .is_err()
        );
        assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
    }
    assert_eq!(session.eval("chooseOpsMethod.bar<-function(x,y,mx,my,cl,reverse)NULL;suppressWarnings(identical(unclass(foo+bar),3))").unwrap().trim(), "[1] TRUE");
}
