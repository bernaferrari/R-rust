use r_embed::RSession;

#[test]
fn capture_output_writes_and_appends_filename() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("local({ f<-tempfile(); x<-capture.output(print(1),file=f); y<-capture.output(print(2),file=f,append=TRUE); identical(readLines(f),c('[1] 1','[1] 2')) })")
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn capture_output_keeps_caller_owned_connection_open() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("local({ f<-tempfile(); con<-file(f,'w+'); x<-capture.output(print(1),file=con); still<-isOpen(con); close(con); identical(c(still,readLines(f)),c(TRUE,'[1] 1')) })")
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn capture_file_is_streamed_and_returns_invisible_null() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("local({f<-tempfile();v<-withVisible(capture.output({cat('first\\n');stopifnot(identical(readLines(f),'first'));cat('tail')},file=f));!v$visible&&is.null(v$value)&&identical(readLines(f,warn=FALSE),c('first','tail'))})").unwrap().trim(), "[1] TRUE");
}

#[test]
fn capture_file_keeps_partial_output_on_error_and_restores_capture() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("local({f<-tempfile();tryCatch(capture.output({cat('before\\n');stop('boom')},file=f),error=function(e)NULL);identical(readLines(f),'before')&&identical(capture.output(cat('after')),'after')})").unwrap().trim(), "[1] TRUE");
}

#[test]
fn capture_file_message_and_split_modes() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("local({f<-tempfile();capture.output(message('hello'),file=f,type='message');identical(readLines(f),'hello')})").unwrap().trim(), "[1] TRUE");
    assert_eq!(s.eval("local({f<-tempfile();x<-capture.output(capture.output(cat('tee\\n'),file=f,split=TRUE));identical(x,'tee')&&identical(readLines(f),'tee')})").unwrap().trim(), "[1] TRUE");
}

#[test]
fn sessions_allocate_distinct_uncreated_tempfile_names() {
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads = (0..8)
        .map(|_| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut s = RSession::new().unwrap();
                barrier.wait();
                s.eval("tempfile()").unwrap()
            })
        })
        .collect::<Vec<_>>();
    let names = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        names.len(),
        8,
        "tempfile names collide across sessions before creation"
    );
}

#[test]
fn capture_file_overrides_existing_output_sink() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("local({outer<-tempfile();inner<-tempfile();sink(outer);tryCatch(capture.output(cat('inner'),file=inner),finally=sink());identical(readLines(outer),character())&&identical(readLines(inner,warn=FALSE),'inner')})").unwrap().trim(), "[1] TRUE");
}

#[test]
fn capture_opens_and_closes_deferred_destination() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("local({f<-tempfile();con<-file(f);before<-isOpen(con);v<-withVisible(capture.output(cat('hello'),file=con));closed<-tryCatch({isOpen(con);FALSE},error=function(e)TRUE);!before&&!v$visible&&is.null(v$value)&&closed&&identical(readLines(f,warn=FALSE),'hello')})").unwrap().trim(), "[1] TRUE");
}

#[test]
fn capture_closes_deferred_destination_after_error() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("local({f<-tempfile();con<-file(f);tryCatch(capture.output({cat('before');stop('boom')},file=con),error=function(e)NULL);closed<-tryCatch({isOpen(con);FALSE},error=function(e)TRUE);closed&&identical(readLines(f,warn=FALSE),'before')})").unwrap().trim(), "[1] TRUE");
}
