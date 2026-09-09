use r_embed::RSession;

#[test]
fn file_path_is_deferred_until_explicit_open() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("local({ p<-tempfile(); con<-file(p); before<-isOpen(con); open(con,'w'); during<-isOpen(con); close(con); identical(c(before,during),c(FALSE,TRUE)) })")
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn deferred_file_open_and_invalid_close_match_connection_errors() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("local({ p<-tempfile(); con<-file(p); open(con,'w'); close(con); tryCatch(isOpen(con),error=function(e)conditionMessage(e)) })")
        .unwrap();
    assert!(result.contains("invalid connection"), "{result}");
}

#[test]
fn deferred_file_read_open_reports_empty_file() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("local({ p<-tempfile(); file.create(p); con<-file(p); open(con,'r'); out<-readLines(con); close(con); identical(out,character()) })")
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn deferred_file_missing_read_reports_connection_error() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("local({ p<-tempfile(); con<-file(p); tryCatch({ open(con,'r'); readLines(con) }, error=function(e) conditionMessage(e)) })")
        .unwrap();
    assert!(result.contains("cannot open the connection"), "{result}");
}
