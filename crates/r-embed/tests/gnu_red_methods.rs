//! Deliberately red conformance probes for dispatch edges not covered by the
//! existing S3/S4 suites. Expected values come from the pinned GNU R oracle.

// Beads: rport-sewq (callNextMethod), rport-m4w9 (Recall).
use r_embed::RSession;

#[test]
fn s4_call_next_method_walks_the_inheritance_chain() {
    // GNU R oracle:
    // setGeneric('foo',function(x) standardGeneric('foo')); setMethod('foo','ANY',function(x) 'any'); setMethod('foo','numeric',function(x) paste('num',callNextMethod())); foo(1)
    let mut session = RSession::new().unwrap();
    let value = session
        .eval(
            "local({ setGeneric('foo',function(x) standardGeneric('foo')); setMethod('foo','ANY',function(x) 'any'); setMethod('foo','numeric',function(x) paste('num',callNextMethod())); foo(1) })",
        )
        .unwrap();
    assert_eq!(value.trim(), "[1] \"num any\"");
}

#[test]
fn s4_call_next_method_preserves_s4_class_inheritance() {
    // GNU R oracle:
    // setClass('A',slots=c(x='numeric')); setClass('B',contains='A'); setGeneric('slotshow',function(x) standardGeneric('slotshow')); setMethod('slotshow','A',function(x) 'A'); setMethod('slotshow','B',function(x) paste('B',callNextMethod())); slotshow(new('B',x=1))
    let mut session = RSession::new().unwrap();
    let value = session
        .eval(
            "local({ setClass('A',slots=c(x='numeric')); setClass('B',contains='A'); setGeneric('slotshow',function(x) standardGeneric('slotshow')); setMethod('slotshow','A',function(x) 'A'); setMethod('slotshow','B',function(x) paste('B',callNextMethod())); slotshow(new('B',x=1)) })",
        )
        .unwrap();
    assert_eq!(value.trim(), "[1] \"B A\"");
}

#[test]
fn recall_reenters_the_s3_method_with_updated_arguments() {
    // GNU R oracle:
    // f<-function(x)UseMethod('f'); f.a<-function(x)if(x>0)Recall(x-1)else'done'; f(structure(3,class='a'))
    let mut session = RSession::new().unwrap();
    let value = session
        .eval(
            "local({ f<-function(x)UseMethod('f'); f.a<-function(x)if(x>0)Recall(x-1)else'done'; f(structure(3,class='a')) })",
        )
        .unwrap();
    assert_eq!(value.trim(), "[1] \"done\"");
}
