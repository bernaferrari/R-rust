//! GNU behavior probes: expected results recorded in fixtures/gnu-differential-wave9/oracle.json.
//! Normal assertions intentionally expose unresolved parity gaps; Beads: rport-inie.

use r_embed::RSession;

#[test]
fn s3_nextmethod_walks_class_vector_in_order() {
    let mut s = RSession::new().unwrap();
    let got = s.eval("local({f<-function(x,...)UseMethod('f');f.default<-function(x,...)'default';f.a<-function(x,...)c('a',NextMethod());f.b<-function(x,...)c('b',NextMethod());paste(f(structure(1,class=c('a','b'))),collapse='/')})").unwrap();
    assert_eq!(got.trim(), "[1] \"a/b/default\"");
}

#[test]
fn s3_usemethod_preserves_named_argument_dispatch() {
    let mut s = RSession::new().unwrap();
    let got = s.eval("local({g<-function(x,extra=2)UseMethod('g');g.a<-function(x,extra=0)c(extra,class(x)[1]);paste(g(extra=7,x=structure(1,class='a')),collapse='/')})").unwrap();
    assert_eq!(got.trim(), "[1] \"7/a\"");
}

#[test]
fn serialization_preserves_shared_structure_and_attributes() {
    let mut s = RSession::new().unwrap();
    let got = s.eval("local({e<-new.env(parent=emptyenv());e$x<-1L;z<-unserialize(serialize(list(e,e),NULL));z[[1]]$x<-9L;a<-structure(1:3,class=c('a','b'),foo=list(1,2));b<-unserialize(serialize(a,NULL));identical(z[[2]]$x,9L)&&identical(a,b)})").unwrap();
    assert_eq!(got.trim(), "[1] TRUE");
}

#[test]
fn s3_method_sees_original_match_call_and_dots() {
    let mut s = RSession::new().unwrap();
    let got = s.eval("local({h<-function(x,...)UseMethod('h');h.a<-function(x,...)list(mc=deparse(match.call()),dots=list(...));r<-h(structure(1,class='a'),k=3);paste(r$mc,r$dots$k,sep='|')})").unwrap();
    assert_eq!(
        got.trim(),
        "[1] \"h.a(x = structure(1, class = \\\"a\\\"), k = 3)|3\""
    );
}
