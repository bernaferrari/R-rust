use r_embed::RSession;

#[test]
fn capture_output_prints_visible_values_and_preserves_invisibility() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("identical(capture.output(1,invisible(2),3,file=NULL,append=FALSE,type='output',split=FALSE),c('[1] 1','[1] 3'))").unwrap().trim(),"[1] TRUE");
}

#[test]
fn capture_output_does_not_duplicate_explicit_output() {
    let mut s = RSession::new().unwrap();
    assert_eq!(
        s.eval(r#"identical(capture.output({cat('a\n');print('b')}),c('a','[1] "b"'))"#)
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn capture_output_uses_gnu_print_method_visibility() {
    // Independently executed with the pinned GNU R 4.7.0 r90451 oracle.
    // utils prints visible values in its own frame; an explicit print(x)
    // executes in the expression's caller and can see a local S3 method.
    let cases = [
        r#"local({print.zz<-function(x,...)cat('custom\n');x<-structure(1,class='zz');identical(capture.output(x), c('[1] 1', 'attr(,"class")', '[1] "zz"'))})"#,
        r#"local({print.zz<-function(x,...)cat('custom\n');x<-structure(1,class='zz');identical(capture.output(print(x)), 'custom')})"#,
        r#"local({print.zz<-function(x,...)cat('custom\n');x<-structure(1,class='zz');identical(utils::capture.output(print(x)), 'custom')})"#,
        r#"print.zz<-function(x,...)cat('global\n'); local({print.zz<-function(x,...)cat('local\n');x<-structure(1,class='zz');identical(capture.output(x),'global') && identical(capture.output(print(x)),'local')})"#,
        r#"rm(print.zz); !exists('print.zz', envir=globalenv(), inherits=FALSE)"#,
    ];
    let portable = r_embed::RuntimePathPolicy::new(Vec::new(), "/tmp");
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        for code in cases {
            assert_eq!(session.eval(code).unwrap().trim(), "[1] TRUE", "{code}");
        }
    }
}

#[test]
fn capture_output_restores_outer_capture_after_errors() {
    let mut s = RSession::new().unwrap();
    assert_eq!(
        s.eval(r#"identical(capture.output(capture.output(1)), '[1] "[1] 1"')"#)
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(s.eval("identical(capture.output({tryCatch(capture.output({cat('discarded\n');stop('boom')}),error=function(e)NULL);cat('after\n')}),'after')").unwrap().trim(),"[1] TRUE");
    assert!(s.eval("capture.output(stop('again'))").is_err());
    assert_eq!(s.eval("cat('recovered')").unwrap(), "recovered");
}

#[test]
fn host_output_preserves_explicit_bytes_and_real_printer_newlines() {
    let mut session = RSession::new().unwrap();
    for (code, expected) in [
        ("cat('recovered')", "recovered"),
        ("cat(' a  \\n\\n')", " a  \n\n"),
        ("cat('\\r')", "\r"),
        ("cat(''); invisible(7L)", ""),
        ("x <- 41; x + 1", "[1] 42\n"),
        ("cat('prefix'); 7L", "prefix[1] 7\n"),
        (
            "f <- function() { on.exit(cat('exit')); return(7L) }; f()",
            "exit[1] 7\n",
        ),
        ("1L; 2L", "[1] 1\n[1] 2\n"),
        (
            "print.zz <- function(x, ...) cat('custom  '); structure(1, class='zz')",
            "custom  ",
        ),
        (
            "print.zz <- function(x, ...) cat('custom\\n\\n'); structure(1, class='zz')",
            "custom\n\n",
        ),
        (
            "print.zz <- function(x, ...) cat('custom  '); structure(1, class='zz'); 2L",
            "custom  [1] 2\n",
        ),
    ] {
        assert_eq!(session.eval(code).unwrap(), expected, "{code}");
    }
    assert!(session.eval("cat('before  '); stop('boom')").is_err());
    assert_eq!(session.eval("cat('recovered')").unwrap(), "recovered");
}
