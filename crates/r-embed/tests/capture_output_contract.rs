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
fn capture_output_dispatches_custom_print_for_visible_values() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("local({print.zz<-function(x,...)cat('custom\n');x<-structure(1,class='zz');identical(capture.output(x), 'custom')})").unwrap().trim(),"[1] TRUE");
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
