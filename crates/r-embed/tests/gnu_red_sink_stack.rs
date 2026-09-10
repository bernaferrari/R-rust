//! GNU oracle bac583951b728e97b9786804d3b4081f0fe18df5: each expression returns TRUE.
//! Beads: rport-fjge. Assert destinations separately to detect misplaced output.
use r_embed::RSession;

#[test]
fn popping_sink_restores_previous_destination() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("local({a<-tempfile();b<-tempfile();sink(a);sink(b);cat('b');sink();cat('a');sink();identical(readLines(a,warn=FALSE),'a')&&identical(readLines(b,warn=FALSE),'b')})").unwrap().trim(), "[1] TRUE");
}

#[test]
fn split_sink_inside_capture_writes_both_destinations() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("local({p<-tempfile();v<-capture.output({sink(p,split=TRUE);cat('both\\n');sink()});identical(v,'both')&&identical(readLines(p),'both')})").unwrap().trim(), "[1] TRUE");
}
