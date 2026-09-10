//! Expected TRUE results verified with GNU R oracle bac583951b728e97b9786804d3b4081f0fe18df5.
//! Beads: rport-bt3e / rport-rp3v.
use r_embed::RSession;

#[test]
fn structure_does_not_class_the_original_numeric_value() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "x<-1:3;y<-structure(x,class='a');identical(c(is.object(x),is.object(y),identical(unclass(y),x)),c(FALSE,TRUE,TRUE))",
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn evaluating_quoted_structure_keeps_the_source_ast_unchanged() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "q<-quote(structure(1,class='a'));before<-deparse(q);r<-eval(q);identical(before,deparse(q))&&identical(class(r),'a')",
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn structure_does_not_change_s3_call_metadata_or_argument_class() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "f<-function(x){structure(x,class='a');c(deparse(sys.call()),deparse(match.call()),class(x))};identical(f(1:2),c('f(1:2)','f(x = 1:2)','integer'))",
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn s3_method_preserves_substitute_and_match_call_source() {
    let mut session = RSession::new().unwrap();
    let got = session.eval("local({h<-function(x,...)UseMethod('h');h.a<-function(x,...)list(substitute(x),substitute(list(...)),match.call());identical(h(structure(1,class='a'),k=3),list(quote(structure(1,class='a')),quote(list(k=3)),quote(h.a(x=structure(1,class='a'),k=3))))})").unwrap();
    assert_eq!(got.trim(), "[1] TRUE");
}
